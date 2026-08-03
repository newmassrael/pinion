#!/usr/bin/env python3
"""R707 hello-table demo — interactive data grid (WAI-ARIA grid).

Drives the live `hello-table` window over JSON-RPC 2.0 — the AI-first
path (§2 #2) — to verify the data table's cell content, single-row
selection, 2-D keyboard roving (APG data grid), Enter activation, and the
scene/invoke send wire entirely through RPC. No display, no screenshot:
every assertion is a typed `scene/query` / `scene/click` / `scene/key`
round-trip plus `scene/snapshot` (paint source) for structural shape.

The table holds ONE `TableExternal` at the composite root "table" (the
state-scene ROOT external), so its introspect slots address as
`/external/<slot>` (R666 §5.34 root-external path). Cells are composite
tags "table#<row>_<col>"; the R51.42 `'#'`-split routes a click on cell
(r, c) to the coordinator's `"<r>_<c>:<EventName>"` send.

a11y note: pinion has no `access/node` RPC method (the AccessKit tree is
emitted to the platform AT, not the §5.12 JSON-RPC surface). The grid /
row / gridcell / columnheader role contributions — including the first
use of the `row` role — are pinned by the Rust unit tests in
`examples/hello-table/src/main.rs` (access_node tests). This demo asserts
the a11y-relevant scene shape indirectly: the grid root, header-row,
columnheader, data-row, and cell paint tags are all present in the paint
snapshot, which is the substrate the `access_node` walker stamps.

Run from the workspace root:
    cargo build -p hello-table --release
    python3 tools/demos/r707_table.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    assert_eq,
    find_by_tag,
    isolated_storage_dir,
    run_demo,
)

T = "table"
VIEWPORT = (600, 360)


def _q(d, slot: str):
    return d.query(f"/external/{slot}")


def _present(d, tag: str) -> bool:
    snap = d.snapshot(source="paint", viewport=VIEWPORT)
    return find_by_tag(snap, tag) is not None


def _focused(d):
    return d.request("focus/get").result.get("focused")


def _focus_set(d, tag: str):
    return d.request("focus/set", {"tag": tag}).result.get("focused")


def body() -> None:
    # Isolate any persistence side effects (mirror the r666 harness
    # convention; the table itself does not persist).
    with isolated_storage_dir("r707-table"):
        with RpcSubprocess("hello-table") as d:
            # ── 1. initial state: 6x4 grid, no selection / focus ────
            assert_eq(_q(d, "rows"), 6, "6 data rows")
            assert_eq(_q(d, "cols"), 4, "4 columns")
            assert_eq(_q(d, "selected"), False, "no initial selection")
            assert_eq(_q(d, "selected_row"), -1, "selected_row == -1 when none")
            assert_eq(_q(d, "focused_row"), -1, "focused_row == -1 before nav")
            assert_eq(_q(d, "focused_col"), 0, "focused_col defaults to 0")

            # ── 2. cell content via introspect (AI reads the data) ──
            assert_eq(_q(d, "header.0"), "Widget", "column 0 header")
            assert_eq(_q(d, "header.3"), "Role", "column 3 header")
            assert_eq(_q(d, "cell.0.0"), "Tabs", "row 0 col 0 cell")
            assert_eq(_q(d, "cell.5.0"), "Table", "row 5 col 0 cell")
            assert_eq(_q(d, "cell.5.3"), "grid", "row 5 col 3 cell")

            # ── 3. paint-tag shape (the a11y walker's substrate) ────
            assert _present(d, T), "grid root tag present"
            assert _present(d, f"{T}_hrow"), "header row strip present"
            for col in range(4):
                assert _present(d, f"{T}_ch{col}"), f"columnheader {col} present"
            for row in range(6):
                assert _present(d, f"{T}_row{row}"), f"data row {row} present"
            assert _present(d, f"{T}#0_0"), "cell 0_0 present"
            assert _present(d, f"{T}#5_3"), "cell 5_3 present"
            assert not _present(d, f"{T}_row6"), "no phantom row 6"
            assert not _present(d, f"{T}#0_4"), "no phantom col 4"

            # ── 4. click cell (2,1) → row 2 selected exclusively ────
            d.click(path=f"{T}#2_1")
            assert_eq(_q(d, "selected"), True, "selected flag true after click")
            assert_eq(_q(d, "selected_row"), 2, "row 2 selected")
            assert_eq(_q(d, "selected.2"), True, "selected.2 true")
            assert_eq(_q(d, "selected.1"), False, "selected.1 false")
            # WAI-ARIA "activation moves focus": the active descendant
            # syncs to the clicked cell.
            assert_eq(_q(d, "focused_row"), 2, "active descendant row syncs to click")
            assert_eq(_q(d, "focused_col"), 1, "active descendant col syncs to click")

            # ── 5. switch rows: click cell (4,0) ────────────────────
            d.click(path=f"{T}#4_0")
            assert_eq(_q(d, "selected_row"), 4, "row 4 selected")
            assert_eq(_q(d, "selected.2"), False, "row 2 deselected (exclusion)")
            assert_eq(_q(d, "focused_row"), 4, "active descendant follows to row 4")
            assert_eq(_q(d, "focused_col"), 0, "active descendant col 0")
            # Click a different cell in the SAME (already-selected) row:
            # selection is unchanged but the cursor must follow the click
            # (WAI-ARIA "clicking a cell moves focus to it").
            d.click(path=f"{T}#4_2")
            assert_eq(_q(d, "selected_row"), 4, "selection unchanged on same-row click")
            assert_eq(_q(d, "focused_col"), 2, "cursor follows click within selected row")
            # Restore the cursor to (4,0) for the keyboard-roving section.
            d.click(path=f"{T}#4_0")
            assert_eq(_q(d, "focused_col"), 0, "cursor back to col 0")

            # ── 6. 2-D keyboard roving (single Tab stop + roving) ───
            assert_eq(_focus_set(d, T), T, "focus set on grid root")
            d.key(path=T, name="ArrowUp")
            assert_eq(_q(d, "focused_row"), 3, "ArrowUp -> row 3")
            d.key(path=T, name="ArrowUp")
            assert_eq(_q(d, "focused_row"), 2, "ArrowUp -> row 2")
            d.key(path=T, name="ArrowRight")
            assert_eq(_q(d, "focused_col"), 1, "ArrowRight -> col 1")
            d.key(path=T, name="ArrowRight")
            assert_eq(_q(d, "focused_col"), 2, "ArrowRight -> col 2")
            d.key(path=T, name="ArrowLeft")
            assert_eq(_q(d, "focused_col"), 1, "ArrowLeft -> col 1")
            d.key(path=T, name="Home")
            assert_eq(_q(d, "focused_col"), 0, "Home -> first column")
            d.key(path=T, name="End")
            assert_eq(_q(d, "focused_col"), 3, "End -> last column")
            d.key(path=T, name="PageUp")
            assert_eq(_q(d, "focused_row"), 0, "PageUp -> first row")
            d.key(path=T, name="PageDown")
            assert_eq(_q(d, "focused_row"), 5, "PageDown -> last row")
            # ArrowDown on the last row clamps within the dataset.
            d.key(path=T, name="ArrowDown")
            assert_eq(_q(d, "focused_row"), 5, "ArrowDown clamps at last row")
            # Shell focus never left the grid root through all the roving.
            assert_eq(_focused(d), T, "shell focus stays on the grid root")

            # ── 7. Enter activates the active-descendant row ────────
            # Active descendant is now (5, 3) (last row from PageDown,
            # last col from End). Enter selects row 5.
            d.key(path=T, name="Enter")
            assert_eq(_q(d, "selected_row"), 5, "Enter selects active row 5")

            # ── 8. scene/invoke send wire form (the click funnel) ───
            # Selecting cell (1,0) through the introspect send wire
            # returns the new selected row, mirroring the composite click.
            for ev in ("PointerEnter", "PointerDown", "PointerUp"):
                out = d.invoke("/external/send", f"1_0:{ev}")
            assert_eq(out, 1, "send 1_0:PointerUp activates -> selected row 1")
            assert_eq(_q(d, "selected_row"), 1, "introspect send selected row 1")

            # ── 9. scene/simulate hypothetical with rollback ────────
            # R646 §5.34 — steps are `{path, value}` slot writes applied
            # against a snapshot, captured after the final step, then
            # rolled back. The response result IS the final-projection
            # snapshot node directly. The table's writable slots are the
            # roving active descendant `focused_row` / `focused_col`:
            # preview moving the cursor to (0, 2); the live cursor must be
            # untouched afterwards.
            before_row = _q(d, "focused_row")
            before_col = _q(d, "focused_col")
            steps = [
                {"path": "/external/focused_row", "value": 0},
                {"path": "/external/focused_col", "value": 2},
            ]
            resp = d.request("scene/simulate", {"steps": steps})
            assert resp is not None, "simulate returned a response"
            final = resp.result
            assert isinstance(final, dict) and final.get("type"), \
                "simulate returns the final-projection snapshot node"
            assert_eq(_q(d, "focused_row"), before_row,
                      "live active descendant row untouched after simulate")
            assert_eq(_q(d, "focused_col"), before_col,
                      "live active descendant col untouched after simulate")

            # ── 10. negatives: bad send / unknown slot reject cleanly
            raised = False
            try:
                d.invoke("/external/send", "9_9:PointerUp")
            except RpcError:
                raised = True
            assert raised, "out-of-range cell index must be rejected"
            raised = False
            try:
                _q(d, "no_such_slot")
            except RpcError:
                raised = True
            assert raised, "unknown introspect slot must raise, not silently pass"


if __name__ == "__main__":
    sys.exit(run_demo("R707 interactive data Table (WAI-ARIA grid)", body))
