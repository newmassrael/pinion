#!/usr/bin/env python3
"""R758 §5.38 §5.40 cumulative star rating (`hello-rating`).

A 5-star rating built as pure composition over `RadioGroupExternal` (the
hello-segmented-button mould): a whole-star rating is a discrete value
selector — picking the 3rd star means "3 stars" — which is the WAI-ARIA
radiogroup shape, so the binding reuses the framework coordinator (mutual
exclusion, single-tab-stop roving keyboard, the `"selected"` intent, the
AccessKit radiogroup+radio tree) and adds zero new interaction substrate.

The one genuinely new piece is the **cumulative star paint with hover
preview** (1st star-paint consumer, inline): stars `0..=k` paint filled
(`★`) while `k` is the hovered star — a live preview of the rating a click
would commit — falling back to the committed selection when nothing is
hovered. The hover *feedback is the fill-extent change itself*, so the
stars need no per-cell state-layer box.

Phase 1 — RPC introspection / behaviour (the AI-first surface):
  * boot: 3 of 5 stars (`selected_index == 2`), 3 filled `★` + 2 empty `☆`
    glyphs, every `rating#<i>` sub-target + the `rating` group present;
  * AI-first hover preview: `scene/hover` on the 5th star fills all 5
    (preview), introspected purely by counting filled glyphs in the paint
    scene + the per-cell `state.4 == Hover`; hovering a lower star
    previews fewer; `pointer_leave` restores the committed 3;
  * `scene/click` on a star commits that rating (`selected_index` snaps,
    the fill follows) — the survivors' radios stay 1-of-N exclusive;
  * keyboard (single Tab stop): digit `1`..`5` set that many stars,
    `ArrowUp` / `ArrowDown` raise / lower, `Home` / `End` jump to 1 / 5.

Phase 2 — live-pixel (PINION_SCREENSHOT boot frame; source of truth = the
paint scene's own glyph colour + window fill, so the assertion is
introspection<->screen parity, not a hardcoded palette guess):
  * a filled star's centre is the Accent glyph colour;
  * an empty star's hollow centre shows the Surface window background;
  * the window background is Surface.

Run from the workspace root:
    cargo build -p hello-rating --release
    python3 tools/demos/r758_rating.py
"""

from __future__ import annotations

import os
import subprocess
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    WORKSPACE_ROOT,
    abs_rects_of,
    assert_eq,
    assert_pixel_eq,
    find_by_tag,
    read_png_rgba8,
    run_demo,
    sample_png_points,
)

EXAMPLE = "hello-rating"
VIEWPORT = (320, 140)
GROUP = "rating"
N = 5
STAR_FILLED = "★"  # ★ — matches main.rs STAR_FILLED
STAR_EMPTY = "☆"   # ☆ — matches main.rs STAR_EMPTY


def star_tag(i: int) -> str:
    return f"rating#{i}"


def sel(tf):
    return tf.query("/external/selected_index")


def cell_state(tf, i: int):
    return tf.query(f"/external/state.{i}")


def paint(tf):
    return tf.snapshot(source="paint", viewport=VIEWPORT)


def glyph_counts(node) -> tuple[int, int]:
    """(filled, empty) star glyph counts in the paint scene."""
    out: list[str] = []

    def walk(n) -> None:
        if n.get("type") == "Text":
            out.append(n.get("content", ""))
        for c in n.get("children") or []:
            walk(c)

    walk(node)
    return out.count(STAR_FILLED), out.count(STAR_EMPTY)


def first_text(node):
    if node.get("type") == "Text":
        return node
    for c in node.get("children") or []:
        hit = first_text(c)
        if hit:
            return hit
    return None


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── boot: 3 of 5 stars ───────────────────────────────────────────
        assert_eq(sel(tf), 2, "boot rating is 3 stars (selected_index 2)")
        snap = paint(tf)
        assert find_by_tag(snap, GROUP) is not None, "rating group row present"
        for i in range(N):
            assert find_by_tag(snap, star_tag(i)) is not None, f"star {i} sub-target present"
        assert_eq(glyph_counts(snap), (3, 2), "boot paints 3 filled + 2 empty stars")

        # Capture boot geometry + the Accent glyph colour + Surface tone for
        # the Phase 2 pixel guard (the screenshot is a fresh boot).
        rects = abs_rects_of(snap)
        accent = first_text(find_by_tag(snap, star_tag(0)))["style"]["fg_color"]
        accent_rgb = (accent["r"], accent["g"], accent["b"])
        surface = snap["style"]["fill"]
        surface_rgb = (surface["r"], surface["g"], surface["b"])
        assert accent_rgb != surface_rgb, "filled-star Accent is distinct from Surface"

        # ── AI-first hover preview: hovering star k fills k+1 stars ───────
        for k in (4, 3, 0):
            tf.hover(path=star_tag(k))
            filled, _ = glyph_counts(paint(tf))
            assert_eq(filled, k + 1, f"hovering star {k} previews {k + 1} filled stars")
            assert_eq(cell_state(tf, k), "Hover", f"hovered star {k} reports Hover")
            tf.pointer_leave()
        # After leaving, the fill returns to the committed 3 stars.
        assert_eq(glyph_counts(paint(tf)), (3, 2), "pointer_leave restores the committed 3 stars")
        assert_eq(sel(tf), 2, "hover preview never mutates the committed selection")

        # ── click commits a new rating (1-of-N exclusive) ────────────────
        tf.click(path=star_tag(0))
        tf.pointer_leave()
        assert_eq(sel(tf), 0, "clicking the 1st star commits a 1-star rating")
        assert_eq(glyph_counts(paint(tf)), (1, 4), "1 filled + 4 empty after the click")
        tf.click(path=star_tag(4))
        tf.pointer_leave()
        assert_eq(sel(tf), 4, "clicking the 5th star commits a 5-star rating")
        assert_eq(glyph_counts(paint(tf)), (5, 0), "all 5 filled")
        # Exactly one radio is selected at a time (mutual exclusion).
        assert_eq(tf.query("/external/selected.4"), True, "5th star radio selected")
        assert_eq(tf.query("/external/selected.0"), False, "1st star radio not selected")

        # ── keyboard (single Tab stop) ───────────────────────────────────
        assert_eq(
            tf.request("focus/set", {"tag": GROUP}).result.get("focused"),
            GROUP,
            "the rating is a single Tab stop",
        )
        tf.key(path=GROUP, name="3")
        assert_eq(sel(tf), 2, "digit 3 sets a 3-star rating")
        tf.key(path=GROUP, name="ArrowUp")
        assert_eq(sel(tf), 3, "ArrowUp raises to 4 stars")
        tf.key(path=GROUP, name="ArrowDown")
        assert_eq(sel(tf), 2, "ArrowDown lowers to 3 stars")
        tf.key(path=GROUP, name="End")
        assert_eq(sel(tf), 4, "End jumps to 5 stars")
        tf.key(path=GROUP, name="Home")
        assert_eq(sel(tf), 0, "Home jumps to 1 star")
        tf.key(path=GROUP, name="5")
        assert_eq(sel(tf), 4, "digit 5 sets a 5-star rating")

    # ── Phase 2 — live-pixel (boot frame: 3 filled + 2 empty stars) ──────
    png = read_png_rgba8(capture_screenshot())
    assert (png.width, png.height) == VIEWPORT, \
        f"screenshot {png.width}x{png.height} != viewport {VIEWPORT}"

    def center(tag: str) -> tuple[int, int]:
        x, y, w, h = rects[tag]
        return (x + w // 2, y + h // 2)

    filled_pt = center(star_tag(0))   # boot: filled
    empty_pt = center(star_tag(4))    # boot: empty (hollow ☆)
    window_pt = (4, 4)
    p_filled, p_empty, p_window = sample_png_points(png, [filled_pt, empty_pt, window_pt])
    assert_pixel_eq(p_filled, (*accent_rgb, 255),
                    f"filled star centre is the Accent glyph colour {filled_pt}", tolerance=12)
    assert_pixel_eq(p_empty, (*surface_rgb, 255),
                    f"empty star hollow centre shows Surface {empty_pt}", tolerance=10)
    assert_pixel_eq(p_window, (*surface_rgb, 255),
                    f"window background is Surface {window_pt}", tolerance=8)


def capture_screenshot() -> Path:
    out = Path(tempfile.mkdtemp(prefix="pinion-r758-")) / "rating.png"
    binary = WORKSPACE_ROOT / "target" / "release" / EXAMPLE
    cmd = [str(binary)] if binary.exists() else [
        "cargo", "run", "-p", EXAMPLE, "--quiet", "--release",
    ]
    env = os.environ.copy()
    env["PINION_SCREENSHOT"] = str(out)
    res = subprocess.run(
        cmd, cwd=WORKSPACE_ROOT, env=env,
        capture_output=True, text=True, check=False, timeout=120.0,
    )
    if res.returncode != 0:
        raise AssertionError(
            f"PINION_SCREENSHOT capture exited {res.returncode}:\n  stderr: {res.stderr!r}"
        )
    if not out.exists():
        raise AssertionError(f"PINION_SCREENSHOT produced no file at {out}")
    return out


if __name__ == "__main__":
    sys.exit(run_demo("R758 cumulative star rating", body))
