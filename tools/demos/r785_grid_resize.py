#!/usr/bin/env python3
"""R785 §5.27 — per-column width model + AI-first column resize E2E.

Drives the `hello-grid-hscroll` binding (now column-resizable) via JSON-RPC.
First consumer of the R785 `ColumnWidths` reactive holder + `ColumnWidthExternal`:
each column has its own width (seeded uniform 130), and an AI agent resizes a
column with `invoke set_col_width "<col>=<width>"`. Widening a column past the
window grows the R784 horizontal-scroll extent — the two axes compose.

The decisive witness is scene-as-data (§2 #7), no pixels required:

  (A) width model at boot — the column-width external reports cols=8, the
      seeded uniform widths, total = NCOLS*COL_W, and the per-column query;
      each data cell's rendered width matches the model.
  (B) AI-first resize — `invoke set_col_width "0=300"` returns the applied
      width; the model (widths / total / width.0) updates; the rendered col-0
      cell widens to 300 and col-1 shifts right by the delta; the horizontal
      scroll's max_x grows by the same delta (resize ∘ h-scroll compose).
  (C) clamp + restore — a sub-min width clamps up to min_width; an
      out-of-range column is a no-op; `intervene widths` restores the whole
      vector.
"""

from __future__ import annotations

import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_until,
)

EXAMPLE = "hello-grid-hscroll"
WIN = (520, 440)
NCOLS = 8
COL_W = 130
ROW_H = 36
TABLE_TAG = "ghs"
H_SCROLL_TAG = "ghs_hscroll"
COLS_TAG = "ghs_cols"
PAUSE = 0.12


def col_path(slot: str) -> str:
    return f"/{COLS_TAG}/external/{slot}"


def cell_w(rects, tag: str) -> int:
    assert tag in rects, f"{tag} present"
    return rects[tag][2]


def cell_x(rects, tag: str) -> int:
    assert tag in rects, f"{tag} present"
    return rects[tag][0]


def hscroll_max_x(tf) -> int:
    """Scroll the outer horizontal scroll to the clamp and read offset_x =
    total_w - viewport_w (R784): the live h-scroll extent."""
    tf.scroll(H_SCROLL_TAG, to=(10 ** 9, 0))
    time.sleep(PAUSE)
    snap = tf.snapshot(source="paint", viewport=WIN)
    node = find_by_tag(snap, H_SCROLL_TAG)
    assert node is not None, "horizontal scroll present"
    offset = int(node.get("offset_x", -1))
    tf.scroll(H_SCROLL_TAG, to=(0, 0))  # reset
    time.sleep(PAUSE)
    return offset


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:

        def snap_now():
            return tf.snapshot(source="paint", viewport=WIN)

        # ── (A) width model at boot ─────────────────────────────────
        assert_eq(tf.query(col_path("cols")), NCOLS, "model knows the column count")
        assert_eq(tf.query(col_path("widths")), ",".join([str(COL_W)] * NCOLS),
                  "seeded uniform widths")
        assert_eq(tf.query(col_path("total")), NCOLS * COL_W, "total = sum of widths")
        assert_eq(tf.query(col_path("width.0")), COL_W, "per-column width query")
        assert_eq(tf.query(col_path("width.7")), COL_W, "last column width query")
        min_w = tf.query(col_path("min_width"))
        assert isinstance(min_w, int) and min_w > 0, f"min_width is a positive int, got {min_w}"

        rects = abs_rects_of(snap_now())
        # Every column renders at the seeded uniform width at boot.
        for c in range(NCOLS):
            assert_eq(cell_w(rects, f"ghs#0_{c}"), COL_W, f"col {c} cell at the seeded width")
        x1_boot = cell_x(rects, "ghs#0_1")
        x0_boot = cell_x(rects, "ghs#0_0")
        assert_eq(x1_boot - x0_boot, COL_W, "col-1 starts one column-width right of col-0")

        max_x_boot = hscroll_max_x(tf)
        assert max_x_boot > 0, f"columns overflow => positive h-scroll extent, got {max_x_boot}"

        # ── (B) AI-first resize: widen column 0 to 300 ──────────────
        applied = tf.invoke(col_path("set_col_width"), "0=300")
        assert_eq(applied, 300, "set_col_width returns the applied width in one round-trip")
        assert_eq(tf.query(col_path("width.0")), 300, "model width.0 updated")
        assert_eq(tf.query(col_path("total")), NCOLS * COL_W + (300 - COL_W),
                  "total grew by the width delta")
        widths_csv = tf.query(col_path("widths"))
        assert widths_csv.startswith("300,"), f"widths reflect the resize, got {widths_csv!r}"
        assert_eq(tf.query(col_path("width.7")), COL_W, "other columns unchanged by the resize")

        # The rendered grid tracks the model: col-0 widens, col-1 shifts right.
        def col0_widened():
            r = abs_rects_of(snap_now())
            return r if cell_w(r, "ghs#0_0") == 300 else None

        rects2 = wait_until(col0_widened, desc="col-0 cell repaints at width 300")
        assert_eq(cell_w(rects2, "ghs#0_0"), 300, "col-0 cell widened to 300")
        delta = 300 - COL_W
        assert_eq(cell_x(rects2, "ghs#0_1"), x1_boot + delta,
                  "col-1 shifted right by the resize delta")
        assert_eq(cell_w(rects2, "ghs#0_1"), COL_W, "col-1 keeps its own width (unchanged)")

        # resize ∘ h-scroll compose: the h-scroll extent grew by the delta.
        max_x_wide = hscroll_max_x(tf)
        assert_eq(max_x_wide, max_x_boot + delta,
                  "widening a column grew the horizontal scroll extent by the delta")

        # ── (C) clamp + out-of-range + restore ──────────────────────
        clamped = tf.invoke(col_path("set_col_width"), "1=5")
        assert_eq(clamped, min_w, "sub-min width clamps up to min_width")
        assert_eq(tf.query(col_path("width.1")), min_w, "model reflects the clamp")

        noop = tf.invoke(col_path("set_col_width"), "99=200")
        assert_eq(noop, 0, "out-of-range column resize is a no-op (returns 0)")
        assert_eq(tf.query(col_path("cols")), NCOLS, "no phantom column appended")

        tf.intervene(col_path("widths"), "100,100,100,100,100,100,100,100")
        assert_eq(tf.query(col_path("total")), 800, "intervene widths restored the whole vector")
        assert_eq(tf.query(col_path("width.0")), 100, "restored width.0")
        assert_eq(tf.query(col_path("cols")), NCOLS, "restore kept the column count")

        def col0_restored():
            r = abs_rects_of(snap_now())
            return r if cell_w(r, "ghs#0_0") == 100 else None

        rects3 = wait_until(col0_restored, desc="grid repaints after widths restore")
        assert_eq(cell_w(rects3, "ghs#0_0"), 100, "rendered col-0 width follows the restore")
        # All eight columns now render at the uniform restored width.
        for c in range(NCOLS):
            assert_eq(cell_w(rects3, f"ghs#0_{c}"), 100, f"col {c} at restored width")


if __name__ == "__main__":
    sys.exit(run_demo("R785 §5.27 — per-column width model + column resize", body))
