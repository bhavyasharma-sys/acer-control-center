"""Telemetry read straight from /proc and /sys, plus nvidia-smi.

Every reader returns None when its source is missing, so the UI can say
"Unavailable" instead of inventing a zero. Nothing here touches the kernel
driver or the daemon; it is plain file reads and one subprocess with a hard
timeout.
"""

from __future__ import annotations

import os
import shutil
import subprocess
import time

MIB = 1024 * 1024


def read_text(path: str) -> str | None:
    try:
        with open(path, "r", encoding="utf-8", errors="replace") as handle:
            return handle.read()
    except OSError:
        return None


def read_int(path: str) -> int | None:
    text = read_text(path)
    if text is None:
        return None
    try:
        return int(text.strip())
    except ValueError:
        return None


# ---------------------------------------------------------------------- cpu

def parse_cpu_sample(text: str | None) -> tuple[int, int] | None:
    """First line of /proc/stat -> (total jiffies, idle jiffies)."""
    if not text:
        return None
    first = text.splitlines()[:1]
    if not first:
        return None
    fields = first[0].split()
    if not fields or fields[0] != "cpu":
        return None
    try:
        values = [int(field) for field in fields[1:]]
    except ValueError:
        return None
    if len(values) < 5:
        return None
    idle = values[3] + values[4]  # idle + iowait
    return sum(values), idle


def cpu_usage_percent(
    previous: tuple[int, int] | None, current: tuple[int, int] | None
) -> int | None:
    if previous is None or current is None:
        return None
    total_delta = current[0] - previous[0]
    idle_delta = current[1] - previous[1]
    if total_delta <= 0 or idle_delta > total_delta:
        return None
    used = (total_delta - idle_delta) * 100 // total_delta
    return max(0, min(100, used))


def cpu_clocks() -> tuple[int | None, int | None]:
    """(average current MHz of online cores, maximum MHz)."""
    base = "/sys/devices/system/cpu"
    try:
        entries = sorted(os.listdir(base))
    except OSError:
        return None, None
    currents: list[int] = []
    maximum: int | None = None
    for name in entries:
        if not name.startswith("cpu") or not name[3:].isdigit():
            continue
        cpufreq = os.path.join(base, name, "cpufreq")
        current = None
        for filename in ("scaling_cur_freq", "cpuinfo_cur_freq"):
            current = read_int(os.path.join(cpufreq, filename))
            if current is not None:
                break
        if current is None:
            continue
        currents.append(current)
        for filename in ("scaling_max_freq", "cpuinfo_max_freq"):
            value = read_int(os.path.join(cpufreq, filename))
            if value is not None:
                maximum = value if maximum is None else max(maximum, value)
                break
    if not currents:
        return None, None
    average = sum(currents) // len(currents) // 1000
    return average, (maximum // 1000 if maximum else None)


# --------------------------------------------------------------- temps/fans

def _hwmon_dirs() -> list[str]:
    try:
        names = os.listdir("/sys/class/hwmon")
    except OSError:
        return []
    return [os.path.join("/sys/class/hwmon", name) for name in sorted(names)]


def _is_gpu_name(name: str) -> bool:
    return any(key in name for key in ("nvidia", "gpu", "amdgpu", "nouveau"))


def _is_cpu_name(name: str) -> bool:
    return any(
        key in name for key in ("coretemp", "k10temp", "zenpower", "cpu", "acpitz")
    )


def temperatures() -> tuple[int | None, int | None]:
    """(cpu °C, gpu °C)."""
    cpu: int | None = None
    gpu: int | None = None
    for directory in _hwmon_dirs():
        name = (read_text(os.path.join(directory, "name")) or "").strip().lower()
        try:
            entries = sorted(os.listdir(directory))
        except OSError:
            continue
        for entry in entries:
            if not entry.startswith("temp") or not entry.endswith("_input"):
                continue
            value = read_int(os.path.join(directory, entry))
            if value is None:
                continue
            celsius = value // 1000
            label = (
                read_text(os.path.join(directory, entry[: -len("_input")] + "_label"))
                or ""
            ).lower()
            if _is_gpu_name(name) or "gpu" in label:
                gpu = celsius if gpu is None else max(gpu, celsius)
            elif _is_cpu_name(name) or any(
                key in label for key in ("package", "tctl", "tdie", "cpu", "acpi")
            ):
                cpu = celsius if cpu is None else max(cpu, celsius)
    if gpu is None:
        gpu = nvidia_snapshot().get("temperature_c")
    if cpu is None:
        cpu = _thermal_zone_cpu()
    return cpu, gpu


def _thermal_zone_cpu() -> int | None:
    try:
        names = sorted(os.listdir("/sys/class/thermal"))
    except OSError:
        return None
    for name in names:
        zone = os.path.join("/sys/class/thermal", name)
        kind = (read_text(os.path.join(zone, "type")) or "").lower()
        if any(key in kind for key in ("x86_pkg", "cpu", "package", "acpi")):
            value = read_int(os.path.join(zone, "temp"))
            if value is not None:
                return value // 1000
    return None


def fans() -> tuple[int | None, int | None]:
    """(cpu fan RPM, gpu fan RPM)."""
    cpu: int | None = None
    gpu: int | None = None
    for directory in _hwmon_dirs():
        name = (read_text(os.path.join(directory, "name")) or "").strip().lower()
        try:
            entries = sorted(os.listdir(directory))
        except OSError:
            continue
        for entry in entries:
            if not entry.startswith("fan") or not entry.endswith("_input"):
                continue
            rpm = read_int(os.path.join(directory, entry))
            if rpm is None:
                continue
            if _is_gpu_name(name):
                gpu = rpm if gpu is None else max(gpu, rpm)
            else:
                cpu = rpm if cpu is None else max(cpu, rpm)
    return cpu, gpu


# --------------------------------------------------------------------- gpu

NVIDIA_QUERY = [
    "nvidia-smi",
    "--query-gpu=temperature.gpu,clocks.sm,utilization.gpu,"
    "memory.used,memory.total,power.draw,fan.speed",
    "--format=csv,noheader,nounits",
    "-i",
    "0",
]


def parse_nvidia_line(line: str | None) -> dict | None:
    if not line:
        return None
    fields = [field.strip() for field in line.split(",")]
    if len(fields) not in (6, 7):
        return None

    def number(index: int) -> float | None:
        raw = fields[index]
        if raw in ("", "N/A", "[N/A]", "Not Supported", "Not Available"):
            return None
        try:
            return float(raw)
        except ValueError:
            return None

    temperature = number(0)
    clock = number(1)
    usage = number(2)
    memory_used = number(3)
    memory_total = number(4)
    power = number(5)
    fan = number(6) if len(fields) == 7 else None
    return {
        "temperature_c": int(temperature) if temperature is not None else None,
        "clock_mhz": int(clock) if clock is not None else None,
        "usage_percent": (
            max(0, min(100, int(usage))) if usage is not None else None
        ),
        "memory_mib": (
            [int(memory_used), int(memory_total)]
            if memory_used is not None and memory_total is not None
            else None
        ),
        "power_watts": round(power, 1) if power is not None else None,
        "fan_percent": (
            max(0, min(100, int(fan))) if fan is not None else None
        ),
    }


def hwmon_fan_exposed() -> bool:
    """True when any hwmon chip exposes at least one fan input."""
    for directory in _hwmon_dirs():
        try:
            entries = os.listdir(directory)
        except OSError:
            continue
        if any(entry.startswith("fan") and entry.endswith("_input")
               for entry in entries):
            return True
    return False


def run_nvidia_smi(timeout: float = 3.0) -> str | None:
    if shutil.which("nvidia-smi") is None:
        return None
    try:
        completed = subprocess.run(
            NVIDIA_QUERY,
            capture_output=True,
            text=True,
            timeout=timeout,
            check=False,
        )
    except (OSError, subprocess.SubprocessError):
        return None
    if completed.returncode != 0:
        return None
    return completed.stdout


def nvidia_snapshot() -> dict:
    output = run_nvidia_smi()
    if not output:
        return {}
    lines = output.splitlines()
    if not lines:
        return {}
    return parse_nvidia_line(lines[0]) or {}


# ------------------------------------------------------------------ memory

def parse_meminfo(text: str | None) -> tuple[int, int] | None:
    """(used MiB, total MiB) from /proc/meminfo."""
    if not text:
        return None
    values: dict[str, int] = {}
    for line in text.splitlines():
        parts = line.split()
        if len(parts) >= 2 and parts[0].endswith(":"):
            try:
                values[parts[0][:-1]] = int(parts[1])
            except ValueError:
                continue
    total = values.get("MemTotal")
    available = values.get("MemAvailable", values.get("MemFree"))
    if total is None or available is None:
        return None
    used = max(0, total - available)
    return used // 1024, total // 1024


def memory() -> tuple[int, int] | None:
    return parse_meminfo(read_text("/proc/meminfo"))


def disk() -> tuple[int, int] | None:
    """(used MiB, total MiB) of the root filesystem."""
    try:
        stats = os.statvfs("/")
    except OSError:
        return None
    total = stats.f_blocks * stats.f_frsize
    free = stats.f_bavail * stats.f_frsize
    if total <= 0:
        return None
    return (total - free) // MIB, total // MIB


# ------------------------------------------------------------------ battery

def battery() -> dict | None:
    base = "/sys/class/power_supply"
    try:
        names = sorted(os.listdir(base))
    except OSError:
        return None
    for name in names:
        supply = os.path.join(base, name)
        kind = (read_text(os.path.join(supply, "type")) or "").strip().lower()
        if kind != "battery":
            continue
        capacity = read_int(os.path.join(supply, "capacity"))
        status = (read_text(os.path.join(supply, "status")) or "").strip()
        return {
            "name": name,
            "percent": capacity,
            "status": status or None,
        }
    return None


# ---------------------------------------------------------------- snapshot

class Telemetry:
    """Holds the previous /proc/stat sample so usage can be a real delta."""

    def __init__(self) -> None:
        self._previous_cpu: tuple[int, int] | None = None

    def snapshot(self) -> dict:
        cpu_sample = parse_cpu_sample(read_text("/proc/stat"))
        usage = cpu_usage_percent(self._previous_cpu, cpu_sample)
        self._previous_cpu = cpu_sample
        clock_current, clock_max = cpu_clocks()
        cpu_temp, gpu_temp = temperatures()
        cpu_fan, gpu_fan = fans()
        gpu = nvidia_snapshot()
        mem = memory()
        storage = disk()
        return {
            "cpu_usage_percent": usage,
            "cpu_temperature_c": cpu_temp,
            "gpu_temperature_c": gpu.get("temperature_c", gpu_temp),
            "cpu_clock_mhz": clock_current,
            "cpu_clock_max_mhz": clock_max,
            "gpu_clock_mhz": gpu.get("clock_mhz"),
            "gpu_usage_percent": gpu.get("usage_percent"),
            "gpu_power_watts": gpu.get("power_watts"),
            "cpu_fan_rpm": cpu_fan,
            "gpu_fan_rpm": gpu_fan,
            "gpu_fan_percent": gpu.get("fan_percent"),
            "cpu_fan_exposed": hwmon_fan_exposed(),
            "memory_mib": list(mem) if mem else None,
            "storage_mib": list(storage) if storage else None,
            "gpu_memory_mib": gpu.get("memory_mib"),
            "battery": battery(),
            "sampled_at": time.time(),
        }
