#!/usr/bin/env python3
"""R982 §5.40 §2 #7 — AT-reachable data-grid ROW reset via a rowheader.

R980 made the data grid's CELL + COLUMN reset AT-reachable (buttons hung off the
gridcell / columnheader), but left the ROW reset paint-only: its dot lives in the
handle gutter, and a `button` is not a valid child of a bare grid `row`
(R937.1) — so there was no WAI-ARIA host. That was a one-gate asymmetry (the row
dot painted with no a11y peer), flagged in the R981 session review.

R982 closes it: each data row gains a `rowheader` cell (the gutter slot, its
natural row-header home, named "Row N") as its FIRST child, and a modified row
hosts its reset `button` there — the WAI-ARIA-valid host the cell + column reset
already had. The reset Click routes through the existing `access_child_invoke`
reset-prefix branch (`resetrow<row>` → the send funnel, the pointer twin), so no
new routing was needed — only the emission.

Verified over the R979 `scene/access` surface:
  (A) every data row leads with a named `rowheader` cell;
  (B) a modified row's rowheader hosts a reset `button` (the boot grid is fully
      modified, so every row shows one);
  (C) an AT Click (its send-wire twin) on the row reset clears the whole row and
      the button leaves the tree — while the rowheader cell persists (structural).

Run from the workspace root:
    cargo build -p hello-data-grid --release
    python3 tools/demos/r982_data_grid_row_reset.py

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
NROWS = 4


def access(tf):
    return tf.request("scene/access").result



def body() -> None:
    with RpcSubprocess(EXAMPLE, request_timeout=12.0) as tf:
        wait_snap(
            tf,
            lambda s: find_by_tag(s, f"{GRID}#0_0") is not None,
            viewport=VIEWPORT,
            desc="grid painted",
        )
        for row in range(NROWS):
            assert_eq(tf.query(f"{EXT}/row_modified.{row}"), True, f"row {row} boots modified")

        acc = access(tf)

        # ── (A) every data row leads with a named rowheader cell ─────
        rowheaders = [n for n in acc["nodes"] if n.get("role") == "rowheader"]
        assert_eq(len(rowheaders), NROWS, "each data row has a rowheader cell")
        names = sorted(h["name"] for h in rowheaders)
        assert_eq(names, [f"Row {i + 1}" for i in range(NROWS)], "rowheaders are named Row 1..N")
        # the header row is NOT given a rowheader (it keeps its columnheaders)
        data_rows = [n for n in acc["nodes"] if n.get("role") == "row" and n.get("tag") != "dg_header"]
        assert_eq(len(data_rows), NROWS, "there are NROWS data rows")
        for r in data_rows:
            first = r["children"][0]
            fh = access_node_by_tag(acc, first)
            assert fh is not None and fh["role"] == "rowheader", \
                f"data row {r['tag']} leads with a rowheader (got {first})"

        # ── (B) a modified row's rowheader hosts a reset button ──────
        rh1 = next(h for h in rowheaders if h["name"] == "Row 1")
        reset_tag = next((c for c in rh1.get("children", []) if "#resetrow" in c), None)
        assert reset_tag is not None, "the modified row's rowheader hosts a reset button child"
        reset_btn = access_node_by_tag(acc, reset_tag)
        assert_eq(reset_btn["role"], "button", "the row reset affordance is a button (AT-reachable)")
        assert_eq(reset_btn["name"], "Reset row 1 to default", "the row reset button is named")
        # the button is NOT a direct child of the row (it hangs off the rowheader)
        owning_row = next(r for r in data_rows if rh1["tag"] in r["children"])
        assert reset_tag not in owning_row["children"], "the reset button hangs off the rowheader, not the row"

        # ── (C) an AT Click on the row reset clears the whole row ────
        sub = reset_tag.split("#", 1)[1]            # "resetrow<src>"
        src = int(sub.removeprefix("resetrow"))     # the source row index
        assert_eq(tf.query(f"{EXT}/row_modified.{src}"), True, "the addressed row is modified")
        tf.invoke(f"{EXT}/send", f"{sub}:PointerUp")
        wait_until(lambda: tf.query(f"{EXT}/row_modified.{src}") is False,
                   timeout=4.0, interval=0.03, desc="the AT row reset cleared the whole row")
        acc = access(tf)
        assert access_node_by_tag(acc, reset_tag) is None, "the row reset button leaves the tree once the row is default"
        # the rowheader cell itself persists (structural, not modified-gated)
        assert access_node_by_tag(acc, rh1["tag"]) is not None, "the rowheader cell persists after reset"
        assert access_node_by_tag(acc, rh1["tag"])["role"] == "rowheader", "still a rowheader"
        # the other rows keep their reset buttons (only the addressed row cleared)
        other_resets = [n for n in acc["nodes"]
                        if n.get("role") == "button" and "#resetrow" in n.get("tag", "")]
        assert_eq(len(other_resets), NROWS - 1, "the remaining modified rows keep their reset buttons")


if __name__ == "__main__":
    sys.exit(run_demo("R982 §5.40 — AT-reachable data-grid row reset via rowheader", body))
