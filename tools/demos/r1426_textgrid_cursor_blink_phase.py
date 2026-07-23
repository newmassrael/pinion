#!/usr/bin/env python3
"""R1426 §5.41 — render-time cursor blink PHASE on a cell-native `TextGrid`.

R1425 landed the DECSCUSR blink-vs-steady MODE as data (`GridCursor::blink`).
R1426 lands the render-time PHASE animation the mode drives: a cursor in the
blinking mode alternates ON / OFF at the 530 ms `CaretBlink` half-period in both
backends (the Vello window and the painted TUI cell), while a steady cursor
stays drawn. The phase is a renderer-owned, paint-time concern — every real
terminal keeps the blink timer in the widget, not the PTY app — driven by a
per-window free-running clock (no reset-on-activity), armed only while a visible
blinking cursor is on screen so an idle window releases the frame loop.

The headline INVARIANT this demo proves over RPC is §2 #7: the render-time phase
is NEVER folded into the scene / snapshot. `scene/tick` advances the blink clock
(the AI-first deterministic frame-stepping primitive), yet every `scene/snapshot`
reports the SAME cursor `blink` / `visible` — the mode and the DECTCEM state are
stable data, and the on/off phase is observable only in pixels (the live surface
animates; a headless / snapshot render pins the steady ON phase so a golden
capture never flakes on the wall-clock phase). "App hid the cursor" (visible) and
"blink off-phase" therefore stay distinguishable to any consumer.

Run from the workspace root:
    cargo build -p hello-textgrid --release
    python3 tools/demos/r1426_textgrid_cursor_blink_phase.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (
    RpcSubprocess,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_snap,
)

WIN = (680, 900)
BLINK_TAG = "htg_cursor_blink"
BAR_TAG = "htg_cursor"
COLOR_TAG = "htg_cursor_color"

# The default cursor a grid carries when its producer set none: hidden, home,
# block, no explicit OSC-12 colour, steady (blink False) — unchanged by R1426
# (the phase is never a scene field).
DEFAULT_CURSOR = {
    "col": 0,
    "row": 0,
    "shape": "block",
    "visible": False,
    "cursor_color": None,
    "blink": False,
}

# The blinking block cursor the modal-editor grid drives (col 2, row 1). This is
# the FULL wire shape a `scene/snapshot` reports for it, phase-independent.
BLINK_CURSOR = {
    "col": 2,
    "row": 1,
    "shape": "block",
    "visible": True,
    "cursor_color": None,
    "blink": True,
}


def cursor_of(tf: RpcSubprocess, tag: str) -> dict:
    snap = wait_snap(
        tf,
        lambda s: find_by_tag(s, tag) is not None,
        source="paint",
        viewport=WIN,
        desc=f"{tag} present",
    )
    grid = find_by_tag(snap, tag)
    assert grid is not None, f"{tag} present in paint scene"
    return grid["cursor"]


def body() -> None:
    with RpcSubprocess("hello-textgrid") as tf:
        # Gate on the blink grid's projection resolving (cols == 8, 2 rows).
        snap = wait_snap(
            tf,
            lambda s: (find_by_tag(s, BLINK_TAG) or {}).get("cols") == 8
            and len((find_by_tag(s, BLINK_TAG) or {}).get("grid_rows", [])) == 2,
            source="paint",
            viewport=WIN,
            desc=f"{BLINK_TAG} projection resolved",
        )

        # --- The blinking-mode cursor reports its MODE as data (R1425), and
        #     R1426 does NOT change any of it — the phase is not a scene field.
        grid = find_by_tag(snap, BLINK_TAG)
        assert grid is not None, "blink grid present"
        cur = grid["cursor"]
        assert_eq(cur, BLINK_CURSOR, "blinking cursor full wire shape")
        assert_eq(cur["blink"], True, "cursor is a blinking DECSCUSR variant")
        assert_eq(cur["visible"], True, "cursor is shown (DECTCEM)")
        # The mode is read APART from show/hide — two independent first-class facts.
        assert cur["blink"] is True and cur["visible"] is True, (
            "blink mode and DECTCEM visibility are distinct facts, both True here"
        )
        assert isinstance(cur["blink"], bool), "blink is a boolean mode, not a phase"
        # There is NO phase field on the wire — the snapshot carries only the
        # closed cursor dict (the render-time phase lives in the renderer).
        assert set(cur.keys()) == set(BLINK_CURSOR.keys()), (
            "cursor wire keys are exactly the closed R975/R1424/R1425 set — "
            "no render-time phase key leaked in"
        )

        # === HEADLINE (§2 #7): advancing the blink clock via `scene/tick` must
        #     NOT change the snapshot. The phase alternates in the renderer; the
        #     scene data is stable, so "app hid the cursor" and "blink off-phase"
        #     stay distinguishable. Step well past several 530 ms half-periods.
        baseline_blink = cursor_of(tf, BLINK_TAG)
        baseline_bar = cursor_of(tf, BAR_TAG)
        assert_eq(baseline_blink, BLINK_CURSOR, "baseline blinking cursor")
        # 8 half-period steps = 4 full ON/OFF cycles; the phase flips many times.
        for step in range(8):
            tf.tick(0.53)
            after = cursor_of(tf, BLINK_TAG)
            assert_eq(
                after,
                baseline_blink,
                f"blinking cursor snapshot is byte-stable across tick #{step + 1} "
                "(phase never folds into the wire)",
            )
            # visible / blink specifically never flip on the wire.
            assert after["visible"] is True, f"tick #{step + 1}: still visible (DECTCEM)"
            assert after["blink"] is True, f"tick #{step + 1}: still the blinking mode"
        # A large single step (many periods at once) is likewise invisible on wire.
        tf.tick(5.0)
        assert_eq(cursor_of(tf, BLINK_TAG), BLINK_CURSOR, "big tick leaves the wire stable")

        # --- The STEADY sibling (a bar) is unaffected by ticks too: blink False,
        #     visible True — a steady cursor never enters the blink clock.
        assert_eq(baseline_bar["shape"], "bar", "sibling cursor shape = bar")
        assert_eq(baseline_bar["blink"], False, "sibling bar cursor is steady")
        assert_eq(baseline_bar["visible"], True, "sibling bar cursor is shown")
        steady_after = cursor_of(tf, BAR_TAG)
        assert_eq(steady_after, baseline_bar, "steady cursor snapshot stable across ticks")
        assert cur["blink"] != baseline_bar["blink"], (
            "the two grids witness both DECSCUSR blink modes (blinking vs steady)"
        )

        # --- Regression: the R1424 OSC-12 colour grid is untouched, and steady.
        color_cur = cursor_of(tf, COLOR_TAG)
        assert_eq(color_cur["cursor_color"], "#2ecc71", "R1424 OSC-12 green intact")
        assert_eq(color_cur["blink"], False, "coloured cursor is steady (blink additive)")
        assert_eq(color_cur["visible"], True, "coloured cursor still visible")

        # --- Regression: scaffold grids still report the DEFAULT cursor (hidden,
        #     steady) — the mode and phase are opt-in per projection.
        final = tf.snapshot(source="paint", viewport=WIN)
        for tag in ("htg_default", "htg_measured", "htg_content", "htg_attrs"):
            other = find_by_tag(final, tag)
            assert other is not None, f"{tag} still present"
            assert_eq(
                other["cursor"],
                DEFAULT_CURSOR,
                f"{tag} carries the default cursor (hidden + steady)",
            )

        # --- Regression: R973 colours + the R1403 hyperlink grid are intact.
        content = find_by_tag(final, "htg_content")
        assert_eq(len(content["grid_rows"]), 4, "htg_content still 4 rows")
        assert_eq(
            content["grid_rows"][0]["runs"][0]["bg"]["index"],
            0,
            "htg_content bar[0] bg still index 0",
        )
        assert find_by_tag(final, "htg_hyperlink") is not None, "htg_hyperlink still present"


def main() -> int:
    return run_demo("r1426_textgrid_cursor_blink_phase", body)


if __name__ == "__main__":
    sys.exit(main())
