#!/usr/bin/env python3
"""R983 §5.40 §2 #7 — AT-reachable data-grid reset in the GROUPED treegrid.

R980/R982 made the FLAT grid's cell / column / row reset AT-reachable: a reset
`button` hung off the gridcell / columnheader / rowheader, verified over
`scene/access`. The GROUPED treegrid path emitted none of them — a sighted user
saw + clicked the SAME painted reset dots, but an AT user got no reset under
grouping. That was the exact asymmetry R982 closed for the flat grid, one grid
mode over (flagged as the R982 carry).

R983 closes it: the same `emit_reset_affordances` SSOT runs over the grouped
treegrid's VISIBLE data sources. Group headers hold no cells, and a collapsed
group's members are already windowed out of the flatten, so no reset button
orphans onto an absent node. The reset routing was already mode-independent
(the `access_child_invoke` reset-prefix branch -> the `send` funnel), so only the
EMISSION was added — cell / column / row reset are now AT-reachable in BOTH
grid modes.

Verified over the R979 `scene/access` surface, grouped by Type:
  (A) the grid is a `treegrid`; each visible data row leads with a `rowheader`;
  (B) a modified cell / column / row each host a named reset `button`, each hung
      off exactly one host node present in the tree (no orphan);
  (C) collapsing a group removes its rows' cell + row resets with NO orphan node,
      while a column reset persists (its predicate is model-wide, not
      window-scoped); expanding restores them;
  (D) an AT Click (the send-wire twin) on a cell / row / column reset clears the
      target, and the cleared affordance leaves the tree.

Run from the workspace root:
    cargo build -p hello-data-grid --release
    python3 tools/demos/r983_grouped_grid_reset.py

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
    wait_query,
    wait_snap,
    wait_until,
)

EXAMPLE = "hello-data-grid"
GRID = "data_grid"
EXT = "/external"
VIEWPORT = (460, 348)
# Group by Type (col 1): group 0 = sprite (sources 0, 2), group 1 = mesh (1, 3).
SPRITE_SOURCES = (0, 2)


def access(tf) -> dict:
    return tf.request("scene/access").result



def buttons(result, predicate):
    return [
        n
        for n in result["nodes"]
        if n.get("role") == "button" and predicate(n.get("tag", ""))
    ]


def sub_of(tag: str) -> str:
    return tag.split("#", 1)[1]


def is_cell_reset(tag: str) -> bool:
    # A cell reset sub is `reset<row>_<col>`; `resetcol*` / `resetrow*` are not.
    if "#reset" not in tag:
        return False
    rest = sub_of(tag).removeprefix("reset")
    return rest[:1].isdigit()


def cell_of(tag: str) -> tuple[int, int]:
    row, col = sub_of(tag).removeprefix("reset").split("_")
    return int(row), int(col)


def body() -> None:
    with RpcSubprocess(EXAMPLE, request_timeout=12.0) as tf:
        wait_snap(
            tf,
            lambda s: find_by_tag(s, f"{GRID}#0_0") is not None,
            viewport=VIEWPORT,
            desc="grid painted",
        )

        # Group by Type (col 1): the boot grid is fully modified.
        assert_eq(tf.invoke(f"{EXT}/set_group", "1"), 2, "grouped by Type -> 2 groups")
        wait_query(tf, f"{EXT}/group", "1", desc="grouped by Type")
        assert_eq(tf.query(f"{EXT}/visible_len"), 6, "2 headers + 4 data rows")

        acc = access(tf)

        # ── (A) treegrid + a named rowheader per visible data row ────────
        root = access_node_by_tag(acc, GRID)
        assert root is not None, "the grid root is in the access tree"
        assert_eq(root["role"], "treegrid", "the grouped grid is a treegrid")
        rowheaders = [n for n in acc["nodes"] if n.get("role") == "rowheader"]
        assert_eq(len(rowheaders), 4, "a rowheader per visible data row")
        names = sorted(h["name"] for h in rowheaders)
        assert_eq(names, [f"Row {i + 1}" for i in range(4)], "rowheaders named Row 1..4")
        data_rows = [n for n in acc["nodes"] if n.get("role") == "row" and n.get("level") == 2]
        assert_eq(len(data_rows), 4, "4 level-2 data rows (2 per group)")
        for r in data_rows:
            first = access_node_by_tag(acc, r["children"][0])
            assert first is not None and first["role"] == "rowheader", \
                f"data row {r['tag']} leads with a rowheader (the row-reset host)"

        # ── (B) cell / column / row reset buttons are emitted + named ────
        cell_resets = buttons(acc, is_cell_reset)
        col_resets = buttons(acc, lambda t: "#resetcol" in t)
        row_resets = buttons(acc, lambda t: "#resetrow" in t)
        assert len(cell_resets) > 0, "modified cells host reset buttons in grouped mode"
        assert len(col_resets) > 0, "modified columns host reset buttons in grouped mode"
        assert_eq(len(row_resets), 4, "every visible (modified) row hosts a row reset")
        for rb in cell_resets + col_resets + row_resets:
            assert rb.get("name"), f"reset button {rb['tag']} is named (AT-announceable)"
        # Every cell / row reset hangs off exactly one host present in the tree.
        for rb in cell_resets + row_resets:
            owners = [n for n in acc["nodes"] if rb["tag"] in n.get("children", [])]
            assert_eq(len(owners), 1, f"reset {rb['tag']} hangs off exactly one present host")

        # ── (C) collapse the sprite group: its resets leave, no orphan ───
        assert_eq(tf.invoke(f"{EXT}/toggle_group", 0), True, "collapse the sprite group")
        wait_query(tf, f"{EXT}/visible_len", 4, desc="2 headers + 2 mesh rows")
        acc_c = access(tf)
        assert_eq(
            len([n for n in acc_c["nodes"] if n.get("role") == "rowheader"]),
            2,
            "only the mesh group's 2 rows keep a rowheader after collapse",
        )
        # No cell / row reset addresses a collapsed source (0 or 2); each surviving
        # reset still hangs off a present host (no dangling button node).
        for rb in buttons(acc_c, lambda t: True):
            tag = rb["tag"]
            for src in SPRITE_SOURCES:
                assert f"#reset{src}_" not in tag, f"no cell reset orphans onto collapsed source {src}: {tag}"
                assert sub_of(tag) != f"resetrow{src}", f"no row reset orphans onto collapsed source {src}: {tag}"
            owners = [n for n in acc_c["nodes"] if tag in n.get("children", [])]
            assert_eq(len(owners), 1, f"surviving reset {tag} still hangs off one present host")
        # A column reset is unaffected by collapse (col_modified reads the whole
        # model, not the visible window).
        assert len(buttons(acc_c, lambda t: "#resetcol" in t)) > 0, \
            "column resets persist through a collapse (model-wide predicate)"
        # Expanding restores the sprite group's rows + their resets.
        assert_eq(tf.invoke(f"{EXT}/toggle_group", 0), False, "expand the sprite group")
        wait_query(tf, f"{EXT}/visible_len", 6, desc="all 4 data rows visible again")
        acc = access(tf)
        assert_eq(
            len([n for n in acc["nodes"] if n.get("role") == "rowheader"]),
            4,
            "all 4 rowheaders return after expand",
        )

        # ── (D) an AT Click on each reset clears its target ──────────────
        # A cell reset NOT in the group column (col 1), so the row stays put.
        cb = next(b for b in buttons(acc, is_cell_reset) if cell_of(b["tag"])[1] != 1)
        r, c = cell_of(cb["tag"])
        assert_eq(tf.query(f"{EXT}/modified.{r}.{c}"), True, "the addressed cell is modified")
        tf.invoke(f"{EXT}/send", f"{sub_of(cb['tag'])}:PointerUp")
        wait_until(
            lambda: tf.query(f"{EXT}/modified.{r}.{c}") is False,
            timeout=4.0, interval=0.03, desc="the AT cell reset cleared the cell",
        )
        assert access_node_by_tag(access(tf), cb["tag"]) is None, "the cleared cell's reset button leaves the tree"

        # A column reset clears the whole column.
        acc = access(tf)
        cc = next(iter(buttons(acc, lambda t: "#resetcol" in t)))
        col = int(sub_of(cc["tag"]).removeprefix("resetcol"))
        assert_eq(tf.query(f"{EXT}/col_modified.{col}"), True, "the addressed column is modified")
        tf.invoke(f"{EXT}/send", f"{sub_of(cc['tag'])}:PointerUp")
        wait_until(
            lambda: tf.query(f"{EXT}/col_modified.{col}") is False,
            timeout=4.0, interval=0.03, desc="the AT column reset cleared the column",
        )

        # A row reset clears the whole row; its rowheader (structural) persists.
        acc = access(tf)
        rb = next(iter(buttons(acc, lambda t: "#resetrow" in t)))
        src = int(sub_of(rb["tag"]).removeprefix("resetrow"))
        host = next(n for n in acc["nodes"] if rb["tag"] in n.get("children", []))
        assert_eq(host["role"], "rowheader", "the row reset hangs off a rowheader")
        assert_eq(tf.query(f"{EXT}/row_modified.{src}"), True, "the addressed row is modified")
        tf.invoke(f"{EXT}/send", f"{sub_of(rb['tag'])}:PointerUp")
        wait_until(
            lambda: tf.query(f"{EXT}/row_modified.{src}") is False,
            timeout=4.0, interval=0.03, desc="the AT row reset cleared the whole row",
        )
        acc = access(tf)
        assert access_node_by_tag(acc, rb["tag"]) is None, "the cleared row's reset button leaves the tree"
        assert access_node_by_tag(acc, host["tag"]) is not None, "the rowheader cell persists after reset"
        assert_eq(access_node_by_tag(acc, host["tag"])["role"], "rowheader", "still a rowheader")


if __name__ == "__main__":
    sys.exit(run_demo("R983 §5.40 — AT-reachable grouped data-grid reset", body))
