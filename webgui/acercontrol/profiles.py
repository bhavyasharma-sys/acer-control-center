"""Power profiles, stored in the same file the desktop app uses.

Format is seven whitespace-separated integers on one line:
    p0min p0max p1min p1max p2min p2max selected
Caps are validated to 1..=500 W and the selected index to 0..=2, exactly like
the Rust implementation, so both frontends agree on what a valid file is.
"""

from __future__ import annotations

import os
import tempfile

DEFAULTS: list[list[int]] = [[40, 75], [30, 60], [15, 30]]
NAMES = ["Performance", "Balanced", "Battery Saver"]
MIN_WATTS = 1
MAX_WATTS = 500


def config_path() -> str:
    home = os.environ.get("XDG_CONFIG_HOME") or os.path.join(
        os.path.expanduser("~"), ".config"
    )
    return os.path.join(home, "acer-control-center", "power-profiles")


def parse(text: str) -> tuple[list[list[int]], int] | None:
    fields = text.split()
    if len(fields) != 7:
        return None
    try:
        values = [int(field) for field in fields]
    except ValueError:
        return None
    caps, selected = values[:6], values[6]
    if not all(MIN_WATTS <= cap <= MAX_WATTS for cap in caps):
        return None
    if not 0 <= selected <= 2:
        return None
    profiles = [caps[0:2], caps[2:4], caps[4:6]]
    return profiles, selected


def load() -> tuple[list[list[int]], int]:
    try:
        with open(config_path(), "r", encoding="utf-8") as handle:
            parsed = parse(handle.read())
    except OSError:
        parsed = None
    if parsed is None:
        return [list(profile) for profile in DEFAULTS], 2
    return parsed


def save(profiles: list[list[int]], selected: int) -> tuple[bool, str | None]:
    if len(profiles) != 3 or any(len(pair) != 2 for pair in profiles):
        return False, "expected three profiles of two wattages each"
    if not 0 <= selected <= 2:
        return False, "selected profile must be 0, 1 or 2"
    for pair in profiles:
        for watts in pair:
            if not isinstance(watts, int) or not MIN_WATTS <= watts <= MAX_WATTS:
                return False, f"wattage {watts} is outside {MIN_WATTS}..{MAX_WATTS} W"
    line = " ".join(str(watts) for pair in profiles for watts in pair)
    line += f" {selected}\n"
    path = config_path()
    directory = os.path.dirname(path)
    try:
        os.makedirs(directory, exist_ok=True)
        descriptor, temporary = tempfile.mkstemp(dir=directory, prefix=".profiles-")
        with os.fdopen(descriptor, "w", encoding="utf-8") as handle:
            handle.write(line)
        os.replace(temporary, path)
    except OSError as error:
        return False, str(error)
    return True, None
