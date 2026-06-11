#!/usr/bin/env python3
"""R894 §5.40 — per-column validation (clamp) on the EDITABLE data grid.

Drives `hello-data-grid` via JSON-RPC. Each numeric column carries an optional
range; a committed (keyboard) or programmatic (AI) value outside it is CLAMPED
to the nearest bound through one `clamp_for_col` gate, so neither path can
store an out-of-range value (the bounded-spinbox / property-inspector
contract). The constraint is AI-readable (`query col_range.<col>`), and the
clamp surfaces as the read-back value (setter-returns-read-outcome).

Ranges: Count (col 2, Int) = 0..1000; Scale (col 3, Float) = 0..10;
Asset / Type / Active are unbounded.

Scope (exact count = 30 checks):
  (A) the ranges are AI-readable                        [4]
  (B) intervene (AI write) clamps to the range          [5]
  (C) in-range values pass through unchanged            [2]
  (D) an unbounded column is verbatim                   [1]
  (E) a keyboard commit clamps the same way             [2]
  (F) clamp composes with sort (clamped value sorts)    [2]
  (G) the clamped value is what the grid paints         [2]
  (H) re-read sweep confirms every cell within bounds   [12]
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    run_demo,
    wait_query,
    wait_snap,
    wait_until,
)

VIEWPORT = (460, 348)
GRID = "data_grid"


def _focus_grid(tf) -> None:
    tf.request("focus/set", {"tag": GRID})
    wait_until(
        lambda: tf.request("focus/get").result.get("focused") == GRID,
        timeout=4.0,
        interval=0.03,
        desc="grid owns keyboard focus",
    )


def _texts(snap) -> list:
    out: list = []

    def walk(node) -> None:
        if isinstance(node, dict):
            if node.get("type") == "Text" and isinstance(node.get("content"), str):
                out.append(node["content"])
            for v in node.values():
                walk(v)
        elif isinstance(node, list):
            for v in node:
                walk(v)

    walk(snap)
    return out


def body() -> None:
    with RpcSubprocess("hello-data-grid", boot_grace=1.5) as tf:
        # ── (A) the column ranges are AI-readable ───────────────────
        assert_eq(tf.query("/external/col_range.2"), "0..1000", "Count range")
        assert_eq(tf.query("/external/col_range.3"), "0..10", "Scale range")
        assert_eq(tf.query("/external/col_range.0"), "none", "Asset unbounded")
        assert_eq(tf.query("/external/col_range.4"), "none", "Active unbounded")
        # (an out-of-range column is an UnknownPath over the wire; the Rust
        # `None` -> error mapping is covered by the r894_col_range unit test.)

        # ── (B) intervene (AI write) clamps to the range ────────────
        tf.request("scene/intervene", {"path": "/external/value.0.2", "value": 5000})
        assert_eq(tf.query("/external/value.0.2"), 1000, "Count 5000 -> clamp to max 1000")
        tf.request("scene/intervene", {"path": "/external/value.1.3", "value": 50.0})
        assert_eq(tf.query("/external/value.1.3"), 10.0, "Scale 50 -> clamp to max 10")
        tf.request("scene/intervene", {"path": "/external/value.2.3", "value": -5.0})
        assert_eq(tf.query("/external/value.2.3"), 0.0, "Scale -5 -> clamp to min 0")
        tf.request("scene/intervene", {"path": "/external/value.3.2", "value": -99})
        assert_eq(tf.query("/external/value.3.2"), 0, "Count -99 -> clamp to min 0")
        tf.request("scene/intervene", {"path": "/external/value.2.2", "value": 1000})
        assert_eq(tf.query("/external/value.2.2"), 1000, "Count 1000 -> the bound itself")

        # ── (C) in-range values pass through unchanged ──────────────
        tf.request("scene/intervene", {"path": "/external/value.0.2", "value": 42})
        assert_eq(tf.query("/external/value.0.2"), 42, "in-range Count kept verbatim")
        tf.request("scene/intervene", {"path": "/external/value.1.3", "value": 3.5})
        assert_eq(tf.query("/external/value.1.3"), 3.5, "in-range Scale kept verbatim")

        # ── (D) an unbounded column is verbatim ─────────────────────
        tf.request("scene/intervene", {"path": "/external/value.0.0", "value": "LongAssetName"})
        assert_eq(tf.query("/external/value.0.0"), "LongAssetName", "unbounded Text unclamped")

        # ── (E) a keyboard commit clamps the same way ───────────────
        _focus_grid(tf)
        tf.request("scene/intervene", {"path": "/external/focused_row", "value": 1})
        tf.request("scene/intervene", {"path": "/external/focused_col", "value": 2})
        tf.invoke("/external/begin", None)
        wait_query(tf, "/external/editing_row", 1, desc="editing Tree's Count cell")
        for _ in range(2):  # clear the seeded "24"
            tf.key(path="data_grid_edit", name="Backspace")
        for ch in "9999":
            tf.key(path="data_grid_edit", name=ch)
        tf.key(path="data_grid_edit", name="Enter")
        wait_query(tf, "/external/value.1.2", 1000, desc="keyboard commit clamps 9999 -> 1000")
        assert_eq(tf.query("/external/editing_row"), None, "edit latch closed")

        # ── (F) clamp composes with sort ────────────────────────────
        # Tree (1) and Coin (2) are both clamped to 1000; sorting Count
        # ascending puts the two clamped maxima last, in stable source order.
        tf.invoke("/external/cycle_sort", 2)
        wait_query(tf, "/external/sort", "2:ascending", desc="sort Count ascending")
        assert_eq(tf.query("/external/source_at.2"), 1, "Tree (1000) second-to-last")
        assert_eq(tf.query("/external/source_at.3"), 2, "Coin (1000) last (stable tie)")
        tf.request("scene/intervene", {"path": "/external/sort", "value": "none"})

        # ── (G) the clamped value is what the grid paints ───────────
        snap = wait_snap(
            tf,
            lambda s: "1000" in _texts(s),
            viewport=VIEWPORT,
            desc="the clamped 1000 is painted (not the typed 9999)",
        )
        assert_eq("9999" in _texts(snap), False, "the out-of-range input never reaches the cell")

        # ── (H) re-read sweep: every numeric cell within its bounds ─
        for r in range(4):
            cnt = tf.query(f"/external/value.{r}.2")
            assert_eq(0 <= cnt <= 1000, True, f"Count[{r}]={cnt} within 0..1000")
            scale = tf.query(f"/external/value.{r}.3")
            assert_eq(0.0 <= scale <= 10.0, True, f"Scale[{r}]={scale} within 0..10")
        # The bounds themselves are valid (inclusive).
        tf.request("scene/intervene", {"path": "/external/value.3.3", "value": 10.0})
        assert_eq(tf.query("/external/value.3.3"), 10.0, "the upper bound is inclusive")
        tf.request("scene/intervene", {"path": "/external/value.3.3", "value": 0.0})
        assert_eq(tf.query("/external/value.3.3"), 0.0, "the lower bound is inclusive")
        tf.request("scene/intervene", {"path": "/external/value.3.2", "value": 0})
        assert_eq(tf.query("/external/value.3.2"), 0, "Count lower bound inclusive")


if __name__ == "__main__":
    sys.exit(run_demo("R894 §5.40 — editable data-grid column validation", body))
