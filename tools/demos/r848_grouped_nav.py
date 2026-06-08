#!/usr/bin/env python3
"""R848 §5.27 §5.40 — grouped-collection keyboard navigation.

Drives `hello-grouped-grid` (a `grid`) and `hello-grouped-list` (a `tree`)
over JSON-RPC. R843-R846 made the grouped Model/View collections clickable +
RPC-drivable but **mouse-only**; R848 lands the windowed-roving keyboard axis
the three earlier rounds deferred, as one SSOT controller
(`group_nav_key`) shared by both shapes:

  * Arrow / Home / End rove a cursor over every visible row (headers AND data),
    linear clamp (no wrap).
  * ArrowRight expands a collapsed group header (or steps into its first child);
    ArrowLeft collapses an expanded header (or climbs a data row to its header).
  * Enter / Space toggle a header / re-affirm a data selection.
  * Landing on a data row selects its source (selection-follows-focus) and
    scrolls it into view; the focus ring frames the cursor row.

The decisive witnesses are scene-as-data (§2 #7): the `cursor` query tracks the
roving position, the focus ring is concentric around exactly the cursor's row,
and **cursor ⊥ selection** — the cursor can rest on a group header while a data
row stays selected.

  (A) grid boot — nothing selected, no cursor.
  (B) grid roving — Arrow/Home rove the cursor over headers + data; the ring
      tracks it; data rows select (selection-follows-focus); a header does not.
  (C) grid cursor ⊥ selection — Home onto a header leaves the data selection.
  (D) grid collapse/expand by keyboard — Left collapses an expanded header,
      Right expands a collapsed one, Right steps into a child, Left climbs back.
  (E) grid End — scroll-into-view at scale: the last row materializes.
  (F) grid AI-first — `intervene cursor` and `invoke select` drive the same slots.
  (G) list — the second consumer (a tree): roving + collapse/expand + ring.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    assert_focus_ring_concentric,
    run_demo,
    wait_until,
)

GRID = "hello-grouped-grid"
GRID_TAG = "ggrid"
GRID_GROUP = "ggrp"
GRID_WIN = (400, 540)

LIST = "hello-grouped-list"
LIST_TAG = "glist"
LIST_GROUP = "ggroup"
LIST_WIN = (380, 520)

N = 10_000
NGROUPS = 6
VISIBLE_BOOT = N + NGROUPS  # 6 headers + 10000 data rows
MESH_MEMBERS = len([i for i in range(N) if i % NGROUPS == 0])  # group 0 size


def cursor_of(tf, group_tag):
    return tf.query(f"/{group_tag}/external/{ 'cursor' }")


def visible_len(tf, group_tag):
    return tf.query(f"/{group_tag}/external/visible_len")


def collapsed(tf, group_tag, g):
    return tf.query(f"/{group_tag}/external/collapsed.{g}")


def kind_at(tf, group_tag, pos):
    return tf.query(f"/{group_tag}/external/kind_at.{pos}")


def selected(tf):
    return tf.query("/external/selected")


def wait_cursor(tf, group_tag, expected, desc):
    wait_until(lambda: cursor_of(tf, group_tag) == expected, desc=desc)


def grid_section() -> None:
    with RpcSubprocess(GRID, boot_grace=1.5) as tf:
        # ── (A) boot ────────────────────────────────────────────────
        assert_eq(selected(tf), None, "grid: nothing selected at boot")
        assert_eq(cursor_of(tf, GRID_GROUP), None, "grid: no cursor at boot")
        assert_eq(visible_len(tf, GRID_GROUP), VISIBLE_BOOT, "grid: 6 headers + 10000 rows")
        assert_eq(kind_at(tf, GRID_GROUP, 0), "header", "grid: row 0 is a group header")
        assert_eq(kind_at(tf, GRID_GROUP, 1), "data", "grid: row 1 is a data row")

        # ── (B) roving + selection-follows-focus + focus ring ───────
        tf.request("focus/set", {"tag": GRID_TAG})
        tf.key(path=GRID_TAG, name="ArrowDown")
        wait_cursor(tf, GRID_GROUP, 0, "first ArrowDown lands on the first row (the header)")
        assert_eq(selected(tf), None, "grid: cursor on a header selects nothing")
        # The focus ring frames exactly the cursor's header row.
        snap = tf.snapshot(source="paint", viewport=GRID_WIN)
        assert_eq(
            assert_focus_ring_concentric(snap),
            f"{GRID_GROUP}#0",
            "grid: focus ring frames the cursor's group-0 header",
        )

        tf.key(path=GRID_TAG, name="ArrowDown")
        wait_cursor(tf, GRID_GROUP, 1, "ArrowDown steps onto the first data row")
        assert_eq(selected(tf), 0, "grid: landing on a data row selects its source (0)")
        tf.key(path=GRID_TAG, name="ArrowDown")
        tf.key(path=GRID_TAG, name="ArrowDown")
        wait_cursor(tf, GRID_GROUP, 3, "two more ArrowDowns advance to data row 3")
        assert_eq(selected(tf), 12, "grid: data row 3 is Mesh member source 12")
        tf.key(path=GRID_TAG, name="ArrowUp")
        wait_cursor(tf, GRID_GROUP, 2, "ArrowUp steps back to data row 2")
        assert_eq(selected(tf), 6, "grid: data row 2 is Mesh member source 6")
        # The ring now frames the data row, not the header.
        snap = tf.snapshot(source="paint", viewport=GRID_WIN)
        assert_eq(
            assert_focus_ring_concentric(snap),
            f"{GRID_TAG}#6",
            "grid: focus ring tracked the cursor onto the data row",
        )

        # ── (C) cursor ⊥ selection ──────────────────────────────────
        tf.key(path=GRID_TAG, name="Home")
        wait_cursor(tf, GRID_GROUP, 0, "Home jumps the cursor to the first row (header)")
        assert_eq(selected(tf), 6, "grid: selection survives — cursor on a header, data 6 still selected")

        # ── (D) collapse / expand by keyboard ───────────────────────
        tf.key(path=GRID_TAG, name="ArrowLeft")  # collapse the expanded header
        wait_until(lambda: collapsed(tf, GRID_GROUP, 0) is True, desc="ArrowLeft collapses the header")
        assert_eq(cursor_of(tf, GRID_GROUP), 0, "grid: cursor stays on the header through collapse")
        assert_eq(visible_len(tf, GRID_GROUP), VISIBLE_BOOT - MESH_MEMBERS, "grid: Mesh rows vanish")
        tf.key(path=GRID_TAG, name="ArrowRight")  # expand the collapsed header
        wait_until(lambda: collapsed(tf, GRID_GROUP, 0) is False, desc="ArrowRight expands the header")
        assert_eq(visible_len(tf, GRID_GROUP), VISIBLE_BOOT, "grid: Mesh rows return")
        tf.key(path=GRID_TAG, name="ArrowRight")  # step into the first child
        wait_cursor(tf, GRID_GROUP, 1, "ArrowRight on an expanded header steps into its first child")
        assert_eq(selected(tf), 0, "grid: stepping into the child selects source 0")
        tf.key(path=GRID_TAG, name="ArrowLeft")  # climb back to the header
        wait_cursor(tf, GRID_GROUP, 0, "ArrowLeft on a data row climbs to its group header")

        # ── (E) End — scroll-into-view at scale ─────────────────────
        last_pos = VISIBLE_BOOT - 1
        last_source = max(i for i in range(N) if i % NGROUPS == NGROUPS - 1)
        tf.key(path=GRID_TAG, name="End")
        wait_cursor(tf, GRID_GROUP, last_pos, "End jumps the cursor to the last row")
        assert_eq(selected(tf), last_source, "grid: End selects the last data row's source")

        def last_row_rendered():
            s = tf.snapshot(source="paint", viewport=GRID_WIN)
            return s if f"{GRID_TAG}#{last_source}" in abs_rects_of(s) else None

        snap = wait_until(last_row_rendered, desc="End scrolled the last row into the window")
        assert f"{GRID_TAG}#{last_source}" in abs_rects_of(snap), "grid: last row is now a rendered node"
        assert f"{GRID_TAG}#0" not in abs_rects_of(snap), "grid: the first row has left the window"

        # ── (F) AI-first: intervene cursor + invoke select ──────────
        tf.intervene(f"/{GRID_GROUP}/external/cursor", 1)
        assert_eq(cursor_of(tf, GRID_GROUP), 1, "grid: intervene drives the cursor slot")
        tf.intervene(f"/{GRID_GROUP}/external/cursor", None)
        assert_eq(cursor_of(tf, GRID_GROUP), None, "grid: intervene clears the cursor")
        assert_eq(tf.invoke("/external/select", 42), 42, "grid: invoke select returns the source")
        assert_eq(selected(tf), 42, "grid: RPC selection independent of the cursor")


def list_section() -> None:
    with RpcSubprocess(LIST, boot_grace=1.5) as tf:
        # The second consumer — a grouped *tree* — shares the same controller.
        assert_eq(cursor_of(tf, LIST_GROUP), None, "list: no cursor at boot")
        assert_eq(visible_len(tf, LIST_GROUP), VISIBLE_BOOT, "list: 6 headers + 10000 rows")

        tf.request("focus/set", {"tag": LIST_TAG})
        tf.key(path=LIST_TAG, name="ArrowDown")
        wait_cursor(tf, LIST_GROUP, 0, "list: first ArrowDown lands on the header")
        snap = tf.snapshot(source="paint", viewport=LIST_WIN)
        assert_eq(
            assert_focus_ring_concentric(snap),
            f"{LIST_GROUP}#0",
            "list: focus ring frames the cursor's header",
        )
        tf.key(path=LIST_TAG, name="ArrowDown")
        wait_cursor(tf, LIST_GROUP, 1, "list: ArrowDown steps onto the first data row")
        assert_eq(selected(tf), 0, "list: landing on a data row selects its source")

        # cursor ⊥ selection: Home back onto the header keeps the selection.
        tf.key(path=LIST_TAG, name="Home")
        wait_cursor(tf, LIST_GROUP, 0, "list: Home returns the cursor to the header")
        assert_eq(selected(tf), 0, "list: selection survives the cursor leaving the data row")

        # collapse / expand by keyboard.
        tf.key(path=LIST_TAG, name="ArrowLeft")
        wait_until(lambda: collapsed(tf, LIST_GROUP, 0) is True, desc="list: ArrowLeft collapses the header")
        assert_eq(visible_len(tf, LIST_GROUP), VISIBLE_BOOT - MESH_MEMBERS, "list: collapsed group hides its rows")
        tf.key(path=LIST_TAG, name="ArrowRight")
        wait_until(lambda: collapsed(tf, LIST_GROUP, 0) is False, desc="list: ArrowRight expands the header")
        assert_eq(visible_len(tf, LIST_GROUP), VISIBLE_BOOT, "list: expand restores every row")


def body() -> None:
    grid_section()
    list_section()


if __name__ == "__main__":
    sys.exit(run_demo("R848 §5.27 §5.40 — grouped-collection keyboard navigation", body))
