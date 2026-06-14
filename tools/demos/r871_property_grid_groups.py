#!/usr/bin/env python3
"""R871 / R921 §5.27 §5.40 §5.38 — property-grid collapsible category sections.

The Inspector grouping every DCC details panel has: typed property rows nest
under named, collapsible **category** branches (Identity / Appearance / Physics
/ Stats / Transform). Since R921 the panel is a WAI-ARIA **tree** (categories
and struct properties are both collapsible branches), so a category collapses
through the same per-branch wire a struct does: `toggle_branch` / `expanded.<id>`
on the primary coordinator, the keyboard `ArrowLeft` / `ArrowRight` / `Enter`,
or a click on the header. The typed-cell editing (R836/R867/R869) keeps working
inside the categories — the *value index* stays each leaf's stable identity.

Two externals cooperate:
  property_grid       (primary)  -> the value model + edits + per-branch collapse:
    /external/expanded.<branch_id> -> a branch's collapse flag (read / intervene)
    /external/toggle_branch        -> invoke(text id): flip a branch's collapse
    /external/cursor               -> the roving cursor node id (read / intervene)
  property_grid_tree  (RO)       -> the visible-row flatten:
    /external/row_count / id_at.<pos> / label_at.<pos> / cursor_index

Verified (>= 30 assertions):
  (A) boot taxonomy — 6 categories in the 28-row tree, their labels
  (B) RPC collapse/expand — toggle_branch + expanded.<id> read/intervene
  (C) keyboard collapse — ArrowLeft/Right/Enter on a focused category branch
  (D) pointer — clicking a category header toggles its collapse
  (E) cursor roving — ArrowDown roves over the flatten; the cursor clamps
  (F) editing inside a category still works — bool toggle + text edit
  (G) paint — a collapsed category hides its rows but keeps its header
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
    wait_until,
)

VIEWPORT = (460, 820)

GRID = "property_grid"
EDIT = "property_grid_edit"
TREE = "property_grid_tree"

IDENTITY = "cat.Identity"
APPEARANCE = "cat.Appearance"
PHYSICS = "cat.Physics"


def _focus_grid(tf) -> None:
    tf.request("focus/set", {"tag": GRID})
    wait_until(
        lambda: tf.request("focus/get").result.get("focused") == GRID,
        timeout=4.0, interval=0.03, desc="grid owns keyboard focus",
    )


def _rows(tf) -> int:
    return tf.query(f"/{TREE}/external/row_count")


def _expanded(tf, branch: str):
    return tf.query(f"/{GRID}/external/expanded.{branch}")


def body() -> None:
    with RpcSubprocess("hello-property-grid", boot_grace=1.5) as tf:
        # ── (A) boot category taxonomy ───────────────────────────────
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert find_by_tag(snap, GRID) is not None, "grid present"
        assert_eq(_rows(tf), 28, "6 categories + 2 structs + 1 array + 19 leaves")
        # Categories lead the flatten (each with its leaves), before the structs.
        assert_eq(tf.query(f"/{TREE}/external/label_at.0"), "Identity", "category 0 = Identity")
        assert_eq(tf.query(f"/{TREE}/external/id_at.0"), IDENTITY, "row 0 id")
        assert_eq(tf.query(f"/{TREE}/external/label_at.4"), "Appearance", "category at pos 4")
        assert_eq(tf.query(f"/{TREE}/external/label_at.9"), "Physics", "category at pos 9")
        assert_eq(tf.query(f"/{TREE}/external/label_at.12"), "Stats", "category at pos 12")
        assert_eq(tf.query(f"/{TREE}/external/label_at.14"), "Transform", "category at pos 14")
        assert_eq(tf.query(f"/{TREE}/external/id_at.1"), "0", "row 1 is the Name leaf (value 0)")
        assert_eq(tf.query("/external/cursor"), None, "no cursor at boot")

        # ── (B) RPC collapse / expand a category ─────────────────────
        assert_eq(_expanded(tf, IDENTITY), True, "Identity boots expanded")
        # toggle_branch returns the resulting expanded flag; collapsing Identity
        # hides its 3 leaves (Name / Tag / Layer).
        assert_eq(tf.invoke(f"/{GRID}/external/toggle_branch", IDENTITY), False, "collapse Identity")
        assert_eq(_expanded(tf, IDENTITY), False, "Identity collapsed")
        assert_eq(_expanded(tf, APPEARANCE), True, "Appearance still expanded")
        wait_until(lambda: _rows(tf) == 25, timeout=4.0, interval=0.03,
                   desc="collapsing Identity hides its 3 leaves (28 - 3)")
        # Row 1 is now the Appearance category (the Identity leaves are gone).
        assert_eq(tf.query(f"/{TREE}/external/id_at.1"), APPEARANCE, "collapsed: row 1 now Appearance")
        assert_eq(tf.invoke(f"/{GRID}/external/toggle_branch", IDENTITY), True, "re-expand Identity")
        wait_until(lambda: _rows(tf) == 28, timeout=4.0, interval=0.03, desc="re-expanded -> 28")
        # Admin intervene of a collapse flag (the restore path).
        tf.intervene(f"/{GRID}/external/expanded.{PHYSICS}", False)
        assert_eq(_expanded(tf, PHYSICS), False, "intervene collapses Physics")
        tf.intervene(f"/{GRID}/external/expanded.{PHYSICS}", True)
        wait_until(lambda: _rows(tf) == 28, timeout=4.0, interval=0.03, desc="all expanded again")

        # ── (C) keyboard collapse / expand on a category branch ──────
        _focus_grid(tf)
        tf.intervene("/external/cursor", IDENTITY)
        tf.key(path=GRID, name="ArrowLeft")  # collapse the focused branch
        wait_until(lambda: _expanded(tf, IDENTITY) is False, timeout=4.0,
                   interval=0.03, desc="ArrowLeft collapses the focused category")
        wait_until(lambda: _rows(tf) == 25, timeout=4.0, interval=0.03, desc="Identity rows hidden")
        tf.key(path=GRID, name="ArrowRight")  # expand it again
        wait_until(lambda: _expanded(tf, IDENTITY) is True, timeout=4.0,
                   interval=0.03, desc="ArrowRight expands the focused category")
        tf.key(path=GRID, name="Enter")  # Enter on a branch toggles too
        wait_until(lambda: _expanded(tf, IDENTITY) is False, timeout=4.0,
                   interval=0.03, desc="Enter on a branch collapses")
        tf.key(path=GRID, name="ArrowRight")  # restore
        wait_until(lambda: _expanded(tf, IDENTITY) is True, timeout=4.0,
                   interval=0.03, desc="restored expanded")

        # ── (D) pointer: clicking a category header toggles collapse ─
        tf.click(path=f"{GRID}#{APPEARANCE}")
        wait_until(lambda: _expanded(tf, APPEARANCE) is False, timeout=4.0,
                   interval=0.03, desc="header click collapses Appearance")
        tf.click(path=f"{GRID}#{APPEARANCE}")
        wait_until(lambda: _expanded(tf, APPEARANCE) is True, timeout=4.0,
                   interval=0.03, desc="header click re-expands Appearance")

        # ── (E) cursor roving over the flatten, clamped ──────────────
        tf.intervene("/external/cursor", None)
        _focus_grid(tf)
        tf.key(path=GRID, name="ArrowDown")  # from no cursor -> visual row 0
        wait_until(lambda: tf.query(f"/{TREE}/external/cursor_index") == 0, timeout=4.0,
                   interval=0.03, desc="ArrowDown -> visual row 0 (category)")
        tf.key(path=GRID, name="ArrowDown")  # -> row 1 (the first leaf)
        wait_until(lambda: tf.query(f"/{TREE}/external/cursor_index") == 1, timeout=4.0,
                   interval=0.03, desc="ArrowDown -> visual row 1 (a leaf)")
        assert_eq(tf.query("/external/cursor"), "0", "the cursor is the Name leaf id")
        last = _rows(tf) - 1
        tf.key(path=GRID, name="End")
        wait_until(lambda: tf.query(f"/{TREE}/external/cursor_index") == last, timeout=4.0,
                   interval=0.03, desc="End -> last visual row")
        tf.key(path=GRID, name="ArrowDown")
        assert_eq(tf.query(f"/{TREE}/external/cursor_index"), last, "ArrowDown at bottom clamps")

        # ── (F) editing inside a category still works ────────────────
        assert_eq(tf.query("/external/value.2"), True, "Visible true before")
        assert_eq(tf.invoke("/external/toggle", 2), True, "toggle value 2 toggled a bool")
        assert_eq(tf.query("/external/value.2"), False, "Visible toggled to false")
        _focus_grid(tf)
        tf.intervene("/external/cursor", "1")  # Tag (value 1)
        tf.key(path=GRID, name="Enter")
        wait_until(lambda: tf.query("/external/editing") == "1", timeout=4.0,
                   interval=0.03, desc="Enter edits the Tag row inside Identity")
        wait_until(lambda: tf.request("focus/get").result.get("focused") == EDIT,
                   timeout=4.0, interval=0.03, desc="focus moved into the inline field")
        for _ in range(4):  # clear "hero"
            tf.key(path=EDIT, name="Backspace")
        tf.text("boss", path=EDIT)
        tf.key(path=EDIT, name="Enter")
        wait_until(lambda: tf.query("/external/value.1") == "boss", timeout=4.0,
                   interval=0.03, desc="commit the edited Tag inside the category")

        # ── (G) paint: collapse hides rows, keeps the header ─────────
        expanded = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert find_by_tag(expanded, f"{GRID}#0") is not None, "Name row painted when expanded"
        assert find_by_tag(expanded, f"{GRID}#{IDENTITY}") is not None, "Identity header painted"
        tf.intervene(f"/{GRID}/external/expanded.{IDENTITY}", False)
        wait_until(
            lambda: find_by_tag(tf.snapshot(source="paint", viewport=VIEWPORT), f"{GRID}#0") is None,
            timeout=4.0, interval=0.05, desc="Name row hidden when Identity collapses")
        collapsed = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert find_by_tag(collapsed, f"{GRID}#{IDENTITY}") is not None, "header stays on collapse"
        assert find_by_tag(collapsed, f"{GRID}#1") is None, "Tag row hidden too"


if __name__ == "__main__":
    sys.exit(run_demo("hello-property-grid R871 / R921 §5.27 collapsible category sections", body))
