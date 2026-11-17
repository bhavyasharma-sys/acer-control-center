"""The desktop window: pygame over the shared Python backend.

Hardware calls and telemetry come from webgui.acercontrol (the same backend the
browser frontend uses), invoked in-process. Actions run on worker threads so
the window never freezes waiting on the daemon or on nvidia-smi.
"""

from __future__ import annotations

import argparse
import math
import os
import sys
import threading
import time

import pygame

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from webgui.acercontrol import hardware  # noqa: E402
from webgui.acercontrol import profiles as profiles_backend  # noqa: E402
from webgui.acercontrol import server as backend  # noqa: E402

from . import layout as flow  # noqa: E402
from .pages import Pages  # noqa: E402
from .theme import C, FONTS, apply as apply_theme  # noqa: E402
from .widgets import SliderRow, StepperRow  # noqa: E402

TABS = ["dashboard", "cooling", "keyboard", "performance", "battery", "settings"]
CARD_MIN = 300
GAP = 16


class App:
    def __init__(self, size=(1280, 800)) -> None:
        pygame.init()
        pygame.display.set_caption("Acer Control Center")
        self.size = size
        self.screen = pygame.display.set_mode(size, pygame.RESIZABLE)
        self.clock = pygame.time.Clock()
        self.pages = Pages()
        self.tab = "dashboard"
        self.scroll = {tab: 0 for tab in TABS}
        self.state: dict = {}
        self.telemetry: dict = {}
        self._lock = threading.Lock()
        self.notice: tuple[str, bool, float] | None = None
        self.dragging: SliderRow | None = None
        self.tab_rects: list[tuple[pygame.Rect, str]] = []
        self.card_hits: list = []
        self.running = True

        # First paint must not be a blank placeholder: read once synchronously,
        # then let the threads keep it fresh.
        try:
            state = backend.api_state()
            state["busctl"] = backend.api_about().get("busctl")
            self.state = state
            self.telemetry = backend.TELEMETRY.snapshot()
        except Exception:
            pass

        threading.Thread(target=self._poll_state, daemon=True).start()
        threading.Thread(target=self._poll_telemetry, daemon=True).start()

    # ------------------------------------------------------------ background

    def _poll_state(self) -> None:
        while True:
            try:
                state = backend.api_state()
                state["busctl"] = backend.api_about().get("busctl")
                with self._lock:
                    self.state = state
            except Exception as error:  # keep the window alive regardless
                with self._lock:
                    self.state = {"daemon_available": False, "daemon_error": str(error)}
            time.sleep(2.0)

    def _poll_telemetry(self) -> None:
        while True:
            try:
                snapshot = backend.TELEMETRY.snapshot()
                with self._lock:
                    self.telemetry = snapshot
            except Exception:
                pass
            time.sleep(1.0)

    def snapshot_pair(self):
        with self._lock:
            return dict(self.state), dict(self.telemetry)

    def later(self, fn, ok_message: str) -> None:
        def worker():
            try:
                ok, error = fn()
            except Exception as error:  # noqa: BLE001 - report, never crash
                ok = False
            self.set_notice(ok_message if ok else (error or "the request failed"), ok)

        threading.Thread(target=worker, daemon=True).start()

    def set_notice(self, text: str, good: bool) -> None:
        self.notice = (text, good, time.time())

    # --------------------------------------------------------------- actions

    def select_profile(self, index: int) -> None:
        current = self.state.get("profiles") or profiles_backend.DEFAULTS
        ok, error = profiles_backend.save(current, index)
        self.set_notice(error or "Selection stored. Press Apply to write the caps.", ok)

    def set_fan(self, mode: str) -> None:
        self.later(lambda: hardware.set_fan_max() if mode == "max"
                   else hardware.set_fan_auto(), f"Fan set to {mode}.")

    def set_color(self, rgb) -> None:
        self.later(lambda: hardware.set_keyboard_color(*rgb), "Keyboard colour set.")

    def toggle_breathing(self) -> None:
        turning_on = not self.state.get("breathing")
        self.later(lambda: hardware.start_breathing() if turning_on
                   else hardware.stop_animation(),
                   "Breathing on." if turning_on else "Animation stopped.")

    def set_idle(self, seconds: int) -> None:
        ok, error = backend.write_idle_timeout(seconds)
        self.set_notice(error or "Idle timeout stored.", ok)

    def apply_power(self) -> None:
        index = self.state.get("selected_profile", 2)
        self.later(lambda: _apply(index), "Power caps applied and verified by readback.")

    def save_profiles(self) -> None:
        gathered = []
        for index in range(3):
            gathered.append([self.pages.steppers[index * 2].value,
                             self.pages.steppers[index * 2 + 1].value])
        selected = self.state.get("selected_profile", 2)
        ok, error = profiles_backend.save(gathered, selected)
        self.set_notice(error or "Profiles saved.", ok)

    def push_brightness(self, value: int) -> None:
        self.later(lambda: hardware.set_keyboard_brightness(value),
                   f"Brightness set to {value} %.")

    # ----------------------------------------------------------------- input

    def on_click(self, pos) -> None:
        for rect, tab in self.tab_rects:
            if rect.collidepoint(pos):
                self.tab = tab
                return
        for rect, target in self.card_hits:
            if rect.collidepoint(pos):
                widget, value = target
                if isinstance(widget, SliderRow):
                    self.dragging = widget
                    widget.drag_to(pos[0])
                elif widget is self.pages.modes:
                    widget.choose(value)
                    self.select_profile(value)
                elif widget in (self.pages.fan_mode, self.pages.cooling_fan):
                    self.set_fan(value)
                elif widget is self.pages.idle:
                    widget.choose(value)
                    self.set_idle(value)
                elif widget is self.pages.swatches:
                    widget.choose(value)
                    self.set_color(value)
                elif widget is self.pages.breathing:
                    self.toggle_breathing()
                elif widget is self.pages.apply_button:
                    self.apply_power()
                elif widget is self.pages.save_button:
                    self.save_profiles()
                elif isinstance(widget, StepperRow):
                    widget.nudge(value)
                return

    def handle_events(self) -> None:
        for event in pygame.event.get():
            if event.type == pygame.QUIT:
                self.running = False
            elif event.type == pygame.KEYDOWN:
                if event.key == pygame.K_ESCAPE or event.key == pygame.K_q:
                    self.running = False
                elif pygame.K_1 <= event.key <= pygame.K_6:
                    self.tab = TABS[event.key - pygame.K_1]
            elif event.type == pygame.MOUSEWHEEL:
                self.scroll[self.tab] = max(0, self.scroll[self.tab] - event.y * 40)
            elif event.type == pygame.MOUSEBUTTONDOWN and event.button == 1:
                self.on_click(event.pos)
            elif event.type == pygame.MOUSEMOTION and self.dragging is not None:
                self.dragging.drag_to(event.pos[0])
            elif event.type == pygame.MOUSEBUTTONUP and self.dragging is not None:
                value = self.dragging.value
                self.dragging = None
                self.push_brightness(value)

    # ------------------------------------------------------------------ draw

    def draw(self) -> int:
        state, telemetry = self.snapshot_pair()
        self.pages.update(state, telemetry)
        width, height = self.screen.get_size()
        self.screen.fill(C().BG)

        # header
        pygame.draw.rect(self.screen, C().SURFACE, (0, 0, width, 54))
        pygame.draw.line(self.screen, C().CARD_EDGE, (0, 54), (width, 54), 1)
        pygame.draw.rect(self.screen, C().ACCENT, (20, 22, 12, 12), border_radius=3)
        brand = FONTS.get(16, True).render("ACER", True, C().ACCENT)
        self.screen.blit(brand, (42, 18))
        if C().CHAMFER:
            cx = width // 2
            plate = [(cx - 170, 0), (cx + 170, 0), (cx + 146, 48), (cx - 146, 48)]
            pygame.draw.polygon(self.screen, (9, 11, 12), plate)
            title = FONTS.get(17, True).render("Control Center", True, C().TEXT)
            self.screen.blit(title, title.get_rect(center=(cx, 24)))
        else:
            title = FONTS.get(17).render("Control Center", True, C().TEXT)
            self.screen.blit(title, (42 + brand.get_width() + 14, 17))
        reachable = state.get("daemon_available")
        pill_text = "daemon online" if reachable else "daemon offline"
        pill_font = FONTS.get(11, True)
        pill = pill_font.render(pill_text.upper(), True, C().COOL if reachable else C().HOT)
        pill_rect = pygame.Rect(width - pill.get_width() - 44, 16, pill.get_width() + 28, 24)
        pygame.draw.rect(self.screen,
                         C().PILL_GOOD_BG if reachable else C().PILL_BAD_BG,
                         pill_rect, border_radius=12)
        self.screen.blit(pill, (pill_rect.x + 14, pill_rect.y + 5))
        version = FONTS.get(11).render(f"v{state.get('frontend_version', '?')}", True, C().MUTED)
        self.screen.blit(version, (pill_rect.x - version.get_width() - 14, 21))

        # navigation: left icon rail (dark look) or top tab bar (light look)
        self.tab_rects = []
        left_margin = 20
        if C().nav == "notch":
            nav_w = 168
            for index, tab in enumerate(TABS):
                rect = pygame.Rect(18, 76 + index * 46, nav_w, 38)
                active = tab == self.tab
                cut = 12
                pts = [(rect.x + cut, rect.y), (rect.right, rect.y),
                       (rect.right, rect.bottom), (rect.x, rect.bottom)]
                pygame.draw.polygon(self.screen, C().ACCENT if active else C().CARD, pts)
                pygame.draw.polygon(self.screen,
                                    C().ACCENT if active else C().CARD_EDGE, pts, 1)
                label = FONTS.get(12, active).render(
                    tab.capitalize(), True, C().TEXT if active else C().MUTED)
                self.screen.blit(label, label.get_rect(midleft=(rect.x + cut + 14,
                                                                 rect.centery)))
                self.tab_rects.append((rect, tab))
            left_margin = 18 + nav_w + 22
        elif C().nav == "rail":
            rail_w = 86
            pygame.draw.rect(self.screen, C().SURFACE, (0, 54, rail_w, height - 54))
            pygame.draw.line(self.screen, C().CARD_EDGE, (rail_w, 54), (rail_w, height), 1)
            for index, tab in enumerate(TABS):
                rect = pygame.Rect(8, 68 + index * 66, rail_w - 16, 60)
                active = tab == self.tab
                if active:
                    pygame.draw.rect(self.screen, C().CARD, rect, border_radius=10)
                    pygame.draw.rect(self.screen, C().ACCENT,
                                     pygame.Rect(8, rect.y + 12, 3, rect.h - 24),
                                     border_radius=2)
                draw_icon(self.screen, tab, (rect.centerx, rect.y + 22),
                          C().ACCENT if active else C().MUTED)
                label = FONTS.get(9, True).render(
                    tab.capitalize(), True, C().ACCENT if active else C().MUTED)
                self.screen.blit(label, label.get_rect(center=(rect.centerx, rect.y + 46)))
                self.tab_rects.append((rect, tab))
            left_margin = rail_w + 20
        else:
            cursor = left_margin
            tab_font = FONTS.get(13, True)
            for tab in TABS:
                label = tab_font.render(tab.capitalize(), True,
                                        C().ACCENT if tab == self.tab else C().MUTED)
                rect = pygame.Rect(cursor, 62, label.get_width() + 30, 32)
                if tab == self.tab:
                    pygame.draw.rect(self.screen, C().CARD, rect, border_radius=9)
                    pygame.draw.rect(self.screen, C().CARD_EDGE, rect, width=1,
                                     border_radius=9)
                self.screen.blit(label, (rect.x + 15, rect.y + 8))
                self.tab_rects.append((rect, tab))
                cursor += rect.width + 6

        if not reachable:
            banner = FONTS.get(12).render(
                f"Hardware control unavailable: {state.get('daemon_error') or 'daemon not on the system bus'}. Telemetry still works.",
                True, C().BANNER_TEXT)
            pygame.draw.rect(self.screen, C().BANNER_BG,
                             (left_margin, 100,
                              min(width - left_margin - 20, banner.get_width() + 28), 28),
                             border_radius=8)
            self.screen.blit(banner, (left_margin + 14, 106))

        # content
        area_top = 140 if not reachable else 112
        area = pygame.Rect(left_margin, area_top, width - left_margin - 20,
                           height - area_top - 46)
        if C().CHAMFER:
            from .widgets import chamfer_points
            panel = area.inflate(24, 20)
            pts = chamfer_points(panel, 18)
            pygame.draw.polygon(self.screen, C().SURFACE, pts)
            pygame.draw.polygon(self.screen, C().ACCENT_DIM, pts, 1)
        cards = self.pages.cards(self.tab)
        # The stepper table reads best as one centred column of its own.
        if self.tab == "performance":
            grid_width = min(area.width, 640)
        else:
            grid_width = area.width
        grid_x = area.x + (area.width - grid_width) // 2
        columns = flow.column_count(grid_width, CARD_MIN, GAP)
        hint = flow.column_width(grid_width, columns, GAP)
        heights = [card.measure(hint) for card in cards]
        rects = flow.flow_grid(heights, grid_x, area.y - self.scroll[self.tab],
                               grid_width, CARD_MIN, GAP)
        content_bottom = max((y + h) for (_, y, _, h) in rects) if rects else area.y
        max_scroll = max(0, content_bottom - (area.y + area.height))
        self.scroll[self.tab] = min(self.scroll[self.tab], max_scroll)

        surface = self.screen
        self.card_hits = []
        clip = pygame.Rect(0, area.y, width, area.height)
        for card, rect in zip(cards, rects):
            card_rect = pygame.Rect(*rect)
            if card_rect.bottom < clip.top or card_rect.top > clip.bottom:
                continue
            card.draw(surface, card_rect)
            for hit_rect, target in card.hits():
                self.card_hits.append((hit_rect, target))
        if max_scroll > 0:
            track = pygame.Rect(width - 10, area.y, 4, area.height)
            pygame.draw.rect(surface, C().BAR_BG, track, border_radius=2)
            thumb = max(30, int(area.height * area.height / (content_bottom - area.y)))
            offset = int((track.h - thumb) * self.scroll[self.tab] / max_scroll)
            pygame.draw.rect(surface, C().ACCENT_DIM,
                             pygame.Rect(track.x, track.y + offset, track.w, thumb),
                             border_radius=2)

        # notice bar
        if self.notice:
            text, good, stamp = self.notice
            if time.time() - stamp < 5:
                font = FONTS.get(12, True)
                label = font.render(("✓ " if good else "⚠ ") + text, True,
                                    C().COOL if good else C().HOT)
                pygame.draw.rect(self.screen, C().SURFACE, (0, height - 40, width, 40))
                pygame.draw.line(self.screen, C().CARD_EDGE, (0, height - 40),
                                 (width, height - 40), 1)
                self.screen.blit(label, (20, height - 28))

        pygame.display.flip()
        return content_bottom

    def run(self) -> None:
        while self.running:
            self.handle_events()
            self.draw()
            self.clock.tick(30)
        pygame.quit()


def draw_icon(surface, tab: str, center, color) -> None:
    x, y = center
    if tab == "dashboard":
        pygame.draw.arc(surface, color, pygame.Rect(x - 9, y - 8, 18, 18), 0.0, math.pi, 2)
        pygame.draw.line(surface, color, (x, y + 1), (x + 5, y - 5), 2)
    elif tab == "cooling":
        pygame.draw.circle(surface, color, (x, y), 3)
        for angle in (0.4, 2.5, 4.6):
            pygame.draw.line(surface, color, (x, y),
                             (x + 9 * math.cos(angle), y + 9 * math.sin(angle)), 2)
    elif tab == "keyboard":
        pygame.draw.rect(surface, color, pygame.Rect(x - 9, y - 5, 18, 10), width=2,
                         border_radius=2)
        for dx in (-5, 0, 5):
            pygame.draw.line(surface, color, (x + dx, y - 1), (x + dx, y + 1), 1)
    elif tab == "performance":
        pygame.draw.polygon(surface, color, [
            (x + 2, y - 9), (x - 5, y + 1), (x - 1, y + 1),
            (x - 2, y + 9), (x + 5, y - 1), (x + 1, y - 1)])
    elif tab == "battery":
        pygame.draw.rect(surface, color, pygame.Rect(x - 8, y - 5, 14, 10), width=2,
                         border_radius=2)
        pygame.draw.rect(surface, color, pygame.Rect(x + 7, y - 2, 3, 4))
    else:
        pygame.draw.circle(surface, color, (x, y), 6, 2)
        for angle in range(6):
            a = angle * math.pi / 3
            pygame.draw.line(surface, color,
                             (x + 6 * math.cos(a), y + 6 * math.sin(a)),
                             (x + 9 * math.cos(a), y + 9 * math.sin(a)), 2)


def card_width_hint(width: int, card_min: int = CARD_MIN) -> int:
    columns = flow.column_count(width - 40, card_min, GAP)
    return flow.column_width(width - 40, columns, GAP)


def _apply(index: int):
    result = backend.apply_power(index)
    if result.get("ok"):
        return True, None
    return False, result.get("error") or "the daemon did not accept the caps"


def render_screenshots(outdir: str, size=(1280, 800), theme: str | None = None) -> list[str]:
    """Render one frame per tab and save PNGs, without entering the loop."""
    if theme:
        apply_theme(theme)
    os.makedirs(outdir, exist_ok=True)
    app = App(size)
    state, telemetry = app.snapshot_pair()
    app.pages.update(state, telemetry)
    paths = []
    for tab in TABS:
        app.tab = tab
        app.draw()
        path = os.path.join(outdir, f"{tab}.png")
        pygame.image.save(app.screen, path)
        paths.append(path)
    pygame.quit()
    return paths


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(prog="acer-control-center")
    parser.add_argument("--size", default="1280x800",
                        help="window size, WxH (default 1280x800)")
    parser.add_argument("--screenshot", metavar="DIR",
                        help="render every tab to DIR as PNG and exit")
    parser.add_argument("--theme", choices=("predator", "dark", "light"),
                        default="predator",
                        help="predator = PredatorSense replica (default); "
                             "dark = left icon rail; light = AcerSense-style tabs")
    options = parser.parse_args(argv)
    apply_theme(options.theme)
    width, _, height = options.size.partition("x")
    size = (int(width or 1280), int(height or 800))
    if options.screenshot:
        for path in render_screenshots(options.screenshot, size):
            print(path)
        return 0
    App(size).run()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
