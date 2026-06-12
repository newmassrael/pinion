#!/usr/bin/env python3
"""R902 §5.40 — tree multi-select (scene outliner) E2E.

Drives `hello-tree-grid`'s R902 multi-select via JSON-RPC. The selection is a
SET of rows by **stable id** (robust across expand / collapse, where a flat
index would shift), decoupled from the keyboard cursor. The self-hosted
editor's "select N assets -> batch rename / delete" affordance.

Witnesses (§2 #7 scene-as-data, no pixels required):

  (A) boot — the selection is seeded on the cursor row (`f0`); the `tgrid_select`
      coordinator reports the set / count / anchor.
  (B) paint — the selected rows carry the accent-wash fill across BOTH panes
      (name cell + metadata strip); the cursor row's fill is DEEPER than a plain
      selected row's; an unselected row is transparent.
  (C) AI-first RPC — `invoke` select / toggle / extend_to / select_all / clear
      and `intervene("selection")` drive the SAME funnel the keyboard does, and
      `query` mirrors every write (read / write symmetry); an unknown id is
      dropped on the write path.
  (D) stable-id robustness — a selected row stays selected (BY ID) while its
      ancestor is collapsed away, and re-appears selected on re-expand (a flat
      index would have pointed at a different row).
  (E) keyboard chords — `Shift`+nav extends the range from the anchor,
      `Ctrl+Space` toggles the cursor row, `Ctrl+A` selects every visible row,
      and plain nav replaces the selection with the new cursor
      (selection-follows-focus).
  (F) click — a plain click selects the row (replace) AND toggles its expand.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_query,
    wait_until,
)

EXAMPLE = "hello-tree-grid"
WIN = (480, 460)
FOLDERS = 24
OBJECTS_PER = 12
EXPANDED_AT_BOOT = 3
BOOT_ROWS = FOLDERS + EXPANDED_AT_BOOT * OBJECTS_PER  # 60
TREE_TAG = "tgrid"
ROOT_TAG = "tgrid_root"
STATE_TAG = "tgrid_state"
SELECT_TAG = "tgrid_select"
V_SCROLL_TAG = "tgrid_scroll"
PITCH = 48  # ROW_PITCH; must match TreeViewStyle::m3_default().row_height
AT = (12.0, 80.0)  # any in-window point; named keys route to the focused root


def sel_path(slot: str) -> str:
    return f"/{SELECT_TAG}/external/{slot}"


def name_cell(rid: str) -> str:
    return f"{TREE_TAG}#{rid}"


def data_strip(rid: str) -> str:
    return f"{TREE_TAG}_drow{rid}"


def fill_of(snap, tag: str):
    node = find_by_tag(snap, tag)
    assert node is not None, f"{tag} present in the paint scene"
    return (node.get("style") or {}).get("fill") or {}


def alpha(fill) -> int:
    return int(fill.get("a") or 0)


def visible_ids(tf) -> list[str]:
    n = int(tf.query(f"/{STATE_TAG}/external/row_count"))
    return [tf.query(f"/{STATE_TAG}/external/id_at.{i}") for i in range(n)]


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:

        def snap_now():
            return tf.snapshot(source="paint", viewport=WIN)

        def selection() -> list:
            return tf.query(sel_path("selection"))

        def count() -> int:
            return int(tf.query(sel_path("count")))

        # ── (A) boot: the selection is seeded on the cursor row ──────────
        assert_eq(selection(), ["f0"], "boot selection = the cursor row {f0}")
        assert_eq(count(), 1, "boot selection count = 1")
        assert_eq(tf.query(sel_path("anchor")), "f0", "boot range anchor = f0")
        assert_eq(tf.query(f"/{STATE_TAG}/external/cursor"), "f0", "boot cursor = f0")

        # ── (B) paint: selection wash across both panes; cursor deeper ───
        snap = snap_now()
        # Boot: f0 is selected AND the cursor -> the deeper wash (opaque); its
        # rendered sibling f0-o0 is unselected -> transparent.
        f0_boot = fill_of(snap, name_cell("f0"))
        assert alpha(f0_boot) > 0, f"selected cursor row painted opaque, got {f0_boot}"
        assert_eq(alpha(fill_of(snap, name_cell("f0-o0"))), 0, "unselected row is transparent")

        # Toggle a second row in (the cursor moves onto it): now f0 is a plain
        # selected row (lighter wash) and f0-o0 is the selected cursor (deeper).
        tf.invoke(sel_path("toggle"), "f0-o0")
        wait_query(tf, sel_path("selection"), ["f0", "f0-o0"], desc="toggle adds f0-o0")
        snap = snap_now()
        plain_sel = fill_of(snap, name_cell("f0"))       # selected, not cursor
        cursor_sel = fill_of(snap, name_cell("f0-o0"))   # selected AND cursor
        assert alpha(plain_sel) > 0 and alpha(cursor_sel) > 0, "both selected rows painted opaque"
        assert plain_sel != cursor_sel, \
            f"the cursor row's fill is deeper than a plain selected row's, got {cursor_sel} vs {plain_sel}"
        # Cross-pane: the metadata strip carries the SAME wash as the name cell.
        assert_eq(fill_of(snap, data_strip("f0-o0")), cursor_sel, "metadata strip matches the name cell")
        assert_eq(alpha(fill_of(snap, data_strip("f0-o1"))), 0, "an unselected strip is transparent")

        # ── (C) AI-first RPC: invoke / intervene / query symmetry ───────
        # invoke returns the resulting set so the caller sees the outcome.
        assert_eq(tf.invoke(sel_path("select"), "f3"), ["f3"], "invoke select replaces with {f3}")
        assert_eq(tf.query(sel_path("anchor")), "f3", "select re-seeds the anchor")
        assert_eq(tf.invoke(sel_path("toggle"), "f5"), ["f3", "f5"], "toggle adds f5")
        assert_eq(tf.invoke(sel_path("toggle"), "f3"), ["f5"], "toggle removes f3")
        # extend_to ranges from the anchor (f5) over the visible flattening.
        assert_eq(tf.invoke(sel_path("select"), "f3"), ["f3"], "re-anchor at f3")
        ext = tf.invoke(sel_path("extend_to"), "f5")
        assert_eq(ext, ["f3", "f4", "f5"], "extend_to f3->f5 spans the visible run")
        # intervene (admin restore) replaces; an unknown id is dropped on write.
        tf.intervene(sel_path("selection"), ["f10", "f11", "ghost"])
        wait_query(tf, sel_path("selection"), ["f10", "f11"], desc="intervene drops the phantom id")
        assert_eq(count(), 2, "count mirrors the restored set")
        # select_all selects every VISIBLE row (the tree Ctrl+A convention).
        all_set = tf.invoke(sel_path("select_all"), None)
        assert_eq(len(all_set), BOOT_ROWS, "select_all selects every visible row")
        # clear empties the set (returns []); Null intervene also clears.
        assert_eq(tf.invoke(sel_path("clear"), None), [], "clear empties the set")
        assert_eq(count(), 0, "count = 0 after clear")

        # ── (D) stable-id robustness across collapse ─────────────────────
        # Seed the cursor on f0, then ADD a child of f0 to the set WITHOUT moving
        # the cursor (admin intervene leaves the cursor put), so an ArrowLeft can
        # collapse f0 without selection-follows-focus disturbing the set.
        tf.invoke(sel_path("select"), "f0")
        tf.intervene(sel_path("selection"), ["f0", "f0-o5"])
        wait_query(tf, sel_path("selection"), ["f0", "f0-o5"], desc="set {f0, f0-o5}, cursor still f0")
        assert "f0-o5" in visible_ids(tf), "the child is visible while f0 is expanded"
        # Collapse f0 from the keyboard (cursor on f0, no selection change).
        tf.request("focus/set", {"tag": ROOT_TAG})
        tf.key(at=AT, name="ArrowLeft")
        wait_until(lambda: "f0-o5" not in visible_ids(tf), desc="f0 collapsed -> child off-screen")
        assert_eq(selection(), ["f0", "f0-o5"], "the collapsed-away child STAYS selected by stable id")
        # Re-expand: the child re-appears, still selected (a flat index would not
        # have survived the row shifts).
        tf.key(at=AT, name="ArrowRight")
        wait_until(lambda: "f0-o5" in visible_ids(tf), desc="f0 re-expanded -> child back")
        assert_eq(selection(), ["f0", "f0-o5"], "re-expanding restores the still-selected child")

        # ── (E) keyboard chords ──────────────────────────────────────────
        # Baseline: select f1 (expanded at boot), cursor + anchor = f1.
        tf.invoke(sel_path("select"), "f1")
        assert_eq(tf.query(f"/{STATE_TAG}/external/cursor"), "f1", "cursor reset to f1")
        # Shift+Down extends the range f1 -> f1-o0 (f1 is expanded -> descends).
        tf.modifiers(shift=True)
        tf.key(at=AT, name="ArrowDown")
        tf.modifiers()  # release
        wait_query(tf, sel_path("selection"), ["f1", "f1-o0"], desc="Shift+Down extends from anchor f1")
        assert_eq(tf.query(sel_path("anchor")), "f1", "anchor holds put while extending")
        # Ctrl+Space toggles the cursor row (f1-o0) OUT of the set (cursor stays).
        tf.modifiers(ctrl=True)
        tf.key(at=AT, name="Space")
        tf.modifiers()
        wait_query(tf, sel_path("selection"), ["f1"], desc="Ctrl+Space toggled the cursor row off")
        assert_eq(tf.query(f"/{STATE_TAG}/external/cursor"), "f1-o0", "Ctrl+Space leaves the cursor put")
        # Ctrl+A selects every visible row.
        tf.modifiers(ctrl=True)
        tf.key(at=AT, name="a")
        tf.modifiers()
        wait_query(tf, sel_path("count"), BOOT_ROWS, desc="Ctrl+A selects every visible row")
        # Plain Down replaces the whole set with the new cursor (follows focus).
        tf.key(at=AT, name="ArrowDown")
        wait_until(lambda: count() == 1, desc="plain nav replaces the selection")
        assert_eq(selection(), ["f1-o1"], "selection-follows-focus: the new cursor only")

        # ── (F) click selects (replace) AND toggles expand ──────────────
        # f7 is a collapsed folder mid-list; scroll it to the top of the window
        # (by its live visible index, robust to the current expand state), then
        # click its name cell — the click selects it (replace) and expands it.
        f7_idx = visible_ids(tf).index("f7")
        tf.scroll(V_SCROLL_TAG, to=(0, f7_idx * PITCH))
        wait_until(
            lambda: name_cell("f7") in abs_rects_of(snap_now()),
            desc="scroll f7 into the window",
        )
        assert "f7-o0" not in visible_ids(tf), "f7 starts collapsed (no children visible)"
        tf.click(path=name_cell("f7"))
        wait_query(tf, sel_path("selection"), ["f7"], desc="click replaces the selection with f7")
        assert_eq(tf.query(f"/{STATE_TAG}/external/cursor"), "f7", "click moves the cursor to f7")
        assert "f7-o0" in visible_ids(tf), "click also expanded f7 (its first child is visible)"

        # ── (G) modifier pointer-clicks — the keyboard chords' pointer peer ──
        # f7 is now expanded; its leaf children f7-o* are rendered. The held
        # modifiers ride the click wire (tf.modifiers -> dispatch_send_mods ->
        # the composite click intent -> the SelectionChord reducer): plain click
        # replaces, Ctrl-click toggles, Shift-click extends from the anchor.
        wait_until(
            lambda: name_cell("f7-o5") in abs_rects_of(snap_now()),
            desc="f7's leaf children are rendered",
        )
        tf.click(path=name_cell("f7-o1"))
        wait_query(tf, sel_path("selection"), ["f7-o1"], desc="plain click on a leaf replaces + anchors")
        tf.modifiers(ctrl=True)
        tf.click(path=name_cell("f7-o3"))
        tf.modifiers()
        wait_query(tf, sel_path("selection"), ["f7-o1", "f7-o3"], desc="Ctrl-click toggles a 2nd row in")
        tf.modifiers(shift=True)
        tf.click(path=name_cell("f7-o5"))
        tf.modifiers()
        wait_query(
            tf, sel_path("selection"), ["f7-o3", "f7-o4", "f7-o5"],
            desc="Shift-click extends the range from the anchor (f7-o3)",
        )


if __name__ == "__main__":
    sys.exit(run_demo("r902_tree_multi_select", body))
