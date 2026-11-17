"""Flow-grid layout.

Cards are placed left to right into as many columns as fit, and every card in
a row is stretched to the tallest card of that row. Because positions are
computed from measured heights, cards can neither overlap nor run off the edge
of the window -- the two defects the previous immediate-mode UI shipped with.
"""

from __future__ import annotations

from typing import Sequence


def column_count(available_width: int, card_min: int, gap: int) -> int:
    if card_min + gap <= 0:
        return 1
    columns = (available_width + gap) // (card_min + gap)
    return max(1, int(columns))


def column_width(available_width: int, columns: int, gap: int) -> int:
    return max(1, (available_width - gap * (columns - 1)) // columns)


def flow_grid(
    heights: Sequence[int],
    area_x: int,
    area_y: int,
    area_width: int,
    card_min: int,
    gap: int,
) -> list[tuple[int, int, int, int]]:
    """Return (x, y, w, h) for each card, rows aligned to the tallest card."""
    columns = column_count(area_width, card_min, gap)
    width = column_width(area_width, columns, gap)
    rects: list[tuple[int, int, int, int]] = []
    row_start = 0
    cursor_y = area_y
    while row_start < len(heights):
        row = heights[row_start:row_start + columns]
        row_height = max(row)
        for offset, _ in enumerate(row):
            index = row_start + offset
            x = area_x + offset * (width + gap)
            rects.append((x, cursor_y, width, row_height))
        cursor_y += row_height + gap
        row_start += columns
    return rects


def rects_overlap(a: tuple[int, int, int, int], b: tuple[int, int, int, int]) -> bool:
    ax, ay, aw, ah = a
    bx, by, bw, bh = b
    return ax < bx + bw and bx < ax + aw and ay < by + bh and by < ay + ah
