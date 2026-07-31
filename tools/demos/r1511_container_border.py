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
  * Fill the rect instead of stroking it and the HOLLOW assertions fail —
    the interior probes read ink where the page colour belongs.

Neither collapses into the other, so the demo pins the shape of what is
painted rather than merely that something is.

The checked state is asserted through the same lens from the same process
(`scene/screenshot` captures the live surface, so state can be driven between
captures): checking the box fills the SAME rect the outline occupied. That is
the second direction — the outline is not a static decoration, it is the
declaration the widget swaps out.

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

        for label, x, y in (
            ("centre", mx, my),
            ("upper-left inner", x0 + BORDER_W + 2, y0 + BORDER_W + 2),
            ("lower-right inner", x1 - BORDER_W - 2, y1 - BORDER_W - 2),
        ):
            assert not is_ink(unchecked, x, y, page), (
                f"an unchecked box is HOLLOW — {label} must read the page, got "
                f"{png_pixel(unchecked, x, y)}"
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
        tf.click(path=TAG)
        checked = capture(tf, "checked")
        cx0, cy0, cx1, cy1 = find_box(checked, page)
        assert_eq((cx0, cy0, cx1, cy1), (x0, y0, x1, y1), "checked box occupies the same rect")
        assert is_ink(checked, mx, my, page), (
            "a checked box is FILLED — the centre carries ink"
        )
        centre = png_pixel(checked, mx, my)
        assert centre != page, "checked centre differs from the page"
        for frac in (0.35, 0.5, 0.65):
            sx = x0 + int((x1 - x0) * frac)
            assert is_ink(checked, sx, my, page), f"checked interior inked at x-frac {frac}"

        # ── (G) unchecking restores the hollow outline ─────────────────
        tf.click(path=TAG)
        again = capture(tf, "unchecked-again")
        ax0, ay0, ax1, ay1 = find_box(again, page)
        assert_eq((ax0, ay0, ax1, ay1), (x0, y0, x1, y1), "box rect survives the round trip")
        assert not is_ink(again, mx, my, page), "unchecking returns the hollow interior"
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
