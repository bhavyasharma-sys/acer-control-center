"""The HTTP server: static frontend plus a small JSON API.

Standard library only. Every hardware action goes through `hardware.call`, so
a machine without the daemon still serves the UI and reports "unavailable"
instead of failing.
"""

from __future__ import annotations

import json
import os
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlparse

from . import __version__, hardware, profiles, telemetry

STATIC_DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), "static")

TELEMETRY = telemetry.Telemetry()
STATE_LOCK = threading.Lock()
LAST_APPLY: dict = {"result": None}


def read_idle_timeout() -> int | None:
    home = os.environ.get("XDG_CONFIG_HOME") or os.path.join(
        os.path.expanduser("~"), ".config"
    )
    path = os.path.join(home, "acer-control-center", "keyboard-idle-timeout")
    try:
        with open(path, "r", encoding="utf-8") as handle:
            return int(handle.read().strip())
    except (OSError, ValueError):
        return None


def write_idle_timeout(seconds: int) -> tuple[bool, str | None]:
    if not 0 <= seconds <= 3600:
        return False, "idle timeout must be between 0 and 3600 seconds"
    home = os.environ.get("XDG_CONFIG_HOME") or os.path.join(
        os.path.expanduser("~"), ".config"
    )
    directory = os.path.join(home, "acer-control-center")
    try:
        os.makedirs(directory, exist_ok=True)
        with open(
            os.path.join(directory, "keyboard-idle-timeout"), "w", encoding="utf-8"
        ) as handle:
            handle.write(f"{seconds}\n")
    except OSError as error:
        return False, str(error)
    return True, None


def api_state() -> dict:
    reachable, error = hardware.available()
    stored_profiles, selected = profiles.load()
    return {
        "daemon_available": reachable,
        "daemon_error": error,
        "daemon_version": hardware.version() if reachable else None,
        "frontend_version": __version__,
        "fan_max": hardware.fan_max_selected() if reachable else None,
        "breathing": hardware.breathing_selected() if reachable else None,
        "keyboard_brightness": hardware.keyboard_brightness() if reachable else None,
        "keyboard_color": list(hardware.keyboard_color() or []) if reachable else None,
        "profiles": stored_profiles,
        "selected_profile": selected,
        "profile_names": profiles.NAMES,
        "idle_timeout_seconds": read_idle_timeout(),
        "last_apply": LAST_APPLY["result"],
    }


def api_about() -> dict:
    reachable, _ = hardware.available()
    return {
        "frontend_version": __version__,
        "daemon_version": hardware.version() if reachable else None,
        "daemon_available": reachable,
        "busctl": hardware.busctl_path() is not None,
    }


def apply_power(index: int) -> dict:
    stored_profiles, _ = profiles.load()
    if not 0 <= index <= 2:
        return {"ok": False, "error": "profile index must be 0, 1 or 2"}
    cpu, gpu = stored_profiles[index]
    result = hardware.set_power_caps(cpu, gpu)
    if result is None:
        payload = {
            "ok": False,
            "error": "the daemon did not accept the power caps; is it running?",
        }
    else:
        cpu_message, gpu_message = result
        ok = ("read back" in cpu_message and "not applied" not in cpu_message) and (
            "read back" in gpu_message and "not applied" not in gpu_message
        )
        payload = {
            "ok": ok,
            "cpu_message": cpu_message,
            "gpu_message": gpu_message,
            "index": index,
            "watts": [cpu, gpu],
        }
    with STATE_LOCK:
        LAST_APPLY["result"] = payload
    return payload


class Handler(BaseHTTPRequestHandler):
    server_version = f"AcerControlCenter/{__version__}"
    protocol_version = "HTTP/1.1"

    # ------------------------------------------------------------ plumbing

    def log_message(self, fmt, *args):  # keep the console readable
        pass

    def _send(self, status: int, body: bytes, content_type: str) -> None:
        self.send_response(status)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(body)

    def _json(self, payload, status: int = 200) -> None:
        self._send(
            status,
            json.dumps(payload).encode("utf-8"),
            "application/json; charset=utf-8",
        )

    def _read_json(self) -> dict | None:
        length = int(self.headers.get("Content-Length") or 0)
        if length <= 0 or length > 1_000_000:
            return None
        try:
            parsed = json.loads(self.rfile.read(length).decode("utf-8"))
        except (ValueError, UnicodeDecodeError):
            return None
        return parsed if isinstance(parsed, dict) else None

    # ------------------------------------------------------------ requests

    def do_GET(self) -> None:  # noqa: N802 (stdlib naming)
        path = urlparse(self.path).path
        if path == "/api/telemetry":
            self._json(TELEMETRY.snapshot())
        elif path == "/api/state":
            self._json(api_state())
        elif path == "/api/about":
            self._json(api_about())
        elif path in ("/", "/index.html"):
            self._serve_static("index.html", "text/html; charset=utf-8")
        elif path.startswith("/static/"):
            self._serve_static(path[len("/static/"):], self._guess(path))
        else:
            self._json({"error": "not found"}, 404)

    def do_POST(self) -> None:  # noqa: N802
        path = urlparse(self.path).path
        body = self._read_json()
        if body is None:
            self._json({"error": "expected a JSON object"}, 400)
            return
        if path == "/api/fan":
            mode = body.get("mode")
            if mode == "max":
                ok, error = hardware.set_fan_max()
            elif mode == "auto":
                ok, error = hardware.set_fan_auto()
            else:
                self._json({"error": "mode must be 'max' or 'auto'"}, 400)
                return
            self._json({"ok": ok, "error": error})
        elif path == "/api/keyboard/brightness":
            value = body.get("value")
            if not isinstance(value, int) or not 0 <= value <= 100:
                self._json({"error": "value must be 0..100"}, 400)
                return
            ok, error = hardware.set_keyboard_brightness(value)
            self._json({"ok": ok, "error": error})
        elif path == "/api/keyboard/color":
            try:
                rgb = [int(body[key]) for key in ("r", "g", "b")]
            except (KeyError, TypeError, ValueError):
                self._json({"error": "expected integer r, g and b"}, 400)
                return
            if any(not 0 <= channel <= 255 for channel in rgb):
                self._json({"error": "channels must be 0..255"}, 400)
                return
            ok, error = hardware.set_keyboard_color(*rgb)
            self._json({"ok": ok, "error": error})
        elif path == "/api/keyboard/animation":
            name = body.get("name")
            if name == "breathing":
                ok, error = hardware.start_breathing()
            elif name == "off":
                ok, error = hardware.stop_animation()
            else:
                self._json({"error": "name must be 'breathing' or 'off'"}, 400)
                return
            self._json({"ok": ok, "error": error})
        elif path == "/api/keyboard/idle":
            seconds = body.get("seconds")
            if not isinstance(seconds, int):
                self._json({"error": "seconds must be an integer"}, 400)
                return
            ok, error = write_idle_timeout(seconds)
            self._json({"ok": ok, "error": error})
        elif path == "/api/profiles":
            stored = body.get("profiles")
            selected = body.get("selected")
            if not isinstance(stored, list) or not isinstance(selected, int):
                self._json({"error": "expected profiles list and selected index"}, 400)
                return
            ok, error = profiles.save(stored, selected)
            self._json({"ok": ok, "error": error})
        elif path == "/api/power/apply":
            index = body.get("index")
            if not isinstance(index, int):
                self._json({"error": "index must be an integer"}, 400)
                return
            self._json(apply_power(index))
        else:
            self._json({"error": "not found"}, 404)

    # -------------------------------------------------------------- static

    def _guess(self, path: str) -> str:
        if path.endswith(".css"):
            return "text/css; charset=utf-8"
        if path.endswith(".js"):
            return "application/javascript; charset=utf-8"
        if path.endswith(".svg"):
            return "image/svg+xml"
        return "application/octet-stream"

    def _serve_static(self, name: str, content_type: str) -> None:
        safe = os.path.basename(name)
        full = os.path.join(STATIC_DIR, safe)
        try:
            with open(full, "rb") as handle:
                self._send(200, handle.read(), content_type)
        except OSError:
            self._json({"error": "not found"}, 404)


def serve(host: str = "127.0.0.1", port: int = 8765) -> ThreadingHTTPServer:
    httpd = ThreadingHTTPServer((host, port), Handler)
    httpd.daemon_threads = True
    return httpd
