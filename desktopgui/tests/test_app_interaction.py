"""Headless interaction tests: synthetic mouse and keyboard events through
the real event path, against a fake busctl. No display and no hardware needed.

Run from the repository root:
    SDL_VIDEODRIVER=dummy python3 -m unittest discover -s desktopgui/tests -v
"""

from __future__ import annotations

import os
import stat
import sys
import tempfile
import unittest

os.environ.setdefault("SDL_VIDEODRIVER", "dummy")
os.environ.setdefault("SDL_AUDIODRIVER", "dummy")

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))))

import pygame  # noqa: E402

from desktopgui.app import App  # noqa: E402
from webgui.acercontrol import profiles  # noqa: E402

FAKE_BUSCTL = """#!/bin/sh
case "$*" in
  *Ping*) echo 's "pong"' ;;
  *GetVersion*) echo 's "2.0.0"' ;;
  *GetKeyboardBrightness*) echo 'y 40' ;;
  *SetKeyboardBrightness*) exit 0 ;;
  *SetFanMax*) exit 0 ;;
  *SetPowerCaps*) echo 'ss "CPU RAPL setting read back: 40 W (x)" "NVIDIA software-requested limit read back: 75 W; y"' ;;
  *) exit 1 ;;
esac
"""


class InteractionTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        pygame.init()

    @classmethod
    def tearDownClass(cls):
        pygame.quit()

    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        binary = os.path.join(self._tmp.name, "busctl")
        with open(binary, "w") as handle:
            handle.write(FAKE_BUSCTL)
        os.chmod(binary, os.stat(binary).st_mode | stat.S_IEXEC)
        self._old_path = os.environ.get("PATH", "")
        os.environ["PATH"] = self._tmp.name + os.pathsep + self._old_path
        self._old_config = os.environ.get("XDG_CONFIG_HOME")
        os.environ["XDG_CONFIG_HOME"] = os.path.join(self._tmp.name, "config")
        self.app = App((1100, 760))
        self.app.draw()  # populate hit rectangles

    def tearDown(self):
        os.environ["PATH"] = self._old_path
        if self._old_config is None:
            os.environ.pop("XDG_CONFIG_HOME", None)
        else:
            os.environ["XDG_CONFIG_HOME"] = self._old_config
        self._tmp.cleanup()

    def hit_for(self, widget, value=None):
        for rect, target in self.app.card_hits:
            owned = target[0] is widget
            if owned and (value is None or target[1] == value):
                return rect
        return None

    def test_clicking_a_power_mode_stores_the_selection(self):
        rect = self.hit_for(self.app.pages.modes, 0)
        self.assertIsNotNone(rect, "mode pills should publish hit rects")
        self.app.on_click(rect.center)
        self.assertEqual(profiles.load()[1], 0)
        self.assertEqual(self.app.pages.modes.selected, 0)

    def show(self, tab):
        self.app.tab = tab
        self.app.draw()

    def test_stepper_nudges_within_limits(self):
        self.show("performance")
        stepper = self.app.pages.steppers[0]
        before = stepper.value
        plus = self.hit_for(stepper, 1)
        minus = self.hit_for(stepper, -1)
        self.assertIsNotNone(plus)
        self.assertIsNotNone(minus)
        self.app.on_click(plus.center)
        self.assertEqual(stepper.value, before + stepper.step)
        for _ in range(200):
            self.app.on_click(minus.center)
        self.assertEqual(stepper.value, stepper.low)

    def test_slider_drag_sets_brightness_value(self):
        self.show("keyboard")
        slider = self.app.pages.brightness
        rect = self.hit_for(slider, "drag")
        self.assertIsNotNone(rect)
        self.app.on_click(rect.center)
        self.assertIs(self.app.dragging, slider)
        track = slider._track
        self.app.dragging.drag_to(track.x + int(track.w * 0.7))
        self.assertEqual(slider.value, 70)
        pygame.event.post(pygame.event.Event(pygame.MOUSEBUTTONUP, button=1,
                                             pos=(track.x, track.y)))
        self.app.handle_events()
        self.assertIsNone(self.app.dragging)

    def test_keyboard_switches_tabs(self):
        pygame.event.post(pygame.event.Event(pygame.KEYDOWN, key=pygame.K_3))
        self.app.handle_events()
        self.assertEqual(self.app.tab, "keyboard")
        pygame.event.post(pygame.event.Event(pygame.KEYDOWN, key=pygame.K_ESCAPE))
        self.app.handle_events()
        self.assertFalse(self.app.running)

    def test_tab_bar_click_switches(self):
        rect = next(r for r, tab in self.app.tab_rects if tab == "cooling")
        self.app.on_click(rect.center)
        self.assertEqual(self.app.tab, "cooling")

    def test_layout_never_overlaps_or_clips(self):
        from desktopgui import layout as flow
        for tab in ("dashboard", "cooling", "keyboard", "performance", "battery", "settings"):
            self.app.tab = tab
            cards = self.app.pages.cards(tab)
            width = 1100
            hint = width - 40 - 2 * 16
            heights = [card.measure(hint - 40) for card in cards]
            rects = flow.flow_grid(heights, 20, 120, width - 40, 300, 16)
            for i, first in enumerate(rects):
                self.assertLessEqual(first[0] + first[2], width, f"{tab} card {i} clips right")
                for second in rects[i + 1:]:
                    self.assertFalse(flow.rects_overlap(first, second),
                                     f"{tab}: cards {i} overlap")


if __name__ == "__main__":
    unittest.main()
