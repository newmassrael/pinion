#!/usr/bin/env python3
"""R871 §5.27 §5.40 §5.38 — property-grid collapsible category sections.

The Inspector grouping every DCC details panel has: the flat 12-row typed
property list groups under named, collapsible category sections (Identity /
Appearance / Physics / Stats / Transform). Built as the **3rd structural
consumer** of the R843 `GroupOrderState` substrate (after hello-grouped-list
and hello-grouped-grid) — no new framework crate: the group-by proxy owns the
collapse set + the roving visual-row cursor, the category header skin is the
lifted `pinion_widget_paint::group_header::group_header_row` SSOT, and the
keyboard / a11y reuse `group_nav` / `grouped_grid_access_nodes`. The typed-cell
editing (R836/R867/R869) keeps working inside the categories — the *source*
index stays each row's stable identity.

Two externals cooperate:
  property_grid       (primary)  -> the source-keyed typed value model + edits
  property_grid_cat   (extra)    -> the category collapse set + roving cursor

Group-by proxy slots (`property_grid_cat`):
  /external/group_count           -> 5 categories
  /external/label_at.<pos>        -> a header's category label (null on data)
  /external/member_count_at.<pos> -> a header's member count (null on data)
  /external/kind_at.<pos>         -> "header" | "data"
  /external/source_at.<pos>       -> a data row's source index (null on header)
  /external/collapsed.<group>     -> per-category collapse flag (read/write)
  /external/visible_len           -> headers + visible data rows
  /external/cursor                -> roving visual-row cursor (read/write)
  /external/toggle_group          -> invoke(int): flip a category's collapse
  /external/collapse_all / expand_all -> invoke: mass collapse / expand

Verified (>= 30 assertions):
  (A) boot taxonomy — 5 categories, first-appearance order, member counts
  (B) RPC collapse/expand — toggle_group, collapse_all, expand_all, visible_len
  (C) keyboard collapse — ArrowLeft/Right/Enter on a focused category header
  (D) pointer — clicking a category header toggles its collapse
  (E) cursor roving — ArrowDown roves over headers + data; the cursor clamps
  (F) editing inside a category still works — bool toggle + text edit by source
  (G) paint — a collapsed category hides its data rows but keeps its header
"""

from __future__ import annotations

import sys
import time
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
PAUSE = 0.10

GRID = "property_grid"
EDIT = "property_grid_edit"
CAT = "property_grid_cat"

# The all-expanded flatten (5 headers + 12 data = 17 rows), used to drive the
# cursor deterministically. Visual position -> what is there.
IDENTITY_HEADER = 0      # category 0
APPEARANCE_HEADER = 4    # category 1
PHYSICS_HEADER = 9       # category 2


def _focus_grid(tf) -> None:
    tf.request("focus/set", {"tag": GRID})
    wait_until(
        lambda: tf.request("focus/get").result.get("focused") == GRID,
        timeout=4.0,
        interval=0.03,
        desc="grid owns keyboard focus",
    )


def _cursor_to_source(tf, source: int) -> None:
    """Move the roving cursor onto `source`'s visual row (category expanded)."""
    n = tf.query(f"/{CAT}/external/visible_len")
    for pos in range(n):
        if tf.query(f"/{CAT}/external/source_at.{pos}") == source:
            tf.intervene(f"/{CAT}/external/cursor", pos)
            return
    raise AssertionError(f"source {source} not visible in the flatten")


def body() -> None:
    with RpcSubprocess("hello-property-grid", boot_grace=1.5) as tf:
        # ── (A) boot category taxonomy ───────────────────────────────
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert find_by_tag(snap, GRID) is not None, "grid present"
        assert_eq(tf.query(f"/{CAT}/external/group_count"), 5, "5 categories")
        assert_eq(tf.query(f"/{CAT}/external/visible_len"), 17, "5 headers + 12 data")
        # First-appearance order over the source order.
        assert_eq(tf.query(f"/{CAT}/external/label_at.0"), "Identity", "category 0 = Identity")
        assert_eq(tf.query(f"/{CAT}/external/label_at.4"), "Appearance", "category 1 = Appearance")
        assert_eq(tf.query(f"/{CAT}/external/label_at.9"), "Physics", "category 2 = Physics")
        assert_eq(tf.query(f"/{CAT}/external/label_at.12"), "Stats", "category 3 = Stats")
        assert_eq(tf.query(f"/{CAT}/external/label_at.14"), "Transform", "category 4 = Transform")
        # Member counts + the header/data discriminator.
        assert_eq(tf.query(f"/{CAT}/external/member_count_at.0"), 3, "Identity has 3 members")
        assert_eq(tf.query(f"/{CAT}/external/member_count_at.4"), 4, "Appearance has 4 members")
        assert_eq(tf.query(f"/{CAT}/external/kind_at.0"), "header", "row 0 is a header")
        assert_eq(tf.query(f"/{CAT}/external/kind_at.1"), "data", "row 1 is data")
        assert_eq(tf.query(f"/{CAT}/external/source_at.1"), 0, "row 1 is the Name source")
        assert_eq(tf.query(f"/{CAT}/external/source_at.0"), None, "a header has no source")
        assert_eq(tf.query(f"/{CAT}/external/cursor"), None, "no cursor at boot")

        # ── (B) RPC collapse / expand ────────────────────────────────
        # toggle_group returns the new visible_len; collapsing Identity hides
        # its 3 data rows (Name / Tag / Layer).
        assert_eq(tf.invoke(f"/{CAT}/external/toggle_group", 0), 14, "collapse Identity -> 14")
        assert_eq(tf.query(f"/{CAT}/external/collapsed.0"), True, "Identity collapsed")
        assert_eq(tf.query(f"/{CAT}/external/collapsed.1"), False, "Appearance still expanded")
        # Row 1 is now the Appearance header (the Identity data rows are gone).
        assert_eq(tf.query(f"/{CAT}/external/kind_at.1"), "header", "collapsed: row 1 now a header")
        assert_eq(tf.invoke(f"/{CAT}/external/toggle_group", 0), 17, "re-expand Identity -> 17")
        assert_eq(tf.query(f"/{CAT}/external/collapsed.0"), False, "Identity expanded again")
        # Mass operations.
        assert_eq(tf.invoke(f"/{CAT}/external/collapse_all", 0), 5, "collapse_all -> only 5 headers")
        assert_eq(tf.invoke(f"/{CAT}/external/expand_all", 0), 17, "expand_all -> 17")
        # Admin intervene of a collapse flag (the restore path).
        tf.intervene(f"/{CAT}/external/collapsed.2", True)
        assert_eq(tf.query(f"/{CAT}/external/collapsed.2"), True, "intervene collapses Physics")
        tf.intervene(f"/{CAT}/external/collapsed.2", False)
        assert_eq(tf.query(f"/{CAT}/external/visible_len"), 17, "all expanded again")

        # ── (C) keyboard collapse / expand on a category header ──────
        _focus_grid(tf)
        tf.intervene(f"/{CAT}/external/cursor", IDENTITY_HEADER)
        tf.key(path=GRID, name="ArrowLeft")  # collapse the focused header
        wait_until(lambda: tf.query(f"/{CAT}/external/collapsed.0") is True, timeout=4.0,
                   interval=0.03, desc="ArrowLeft collapses the focused category")
        assert_eq(tf.query(f"/{CAT}/external/visible_len"), 14, "Identity data rows hidden")
        tf.key(path=GRID, name="ArrowRight")  # expand it again
        wait_until(lambda: tf.query(f"/{CAT}/external/collapsed.0") is False, timeout=4.0,
                   interval=0.03, desc="ArrowRight expands the focused category")
        tf.key(path=GRID, name="Enter")  # Enter on a header toggles too
        wait_until(lambda: tf.query(f"/{CAT}/external/collapsed.0") is True, timeout=4.0,
                   interval=0.03, desc="Enter on a header collapses")
        tf.key(path=GRID, name="ArrowRight")  # restore
        wait_until(lambda: tf.query(f"/{CAT}/external/collapsed.0") is False, timeout=4.0,
                   interval=0.03, desc="restored expanded")

        # ── (D) pointer: clicking a category header toggles collapse ─
        tf.click(path=f"{CAT}#1")  # the Appearance header composite tag
        wait_until(lambda: tf.query(f"/{CAT}/external/collapsed.1") is True, timeout=4.0,
                   interval=0.03, desc="header click collapses Appearance")
        tf.click(path=f"{CAT}#1")
        wait_until(lambda: tf.query(f"/{CAT}/external/collapsed.1") is False, timeout=4.0,
                   interval=0.03, desc="header click re-expands Appearance")

        # ── (E) cursor roving over headers + data, clamped ───────────
        tf.intervene(f"/{CAT}/external/cursor", None)
        _focus_grid(tf)
        tf.key(path=GRID, name="ArrowDown")  # from no cursor -> visual row 0
        wait_until(lambda: tf.query(f"/{CAT}/external/cursor") == 0, timeout=4.0,
                   interval=0.03, desc="ArrowDown -> visual row 0 (header)")
        tf.key(path=GRID, name="ArrowDown")  # -> row 1 (the first data row)
        wait_until(lambda: tf.query(f"/{CAT}/external/cursor") == 1, timeout=4.0,
                   interval=0.03, desc="ArrowDown -> visual row 1 (data)")
        assert_eq(tf.query(f"/{CAT}/external/source_at.1"), 0, "row 1 is the Name source")
        last = tf.query(f"/{CAT}/external/visible_len") - 1
        tf.key(path=GRID, name="End")
        wait_until(lambda: tf.query(f"/{CAT}/external/cursor") == last, timeout=4.0,
                   interval=0.03, desc="End -> last visual row")
        tf.key(path=GRID, name="ArrowDown")
        time.sleep(PAUSE)
        assert_eq(tf.query(f"/{CAT}/external/cursor"), last, "ArrowDown at bottom clamps")

        # ── (F) editing inside a category still works ────────────────
        # Toggle the Visible bool (source 2) by its stable index. `toggle`
        # returns whether it toggled a bool (true), not the new value.
        assert_eq(tf.query("/external/value.2"), True, "Visible true before")
        assert_eq(tf.invoke("/external/toggle", 2), True, "toggle source 2 toggled a bool")
        assert_eq(tf.query("/external/value.2"), False, "Visible toggled to false")
        # Keyboard edit a text row deep in a category (Tag, source 1).
        _focus_grid(tf)
        _cursor_to_source(tf, 1)
        tf.key(path=GRID, name="Enter")
        wait_until(lambda: tf.query("/external/editing") == 1, timeout=4.0,
                   interval=0.03, desc="Enter edits the Tag row inside Identity")
        wait_until(lambda: tf.request("focus/get").result.get("focused") == EDIT,
                   timeout=4.0, interval=0.03, desc="focus moved into the inline field")
        for _ in range(4):  # clear "hero"
            tf.key(path=EDIT, name="Backspace")
        tf.text("boss", path=EDIT)
        tf.key(path=EDIT, name="Enter")
        wait_until(lambda: tf.query("/external/value.1") == "boss", timeout=4.0,
                   interval=0.03, desc="commit the edited Tag inside the category")

        # ── (G) paint: collapse hides data rows, keeps the header ────
        expanded = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert find_by_tag(expanded, f"{GRID}#0") is not None, "Name row painted when expanded"
        assert find_by_tag(expanded, f"{CAT}#0") is not None, "Identity header painted"
        tf.intervene(f"/{CAT}/external/collapsed.0", True)
        wait_until(
            lambda: find_by_tag(tf.snapshot(source="paint", viewport=VIEWPORT), f"{GRID}#0") is None,
            timeout=4.0, interval=0.05, desc="Name row hidden when Identity collapses")
        collapsed = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert find_by_tag(collapsed, f"{CAT}#0") is not None, "header stays on collapse"
        assert find_by_tag(collapsed, f"{GRID}#1") is None, "Tag row hidden too"


if __name__ == "__main__":
    sys.exit(run_demo("hello-property-grid R871 §5.27 collapsible category sections", body))
