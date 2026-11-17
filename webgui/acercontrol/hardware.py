"""Talk to acer-control-daemon over the system bus, using `busctl`.

`busctl` ships with systemd, so this needs no Python D-Bus library and no
compiled bindings: hardware control stays in the Rust daemon, and this module
is a thin, testable command builder and output parser. Every function returns
None (or a (False, message) pair) when the daemon is unreachable, so the UI can
show a banner instead of crashing.
"""

from __future__ import annotations

import shlex
import shutil
import subprocess

SERVICE = "io.github.stanlysilas.AcerControl"
PATH = "/io/github/stanlysilas/AcerControl"
INTERFACE = "io.github.stanlysilas.AcerControl"


def busctl_path() -> str | None:
    return shutil.which("busctl")


def build_command(method: str, signature: str | None = None,
                  args: tuple | list | None = None) -> list[str]:
    """The exact argv for one method call, so tests can assert on it."""
    command = ["busctl", "--system", "call", SERVICE, PATH, INTERFACE, method]
    if signature:
        command.append(signature)
        command.extend(str(arg) for arg in (args or ()))
    return command


def parse_busctl_output(text: str) -> list[str]:
    """Parse `busctl` reply values: 'y 42', 'b true', 'ss "a" "b"'.

    The first token is the signature; the rest are the values. Quoted strings
    keep their spaces and understand \\ and \\" escapes.
    """
    tokens = shlex.split(text, posix=True)
    if not tokens:
        return []
    return tokens[1:]


def call(method: str, signature: str | None = None,
         args: tuple | list | None = None,
         timeout: float = 5.0) -> tuple[bool, list[str] | str]:
    """Run one method call.

    Returns (True, values) on success, (False, error message) otherwise.
    """
    binary = busctl_path()
    if binary is None:
        return False, "busctl is not installed; cannot reach the daemon"
    command = build_command(method, signature, args)
    command[0] = binary
    try:
        completed = subprocess.run(
            command, capture_output=True, text=True, timeout=timeout, check=False
        )
    except (OSError, subprocess.SubprocessError) as error:
        return False, f"failed to run busctl: {error}"
    if completed.returncode != 0:
        detail = (completed.stderr or completed.stdout or "").strip()
        return False, detail or f"busctl exited with {completed.returncode}"
    return True, parse_busctl_output(completed.stdout)


# ------------------------------------------------------------------- typed

def _single(method: str, signature: str | None = None, args=None):
    ok, result = call(method, signature, args)
    if not ok:
        return None, result
    values = result or []
    return (values[0] if values else None), None


def available() -> tuple[bool, str | None]:
    ok, result = call("Ping")
    if not ok:
        return False, result if isinstance(result, str) else "daemon unreachable"
    return True, None


def version() -> str | None:
    value, _ = _single("GetVersion")
    return value


def keyboard_brightness() -> int | None:
    value, _ = _single("GetKeyboardBrightness")
    try:
        return int(value) if value is not None else None
    except ValueError:
        return None


def set_keyboard_brightness(value: int) -> tuple[bool, str | None]:
    ok, result = call("SetKeyboardBrightness", "y", (value,))
    return ok, (result if isinstance(result, str) else None)


def set_keyboard_brightness_transient(value: int) -> tuple[bool, str | None]:
    ok, result = call("SetKeyboardBrightnessTransient", "y", (value,))
    return ok, (result if isinstance(result, str) else None)


def keyboard_color() -> tuple[int, int, int] | None:
    ok, result = call("GetKeyboardColor")
    if not ok or not result or len(result) < 3:
        return None
    try:
        return int(result[0]), int(result[1]), int(result[2])
    except ValueError:
        return None


def set_keyboard_color(r: int, g: int, b: int) -> tuple[bool, str | None]:
    ok, result = call("SetKeyboardColor", "yyy", (r, g, b))
    return ok, (result if isinstance(result, str) else None)


def start_breathing() -> tuple[bool, str | None]:
    ok, result = call("StartBreathing")
    return ok, (result if isinstance(result, str) else None)


def stop_animation() -> tuple[bool, str | None]:
    ok, result = call("StopAnimation")
    return ok, (result if isinstance(result, str) else None)


def set_fan_max() -> tuple[bool, str | None]:
    ok, result = call("SetFanMax")
    return ok, (result if isinstance(result, str) else None)


def set_fan_auto() -> tuple[bool, str | None]:
    ok, result = call("SetFanAuto")
    return ok, (result if isinstance(result, str) else None)


def fan_max_selected() -> bool | None:
    value, _ = _single("GetFanMaxSelected")
    if value is None:
        return None
    return value.lower() in ("true", "1")


def breathing_selected() -> bool | None:
    value, _ = _single("GetBreathingSelected")
    if value is None:
        return None
    return value.lower() in ("true", "1")


def set_power_caps(cpu_watts: int, gpu_watts: int) -> tuple[str, str] | None:
    ok, result = call("SetPowerCaps", "uu", (cpu_watts, gpu_watts))
    if not ok or not result or len(result) < 2:
        return None
    return result[0], result[1]
