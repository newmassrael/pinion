#!/usr/bin/env python3
"""R892 §5.40 — column GROUPING folded onto the EDITABLE data grid.

Drives `hello-data-grid` via JSON-RPC. After R886 (sort) and R891 (filter),
R892 folds the GROUP axis the same way, through the cross-grid `group_rows`
free-fn SSOT the read-only `hello-grouped-grid` already speaks: a settable
group-by column flattens the filtered+sorted rows into group runs (a header +
its members), each header collapsible. The wire mirrors the group_order
vocabulary read/write-symmetrically.

The fold's payoff invariant (the group analog of edit-while-sorted /
edit-while-filtered): every grid state is SOURCE-keyed, so a committed edit of
the group-key cell moves its row to another group on the next paint, and
collapsing the cursor's group re-anchors the source-keyed cursor through the
shared reanchor SSOT (now generalised over filter + group + collapse).

Wire surface (all on /external = the grid coordinator), read/write symmetric:
  query  group              -> "none" | "<col>"        (intervene / set_group)
  query  group_count        -> distinct groups (0 ungrouped)
  query  visible_len        -> rendered rows (headers + uncollapsed data)
  query  source_at.<pos>    -> source row at visual pos (Null for a header)
  query  kind_at.<pos>      -> "header" | "data"
  query  label_at.<pos>     -> a header's group label
  query  collapsed.<group>  -> bool  (intervene; toggle_group / *_all)
  send "g<group>:PointerUp" -> a clicked group header toggles collapse

Scope (exact count = 36 checks — in-body assert_eq + wait gates; the
_focus_grid focus wait is setup, not a check):
  (A) boot: ungrouped                                  [3]
  (B) set_group flattens into headers + data           [9]
  (C) collapse hides members + re-anchors the cursor   [5]
  (D) collapse_all / expand_all                        [3]
  (E) group wire round-trip read/write + Null clear    [4]
  (F) edit-while-grouped: a group-key commit regroups  [5]
  (G) keyboard nav skips group headers                 [3]
  (H) status bar + treegrid headers painted            [4]
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

# Type column (col 1) values: sprite, mesh, sprite, mesh.
# Group by Type: group 0 = sprite (rows 0, 2), group 1 = mesh (rows 1, 3).
# Unsorted visible sequence = [H0, D0, D2, H1, D1, D3].


def _focus_grid(tf) -> None:
    tf.request("focus/set", {"tag": GRID})
    wait_until(
        lambda: tf.request("focus/get").result.get("focused") == GRID,
        timeout=4.0,
        interval=0.03,
        desc="grid owns keyboard focus",
    )


def _scan(snap) -> dict:
    out: dict = {"texts": [], "tags": []}

    def walk(node) -> None:
        if isinstance(node, dict):
            if node.get("type") == "Text" and isinstance(node.get("content"), str):
                out["texts"].append(node["content"])
            tag = node.get("tag")
            if isinstance(tag, str):
                out["tags"].append(tag)
            for v in node.values():
                walk(v)
        elif isinstance(node, list):
            for v in node:
                walk(v)

    walk(snap)
    return out


def body() -> None:
    with RpcSubprocess("hello-data-grid", boot_grace=1.5) as tf:
        # ── (A) boot: ungrouped ─────────────────────────────────────
        assert_eq(tf.query("/external/group"), "none", "boot: ungrouped")
        assert_eq(tf.query("/external/group_count"), 0, "no groups")
        assert_eq(tf.query("/external/visible_len"), 4, "flat = NROWS data rows")

        # ── (B) set_group flattens into headers + data ──────────────
        assert_eq(tf.invoke("/external/set_group", "1"), 2,
                  "set_group returns the group count in one round-trip")
        wait_query(tf, "/external/group", "1", desc="grouped by Type")
        assert_eq(tf.query("/external/visible_len"), 6, "2 headers + 4 data rows")
        assert_eq(tf.query("/external/kind_at.0"), "header", "position 0 is a header")
        assert_eq(tf.query("/external/kind_at.1"), "data", "position 1 is a data row")
        assert_eq(tf.query("/external/source_at.0"), None, "header reports Null source")
        assert_eq(tf.query("/external/source_at.1"), 0, "sprite group: Hero")
        assert_eq(tf.query("/external/source_at.2"), 2, "sprite group: Coin")
        assert_eq(tf.query("/external/label_at.0"), "sprite", "first group label")
        assert_eq(tf.query("/external/label_at.3"), "mesh", "second group label")

        # ── (C) collapse hides members + re-anchors the cursor ──────
        tf.request("scene/intervene", {"path": "/external/focused_row", "value": 0})
        assert_eq(tf.invoke("/external/toggle_group", 0), True, "collapse the sprite group")
        wait_query(tf, "/external/collapsed.0", True, desc="group 0 collapsed")
        assert_eq(tf.query("/external/visible_len"), 4, "2 headers + 2 mesh rows")
        # Hero (row 0) was the cursor and is now hidden -> re-anchors to the
        # first visible row (the mesh group's Tree, source 1).
        assert_eq(tf.query("/external/focused_row"), 1,
                  "cursor re-anchored out of the collapsed group")
        tf.invoke("/external/toggle_group", 0)  # re-expand

        # ── (D) collapse_all / expand_all ───────────────────────────
        tf.invoke("/external/collapse_all", None)
        wait_query(tf, "/external/visible_len", 2, desc="collapse_all leaves only headers")
        tf.invoke("/external/expand_all", None)
        wait_query(tf, "/external/visible_len", 6, desc="expand_all restores members")

        # ── (E) group wire round-trip + Null clear ──────────────────
        tf.request("scene/intervene", {"path": "/external/collapsed.1", "value": True})
        assert_eq(tf.query("/external/collapsed.1"), True, "collapsed.<g> is writable")
        tf.request("scene/intervene", {"path": "/external/group", "value": None})
        wait_query(tf, "/external/group", "none", desc="Null clears the group-by")
        assert_eq(tf.query("/external/group_count"), 0, "ungrouped reports 0 groups")

        # ── (F) edit-while-grouped: a group-key commit regroups live ─
        # R940 made Type a Choice column (popup-edited, not inline text), so the
        # group-key edit picks "sprite" from Tree's dropdown (`begin` on a choice
        # cell opens the popup). The payoff is unchanged: committing the group-key
        # cell regroups the row on the next paint.
        _focus_grid(tf)
        tf.invoke("/external/set_group", "1")
        # Focus Tree (row 1, Type=mesh) and pick "sprite" (option 0) from its dropdown.
        tf.request("scene/intervene", {"path": "/external/focused_row", "value": 1})
        tf.request("scene/intervene", {"path": "/external/focused_col", "value": 1})
        tf.invoke("/external/begin", None)  # Type is a choice column -> opens the dropdown
        wait_query(tf, "/external/editing_row", 1, desc="editing Tree's Type cell (dropdown open)")
        tf.invoke("/external/choose", 0)  # pick option 0 = sprite
        wait_query(tf, "/external/value.1.1",
                   {"selected": 0, "label": "sprite",
                    "options": ["sprite", "mesh", "material", "audio", "script"]},
                   desc="the dropdown pick writes the source cell")
        # Tree now joins the sprite group (which leads): [H(sprite), 0, 1, 2,
        # H(mesh), 3]. Group count stays 2; Tree sits among the sprites.
        assert_eq(tf.query("/external/group_count"), 2, "still two Type values")
        assert_eq(tf.query("/external/label_at.0"), "sprite", "sprite group leads")
        assert_eq(tf.query("/external/source_at.2"), 1, "Tree re-grouped into sprite")

        # ── (G) keyboard nav skips group headers ────────────────────
        tf.invoke("/external/set_group", None)  # back to flat for a clean walk
        tf.invoke("/external/set_group", "4")  # group by Active (bool)
        # Active values: On, On, Off, On -> group On = [0,1,3], Off = [2].
        # Visible data order = [0, 1, 3, 2]; ArrowDown walks it across the
        # group boundary (skipping the Off header).
        tf.request("scene/intervene", {"path": "/external/focused_row", "value": 3})
        tf.key(path=GRID, name="ArrowDown")
        wait_query(tf, "/external/focused_row", 2,
                   desc="ArrowDown crosses into the next group, skipping its header")
        tf.key(path=GRID, name="ArrowUp")
        wait_query(tf, "/external/focused_row", 3, desc="ArrowUp steps back over the header")

        # ── (H) status bar + treegrid group headers painted ─────────
        snap = wait_snap(
            tf,
            lambda s: any(
                "showing 4 of 4 · grouped by Active" in t for t in _scan(s)["texts"]
            ),
            viewport=VIEWPORT,
            desc="status bar names the group-by column",
        )
        tags = _scan(snap)["tags"]
        assert_eq(f"{GRID}#g0" in tags, True, "group-0 header painted (routable tag)")
        assert_eq(f"{GRID}#g1" in tags, True, "group-1 header painted")
        # Ungrouping drops the group headers and the status suffix.
        tf.invoke("/external/set_group", None)
        snap = wait_snap(
            tf,
            lambda s: any("showing 4 of 4" in t for t in _scan(s)["texts"])
            and f"{GRID}#g0" not in _scan(s)["tags"],
            viewport=VIEWPORT,
            desc="ungrouped drops the headers + the group suffix",
        )
        assert_eq(f"{GRID}#g0" in _scan(snap)["tags"], False, "no group headers when flat")


if __name__ == "__main__":
    sys.exit(run_demo("R892 §5.40 — editable data-grid column grouping", body))
