#!/usr/bin/env python3
"""R960 §5.38 §5.40 — per-cell modified-from-default + reset on the data grid.

Drives hello-data-grid over JSON-RPC. The editable grid (R837) gains the
Unreal / Qt "reset property to default" affordance at 2-D cell granularity:

  (A) **modified indicator**: a cell that differs from its COLUMN DEFAULT
      (`col_default(col)` — the value a fresh row gets) paints a trailing-edge
      accent dot tagged `data_grid#reset<row>_<col>`. A cell sitting at its
      column default has no dot.
  (B) **click the dot -> reset that cell** to its column default (the value
      collapses, the dot vanishes).
  (C) **RPC peers** (AI-first): `query modified.<row>.<col>` / `modified_count`,
      `invoke reset "<row>_<col>"` / `reset_all`.
  (D) **edit tracks the value**: editing a cell TO its column default clears the
      indicator with no reset; editing away sets it again (no separate dirty bit).

The Asset column (col 0) is the only one fully inside the h-scroll viewport at
boot, so the *pointer* click test uses it; the right columns' dots paint but are
clip-gated (scroll to reveal), so they are exercised through the RPC reset peer.

The reset is the 3rd `reset`-send consumer (hello-property-grid + hello-inspector
are the prior two), but the three diverge in key arity (this is 2-D `<row>_<col>`),
arrow node, and positioning — so only the `value_eq` modified atom is shared
(the decode + paint stay per-binding; the lift the round hypothesised was
falsified by the entry audit).

Run from the workspace root:
    cargo build -p hello-data-grid --release
    python3 tools/demos/r960_data_grid_cell_reset.py

>= 25 assertions.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_query,
    wait_snap,
)

EXAMPLE = "hello-data-grid"
GRID = "data_grid"
EXT = "/external"
VIEWPORT = (460, 348)  # the example's live window (so snapshot == hit-test scene)

ASSET_COL = 0  # Text, column default "" — fully inside the h-scroll viewport
TYPE_COL = 1  # Choice, default = option 0; the seed row-0 cell sits AT default
SCALE_COL = 3  # Float, default 0.0 — off-screen at boot, exercised via RPC


def reset_tag(row: int, col: int) -> str:
    """The cell's reset-dot click target — `data_grid#reset<row>_<col>`."""
    return f"{GRID}#reset{row}_{col}"


def dot_present(tf: RpcSubprocess, row: int, col: int) -> bool:
    snap = tf.snapshot(source="paint", viewport=VIEWPORT)
    return find_by_tag(snap, reset_tag(row, col)) is not None


def body() -> None:
    with RpcSubprocess(EXAMPLE, request_timeout=12.0) as tf:
        wait_snap(
            tf,
            lambda s: find_by_tag(s, f"{GRID}#0_{ASSET_COL}") is not None,
            viewport=VIEWPORT,
            desc="grid painted",
        )

        # ── (A) the modified dot paints on a non-default cell, not a default one
        # Row-0 Asset is the seed "Hero" (column default "") -> modified.
        assert_eq(tf.query(f"{EXT}/value.0.{ASSET_COL}"), "Hero", "seed row-0 Asset is Hero")
        assert_eq(tf.query(f"{EXT}/modified.0.{ASSET_COL}"), True, "Asset differs from default \"\"")
        assert dot_present(tf, 0, ASSET_COL), "a modified cell paints its reset dot"
        # Row-0 Type is the seed's option 0 == the column default -> NOT modified.
        assert_eq(tf.query(f"{EXT}/modified.0.{TYPE_COL}"), False, "row-0 Type sits at its column default")
        assert not dot_present(tf, 0, TYPE_COL), "a cell at its column default has no reset dot"

        # ── (B) click the reset dot -> the cell collapses to its column default
        tf.click(path=reset_tag(0, ASSET_COL))
        wait_query(tf, f"{EXT}/value.0.{ASSET_COL}", "", desc="dot click reset Asset to default \"\"")
        wait_query(tf, f"{EXT}/modified.0.{ASSET_COL}", False, desc="the cell is no longer modified")
        wait_snap(
            tf,
            lambda s: find_by_tag(s, reset_tag(0, ASSET_COL)) is None,
            viewport=VIEWPORT,
            desc="the reset dot vanished once the cell is at default",
        )

        # ── (C) RPC reset peer: reset an off-screen Float cell, then reset_all
        assert_eq(tf.query(f"{EXT}/value.0.{SCALE_COL}"), 1.0, "seed row-0 Scale is 1.0")
        assert_eq(tf.query(f"{EXT}/modified.0.{SCALE_COL}"), True, "Scale differs from default 0.0")
        assert_eq(tf.invoke(f"{EXT}/reset", f"0_{SCALE_COL}"), True, "RPC reset returns was-modified=true")
        wait_query(tf, f"{EXT}/value.0.{SCALE_COL}", 0.0, desc="RPC reset Scale to column default 0.0")
        assert_eq(tf.query(f"{EXT}/modified.0.{SCALE_COL}"), False, "Scale no longer modified")
        # A re-reset of an already-default cell is an idempotent false no-op.
        assert_eq(tf.invoke(f"{EXT}/reset", f"0_{SCALE_COL}"), False, "re-reset is a false no-op")

        # ── (D) editing a cell TO its column default clears the indicator
        assert_eq(tf.query(f"{EXT}/modified.1.{ASSET_COL}"), True, "row-1 Asset (Tree) is modified")
        tf.intervene(f"{EXT}/value.1.{ASSET_COL}", "")
        wait_query(tf, f"{EXT}/modified.1.{ASSET_COL}", False, desc="editing to the default clears modified")
        assert not dot_present(tf, 1, ASSET_COL), "no dot once the edit lands on the default"
        tf.intervene(f"{EXT}/value.1.{ASSET_COL}", "Box")
        wait_query(tf, f"{EXT}/modified.1.{ASSET_COL}", True, desc="editing away from default sets it again")
        assert dot_present(tf, 1, ASSET_COL), "the dot returns when the cell leaves the default"

        # ── (C) reset_all clears every remaining modified cell
        n = tf.query(f"{EXT}/modified_count")
        assert isinstance(n, int) and n > 0, f"there are modified cells to clear (got {n})"
        assert_eq(tf.invoke(f"{EXT}/reset_all", None), n, "reset_all returns the count it cleared")
        wait_query(tf, f"{EXT}/modified_count", 0, desc="every cell now sits at its column default")
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        for row in range(int(tf.query(f"{EXT}/row_count"))):
            for col in range(int(tf.query(f"{EXT}/col_count"))):
                assert find_by_tag(snap, reset_tag(row, col)) is None, (
                    f"no reset dot remains at ({row},{col}) after reset_all"
                )


if __name__ == "__main__":
    sys.exit(run_demo("R960 §5.38 §5.40 — data-grid per-cell modified / reset", body))
