"""Headless tests for the browser frontend's backend.

Run from the repository root:
    python3 -m unittest discover -s webgui/tests -v
A fake `busctl` on PATH exercises the whole D-Bus path without hardware.
"""

from __future__ import annotations

import json
import os
import stat
import sys
import tempfile
import threading
import unittest
import urllib.error
import urllib.request

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))))

from webgui.acercontrol import hardware, profiles, server, telemetry  # noqa: E402

FAKE_BUSCTL = """#!/bin/sh
case "$*" in
  *Ping*) echo 's "pong"' ;;
  *GetVersion*) echo 's "2.0.0"' ;;
  *GetKeyboardBrightness*) echo 'y 60' ;;
  *GetKeyboardColor*) echo 'yyy 0 229 255' ;;
  *GetFanMaxSelected*) echo 'b false' ;;
  *SetPowerCaps*) echo 'ss "CPU RAPL setting read back: 40 W (actual workload enforcement is not measured)" "NVIDIA software-requested limit read back: 75 W; detail"' ;;
  *SetFanMax*) exit 0 ;;
  *) exit 1 ;;
esac
"""


class FakeBusctlMixin:
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        binary = os.path.join(self._tmp.name, "busctl")
        with open(binary, "w", encoding="utf-8") as handle:
            handle.write(FAKE_BUSCTL)
        os.chmod(binary, os.stat(binary).st_mode | stat.S_IEXEC | stat.S_IXGRP)
        self._old_path = os.environ.get("PATH", "")
        os.environ["PATH"] = self._tmp.name + os.pathsep + self._old_path
        self._old_config = os.environ.get("XDG_CONFIG_HOME")
        os.environ["XDG_CONFIG_HOME"] = os.path.join(self._tmp.name, "config")

    def tearDown(self) -> None:
        os.environ["PATH"] = self._old_path
        if self._old_config is None:
            os.environ.pop("XDG_CONFIG_HOME", None)
        else:
            os.environ["XDG_CONFIG_HOME"] = self._old_config
        self._tmp.cleanup()


class TelemetryParsers(unittest.TestCase):
    def test_cpu_sample(self):
        text = "cpu  100 0 50 800 20 0 0 0 0 0\ncpu0 1 0 0 0 0 0 0 0 0 0\n"
        self.assertEqual(telemetry.parse_cpu_sample(text), (970, 820))
        self.assertIsNone(telemetry.parse_cpu_sample(""))
        self.assertIsNone(telemetry.parse_cpu_sample("mem 1 2\n"))
        self.assertIsNone(telemetry.parse_cpu_sample("cpu  1 2\n"))

    def test_cpu_usage(self):
        previous = (1000, 900)
        current = (1100, 950)
        self.assertEqual(telemetry.cpu_usage_percent(previous, current), 50)
        self.assertIsNone(telemetry.cpu_usage_percent(None, current))
        self.assertIsNone(telemetry.cpu_usage_percent(previous, previous))
        # idle grew more than total: saturating backwards delta is not a reading
        self.assertIsNone(telemetry.cpu_usage_percent(previous, (1100, 1200)))

    def test_meminfo(self):
        text = "MemTotal:       16000000 kB\nMemFree:   2000000 kB\nMemAvailable:    8000000 kB\n"
        used, total = telemetry.parse_meminfo(text)
        self.assertEqual(total, 16000000 // 1024)
        self.assertEqual(used, (16000000 - 8000000) // 1024)
        self.assertIsNone(telemetry.parse_meminfo(None))
        self.assertIsNone(telemetry.parse_meminfo("MemTotal: oops kB\n"))

    def test_nvidia_line(self):
        line = "47, 686, 3, 70, 6144, 6.51"
        parsed = telemetry.parse_nvidia_line(line)
        self.assertEqual(parsed["temperature_c"], 47)
        self.assertEqual(parsed["clock_mhz"], 686)
        self.assertEqual(parsed["usage_percent"], 3)
        self.assertEqual(parsed["memory_mib"], [70, 6144])
        self.assertEqual(parsed["power_watts"], 6.5)
        self.assertIsNone(telemetry.parse_nvidia_line("47, 686"))
        missing = telemetry.parse_nvidia_line("N/A, N/A, N/A, N/A, N/A, N/A")
        self.assertIsNone(missing["temperature_c"])
        self.assertIsNone(missing["power_watts"])

    def test_snapshot_never_fakes_zero(self):
        snapshot = telemetry.Telemetry().snapshot()
        for key in ("cpu_temperature_c", "cpu_fan_rpm", "memory_mib"):
            self.assertIn(key, snapshot)


class BusctlContract(FakeBusctlMixin, unittest.TestCase):
    def test_command_shape(self):
        command = hardware.build_command("SetPowerCaps", "uu", (40, 75))
        self.assertEqual(command[:4], ["busctl", "--system", "call", hardware.SERVICE])
        self.assertEqual(command[4:7], [hardware.PATH, hardware.INTERFACE, "SetPowerCaps"])
        self.assertEqual(command[-3:], ["uu", "40", "75"])

    def test_parse_outputs(self):
        self.assertEqual(hardware.parse_busctl_output('y 60'), ["60"])
        self.assertEqual(hardware.parse_busctl_output('b true'), ["true"])
        self.assertEqual(
            hardware.parse_busctl_output('ss "CPU ok" "GPU ok"'), ["CPU ok", "GPU ok"]
        )
        self.assertEqual(hardware.parse_busctl_output(""), [])

    def test_typed_calls_against_fake_busctl(self):
        self.assertEqual(hardware.available(), (True, None))
        self.assertEqual(hardware.version(), "2.0.0")
        self.assertEqual(hardware.keyboard_brightness(), 60)
        self.assertEqual(hardware.keyboard_color(), (0, 229, 255))
        self.assertEqual(hardware.fan_max_selected(), False)
        cpu, gpu = hardware.set_power_caps(40, 75)
        self.assertIn("read back", cpu)
        self.assertIn("read back", gpu)
        self.assertEqual(hardware.set_fan_max(), (True, None))

    def test_missing_busctl_reports_unavailable(self):
        os.environ["PATH"] = "/nonexistent"
        self.assertIsNone(hardware.busctl_path())
        self.assertEqual(hardware.available(), (False,
                                                "busctl is not installed; cannot reach the daemon"))


class Profiles(unittest.TestCase):
    def test_parse(self):
        self.assertEqual(profiles.parse("40 75 30 60 15 30 2\n"),
                         ([[40, 75], [30, 60], [15, 30]], 2))
        self.assertIsNone(profiles.parse("40 75 30 60 15 30\n"))
        self.assertIsNone(profiles.parse("0 75 30 60 15 30 2\n"))
        self.assertIsNone(profiles.parse("40 75 30 60 15 30 9\n"))
        self.assertIsNone(profiles.parse("nope\n"))

    def test_save_load_roundtrip(self):
        with tempfile.TemporaryDirectory() as tmp:
            os.environ["XDG_CONFIG_HOME"] = tmp
            ok, error = profiles.save([[45, 80], [30, 60], [15, 30]], 0)
            self.assertTrue(ok, error)
            self.assertEqual(profiles.load(), ([[45, 80], [30, 60], [15, 30]], 0))
            self.assertFalse(profiles.save([[900, 80], [30, 60], [15, 30]], 0)[0])
            os.environ.pop("XDG_CONFIG_HOME", None)

    def test_load_falls_back_to_defaults(self):
        with tempfile.TemporaryDirectory() as tmp:
            os.environ["XDG_CONFIG_HOME"] = tmp
            self.assertEqual(profiles.load(), (profiles.DEFAULTS, 2))
            os.environ.pop("XDG_CONFIG_HOME", None)


class ServerApi(FakeBusctlMixin, unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.httpd = server.serve("127.0.0.1", 0)
        cls.port = cls.httpd.server_address[1]
        cls.thread = threading.Thread(target=cls.httpd.serve_forever, daemon=True)
        cls.thread.start()

    @classmethod
    def tearDownClass(cls):
        cls.httpd.shutdown()
        cls.httpd.server_close()

    def get(self, path):
        try:
            with urllib.request.urlopen(
                f"http://127.0.0.1:{self.port}{path}", timeout=10
            ) as response:
                return response.status, response.read()
        except urllib.error.HTTPError as error:  # 4xx/5xx are valid answers here
            return error.code, error.read()

    def post(self, path, payload):
        request = urllib.request.Request(
            f"http://127.0.0.1:{self.port}{path}",
            data=json.dumps(payload).encode("utf-8"),
            headers={"Content-Type": "application/json"},
            method="POST",
        )
        try:
            with urllib.request.urlopen(request, timeout=10) as response:
                return response.status, json.loads(response.read().decode("utf-8"))
        except urllib.error.HTTPError as error:
            return error.code, json.loads(error.read().decode("utf-8"))

    def test_index_and_static(self):
        status, body = self.get("/")
        self.assertEqual(status, 200)
        self.assertIn(b"<title>Acer Control Center</title>", body)
        status, css = self.get("/static/style.css")
        self.assertEqual(status, 200)
        self.assertIn(b".cards", css)
        status, js = self.get("/static/app.js")
        self.assertEqual(status, 200)
        self.assertIn(b"renderTelemetry", js)

    def test_telemetry_endpoint(self):
        status, body = self.get("/api/telemetry")
        self.assertEqual(status, 200)
        payload = json.loads(body)
        self.assertIn("cpu_temperature_c", payload)
        self.assertIn("memory_mib", payload)

    def test_state_reports_daemon_via_fake_busctl(self):
        status, body = self.get("/api/state")
        self.assertEqual(status, 200)
        payload = json.loads(body)
        self.assertTrue(payload["daemon_available"])
        self.assertEqual(payload["daemon_version"], "2.0.0")
        self.assertEqual(payload["keyboard_brightness"], 60)

    def test_fan_and_power_actions(self):
        status, payload = self.post("/api/fan", {"mode": "max"})
        self.assertEqual(status, 200)
        self.assertTrue(payload["ok"])
        status, payload = self.post("/api/fan", {"mode": "sideways"})
        self.assertEqual(status, 400)
        status, payload = self.post("/api/power/apply", {"index": 2})
        self.assertEqual(status, 200)
        self.assertTrue(payload["ok"], payload)
        self.assertIn("read back", payload["cpu_message"])

    def test_profiles_round_trip_through_api(self):
        status, payload = self.post("/api/profiles",
                                    {"profiles": [[41, 76], [30, 60], [15, 30]], "selected": 1})
        self.assertEqual(status, 200)
        self.assertTrue(payload["ok"], payload)
        status, body = self.get("/api/state")
        state = json.loads(body)
        self.assertEqual(state["profiles"], [[41, 76], [30, 60], [15, 30]])
        self.assertEqual(state["selected_profile"], 1)

    def test_validation(self):
        self.assertEqual(self.post("/api/keyboard/brightness", {"value": 400})[0], 400)
        self.assertEqual(self.post("/api/keyboard/color", {"r": 1, "g": 2})[0], 400)
        self.assertEqual(self.post("/api/power/apply", {"index": "x"})[0], 400)
        self.assertEqual(self.get("/api/nope")[0], 404)


if __name__ == "__main__":
    unittest.main()
