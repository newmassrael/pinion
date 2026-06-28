#!/usr/bin/env python3
"""R1138 §5.49 §2 #2 — phased / held `scene/drag`: snapshot a held mid-drag.

SCOPE — read this before the assertions. §2 invariant #2 says every input a
human makes must have an RPC peer. A human can press, HOLD a drag mid-gesture,
LOOK at the affordance under the cursor (the insertion line, the drag-image
follower, the dock redock hint), then let go. The pre-R1138 `scene/drag` was
ATOMIC — one call ran press -> march -> release internally and returned only
AFTER the release, so an AI could never snapshot the held mid-drag state. That
gap is what made the R1137 floater redock hint un-verifiable by RPC.

R1138 adds a `phase` selector to `scene/drag` ("full" | "begin" | "move" |
"end"): `begin` presses + marches but HOLDS (no release), `move` re-aims the
held drag, `end` releases. The drag session lives in the router, so it persists
across RPC calls — between `begin` and `end` an AI can `scene/query` the
in-flight preview and `scene/snapshot` the held frame.

The DRIVER is `hello-dnd` (the R742 drag substrate's first consumer): every row
is a drag source + drop target, and `/external/preview` introspects the
in-flight drop as scene-as-data (`{from_visual, insert_at}` while a drag is
live, null at rest). The proof: under a HELD drag the preview is NON-NULL with
the order UNCHANGED (the held mid-drag, now observable), where an atomic drag
leaves the preview null at every RPC-observable moment.

Section roadmap (>=30 assertions across A-F):

  (A) Boot — list root + 4 row tags present, order is identity, the preview is
      null at rest (no drag in flight).
  (B) HELD begin (the §2 #2 closure) — a `phase:"begin"` drag of row 0 over
      row 1's upper gap HOLDS: the preview is now NON-NULL ({from_visual:0,
      insert_at:1}) AND the order is still identity (not released), and the held
      frame snapshots (renderable). This is the mid-drag an atomic drag hides.
  (C) HELD move — a `phase:"move"` re-aims the held drag to row 2's lower gap:
      the preview's insert_at changes (1 -> 3), from_visual stays 0, the order
      is STILL identity (held, not released).
  (D) END — a `phase:"end"` releases: the preview clears to null and the order
      finally reorders to [1,2,0,3] (the last-aimed gap settles).
  (E) Atomic contrast + regression — a plain `phase:"full"` drag still reorders
      in one call, and the preview is null both before and after (its mid-state
      was never RPC-observable — exactly the gap `begin`/`end` closes).
  (F) Integrity — a 2nd held begin->end cycle works and the preview is null at
      rest again; an unknown phase rejects loudly (no silent full-arc drag).
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_until,
)

EXAMPLE = "hello-dnd"
VIEWPORT = (300, 260)
TAG = "dnd"
ROWS = [f"dnd#{i}" for i in range(4)]
IDENTITY = [0, 1, 2, 3]


# ─── helpers ─────────────────────────────────────────────────────────


def _order(tf: RpcSubprocess) -> list:
    return tf.query("/external/order")


def _preview(tf: RpcSubprocess) -> Any:
    return tf.query("/external/preview")


def _row_rects(tf: RpcSubprocess) -> dict:
    snap = tf.snapshot(source="paint", viewport=VIEWPORT)
    return abs_rects_of(snap)


def _center(rects: dict, tag: str) -> tuple:
    x, y, w, h = rects[tag]
    return (float(x + w // 2), float(y + h // 2))


def _gap(rects: dict, tag: str, frac_y: float) -> tuple:
    """A point at vertical fraction `frac_y` of `tag`'s rect (`<0.5` = upper
    half = insert above, `>0.5` = lower half = insert below)."""
    x, y, w, h = rects[tag]
    return (float(x + w // 2), float(y + int(h * frac_y)))


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        rects = _row_rects(tf)

        # ── (A) boot shape ──────────────────────────────────────────
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert find_by_tag(snap, TAG) is not None, "A.1 list root present at boot"
        for r in ROWS:
            assert find_by_tag(snap, r) is not None, f"A.2 row {r} present at boot"
        assert_eq(_order(tf), IDENTITY, "A.3 boot order is identity")
        assert _preview(tf) is None, "A.4 no drag preview at rest"

        # ── (B) HELD begin — the §2 #2 closure ──────────────────────
        # Press row 0, march over row 1's UPPER gap, then HOLD (no release).
        tf.drag(
            from_at=_center(rects, "dnd#0"),
            to_at=_gap(rects, "dnd#1", 0.25),
            steps=6,
            phase="begin",
        )
        wait_until(
            lambda: _preview(tf) is not None,
            desc="B.1 the held drag exposes a non-null preview",
        )
        held = _preview(tf)
        assert isinstance(held, dict), f"B.2 preview is a JSON object, got {held!r}"
        assert_eq(held.get("from_visual"), 0, "B.3 preview names the pressed row (0)")
        assert_eq(held.get("insert_at"), 1, "B.4 held over row 1's upper gap = insert_at 1")
        # The headline: the drag is HELD — the order has NOT changed yet, but the
        # mid-drag is observable (an atomic drag would have released + cleared it).
        assert_eq(_order(tf), IDENTITY, "B.5 a held drag has not reordered (not released)")
        held_snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert find_by_tag(held_snap, TAG) is not None, "B.6 the held frame still renders the list"
        for r in ROWS:
            assert find_by_tag(held_snap, r) is not None, f"B.7 row {r} present mid-hold ({r})"

        # ── (C) HELD move — re-aim without releasing ────────────────
        # March the held drag to row 2's LOWER gap; the preview re-aims.
        tf.drag(
            from_at=_gap(rects, "dnd#1", 0.25),
            to_at=_gap(rects, "dnd#2", 0.75),
            steps=6,
            phase="move",
        )
        wait_until(
            lambda: (_preview(tf) or {}).get("insert_at") == 3,
            desc="C.1 the move re-aims the held preview to insert_at 3",
        )
        moved = _preview(tf)
        assert moved is not None, "C.2 the drag is still held after a move"
        assert_eq(moved.get("from_visual"), 0, "C.3 the pressed row is unchanged by a move")
        assert moved.get("insert_at") != held.get("insert_at"), "C.4 insert_at changed (1 -> 3)"
        assert_eq(_order(tf), IDENTITY, "C.5 a move still has not reordered (held)")

        # ── (D) END — release settles the held drag ─────────────────
        tf.drag(
            from_at=_gap(rects, "dnd#2", 0.75),
            to_at=_gap(rects, "dnd#2", 0.75),
            steps=0,
            phase="end",
        )
        wait_until(
            lambda: _order(tf) == [1, 2, 0, 3],
            desc="D.1 the release settles the reorder at the last-aimed gap",
        )
        assert_eq(_order(tf), [1, 2, 0, 3], "D.2 order reordered to [1,2,0,3]")
        assert _preview(tf) is None, "D.3 the preview clears after the release"

        # ── (E) atomic contrast + regression ────────────────────────
        # A plain "full" drag still works in one call — and its mid-state was
        # never RPC-observable (the preview is null before AND after), which is
        # exactly the gap begin/end closes.
        before = _order(tf)
        assert _preview(tf) is None, "E.1 preview null before the atomic drag"
        tf.drag(
            from_at=_center(rects, "dnd#3"),
            to_at=_gap(rects, "dnd#0", 0.25),
            steps=8,
            phase="full",
        )
        wait_until(
            lambda: _order(tf) != before,
            desc="E.2 the atomic full drag reorders in one call",
        )
        assert _order(tf) != before, "E.3 the full drag changed the order"
        assert _preview(tf) is None, "E.4 preview null after the atomic drag (never observable mid-arc)"

        # ── (F) integrity — 2nd held cycle + unknown-phase rejection ─
        rest = _order(tf)
        tf.drag(
            from_at=_center(rects, "dnd#0"),
            to_at=_gap(rects, "dnd#1", 0.75),
            steps=6,
            phase="begin",
        )
        wait_until(
            lambda: _preview(tf) is not None,
            desc="F.1 a 2nd held begin re-opens an observable preview",
        )
        assert _preview(tf) is not None, "F.2 the 2nd held drag is observable"
        assert_eq(_order(tf), rest, "F.3 still not reordered (held)")
        tf.drag(
            from_at=_gap(rects, "dnd#1", 0.75),
            to_at=_gap(rects, "dnd#1", 0.75),
            steps=0,
            phase="end",
        )
        wait_until(
            lambda: _preview(tf) is None,
            desc="F.4 the 2nd end releases the held drag",
        )
        assert _preview(tf) is None, "F.5 preview null at rest again"
        # An out-of-vocabulary phase rejects loudly (no silent full-arc drag).
        raised = False
        try:
            tf.request("scene/drag", {
                "from": {"x": 10.0, "y": 10.0},
                "to": {"x": 50.0, "y": 50.0},
                "phase": "hover",
            })
        except RpcError as exc:
            raised = exc.code != 0
        assert raised, "F.6 an unknown phase is rejected (invalid_params)"

        print("[demo] r1138_held_drag_snapshot: all sections PASS (held mid-drag is RPC-observable)")


if __name__ == "__main__":
    sys.exit(run_demo("r1138_held_drag_snapshot", body))
