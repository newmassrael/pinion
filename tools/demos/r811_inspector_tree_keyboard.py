#!/usr/bin/env python3
"""R811 §5.16 §5.27 §5.50 — DevTools inspector tree keyboard navigation.

Drives `hello-dock-panels` over JSON-RPC and verifies the second
consumer of the lifted `pinion_core::widgets::tree_nav` substrate
(`hello-tree-view` is the first): the DevTools inspector tree now has
WAI-ARIA APG 6.13 keyboard navigation + expand/collapse, gated on the
inspector tree holding keyboard focus.

Everything is observed from `scene/snapshot from=paint` as scene-as-data
(§2 #7 — no pixels): the visible row set is the inspector's
`inspector_tree#{id}` composite row tags, and the keyboard cursor is the
row carrying the M3 focus state-layer fill (selection-follows-focus, so
arrows move the shared `selected_path` the inspector rings).

Verification scope (>=30 assertions):

  (A) substrate shape — `inspector_tree` External present; the boot
      tree is fully expanded (6 rows) with the expected composite tags.
  (B) focus gate — keys before `focus/set` do NOT navigate the
      inspector (the viewport button keeps Space/Enter when it holds
      focus); after `focus/set` they do (routing vs focus axes).
  (C) vertical nav — Down/Up step, Home/End jump, Up at the top and
      Down at the bottom clamp (a tree has ends — no wrap).
  (D) descend / ascend — Arrow Right on an expanded branch descends to
      its first child; Arrow Left on a leaf ascends to the parent.
  (E) expand / collapse — Arrow Left collapses a branch (its
      descendants drop out of the visible row set); Arrow Right
      re-expands it.
  (F) Space / Enter toggle a branch.
  (G) type-ahead — a printable key jumps to the next matching row.
  (H) Page Down / Page Up clamp to the ends.
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

TREE_TAG = "inspector_tree"
ROW_PREFIX = "inspector_tree#"

# The boot inspector tree (fully expanded) — the two synthetic top rows
# (`state` leaf + `viewport` branch) plus the live viewport-scene
# projection underneath.
EXPECT_BOOT_ROWS = [
    "inspector_tree#state",
    "inspector_tree#viewport",
    "inspector_tree#Container",
    "inspector_tree#Container/Text[0]",
    "inspector_tree#Container/Container[viewport_btn]",
    "inspector_tree#Container/Container[viewport_btn]/Text[0]",
]


def _collect(node, out, pred):
    if isinstance(node, dict):
        tag = node.get("tag")
        if isinstance(tag, str) and pred(tag):
            out.append(node)
        for child in node.get("children") or []:
            _collect(child, out, pred)
        content = node.get("content")
        if isinstance(content, dict):
            _collect(content, out, pred)


def _rows(snap) -> list[str]:
    """The visible inspector row tags, in paint (depth-first) order."""
    out: list[dict] = []
    _collect(snap, out, lambda t: t.startswith(ROW_PREFIX))
    return [n["tag"] for n in out]


def _focused_row(snap) -> str | None:
    """The row carrying the M3 focus state-layer fill (opaque) — the
    keyboard cursor / selection (selection-follows-focus)."""
    out: list[dict] = []
    _collect(snap, out, lambda t: t.startswith(ROW_PREFIX))
    for node in out:
        fill = (node.get("style") or {}).get("fill") or {}
        if fill.get("a", 0) == 255:
            return node["tag"]
    return None


class Checks:
    def __init__(self) -> None:
        self.n = 0

    def eq(self, actual, expected, label) -> None:
        assert_eq(actual, expected, label)
        self.n += 1

    def ok(self, cond, label) -> None:
        if not cond:
            raise AssertionError(f"{label}: expected truthy")
        self.n += 1


def body() -> None:
    c = Checks()
    with RpcSubprocess("hello-dock-panels", boot_grace=1.0) as r:
        paint = lambda: r.snapshot(source="paint")  # noqa: E731
        last = EXPECT_BOOT_ROWS[-1]

        def expect_focus(expected, label):
            wait_until(lambda: _focused_row(paint()) == expected, desc=label)
            c.eq(_focused_row(paint()), expected, label)

        def expect_rowcount(count, label):
            wait_until(lambda: len(_rows(paint())) == count, desc=label)
            c.eq(len(_rows(paint())), count, label)

        # ── (A) substrate shape ────────────────────────────────────
        state_scene = r.snapshot()  # default = state scene
        c.ok(find_by_tag(state_scene, TREE_TAG) is not None,
             "inspector_tree External registered in the state scene")
        boot = paint()
        c.eq(_rows(boot), EXPECT_BOOT_ROWS, "boot rows fully expanded")
        c.eq(len(_rows(boot)), 6, "boot visible row count")
        c.ok(find_by_tag(boot, TREE_TAG) is not None,
             "inspector_tree container present in paint")
        c.eq(_focused_row(boot), None, "no row focused at boot (selection None)")

        # ── (B) focus gate — keys before focus/set do not navigate ──
        r.key(path=TREE_TAG, name="ArrowDown")
        r.key(path=TREE_TAG, name="End")
        # No focus on the inspector yet → selection unchanged (None).
        c.eq(_focused_row(paint()), None,
             "keys before focus/set do not navigate the inspector")

        r.request("focus/set", {"tag": TREE_TAG})
        # First Arrow Down from an empty cursor lands on the first row.
        r.key(path=TREE_TAG, name="ArrowDown")
        expect_focus("inspector_tree#state",
                     "ArrowDown after focus/set lands on the first row (gate opens)")

        # ── (C) vertical nav: step / clamp / Home / End ────────────
        r.key(path=TREE_TAG, name="ArrowDown")
        expect_focus("inspector_tree#viewport", "ArrowDown steps to next visible row")
        r.key(path=TREE_TAG, name="ArrowUp")
        expect_focus("inspector_tree#state", "ArrowUp steps to previous visible row")
        r.key(path=TREE_TAG, name="ArrowUp")  # clamp at top
        c.eq(_focused_row(paint()), "inspector_tree#state",
             "ArrowUp clamps at first row (no wrap)")
        r.key(path=TREE_TAG, name="End")
        expect_focus(last, "End jumps to last visible row")
        r.key(path=TREE_TAG, name="ArrowDown")  # clamp at bottom
        c.eq(_focused_row(paint()), last, "ArrowDown clamps at last row (no wrap)")
        r.key(path=TREE_TAG, name="Home")
        expect_focus("inspector_tree#state", "Home jumps to first visible row")

        # ── (H) Page Down / Page Up clamp (page > listing) ─────────
        r.key(path=TREE_TAG, name="PageDown")
        expect_focus(last, "PageDown clamps to the last row")
        r.key(path=TREE_TAG, name="PageUp")
        expect_focus("inspector_tree#state", "PageUp clamps to the first row")

        # ── (D) descend: Arrow Right on an expanded branch ─────────
        # Move to `viewport` (an expanded branch) and descend.
        r.key(path=TREE_TAG, name="ArrowDown")
        expect_focus("inspector_tree#viewport", "cursor on viewport branch")
        r.key(path=TREE_TAG, name="ArrowRight")
        expect_focus("inspector_tree#Container",
                     "ArrowRight on an expanded branch descends to its first child")
        # ── (D) ascend: Arrow Left on a *leaf* ascends to the parent ─
        # Step onto the Container's first child (a Text leaf), then
        # Arrow Left — a leaf has nothing to collapse, so it ascends.
        r.key(path=TREE_TAG, name="ArrowDown")
        expect_focus("inspector_tree#Container/Text[0]",
                     "ArrowDown steps into the Container's first child (a leaf)")
        r.key(path=TREE_TAG, name="ArrowLeft")
        expect_focus("inspector_tree#Container",
                     "ArrowLeft on a leaf ascends to the parent branch")

        # ── (E) collapse / expand changes the visible row set ──────
        # Step up to `viewport` (an expanded branch) and Arrow Left
        # collapse it → its whole subtree drops out of the visible rows.
        r.key(path=TREE_TAG, name="ArrowUp")
        expect_focus("inspector_tree#viewport", "ArrowUp back to the viewport branch")
        r.key(path=TREE_TAG, name="ArrowLeft")
        wait_until(
            lambda: _rows(paint()) == ["inspector_tree#state", "inspector_tree#viewport"],
            desc="collapsing viewport hides its descendants",
        )
        snap_c = paint()
        c.eq(_rows(snap_c), ["inspector_tree#state", "inspector_tree#viewport"],
             "Arrow Left collapses the viewport branch (visible set shrinks)")
        c.ok("inspector_tree#Container" not in _rows(snap_c),
             "collapsed branch's descendants absent from the visible set")
        c.eq(_focused_row(snap_c), "inspector_tree#viewport",
             "cursor stays on the collapsed branch")
        # Arrow Right re-expands.
        r.key(path=TREE_TAG, name="ArrowRight")
        expect_rowcount(6, "Arrow Right re-expands the viewport branch (6 rows again)")

        # ── (F) Space / Enter toggle a branch ──────────────────────
        # Cursor on `viewport` (expanded). Space collapses it.
        r.key(path=TREE_TAG, name="Space")
        expect_rowcount(2, "Space toggles the focused branch closed")
        # Enter toggles it back open.
        r.key(path=TREE_TAG, name="Enter")
        expect_rowcount(6, "Enter toggles the focused branch open")

        # ── (G) type-ahead jumps to the next matching row ──────────
        r.key(path=TREE_TAG, name="Home")
        expect_focus("inspector_tree#state",
                     "reset cursor to first row before type-ahead")
        # 'v' matches the `viewport scene` row label.
        r.key(path=TREE_TAG, name="v")
        expect_focus("inspector_tree#viewport",
                     "type-ahead 'v' jumps to the viewport row")

        # ── (H) leaf semantics — Arrow Right on a leaf is a no-op ───
        r.key(path=TREE_TAG, name="End")
        expect_focus(last, "cursor on a leaf (End)")
        r.key(path=TREE_TAG, name="ArrowRight")
        c.eq(_focused_row(paint()), last,
             "ArrowRight on a leaf is a consumed no-op")

        # Final: the inspector tree is still a single externalised node
        # in the state scene (substrate unchanged by navigation).
        c.ok(find_by_tag(r.snapshot(), TREE_TAG) is not None,
             "inspector_tree External still registered after navigation")

        print(f"[demo] assertions: {c.n}")
        if c.n < 30:
            raise AssertionError(f"assertion budget not met: {c.n} < 30")


if __name__ == "__main__":
    sys.exit(run_demo("r811_inspector_tree_keyboard", body))
