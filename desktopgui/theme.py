"""Palette, fonts and the two selectable looks.

`C()` returns the active theme so widgets pick colours up at draw time and a
running app can switch looks without a restart.
"""

from __future__ import annotations

import pygame

UNAVAILABLE = "Unavailable"


class Theme:
    def __init__(self, name, nav, bg, surface, card, card_edge, accent, accent_dim,
                 text, muted, cool, warm, hot, bar_bg, missing, radius,
                 good, bad, banner_bg, banner_text,
                 gauge_track, gauge_needle, pill_good_bg, pill_bad_bg,
                 chamfer=False, seg_outline=False, gauge="semi") -> None:
        self.name = name
        self.nav = nav                      # 'rail' (left icons) or 'tabs' (top)
        self.BG = bg
        self.SURFACE = surface
        self.CARD = card
        self.CARD_EDGE = card_edge
        self.ACCENT = accent
        self.ACCENT_DIM = accent_dim
        self.TEXT = text
        self.MUTED = muted
        self.COOL = cool
        self.WARM = warm
        self.HOT = hot
        self.BAR_BG = bar_bg
        self.MISSING = missing
        self.RADIUS = radius
        self.GOOD = good
        self.BAD = bad
        self.BANNER_BG = banner_bg
        self.BANNER_TEXT = banner_text
        self.GAUGE_TRACK = gauge_track
        self.GAUGE_NEEDLE = gauge_needle
        self.PILL_GOOD_BG = pill_good_bg
        self.PILL_BAD_BG = pill_bad_bg
        self.CHAMFER = chamfer          # cut-corner panels instead of rounded ones
        self.SEG_OUTLINE = seg_outline  # selected segment = outlined tile, not filled pill
        self.GAUGE = gauge              # 'semi' dome or 'arc270' speedometer


DARK = Theme(
    name="dark",
    nav="rail",
    bg=(7, 12, 16),
    surface=(12, 20, 26),
    card=(16, 26, 33),
    card_edge=(29, 43, 52),
    accent=(53, 224, 224),
    accent_dim=(26, 143, 150),
    text=(220, 234, 240),
    muted=(125, 147, 158),
    cool=(53, 224, 160),
    warm=(224, 178, 58),
    hot=(255, 107, 107),
    bar_bg=(22, 35, 43),
    missing=(34, 48, 58),
    radius=14,
    good=(53, 224, 160),
    bad=(255, 107, 107),
    banner_bg=(36, 20, 20),
    banner_text=(255, 180, 180),
    gauge_track=(27, 40, 48),
    gauge_needle=(223, 251, 251),
    pill_good_bg=(14, 32, 25),
    pill_bad_bg=(32, 16, 16),
)

LIGHT = Theme(
    name="light",
    nav="tabs",
    bg=(233, 238, 235),
    surface=(244, 247, 245),
    card=(255, 255, 255),
    card_edge=(214, 224, 219),
    accent=(47, 158, 119),
    accent_dim=(35, 122, 92),
    text=(38, 52, 47),
    muted=(112, 128, 122),
    cool=(47, 158, 119),
    warm=(206, 152, 44),
    hot=(214, 74, 74),
    bar_bg=(228, 234, 231),
    missing=(206, 214, 210),
    radius=10,
    good=(47, 158, 119),
    bad=(214, 74, 74),
    banner_bg=(250, 235, 235),
    banner_text=(178, 74, 74),
    gauge_track=(222, 230, 226),
    gauge_needle=(38, 52, 47),
    pill_good_bg=(226, 240, 233),
    pill_bad_bg=(248, 230, 230),
)

PREDATOR = Theme(
    name="predator",
    nav="notch",
    bg=(17, 19, 21),
    surface=(25, 28, 30),
    card=(33, 37, 40),
    card_edge=(56, 62, 66),
    accent=(38, 197, 216),
    accent_dim=(24, 74, 82),
    text=(233, 238, 240),
    muted=(148, 156, 160),
    cool=(38, 197, 216),
    warm=(255, 196, 80),
    hot=(255, 92, 72),
    bar_bg=(42, 47, 51),
    missing=(60, 66, 70),
    radius=4,
    good=(96, 220, 152),
    bad=(255, 112, 100),
    banner_bg=(48, 26, 26),
    banner_text=(255, 172, 160),
    gauge_track=(58, 65, 70),
    gauge_needle=(150, 242, 255),
    pill_good_bg=(20, 40, 44),
    pill_bad_bg=(48, 26, 26),
    chamfer=True,
    seg_outline=True,
    gauge="arc270",
)

CURRENT = DARK


def apply(name: str) -> Theme:
    global CURRENT
    CURRENT = {"light": LIGHT, "predator": PREDATOR, "dark": DARK}[name]
    return CURRENT


def C() -> Theme:
    return CURRENT


class Fonts:
    """Lazily created pygame fonts with a graceful fallback chain."""

    def __init__(self) -> None:
        self._cache: dict[tuple[str, int], pygame.font.Font] = {}

    def _make(self, kind: str, size: int) -> pygame.font.Font:
        key = (kind, size)
        if key in self._cache:
            return self._cache[key]
        font = None
        if kind == "bold":
            for name in ("dejavusans:bold", "notosans:bold", "arial:bold"):
                try:
                    font = pygame.font.SysFont(name, size, bold=True)
                    break
                except Exception:
                    continue
        if font is None:
            for name in ("dejavusans", "notosans", "segoeui", "arial"):
                try:
                    font = pygame.font.SysFont(name, size)
                    break
                except Exception:
                    continue
        if font is None:
            font = pygame.font.Font(pygame.font.get_default_font(), size)
        self._cache[key] = font
        return font

    def get(self, size: int, bold: bool = False) -> pygame.font.Font:
        return self._make("bold" if bold else "regular", size)


FONTS = Fonts()
