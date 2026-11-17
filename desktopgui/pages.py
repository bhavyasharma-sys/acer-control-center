"""The six pages, composed from the widget kit.

Widget instances are created once and mutated as state arrives, so a slider
keeps its drag position and a segmented control keeps its selection between
frames.
"""

from __future__ import annotations

from .theme import C, UNAVAILABLE
from .widgets import (
    BarRow,
    Button,
    Card,
    GaugeBlock,
    Segmented,
    SliderRow,
    StatRow,
    StepperRow,
    SwatchRow,
    TextBlock,
)

FAN_FULL_SCALE = 6000
SWATCHES = [(0, 229, 255), (255, 45, 85), (52, 224, 122), (255, 179, 0),
            (138, 92, 255), (255, 255, 255)]
IDLE_OPTIONS = [("Off", 0)] + [(f"{s} s", s) for s in (5, 10, 15, 20, 25, 30)]


def _message_ok(message: str) -> bool:
    if message.startswith("OK:"):
        return True
    if message.startswith("FAIL:"):
        return False
    return "read back" in message and "not applied" not in message


def temperature_color(value):
    if value is None:
        return C().MUTED
    if value >= 85:
        return C().HOT
    if value >= 70:
        return C().WARM
    return C().COOL


class Pages:
    def __init__(self) -> None:
        # dashboard
        self.modes = Segmented([], selected=None)
        self.apply_button = Button("Apply", primary=True)
        self.apply_status = TextBlock("No apply yet.", 13, C().MUTED)
        self.cpu_clock = StatRow("CPU clock (avg)")
        self.cpu_temp = StatRow("CPU temperature")
        self.gpu_clock = StatRow("GPU clock")
        self.gpu_temp = StatRow("GPU temperature")
        self.cpu_use = BarRow("CPU use")
        self.gpu_use = BarRow("GPU use", C().COOL)
        self.gauge = GaugeBlock()
        self.gauge_note = TextBlock(
            "Average of online cores · scale from the detected cpufreq maximum",
            11, C().MUTED, center=True)
        self.cpu_fan = BarRow("CPU fan", C().WARM)
        self.gpu_fan = BarRow("GPU fan", C().WARM)
        self.fan_mode = Segmented([("Automatic", "auto"), ("Maximum", "max")])
        self.mem = BarRow("Memory", (74, 163, 224))
        self.disk = BarRow("Storage /", (74, 163, 224))
        self.vram = BarRow("VRAM", C().COOL)
        self.sources = TextBlock("/proc/meminfo · statvfs on / · NVIDIA telemetry", 11, C().MUTED)
        self.target_name = TextBlock("—", 20, C().TEXT, bold=True)
        self.target_cpu = StatRow("CPU limit (software request)")
        self.target_gpu = StatRow("GPU limit (software request)")
        self.target_note = TextBlock(
            "Selecting a mode only stores the target. Caps are written when you "
            "press Apply and each write is verified by readback.", 12, C().MUTED)
        self.apply_result = TextBlock("", 12, C().MUTED)
        self.apply_result_gpu = TextBlock("", 12, C().MUTED)

        # cooling
        self.cooling_fan = Segmented([("Automatic", "auto"), ("Maximum", "max")], big=True)
        self.cooling_note = TextBlock(
            "Maximum pins the fans at full speed; automatic returns control to "
            "the firmware curve.", 12, C().MUTED)
        self.cooling_status = TextBlock("", 12, C().MUTED)
        self.c_cpu_fan = StatRow("CPU fan")
        self.c_gpu_fan = StatRow("GPU fan")
        self.c_cpu_temp = StatRow("CPU temperature")
        self.c_gpu_temp = StatRow("GPU temperature")
        self.c_gpu_power = StatRow("GPU power draw")

        # keyboard
        self.brightness = SliderRow("Brightness", 0)
        self.brightness_note = TextBlock("The daemon writes the EC on release.", 12, C().MUTED)
        self.swatches = SwatchRow(SWATCHES)
        self.breathing = Button("Breathing: unknown")
        self.idle = Segmented(IDLE_OPTIONS)
        self.idle_note = TextBlock(
            "Stored for the desktop session, which fades the keyboard after "
            "inactivity and restores it when you return.", 12, C().MUTED)

        # performance
        self.steppers: list[StepperRow] = []
        for index in range(3):
            self.steppers.append(StepperRow(f"Profile {index + 1} CPU", 30))
            self.steppers.append(StepperRow(f"Profile {index + 1} GPU", 60))
        self.save_button = Button("Save profiles", primary=True)
        self.profile_status = TextBlock("", 12, C().MUTED)

        # battery
        self.b_percent = StatRow("Charge")
        self.b_status = StatRow("Status")
        self.b_power = StatRow("GPU power draw")
        self.b_bar = BarRow("Charge", (74, 163, 224))

        # settings
        self.s_frontend = StatRow("Frontend")
        self.s_daemon = StatRow("Daemon")
        self.s_reachable = StatRow("Daemon reachable")
        self.s_busctl = StatRow("busctl present")
        self.s_how = TextBlock(
            "A pygame desktop window over a standard-library Python backend. "
            "Hardware calls go to acer-control-daemon on the system D-Bus via "
            "busctl; telemetry is read from /proc and /sys.", 12, C().MUTED)

    # ------------------------------------------------------------ state in

    def update(self, state: dict, telemetry: dict) -> None:
        names = state.get("profile_names") or ["Performance", "Balanced", "Battery Saver"]
        selected = state.get("selected_profile")
        self.modes.options = [(name, index) for index, name in enumerate(names)]
        self.modes.selected = selected
        self.fan_mode.selected = "max" if state.get("fan_max") else (
            "auto" if state.get("fan_max") is False else None)
        self.cooling_fan.selected = self.fan_mode.selected

        last = state.get("last_apply")
        if last is None:
            self.apply_status.text = "No apply yet."
            self.apply_status.color = C().MUTED
        elif last.get("ok"):
            self.apply_status.text = "Applied and verified by readback."
            self.apply_status.color = C().COOL
        else:
            name = names[selected] if isinstance(selected, int) \
                and selected < len(names) else "Profile"
            self.apply_status.text = last.get("error") or f"{name} not applied yet."
            self.apply_status.color = C().HOT

        self.cpu_clock.value = None if telemetry.get("cpu_clock_mhz") is None \
            else f"{telemetry['cpu_clock_mhz']} MHz"
        self.cpu_temp.value = None if telemetry.get("cpu_temperature_c") is None \
            else f"{telemetry['cpu_temperature_c']} °C"
        self.cpu_temp.color = temperature_color(telemetry.get("cpu_temperature_c"))
        self.gpu_clock.value = None if telemetry.get("gpu_clock_mhz") is None \
            else f"{telemetry['gpu_clock_mhz']} MHz"
        self.gpu_temp.value = None if telemetry.get("gpu_temperature_c") is None \
            else f"{telemetry['gpu_temperature_c']} °C"
        self.gpu_temp.color = temperature_color(telemetry.get("gpu_temperature_c"))

        for bar, key in ((self.cpu_use, "cpu_usage_percent"), (self.gpu_use, "gpu_usage_percent")):
            value = telemetry.get(key)
            bar.fraction = None if value is None else value / 100
            bar.text = UNAVAILABLE if value is None else f"{value} %"

        clock = telemetry.get("cpu_clock_mhz")
        clock_max = telemetry.get("cpu_clock_max_mhz")
        self.gauge.fraction = None if (clock is None or not clock_max) else clock / clock_max
        self.gauge.readout = UNAVAILABLE if clock is None else str(clock)
        self.gauge.scale_max = "—" if clock_max is None else str(clock_max)

        cpu_rpm = telemetry.get("cpu_fan_rpm")
        if cpu_rpm is None and telemetry.get("cpu_fan_exposed") is False:
            self.cpu_fan.fraction = None
            self.cpu_fan.text = "Not exposed by kernel"
        else:
            self.cpu_fan.fraction = None if cpu_rpm is None else cpu_rpm / FAN_FULL_SCALE
            self.cpu_fan.text = UNAVAILABLE if cpu_rpm is None else f"{cpu_rpm} RPM"
        gpu_rpm = telemetry.get("gpu_fan_rpm")
        gpu_percent = telemetry.get("gpu_fan_percent")
        if gpu_rpm is not None:
            self.gpu_fan.fraction = gpu_rpm / FAN_FULL_SCALE
            self.gpu_fan.text = f"{gpu_rpm} RPM"
        elif gpu_percent is not None:
            self.gpu_fan.fraction = gpu_percent / 100
            self.gpu_fan.text = f"{gpu_percent} %"
        else:
            self.gpu_fan.fraction = None
            self.gpu_fan.text = UNAVAILABLE

        for bar, key in ((self.mem, "memory_mib"), (self.disk, "storage_mib")):
            pair = telemetry.get(key)
            if pair:
                bar.fraction = pair[0] / pair[1] if pair[1] else None
                bar.text = f"{pair[0] / 1024:.1f} / {pair[1] / 1024:.1f} GiB"
            else:
                bar.fraction = None
                bar.text = UNAVAILABLE
        vram = telemetry.get("gpu_memory_mib")
        if vram:
            self.vram.fraction = vram[0] / vram[1] if vram[1] else None
            self.vram.text = f"{vram[0]} / {vram[1]} MiB"
        else:
            self.vram.fraction = None
            self.vram.text = UNAVAILABLE

        profiles = state.get("profiles") or [[None, None], [None, None], [None, None]]
        pair = profiles[selected] if isinstance(selected, int) and selected < len(profiles) \
            else [None, None]
        self.target_name.text = names[selected] if isinstance(selected, int) \
            and selected < len(names) else "—"
        self.target_cpu.value = None if pair[0] is None else f"{pair[0]} W"
        self.target_gpu.value = None if pair[1] is None else f"{pair[1]} W"
        if last and (last.get("cpu_message") or last.get("error")):
            if last.get("error"):
                self.apply_result.text = last["error"]
                self.apply_result.color = C().HOT
                self.apply_result_gpu.text = ""
            else:
                cpu_ok = last.get("cpu_ok", _message_ok(last.get("cpu_message") or ""))
                gpu_ok = last.get("gpu_ok", _message_ok(last.get("gpu_message") or ""))
                self.apply_result.text = f"CPU: {last.get('cpu_message')}"
                self.apply_result.color = C().COOL if cpu_ok else C().HOT
                self.apply_result_gpu.text = f"GPU: {last.get('gpu_message')}"
                self.apply_result_gpu.color = C().COOL if gpu_ok else C().HOT
        else:
            self.apply_result.text = ""
            self.apply_result_gpu.text = ""

        # cooling
        self.c_cpu_fan.value = self.cpu_fan.text if self.cpu_fan.text != UNAVAILABLE else None
        self.c_gpu_fan.value = self.gpu_fan.text if self.gpu_fan.text != UNAVAILABLE else None
        self.c_cpu_temp.value = self.cpu_temp.value
        self.c_cpu_temp.color = self.cpu_temp.color
        self.c_gpu_temp.value = self.gpu_temp.value
        self.c_gpu_temp.color = self.gpu_temp.color
        power = telemetry.get("gpu_power_watts")
        self.c_gpu_power.value = None if power is None else f"{power} W"

        # keyboard
        brightness = state.get("keyboard_brightness")
        self.brightness.enabled = brightness is not None or state.get("daemon_available")
        if brightness is not None:
            self.brightness.value = brightness
        breathing = state.get("breathing")
        self.breathing.label = "Breathing: unknown" if breathing is None else \
            f"Breathing: {'on' if breathing else 'off'}"
        idle = state.get("idle_timeout_seconds")
        self.idle.selected = idle if idle in [value for _, value in IDLE_OPTIONS] else None

        # performance steppers mirror stored profiles
        for index in range(3):
            pair = profiles[index] if index < len(profiles) else [None, None]
            self.steppers[index * 2].value = pair[0] or 30
            self.steppers[index * 2 + 1].value = pair[1] or 60

        # battery
        battery = telemetry.get("battery")
        self.b_percent.value = None if not battery else f"{battery.get('percent')} %"
        self.b_status.value = None if not battery or not battery.get("status") \
            else battery["status"]
        self.b_power.value = self.c_gpu_power.value
        if battery and battery.get("percent") is not None:
            self.b_bar.fraction = battery["percent"] / 100
            self.b_bar.text = f"{battery['percent']} %"
        else:
            self.b_bar.fraction = None
            self.b_bar.text = UNAVAILABLE

        # settings
        self.s_frontend.value = state.get("frontend_version")
        self.s_daemon.value = state.get("daemon_version")
        self.s_reachable.value = "yes" if state.get("daemon_available") else "no"

    # ---------------------------------------------------------- card lists

    def cards(self, tab: str) -> list[Card]:
        if tab == "dashboard":
            return [
                Card("Power mode", [self.modes, self.apply_status, self.apply_button]),
                Card("System", [self.cpu_clock, self.cpu_temp, self.gpu_clock,
                                self.gpu_temp, self.cpu_use, self.gpu_use]),
                Card("CPU clock speed", [self.gauge, self.gauge_note]),
                Card("Fans", [self.cpu_fan, self.gpu_fan, self.fan_mode]),
                Card("Memory & storage", [self.mem, self.disk, self.vram, self.sources]),
                Card("Selected power target", [self.target_name, self.target_cpu,
                                               self.target_gpu, self.target_note,
                                               self.apply_result,
                                               self.apply_result_gpu]),
            ]
        if tab == "cooling":
            return [
                Card("Fan mode", [self.cooling_fan, self.cooling_note, self.cooling_status]),
                Card("Live readings", [self.c_cpu_fan, self.c_gpu_fan, self.c_cpu_temp,
                                       self.c_gpu_temp, self.c_gpu_power]),
            ]
        if tab == "keyboard":
            return [
                Card("Brightness", [self.brightness, self.brightness_note]),
                Card("Colour", [self.swatches, self.breathing]),
                Card("Idle behaviour", [self.idle, self.idle_note]),
            ]
        if tab == "performance":
            return [
                Card("Power profiles", [*self.steppers, self.save_button,
                                        self.profile_status]),
            ]
        if tab == "battery":
            return [Card("Battery", [self.b_percent, self.b_status, self.b_power,
                                     self.b_bar])]
        return [
            Card("About", [self.s_frontend, self.s_daemon, self.s_reachable,
                           self.s_busctl, self.s_how]),
        ]
