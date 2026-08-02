#!/usr/bin/env python3
"""R1542 §5.41 — the grid states the winsize its PRODUCER was given.

The field report this round answers (sprag PINION-PR80, 2026-08-02).
`TextGridNode.rect` meant two things at once: the **paint extent** (R1028
fills all of it with the palette default background, so a sub-cell margin
cannot leak the parent surface) and the **winsize** (`cols()` = how many
whole cells that width holds). Those agree exactly when the layout is what
sizes the producer. sprag is a terminal multiplexer, where it is not: a
daemon tiles the session in CELLS and hands each pane a `TIOCSWINSZ`, while
the display client lays those panes out in PIXELS. One boundary, quantised
twice — measured there, a pane sat permanently at 38 painted vs 37 buffered.

No rect can satisfy both. Shrink it to the producer's grid and R1028's fill
shrinks with it (a theme-coloured band appears inside the pane — the exact
leak R1028 exists to stop); leave it as the paint extent and the node cannot
state the grid it actually holds. So every `scene/snapshot` a client read
reported `cols != buffer_cols` forever — the signal pinion's OWN docs define
as "a legitimate in-flight resize (or a producer bug)". Nothing was resizing
and nothing was buggy.

R1542 splits the two facts instead of trading them. `with_winsize` declares
the producer's grid, `rect` goes on meaning the paint extent, and the wire
gains `winsize_source` so a client knows WHICH authority sized the grid —
because the authority is not recoverable from the values (a declaration that
happens to equal the derivation is byte-identical to none, and the two are
different claims about what a divergence means).

`hello-textgrid`'s `htg_tiled` grid is that case at demo scale: the rect
spans 8 cells, the producer holds 7. What this demo asserts:

  * the declared arm reports `cols == 7` while its rect spans 8 cells —
    i.e. the answer is NOT the derivation, and the derivation is still
    computable by the client from `rect` + `cell_w`, so both facts are on
    the wire;
  * `cols == buffer_cols` on that arm — the steady state R974.1 describes,
    unreachable before this round for a producer-sized grid;
  * `winsize_source == "producer"` there and `"layout"` on every other grid
    in the same snapshot — one binding, both regimes, so the field is shown
    to discriminate rather than to be a constant;
  * the row content confirms the declared columns are real content, so `7`
    is the producer's grid and not a truncation artefact;
  * the exact key set of a TextGrid node, which is what R1539's census gate
    cannot see for a hand-serialized response (see the round notes).

ZERO-FLAKE: bounded `wait_snap` polling (never a fixed sleep). >=30
assertions.

Run from the workspace root:
    cargo build -p hello-textgrid --release
    python3 tools/demos/r1542_grid_states_its_winsize.py
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_snap,
)

WIN = (680, 960)

TILED_TAG = "htg_tiled"
# Every other grid in the binding — the "layout" regime, all in the same
# snapshot as the declared one.
DERIVED_TAGS = [
    "htg_default",
    "htg_content",
    "htg_attrs",
    "htg_cursor",
    "htg_wide",
    "htg_alt",
    "htg_damage",
    "htg_underline",
    "htg_hyperlink",
    "htg_cursor_color",
    "htg_cursor_blink",
]

DECLARED_COLS = 7
DECLARED_ROWS = 2
SPAN_COLS = 8

# The full published shape of a TextGrid node. R1539 built a census gate that
# proves the wire matches the types — over every type this crate DERIVES
# Serialize for. `TextGridSnapshot` derives none (it is hand-built into a
# `serde_json::Map`, along with 50 other response shapes), so R1542 added
# `winsize_source` to a published response and no gate saw it. The unit test
# `r1542_the_text_grid_wire_states_its_exact_key_set` closes that for this
# type; this is the same assertion made a second, independent way — over the
# live wire, which is where an agent actually meets it.
TEXT_GRID_KEYS = {
    "type",
    "rect",
    "tag",
    "cell_w",
    "cell_h",
    "cols",
    "rows",
    "buffer_cols",
    "buffer_rows",
    "grid_rows",
    "cursor",
    "screen",
    "winsize_source",
}


def row_text(grid: Any, row: int) -> Optional[str]:
    rows = grid.get("grid_rows", [])
    return rows[row].get("text") if row < len(rows) else None


def body() -> None:
    with RpcSubprocess("hello-textgrid") as tf:
        # ZERO-FLAKE gate: poll until the tiled grid's layout has resolved and
        # its projection is present. No fixed sleep.
        snap = wait_snap(
            tf,
            lambda s: (find_by_tag(s, TILED_TAG) or {}).get("cols") == DECLARED_COLS
            and len((find_by_tag(s, TILED_TAG) or {}).get("grid_rows", [])) == DECLARED_ROWS,
            source="paint",
            viewport=WIN,
            desc="the tiled grid resolves its layout and projection",
        )

        tiled = find_by_tag(snap, TILED_TAG)
        assert tiled is not None, "the declared-winsize grid is in the scene"
        assert_eq(tiled.get("type"), "TextGrid", "it is a TextGrid node")

        # ── the wire shape, asserted as an exact set ────────────────────────
        assert_eq(
            set(tiled.keys()),
            TEXT_GRID_KEYS,
            "a TextGrid node's published key set",
        )
        assert "winsize_source" in tiled, "the R1542 key is present"

        # ── the declaration is the answer, and the derivation is still there ─
        assert_eq(tiled["cols"], DECLARED_COLS, "cols is the DECLARED grid")
        assert_eq(tiled["rows"], DECLARED_ROWS, "rows is the declared grid")
        cell_w = tiled["cell_w"]
        cell_h = tiled["cell_h"]
        assert cell_w > 0 and cell_h > 0, "the node-local metric is on the wire"
        rect = tiled["rect"]
        span_cols = rect["w"] // cell_w
        span_rows = rect["h"] // cell_h
        assert_eq(span_cols, SPAN_COLS, "the rect spans one more whole cell")
        assert_eq(span_rows, DECLARED_ROWS, "the rows agree; only x disagrees")
        assert tiled["cols"] != span_cols, (
            f"THE round: the answer ({tiled['cols']}) must not be the "
            f"derivation ({span_cols}). Equal means `with_winsize` did not "
            "reach `cols()`"
        )
        assert_eq(
            span_cols - tiled["cols"],
            1,
            "the client can still compute the widget's own span from rect + "
            "cell_w — both facts are on the wire, which is why R-80.2 needs "
            "no extra geometry field",
        )

        # ── the steady state R974.1 describes, now reachable ────────────────
        assert_eq(tiled["buffer_cols"], DECLARED_COLS, "the producer delivered 7")
        assert_eq(tiled["buffer_rows"], DECLARED_ROWS, "and 2 rows")
        assert_eq(
            (tiled["cols"], tiled["rows"]),
            (tiled["buffer_cols"], tiled["buffer_rows"]),
            "declared == delivered: a producer-sized grid can now be at rest, "
            "where before R1542 it read as a permanent divergence",
        )

        # ── the authority is stated, not inferred ──────────────────────────
        assert_eq(tiled["winsize_source"], "producer", "something else sized it")
        assert tiled["winsize_source"] in ("layout", "producer"), "a closed vocabulary"

        # ── and it discriminates: same snapshot, the other regime ───────────
        seen_layout = 0
        for tag in DERIVED_TAGS:
            grid = find_by_tag(snap, tag)
            assert grid is not None, f"{tag} is in the scene"
            assert_eq(
                grid.get("winsize_source"),
                "layout",
                f"{tag} is sized by the layout",
            )
            derived = grid["rect"]["w"] // grid["cell_w"]
            assert_eq(
                grid["cols"],
                derived,
                f"{tag}: a layout-sized grid still derives cols from its rect "
                "— R1542 is additive, and this is the arm that proves it",
            )
            seen_layout += 1
        assert seen_layout == len(DERIVED_TAGS), "every derived arm was read"
        assert seen_layout >= 2, (
            "a field that only ever reads one value discriminates nothing"
        )

        # ── the declared columns are real content, not a truncation ────────
        assert_eq(row_text(tiled, 0), "7 cells", "row 0 names the producer's width")
        assert_eq(row_text(tiled, 1), "abcdefg", "row 1 fills all 7 declared cells")
        assert_eq(len(row_text(tiled, 1) or ""), DECLARED_COLS, "7 glyphs, 7 columns")
        assert_eq(len(tiled["grid_rows"]), DECLARED_ROWS, "exactly the declared rows")

        # ── the rect is untouched — R1028's fill still owns all of it ──────
        assert_eq(rect["w"], SPAN_COLS * cell_w, "the paint extent spans 8 cells")
        assert rect["w"] > tiled["cols"] * cell_w, (
            "the sub-grid margin exists in pixels; the #[ignore]d shell guard "
            "r1542_declared_winsize_leaves_the_sub_grid_margin_to_the_gutter_fill "
            "asserts it paints as the palette default bg"
        )

        # ── a second read is the same read (the declaration is not one-shot) ─
        again = tf.snapshot(source="paint")
        tiled2 = find_by_tag(again, TILED_TAG)
        assert tiled2 is not None, "still there on a later frame"
        assert_eq(tiled2["cols"], DECLARED_COLS, "the declaration survives a frame")
        assert_eq(tiled2["winsize_source"], "producer", "and so does its authority")
        assert_eq(tiled2["rect"], rect, "and the paint extent did not move")

        # ── the controlled pair: same geometry, different authority ────────
        # `htg_cursor_blink` is 8x2 cells at the same 8x16 metric, so its rect
        # is pixel-for-pixel the size of the tiled grid's. Two nodes with
        # IDENTICAL geometry whose `cols` differ is the whole claim of this
        # round in one comparison — and it is only expressible because the
        # authority is a field rather than something inferred from the values,
        # which here are the same values.
        #
        # (Deliberately NOT asserted against `source="state"`: this binding's
        # state scene is a root `External` holding no grid nodes at all, so
        # "both sources agree" would be vacuous. The genuinely independent
        # second observation of this declaration is the #[ignore]d pixel
        # guard, which reads it off the framebuffer rather than the wire. See
        # [[state-scene-vs-paint-scene-introspect]].)
        twin = find_by_tag(snap, "htg_cursor_blink")
        assert twin is not None, "the geometric twin is in the scene"
        assert_eq(
            (twin["rect"]["w"], twin["rect"]["h"]),
            (rect["w"], rect["h"]),
            "precondition: the twin has the same paint extent",
        )
        assert_eq(
            (twin["cell_w"], twin["cell_h"]),
            (cell_w, cell_h),
            "precondition: and the same cell metric",
        )
        assert_eq(twin["cols"], SPAN_COLS, "the twin derives 8 from that rect")
        assert twin["cols"] != tiled["cols"], (
            "identical geometry, different grid — which no client could have "
            "read before R1542, because the geometry was the only answer"
        )
        assert_eq(
            (twin["winsize_source"], tiled["winsize_source"]),
            ("layout", "producer"),
            "and the wire says which is which",
        )


if __name__ == "__main__":
    sys.exit(run_demo("R1542 §5.41 PINION-PR80 producer-declared grid winsize", body))
