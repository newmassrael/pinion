#!/usr/bin/env python3
"""R1511 §5.16 §2#6 §2#7 — a container's declared border reaches the pixels.

`BoxStyle` hangs off `Scene::Box` and `Scene::Container` alike, and the two
non-GUI backends read it that way: the TUI walker draws a container's border
as box-drawing cells, and the PDF projector routes `Scene::Container` through
the same `paint_box` that strokes it. The vello adapter did not. It stroked a
border in the `Scene::Box` arm only, so 45 container-borne declarations across
the workspace were dropped silently — the divergence §2 #6 exists to forbid,
made invisible by the fact that nothing failed.

`hello-checkbox` is the sharpest consumer, which is why this demo drives it:
an M3 checkbox in its UNCHECKED state has a transparent fill and no children.
Its outline is the entire widget. Before this round the GUI painted **nothing
at all** where the box is — a 24x24 hole in the middle of the row, with the
label beside it. This asserts the box is there, and that what is there is an
OUTLINE and not a fill.

Falsifiability, in both directions:

  * Stop stroking a container's border and the box vanishes: the locator
    finds no rectangle, and every band assertion fails.
  * Fill the rect instead of stroking it and the HOLLOW assertion fails —
    the interior reads ink where the page colour belongs.

Neither collapses into the other, so the demo pins the shape of what is
painted rather than merely that something is.

The checked state is asserted through the same lens from the same process
(`scene/screenshot` re-renders the live surface, so state can be driven
between captures): checking the box fills the SAME rect the outline occupied.
That is the second direction — the outline is not a static decoration, it is
the declaration the widget swaps out.

R1515 — the interior claims are AREA measures, not point probes. This demo
shipped in R1511 asserting `is_ink(centre)` for the filled state and failed
its first CI run, having passed locally every time. The cause is a property
of the widget, not of the runner: the M3 checkmark is drawn in `on-primary`,
which is near-white, so it is on the PAGE side of the `INK` threshold and
carves a page-coloured trail through the fill. Measured on a real capture,
that trail runs within one pixel of the centre column, and shifting the probe
by two pixels flips the old assertion while the interior fraction does not
move at all (0.9568 either way). Which pixel moved in CI — a font-driven
layout shift, a different rasterisation phase — is not established and does
not need to be: a probe must be located relative to the feature it asserts
about, and `centre` was located from the box while making a claim about the
fill, which contains a feature the locator knows nothing about.

Run:
    cargo build -p hello-checkbox --release
    python3 tools/demos/r1511_container_border.py
"""

from __future__ import annotations

import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    Png,
    RpcSubprocess,
    assert_eq,
    png_pixel,
    read_png_rgba8,
    run_demo,
)

EXAMPLE = "hello-checkbox"
TAG = "main_checkbox"
WIN = (360, 180)

# `CheckboxStyle::m3_filled()` — mirrored so the demo asserts the DECLARED
# geometry rather than whatever it happens to measure. A change to either
# constant must be a deliberate edit here too.
BOX_SIZE = 24
BORDER_W = 2

# Channel distance at which a pixel counts as ink rather than the page. Well
# above the vello area-AA bleed at a rounded corner, well below any real
# colour difference between the surface and the M3 outline.
INK = 40


def is_ink(img: Png, x: int, y: int, page: tuple[int, int, int, int]) -> bool:
    return max(abs(a - b) for a, b in zip(png_pixel(img, x, y), page)) > INK


def capture(tf: RpcSubprocess, name: str) -> Png:
    out = Path(tempfile.mkdtemp(prefix="pinion-r1511-")) / f"{name}.png"
    res = tf.request("scene/screenshot", {"path": "", "out_path": str(out)})
    assert res.result, f"{name}: screenshot returned no result"
    assert_eq((res.result["width"], res.result["height"]), WIN, f"{name} extent")
    assert out.exists(), f"{name}: no PNG at {out}"
    return read_png_rgba8(out)


def find_box(img: Png, page: tuple[int, int, int, int]) -> tuple[int, int, int, int]:
    """Locate the checkbox by its own ink, without being told where it is.

    The row is `[box] [label]`, so the box is the LEFTMOST ink column. Walking
    right from there to the first fully blank column bounds it horizontally;
    the ink rows within that column band bound it vertically. Nothing here
    knows the layout — a moved box is still found, a missing one is not.
    """
    cols = [
        x
        for x in range(img.width)
        if any(is_ink(img, x, y, page) for y in range(img.height))
    ]
    assert cols, "the window has no ink at all"
    x0 = cols[0]
    x1 = x0
    while x1 + 1 in cols:
        x1 += 1
    rows = [y for y in range(img.height) if any(is_ink(img, x, y, page) for x in range(x0, x1 + 1))]
    assert rows, "the leftmost ink column band has no ink rows"
    return x0, rows[0], x1, rows[-1]


def interior_ink(img: Png, page, x0: int, y0: int, x1: int, y1: int) -> float:
    """Fraction of the box's interior — inside the border band — carrying ink.

    R1515 replaces R1511's single-pixel interior probes with this, because the
    point was the wrong shape for this widget. The M3 checkmark is drawn in
    `on-primary`, which is near-white: the SAME side of the `INK` threshold as
    the page. So a checked box is not a solid block of ink — the glyph carves
    a page-coloured trail straight through it, and measured on this window
    that trail passes within ONE pixel of the centre column.

    A point probe there is a coin flip on where the glyph rasterises, and the
    coin came up tails in CI (run 30598731572) while landing heads on every
    local adapter, under Xvfb, and under full CPU load — 20 local runs, zero
    failures. The states themselves are not close at all: the interior is
    0.00 inked when hollow and 0.96 when filled. R1511 reached for the one
    measurement with no margin when the one with total margin was available.
    """
    ix0, iy0 = x0 + BORDER_W + 1, y0 + BORDER_W + 1
    ix1, iy1 = x1 - BORDER_W - 1, y1 - BORDER_W - 1
    total = (ix1 - ix0 + 1) * (iy1 - iy0 + 1)
    assert total > 0, "the interior has area to sample"
    inked = sum(
        1
        for y in range(iy0, iy1 + 1)
        for x in range(ix0, ix1 + 1)
        if is_ink(img, x, y, page)
    )
    return inked / total


# Measured on this window: hollow = 0.0000, filled = 0.9568. Both thresholds
# sit far from both readings, and no single image can satisfy the two — so a
# reported fraction also SAYS which failure happened: ~0.0 means the box is
# hollow (the click never landed, or the fill was dropped), ~0.96 means it is
# filled and something else is wrong.
HOLLOW_MAX = 0.05
FILLED_MIN = 0.80


def band_thickness(img: Png, page, x: int, y: int, dx: int, dy: int) -> int:
    """Count consecutive ink pixels walking inward from (x, y)."""
    n = 0
    while 0 <= x < img.width and 0 <= y < img.height and is_ink(img, x, y, page):
        n += 1
        x += dx
        y += dy
    return n


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) the scene declares the widget ──────────────────────────
        snap = tf.snapshot(source="paint", viewport=WIN)
        assert snap is not None, "paint snapshot produced"
        assert TAG in str(snap), f"{TAG} present in the paint scene"

        unchecked = capture(tf, "unchecked")
        page = png_pixel(unchecked, 2, 2)
        assert page[3] == 255, "the page samples opaque"
        assert min(page[:3]) > 200, f"the M3 light surface is near-white, got {page}"

        # ── (B) the box is THERE, and it is a box ──────────────────────
        x0, y0, x1, y1 = find_box(unchecked, page)
        assert_eq(x1 - x0 + 1, BOX_SIZE, "unchecked box painted width")
        assert_eq(y1 - y0 + 1, BOX_SIZE, "unchecked box painted height")
        assert y0 > 0 and y1 < unchecked.height - 1, "the box is inside the window"

        mx = (x0 + x1) // 2
        my = (y0 + y1) // 2

        # ── (C) all four edges carry the declared stroke ───────────────
        for label, x, y in (
            ("top", mx, y0),
            ("bottom", mx, y1),
            ("left", x0, my),
            ("right", x1, my),
        ):
            assert is_ink(unchecked, x, y, page), f"{label} edge carries the border ink"

        # Sampled at thirds so a single lit pixel cannot satisfy an edge.
        for frac in (0.25, 0.5, 0.75):
            sx = x0 + int((x1 - x0) * frac)
            sy = y0 + int((y1 - y0) * frac)
            assert is_ink(unchecked, sx, y0, page), f"top edge inked at x-frac {frac}"
            assert is_ink(unchecked, sx, y1, page), f"bottom edge inked at x-frac {frac}"
            assert is_ink(unchecked, x0, sy, page), f"left edge inked at y-frac {frac}"
            assert is_ink(unchecked, x1, sy, page), f"right edge inked at y-frac {frac}"

        # ── (D) it is a BORDER: the declared width, and hollow inside ──
        for label, x, y, dx, dy in (
            ("top", mx, y0, 0, 1),
            ("bottom", mx, y1, 0, -1),
            ("left", x0, my, 1, 0),
            ("right", x1, my, -1, 0),
        ):
            got = band_thickness(unchecked, page, x, y, dx, dy)
            assert_eq(got, BORDER_W, f"{label} band is the declared border width")

        hollow = interior_ink(unchecked, page, x0, y0, x1, y1)
        assert hollow <= HOLLOW_MAX, (
            f"an unchecked box is HOLLOW — its whole interior must read the "
            f"page, got {hollow:.4f} inked (threshold {HOLLOW_MAX})"
        )

        # ── (E) the corner radius is real ──────────────────────────────
        # `box_radius = 4`, so the exact corner pixel lies outside the rounded
        # path while the mid-edge is on it. A square stroke would ink both.
        for label, x, y in (
            ("top-left", x0, y0),
            ("top-right", x1, y0),
            ("bottom-left", x0, y1),
            ("bottom-right", x1, y1),
        ):
            assert not is_ink(unchecked, x, y, page), (
                f"{label} corner pixel is outside the rounded stroke"
            )

        # ── (F) checking fills the SAME rect the outline occupied ──────
        # R1570.1 — the click now also FOCUSES the checkbox, because that round
        # made a declared interactive role a focus stop (HTML's native `<input type=checkbox>` and
        # the toolkit's `StrongFocus` both do). The focus ring is painted around the
        # tagged Container, which spans `[box] [label]`, so it merges the two into one
        # continuous ink band and `find_box`'s "leftmost band" walk returns the whole
        # row. That is the ring being real, not the box moving — so the state
        # is put back before the capture rather than the finder being taught to
        # ignore ink.
        tf.click(path=TAG)
        tf.request("focus/set", {"tag": None})
        checked = capture(tf, "checked")
        cx0, cy0, cx1, cy1 = find_box(checked, page)
        assert_eq((cx0, cy0, cx1, cy1), (x0, y0, x1, y1), "checked box occupies the same rect")
        filled = interior_ink(checked, page, x0, y0, x1, y1)
        assert filled >= FILLED_MIN, (
            f"a checked box is FILLED — its interior must carry ink, got "
            f"{filled:.4f} (threshold {FILLED_MIN}). A reading near "
            f"{HOLLOW_MAX} means the box is still HOLLOW: the click did not "
            f"land, or the fill was dropped"
        )
        # …and filled is not the same as opaque-everywhere: the checkmark is
        # drawn in `on-primary`, so some of the interior is legitimately
        # page-coloured. Asserting a strict 1.0 would forbid the glyph.
        assert filled < 1.0, (
            f"the checkmark glyph is present — a perfectly solid interior "
            f"({filled:.4f}) would mean the check was never drawn"
        )

        # ── (G) unchecking restores the hollow outline ─────────────────
        # Same reason as (F): the click focuses, so put the focus back before
        # measuring ink.
        tf.click(path=TAG)
        tf.request("focus/set", {"tag": None})
        again = capture(tf, "unchecked-again")
        ax0, ay0, ax1, ay1 = find_box(again, page)
        assert_eq((ax0, ay0, ax1, ay1), (x0, y0, x1, y1), "box rect survives the round trip")
        back = interior_ink(again, page, x0, y0, x1, y1)
        assert back <= HOLLOW_MAX, (
            f"unchecking returns the hollow interior, got {back:.4f} inked"
        )
        for label, x, y in (("top", mx, y0), ("left", x0, my), ("right", x1, my)):
            assert is_ink(again, x, y, page), f"{label} edge still inked after the round trip"
        assert_eq(
            band_thickness(again, page, mx, y0, 0, 1),
            BORDER_W,
            "top band width survives the round trip",
        )

        # ── (H) the label is separate ink, right of the box ────────────
        label_cols = [
            x
            for x in range(x1 + 1, again.width)
            if any(is_ink(again, x, y, page) for y in range(again.height))
        ]
        assert label_cols, "the row's label paints to the right of the box"
        assert label_cols[0] > x1, "label ink starts after the box's right edge"


if __name__ == "__main__":
    sys.exit(run_demo("R1511 §5.16 §2#6 — container border reaches pixels", body))
