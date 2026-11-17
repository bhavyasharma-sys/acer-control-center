"""Small retained-mode widget kit on top of pygame.

Every block measures its own height for a given width and then draws inside
the rectangle the card hands it, so nothing is ever painted on top of
anything else. Interactive blocks publish their hit rectangles during draw
and the app routes mouse events back to them by tag.
"""

from __future__ import annotations

import math

import pygame

from .theme import (
    ACCENT,
    BAR_BG,
    CARD,
    CARD_EDGE,
    COOL,
    FONTS,
    MISSING,
    MUTED,
    TEXT,
    UNAVAILABLE,
    WARM,
)

SPACING = 10


def wrap_text(font: pygame.font.Font, text: str, max_width: int) -> list[str]:
    if font.size(text)[0] <= max_width:
        return [text]
    lines: list[str] = []
    current = ""
    for word in text.split():
        candidate = f"{current} {word}".strip()
        if font.size(candidate)[0] <= max_width or not current:
            current = candidate
        else:
            lines.append(current)
            current = word
    if current:
        lines.append(current)
    return lines or [""]


def fit_text(font: pygame.font.Font, text: str, max_width: int) -> str:
    if font.size(text)[0] <= max_width:
        return text
    while len(text) > 2 and font.size(text + "…")[0] > max_width:
        text = text[:-1]
    return text + "…"


class Block:
    height = 0

    def measure(self, width: int) -> int:
        raise NotImplementedError

    def draw(self, surface, x: int, y: int, width: int) -> int:
        raise NotImplementedError

    def hits(self) -> list:
        return []


class Space(Block):
    def __init__(self, height: int) -> None:
        self.height = height

    def measure(self, width: int) -> int:
        return self.height

    def draw(self, surface, x, y, width) -> int:
        return self.height


class TextBlock(Block):
    def __init__(self, text: str, size: int = 13, color=MUTED, bold: bool = False,
                 center: bool = False) -> None:
        self.text = text
        self.size = size
        self.color = color
        self.bold = bold
        self.center = center
        self._lines: list[str] = []

    def measure(self, width: int) -> int:
        font = FONTS.get(self.size, self.bold)
        self._lines = wrap_text(font, self.text, width)
        return len(self._lines) * (self.size + 6)

    def draw(self, surface, x, y, width) -> int:
        font = FONTS.get(self.size, self.bold)
        if not self._lines:
            self.measure(width)
        for offset, line in enumerate(self._lines):
            surf = font.render(line, True, self.color)
            dx = x + (width - surf.get_width()) // 2 if self.center else x
            surface.blit(surf, (dx, y + offset * (self.size + 6)))
        return len(self._lines) * (self.size + 6)


class StatRow(Block):
    """Label on the left, value right-aligned. They cannot collide: the value
    is truncated to the space left of the label."""

    def __init__(self, label: str) -> None:
        self.label = label
        self.value = None
        self.color = TEXT

    def measure(self, width: int) -> int:
        return 26

    def draw(self, surface, x, y, width) -> int:
        label_font = FONTS.get(13)
        value_font = FONTS.get(14, True)
        text = UNAVAILABLE if self.value is None else str(self.value)
        color = MUTED if self.value is None else self.color
        label = label_font.render(self.label, True, MUTED)
        surface.blit(label, (x, y + 4))
        room = width - label.get_width() - 16
        surface.blit(
            value_font.render(fit_text(value_font, text, max(24, room)), True, color),
            (x + width - value_font.size(fit_text(value_font, text, max(24, room)))[0],
             y + 3),
        )
        pygame.draw.line(surface, CARD_EDGE, (x, y + 25), (x + width, y + 25), 1)
        return 26


class BarRow(Block):
    def __init__(self, label: str, color=ACCENT) -> None:
        self.label = label
        self.color = color
        self.fraction = None
        self.text = UNAVAILABLE

    def measure(self, width: int) -> int:
        return 30

    def draw(self, surface, x, y, width) -> int:
        label_font = FONTS.get(12)
        value_font = FONTS.get(12, True)
        surface.blit(label_font.render(self.label, True, MUTED), (x, y + 5))
        value = value_font.render(self.text, True, TEXT if self.fraction is not None else MUTED)
        surface.blit(value, (x + width - value.get_width(), y + 5))
        track_x = x + 92
        track_w = max(20, width - 92 - value.get_width() - 16)
        track = pygame.Rect(track_x, y + 8, track_w, 8)
        pygame.draw.rect(surface, BAR_BG, track, border_radius=4)
        if self.fraction is None:
            step = 10
            px = track.x
            while px < track.right:
                pygame.draw.line(surface, MISSING, (px, track.y), (px, track.bottom), 4)
                px += step
        else:
            filled = int(track_w * max(0.0, min(1.0, self.fraction)))
            if filled > 0:
                pygame.draw.rect(surface, self.color,
                                 pygame.Rect(track.x, track.y, filled, track.h),
                                 border_radius=4)
        return 30


class GaugeBlock(Block):
    def __init__(self) -> None:
        self.fraction = None
        self.readout = UNAVAILABLE
        self.scale_max = UNAVAILABLE

    def measure(self, width: int) -> int:
        return int(min(200, max(140, width * 0.52)))

    def draw(self, surface, x, y, width) -> int:
        height = self.measure(width)
        rect = pygame.Rect(x + 20, y, width - 40, height - 34)
        center = (rect.centerx, rect.bottom)
        radius = min(rect.width // 2, rect.height)
        box = pygame.Rect(center[0] - radius, center[1] - radius, radius * 2, radius * 2)
        # pygame measures angles clockwise from +x with y pointing down, so the
        # upper semicircle is 0..pi while the needle angle lives in pi..2pi.
        pygame.draw.arc(surface, (27, 40, 48), box, 0.0, math.pi, 10)
        if self.fraction is not None:
            fraction = max(0.0, min(1.0, self.fraction))
            pygame.draw.arc(surface, ACCENT, box, 0.0, math.pi * fraction, 10)
            theta = math.pi + math.pi * fraction
            tip = (center[0] + (radius - 14) * math.cos(theta),
                   center[1] + (radius - 14) * math.sin(theta))
            pygame.draw.line(surface, (223, 251, 251), center, tip, 3)
        pygame.draw.circle(surface, (12, 20, 26), center, 7)
        pygame.draw.circle(surface, ACCENT, center, 7, 2)
        readout = FONTS.get(24, True).render(self.readout, True, TEXT)
        surface.blit(readout, readout.get_rect(center=(center[0], center[1] - 26)))
        unit = FONTS.get(10).render("MHz average", True, MUTED)
        surface.blit(unit, unit.get_rect(center=(center[0], center[1] - 10)))
        left = FONTS.get(10).render("0", True, MUTED)
        surface.blit(left, (box.left - 4, center[1] + 4))
        right = FONTS.get(10).render(self.scale_max, True, MUTED)
        surface.blit(right, (box.right - right.get_width() + 4, center[1] + 4))
        return height


class Segmented(Block):
    def __init__(self, options: list[tuple[str, object]], selected=None, big: bool = False):
        self.options = options
        self.selected = selected
        self.big = big
        self.tag = None
        self._rects: list[tuple[pygame.Rect, object]] = []

    def measure(self, width: int) -> int:
        return 44 if self.big else 38

    def draw(self, surface, x, y, width) -> int:
        height = self.measure(width)
        font = FONTS.get(14 if self.big else 13, True)
        self._rects = []
        cursor = x
        for label, value in self.options:
            text = font.render(label, True, TEXT)
            pill_w = text.get_width() + 30
            rect = pygame.Rect(cursor, y + 3, pill_w, height - 8)
            selected = value == self.selected
            pygame.draw.rect(surface, ACCENT if selected else (16, 26, 33), rect,
                             border_radius=rect.height // 2)
            pygame.draw.rect(surface, ACCENT if selected else CARD_EDGE, rect,
                             width=1, border_radius=rect.height // 2)
            surface.blit(text, text.get_rect(center=rect.center))
            self._rects.append((rect, value))
            cursor += pill_w + 8
        self.tag = self.tag or id(self)
        return height

    def hits(self):
        return [(rect, (self, value)) for rect, value in self._rects]

    def choose(self, value):
        self.selected = value


class SliderRow(Block):
    def __init__(self, label: str, value: int = 0) -> None:
        self.label = label
        self.value = value
        self.enabled = True
        self._track = pygame.Rect(0, 0, 0, 0)

    def measure(self, width: int) -> int:
        return 46

    def _track_rect(self, x, y, width):
        return pygame.Rect(x + 8, y + 20, width - 16, 8)

    def draw(self, surface, x, y, width) -> int:
        font = FONTS.get(13)
        surface.blit(font.render(f"{self.label}: {self.value}", True, TEXT), (x, y))
        self._track = self._track_rect(x, y, width)
        pygame.draw.rect(surface, BAR_BG, self._track, border_radius=4)
        filled = int(self._track.w * self.value / 100)
        pygame.draw.rect(surface, ACCENT if self.enabled else MUTED,
                         pygame.Rect(self._track.x, self._track.y, filled, self._track.h),
                         border_radius=4)
        handle_x = self._track.x + filled
        pygame.draw.circle(surface, ACCENT if self.enabled else MUTED,
                           (handle_x, self._track.centery), 8)
        return 46

    def hits(self):
        padded = self._track.inflate(10, 26)
        return [(padded, (self, "drag"))]

    def drag_to(self, pos_x):
        if not self.enabled or self._track.w <= 0:
            return
        fraction = (pos_x - self._track.x) / self._track.w
        self.value = int(max(0, min(100, round(fraction * 100))))


class SwatchRow(Block):
    def __init__(self, colors: list[tuple[int, int, int]]) -> None:
        self.colors = colors
        self.selected = None
        self._rects: list[tuple[pygame.Rect, tuple[int, int, int]]] = []

    def measure(self, width: int) -> int:
        return 40

    def draw(self, surface, x, y, width) -> int:
        self._rects = []
        cursor = x
        for color in self.colors:
            rect = pygame.Rect(cursor, y + 4, 30, 30)
            pygame.draw.circle(surface, color, rect.center, 15)
            if color == self.selected:
                pygame.draw.circle(surface, (255, 255, 255), rect.center, 17, 2)
            self._rects.append((rect, color))
            cursor += 40
        return 40

    def hits(self):
        return [(rect, (self, color)) for rect, color in self._rects]

    def choose(self, color):
        self.selected = color


class StepperRow(Block):
    def __init__(self, label: str, value: int, low: int = 1, high: int = 500, step: int = 5):
        self.label = label
        self.value = value
        self.low = low
        self.high = high
        self.step = step
        self._minus = pygame.Rect(0, 0, 0, 0)
        self._plus = pygame.Rect(0, 0, 0, 0)

    def measure(self, width: int) -> int:
        return 36

    def draw(self, surface, x, y, width) -> int:
        font = FONTS.get(13)
        surface.blit(font.render(self.label, True, MUTED), (x, y + 6))
        self._minus = pygame.Rect(x + width - 118, y + 2, 30, 28)
        self._plus = pygame.Rect(x + width - 34, y + 2, 30, 28)
        for rect, glyph in ((self._minus, "-"), (self._plus, "+")):
            pygame.draw.rect(surface, (22, 35, 43), rect, border_radius=8)
            pygame.draw.rect(surface, CARD_EDGE, rect, width=1, border_radius=8)
            text = FONTS.get(16, True).render(glyph, True, TEXT)
            surface.blit(text, text.get_rect(center=rect.center))
        value = FONTS.get(14, True).render(f"{self.value} W", True, TEXT)
        surface.blit(value, (self._minus.x - value.get_width() - 12, y + 7))
        return 36

    def hits(self):
        return [(self._minus, (self, -1)), (self._plus, (self, +1))]

    def nudge(self, direction: int):
        self.value = max(self.low, min(self.high, self.value + direction * self.step))


class Button(Block):
    def __init__(self, label: str, primary: bool = False) -> None:
        self.label = label
        self.primary = primary
        self.enabled = True
        self._rect = pygame.Rect(0, 0, 0, 0)

    def measure(self, width: int) -> int:
        return 40

    def draw(self, surface, x, y, width) -> int:
        font = FONTS.get(14, True)
        text = font.render(self.label, True, (4, 37, 42) if self.primary else TEXT)
        self._rect = pygame.Rect(x, y + 2, text.get_width() + 36, 34)
        fill = ACCENT if self.primary else (22, 35, 43)
        if not self.enabled:
            fill = (30, 42, 50)
        pygame.draw.rect(surface, fill, self._rect, border_radius=10)
        pygame.draw.rect(surface, CARD_EDGE, self._rect, width=1, border_radius=10)
        surface.blit(text, text.get_rect(center=self._rect.center))
        return 40

    def hits(self):
        return [(self._rect, (self, "click"))]


class Card:
    def __init__(self, title: str, blocks: list[Block]) -> None:
        self.title = title
        self.blocks = blocks
        self.rect = pygame.Rect(0, 0, 0, 0)

    def measure(self, width: int) -> int:
        inner = width - 40
        total = 16 + 22
        for block in self.blocks:
            total += block.measure(inner) + SPACING
        return total + 14

    def draw(self, surface, rect: pygame.Rect) -> None:
        self.rect = rect
        pygame.draw.rect(surface, CARD, rect, border_radius=14)
        pygame.draw.rect(surface, CARD_EDGE, rect, width=1, border_radius=14)
        title = FONTS.get(12, True).render(self.title.upper(), True, ACCENT)
        surface.blit(title, (rect.x + 20, rect.y + 14))
        y = rect.y + 16 + 22
        inner = rect.width - 40
        for block in self.blocks:
            height = block.draw(surface, rect.x + 20, y, inner)
            y += height + SPACING

    def hits(self):
        out = []
        for block in self.blocks:
            out.extend(block.hits())
        return out
