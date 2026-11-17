"""Palette and fonts for the desktop app."""

from __future__ import annotations

import pygame

BG = (7, 12, 16)
SURFACE = (12, 20, 26)
CARD = (16, 26, 33)
CARD_EDGE = (29, 43, 52)
ACCENT = (53, 224, 224)
ACCENT_DIM = (26, 143, 150)
TEXT = (220, 234, 240)
MUTED = (125, 147, 158)
COOL = (53, 224, 160)
WARM = (224, 178, 58)
HOT = (255, 107, 107)
BAR_BG = (22, 35, 43)
MISSING = (34, 48, 58)

UNAVAILABLE = "Unavailable"


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
