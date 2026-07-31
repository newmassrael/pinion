#!/usr/bin/env python3
"""R980 §5.40 §2 #7 — AT-reachable data-grid reset, verified over `scene/access`.

R966 landed the data grid's visible reset dots (cell / row / column) but its own
honesty note (bullet D) recorded that reset was POINTER + RPC only — there was
no focusable reset AccessNode for a screen reader to target, so AT-reachable
reset was a documented follow-up. R979 opened `scene/access` (the a11y tree as
data); R980 uses it to close that carry for the data grid:

  (A) each modified CELL gains a reset `button` child of its `gridcell`
      (`data_grid#reset<r>_<c>`, named "Reset <Column> to default");
  (B) each modified COLUMN gains a reset `button` child of its `columnheader`
      (`data_grid#resetcol<c>`, named "Reset <Column> column to default");
  (C) `access_child_invoke` routes an AT Click on a `reset…` child through the
      SAME `send` wire a reset-dot pointer click drains — so this demo activates
      reset through that wire twin (`send "<sub>:PointerUp"`) and watches the
      button vanish from the access tree once the cell / column is default.

The row reset went AT-reachable in R982 (a `rowheader` host — see
tools/demos/r982_data_grid_row_reset.py); the grouped treegrid path is still an
honest carry. The button shape (find host +
push child + emit named button) is the lifted `pinion_a11y::attach_child_button`
SSOT shared with hello-property-grid.

Run from the workspace root:
    cargo build -p hello-data-grid --release
    python3 tools/demos/r980_access_reset.py

>= 30 assertions.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    access_node_by_tag,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_snap,
    wait_until,
)

EXAMPLE = "hello-data-grid"
GRID = "data_grid"
EXT = "/external"
VIEWPORT = (460, 348)
ASSET_COL = 0  # Text column, leftmost — its header + cells sit in the viewport


def access(tf):
    return tf.request("scene/access").result



def reset_child_of(node):
    """The single `reset…` child tag of an access node, or None."""
    for c in node.get("children", []):
        if f"{GRID}#reset" in c:
            return c
    return None


def body() -> None:
    with RpcSubprocess(EXAMPLE, request_timeout=12.0) as tf:
        # Boot: every seed cell differs from its column default, so every cell +
        # column boots modified and every reset button is emitted.
        wait_snap(
            tf,
            lambda s: find_by_tag(s, f"{GRID}#0_{ASSET_COL}") is not None,
            viewport=VIEWPORT,
            desc="grid painted",
        )
        assert_eq(tf.query(f"{EXT}/col_modified.{ASSET_COL}"), True, "Asset column boots modified")

        acc = access(tf)
        assert acc["count"] > 0, "the access tree is non-empty"

        # ── (A) the modified cell (0, Asset) advertises a reset button child ──
        cell = access_node_by_tag(acc, f"{GRID}#0_{ASSET_COL}")
        assert cell is not None, "the (0, Asset) gridcell is in the access tree"
        assert_eq(cell["role"], "gridcell", "it is a gridcell")
        cell_reset_tag = reset_child_of(cell)
        assert cell_reset_tag is not None, "the modified cell advertises a reset button child"
        cell_btn = access_node_by_tag(acc, cell_reset_tag)
        assert cell_btn is not None, "the reset button node is in the tree"
        assert_eq(cell_btn["role"], "button", "the reset affordance is a button (AT-reachable)")
        assert_eq(cell_btn["name"], "Reset Asset to default", "the cell reset button is named")

        # ── (B) the modified column advertises a reset button child of its header
        headers = [n for n in acc["nodes"] if n.get("role") == "columnheader"]
        assert len(headers) >= 6, "every column has a columnheader node"
        asset_header = next((h for h in headers if reset_child_of(h)), None)
        assert asset_header is not None, "a modified column's header carries a reset button child"
        col_reset_tag = reset_child_of(asset_header)
        col_btn = access_node_by_tag(acc, col_reset_tag)
        assert_eq(col_btn["role"], "button", "the column reset is a button")
        assert col_btn["name"].startswith("Reset ") and col_btn["name"].endswith(" column to default"), \
            f"the column reset button is named for its column (got {col_btn['name']!r})"

        # Many reset buttons exist at boot (all cells + columns modified).
        reset_buttons = [n for n in acc["nodes"]
                         if n.get("role") == "button" and f"{GRID}#reset" in n.get("tag", "")]
        assert len(reset_buttons) >= 7, f"a fully-modified grid advertises many reset buttons (got {len(reset_buttons)})"

        # ── (C) activation through the AT wire twin: an AT Click routes to the
        # same `send "<sub>:PointerUp"` wire. Resetting the (0,Asset) cell to its
        # column default removes the button from the access tree.
        assert_eq(tf.query(f"{EXT}/value.0.{ASSET_COL}"), "Hero", "seed (0,Asset) is Hero")
        cell_sub = cell_reset_tag.split("#", 1)[1]  # e.g. "reset0_0"
        tf.invoke(f"{EXT}/send", f"{cell_sub}:PointerUp")
        assert_eq(tf.query(f"{EXT}/value.0.{ASSET_COL}"), "", "the AT-wire reset cleared the cell to its default")
        wait_until(
            lambda: access_node_by_tag(access(tf), cell_reset_tag) is None,
            timeout=4.0, interval=0.03,
            desc="the cell reset button leaves the access tree once the cell is default",
        )
        # The (0,Asset) gridcell no longer lists a reset child.
        acc = access(tf)
        assert reset_child_of(access_node_by_tag(acc, f"{GRID}#0_{ASSET_COL}")) is None, \
            "the cleaned cell advertises no reset child"
        # ...but its column is still modified (other rows), so the header keeps its button.
        assert_eq(tf.query(f"{EXT}/col_modified.{ASSET_COL}"), True, "the Asset column is still modified")
        assert reset_child_of(access_node_by_tag(acc, asset_header["tag"])) is not None, \
            "the column header still advertises its reset button"

        # ── (C) reset the whole Asset column via its header button's AT wire ──
        col_sub = col_reset_tag.split("#", 1)[1]  # "resetcol0"
        tf.invoke(f"{EXT}/send", f"{col_sub}:PointerUp")
        wait_until(
            lambda: tf.query(f"{EXT}/col_modified.{ASSET_COL}") is False,
            timeout=4.0, interval=0.03,
            desc="the AT-wire column reset cleared the whole Asset column",
        )
        for row in range(4):
            assert_eq(tf.query(f"{EXT}/value.{row}.{ASSET_COL}"), "", f"Asset row {row} is the column default")
        acc = access(tf)
        assert access_node_by_tag(acc, col_reset_tag) is None, "the column reset button is gone once the column is clean"
        assert reset_child_of(access_node_by_tag(acc, f"{GRID}#0_{ASSET_COL}")) is None, \
            "no cell reset child remains in the cleaned column"

        # ── out-of-range gridcell tag resolves to no node (graceful) ──
        assert access_node_by_tag(acc, f"{GRID}#99_99") is None, "an out-of-range cell tag has no node"


if __name__ == "__main__":
    sys.exit(run_demo("R980 §5.40 — AT-reachable data-grid reset over scene/access", body))
