#!/usr/bin/env python3
"""R1609 §5.21 §5.39 §5.40 — a tile board is editable without a pointer, and an
edge is a handle.

R1608 shipped the board's drag and left the gesture debt open on two axes: a
resize existed only as a wire verb with no handle to grab, and there was **no
keyboard channel at all** — so the assistive-technology nodes R1608 added let a
screen-reader user *read* the arrangement and never change it. Both close here,
and they close together, because they are the same derivation: move one edge of a
card to a grid line and hold the opposite edge still.

the toolkit's floor for this is MDI child window, which is a real floor — it has keyboard
move *and* resize. Measured against `the toolkit's widget module/src/widgets/widgets/` in the toolkit 6.11.1,
what this proves over the wire that the toolkit cannot do or does not do:

* **No mode.** the toolkit's keyboard editing lives behind `isInInteractiveMode`, entered
  only from the system menu (`_q_enterInteractiveMode` casts `q->sender()` to a
  action and returns unless it is `MoveAction` or `ResizeAction`), so the same
  arrow key means different things depending on state the user cannot see. Here
  the chord says which, and every chord is one flat vocabulary.
* **Escape cancels, and it restores the whole board.** the toolkit saves `oldGeometry` on
  entering interactive mode and never reads it back: `Key_Escape`, `Key_Return`
  and `Key_Enter` all fall to one `leaveInteractiveMode()`, so Escape *commits*.
  And even restoring the rectangle would not be enough, because a move displaces
  *other* cards — which this asserts.
* **A session's reflow is a difference, not a sum.** Several chords that walk one
  card past another push that card once; the per-chord reflows over-count it.
* **Arrow navigation is spatial and total.** MDI area offers
  `activateNextSubWindow`, a walk down a list in creation order, so it has no
  notion of direction. Every arrow's destination is published here.
* **The handle set is enumerable, with the cursor each derives.**
  `Operation` is in a `_p.h` and its region/cursor map is
  private, so no caller can ask a toolkit subwindow what handles it has.
* **The displacement is ANNOUNCED.** the toolkit 6.8 added
  accessible announcement event, and no widget in its widget module fires
  one: the three translation units implementing its MDI child window, its MDI
  area and its size grip contain no
  accessibility notification of any kind, so a toolkit MDI window that moves is silent
  even though `state()` advertises `movable` and
  `sizeable`. Here the board carries an `aria-live` region, which is *readable*
  (§2 #7) precisely because it is declared rather than fired.
* **The keyboard reaches it through the platform path**, not only the verb: a
  real `scene/key` injection with modifiers drives the same keymap.
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
)

WIN = (760, 420)
EXT = "/external"
BOARD = "dashboard"
REFLOW = "dashboard.reflow"

SEED = (
    "throughput@0,0+12x1 latency@0,1+6x1 loss@6,1+6x1 "
    "topology@0,2+4x2 alarms@4,2+8x1"
)

HANDLES = (
    "Left",
    "Right",
    "Top",
    "Bottom",
    "TopLeft",
    "TopRight",
    "BottomLeft",
    "BottomRight",
)


def q(tf: RpcSubprocess, path: str):
    return tf.query(f"{EXT}/{path}")


def inv(tf: RpcSubprocess, path: str, args):
    return tf.invoke(f"{EXT}/{path}", args)


def chord(tf: RpcSubprocess, spelled: str) -> bool:
    """One keyboard chord through the External's own `key` verb."""
    return inv(tf, "key", spelled)


def refused(tf: RpcSubprocess, path: str, args) -> None:
    try:
        inv(tf, path, args)
    except Exception:  # noqa: BLE001 — the refusal is the expected outcome
        return
    raise AssertionError(f"{path}({args!r}) was accepted and should not have been")


def body() -> None:
    with RpcSubprocess("hello-tile-dashboard", boot_grace=1.5) as tf:
        # ── (A) the board has a current card before anything is pressed ─────
        assert_eq(q(tf, "tiles"), SEED, "A: the seed board")
        assert_eq(
            q(tf, "current"),
            "throughput",
            "A: a fresh board already has a current card, so a Tab into it can "
            "act immediately — the toolkit needs a subwindow activated first",
        )
        assert_eq(q(tf, "editing"), "", "A: and no edit session is open yet")
        assert_eq(q(tf, "handle"), "", "A: nothing is grabbed")

        # ── (B) every handle is enumerable, with the cursor it derives ───────
        handles = q(tf, "handles")
        for name in HANDLES:
            assert name in handles, f"B: {name} missing from {handles}"
        assert_eq(
            handles,
            "Left:ColResize Right:ColResize Top:RowResize Bottom:RowResize "
            "TopLeft:NwseResize TopRight:NeswResize BottomLeft:NeswResize "
            "BottomRight:NwseResize",
            "B: ★ eight handles, and each one's CURSOR is derived from which "
            "axes it moves — the toolkit writes the same four cursors as literal values "
            "in nine private `operationMap.insert` rows, and no caller can "
            "enumerate them because the enum lives in a `_p.h`",
        )

        # ── (C) arrow navigation is spatial, and every destination is read ───
        assert_eq(inv(tf, "select", "loss"), "loss", "C: select a card by name")
        assert_eq(
            q(tf, "neighbours"),
            "Left:latency Right:- Up:throughput Down:alarms",
            "C: ★ where each arrow key would go — the spatial relation "
            "`activateNextSubWindow` does not have, since it walks creation order",
        )
        assert chord(tf, "ArrowLeft"), "C: a plain arrow is the board's"
        assert_eq(q(tf, "current"), "latency", "C: and it moved the SELECTION")
        assert_eq(q(tf, "tiles"), SEED, "C: navigating moved no card")
        assert chord(tf, "ArrowLeft"), "C: nothing further left"
        assert_eq(
            q(tf, "current"),
            "latency",
            "C: the selection holds rather than wrapping somewhere the arrow "
            "did not point",
        )

        # ── (D) the keymap has no mode: Shift moves, Alt resizes ────────────
        assert_eq(inv(tf, "select", "topology"), "topology", "D: take the tall card")
        assert chord(tf, "Shift+ArrowRight"), "D: Shift+arrow is Move"
        assert "topology@1,2+4x2" in q(tf, "tiles"), f"D: moved: {q(tf, 'tiles')}"
        assert chord(tf, "Alt+ArrowRight"), "D: Alt+arrow is Grow"
        assert "topology@1,2+5x2" in q(tf, "tiles"), f"D: grew: {q(tf, 'tiles')}"
        assert chord(tf, "Alt+Shift+ArrowRight"), "D: Alt+Shift+arrow is Shrink"
        assert "topology@1,2+4x2" in q(tf, "tiles"), f"D: shrank: {q(tf, 'tiles')}"
        assert chord(tf, "Alt+ArrowLeft"), "D: growing the LEFT side"
        assert "topology@0,2+5x2" in q(tf, "tiles"), (
            "D: ★ one edge moved and the other held, so the column and the width "
            f"changed together: {q(tf, 'tiles')}"
        )
        assert_eq(q(tf, "violations"), 0, "D: and the board stayed legal throughout")

        # ── (E) the session is readable, and Escape restores the WHOLE board ─
        assert_eq(
            q(tf, "editing"),
            "topology",
            "E: the first editing chord opened a session — no menu round trip, "
            "which is the only way into the toolkit's interactive mode",
        )
        assert chord(tf, "Escape"), "E: Escape is the board's"
        assert_eq(
            q(tf, "tiles"),
            SEED,
            "E: ★ Escape restored the cards the session DISPLACED as well as the "
            "one being edited. The toolkit stores `oldGeometry` and never reads it back — "
            "Escape, Return and Enter share one `leaveInteractiveMode()` arm there",
        )
        assert_eq(q(tf, "editing"), "", "E: and the session closed")
        assert "restored" in q(tf, "announcement"), (
            f"E: the cancel is announced: {q(tf, 'announcement')}"
        )

        # ── (F) a session's reflow is a DIFFERENCE, not a sum ────────────────
        assert_eq(inv(tf, "select", "throughput"), "throughput", "F: the header")
        for _ in range(3):
            assert chord(tf, "Shift+ArrowDown"), "F: walk it down through the board"
        session = q(tf, "session_reflow")
        assert session.count("latency") == 1, (
            "F: ★ a card the session pushed on every press appears ONCE in the "
            f"difference; summing the per-press reflows would count it thrice: {session}"
        )
        assert "latency:1>" in session, (
            f"F: and it names where the session STARTED, not the last hop: {session}"
        )
        assert q(tf, "last_reflow") != session, (
            "F: the last press's reflow and the whole session's difference are "
            f"different answers: {q(tf, 'last_reflow')} vs {session}"
        )
        assert chord(tf, "Enter"), "F: Enter commits"
        assert_eq(q(tf, "editing"), "", "F: closing the session")
        assert "committed" in q(tf, "announcement"), (
            f"F: and saying so: {q(tf, 'announcement')}"
        )
        committed = q(tf, "tiles")
        assert chord(tf, "Escape"), "F: Escape after a commit is accepted"
        assert_eq(
            q(tf, "tiles"),
            committed,
            "F: and has nothing to restore rather than reverting an older session",
        )

        # ── (G) a chord at a bound STOPS and says so ─────────────────────────
        assert_eq(inv(tf, "select", "throughput"), "throughput", "G: 12 of 12 wide")
        before = q(tf, "tiles")
        for spelled in ("Shift+ArrowLeft", "Alt+ArrowRight"):
            assert chord(tf, spelled), f"G: {spelled} is the board's"
            assert_eq(q(tf, "tiles"), before, f"G: {spelled} moved nothing")
            assert "already at the edge" in q(tf, "announcement"), (
                f"G: ★ a held key at a bound STOPS and the stop is announced, "
                f"rather than repeating an unchanged slot: {q(tf, 'announcement')}"
            )

        # ── (H) an unknown chord is DECLINED, so the shell keeps Tab ─────────
        assert chord(tf, "Tab") is False, "H: Tab belongs to the focus ring"
        assert chord(tf, "PageDown") is False, "H: and so does paging"
        assert chord(tf, "Hyper+ArrowLeft") is False, (
            "H: an unrecognised modifier makes the WHOLE chord unrecognised "
            "rather than being skipped into a different gesture"
        )

        # ── (I) all eight handles are drivable by name ───────────────────────
        assert_eq(inv(tf, "select", "topology"), "topology", "I: take the tall card")
        for name in HANDLES:
            inv(tf, "drag_handle", f"topology,{name},3,3")
            assert_eq(q(tf, "violations"), 0, f"I: {name} left the board legal")
        assert_eq(
            inv(tf, "drag_handle", "topology,bottomright,5,3"),
            q(tf, "last_reflow"),
            "I: the reply is the reflow, and the name is case-insensitive",
        )
        after = q(tf, "tiles")
        assert "topology@" in after, f"I: the card is still there: {after}"
        refused(tf, "drag_handle", "topology,Middle,2,2")
        refused(tf, "drag_handle", "topology,Top,2")
        refused(tf, "drag_handle", "ghost,Top,2,2")
        assert_eq(
            q(tf, "tiles"),
            after,
            "I: ★ a handle outside the published set is refused, so the accepted "
            "and advertised vocabularies cannot drift — and no refusal moved a card",
        )
        refused(tf, "select", "ghost")
        assert_eq(q(tf, "current"), "topology", "I: a refused select changed nothing")

        # ── (J) the platform key path drives the same keymap ─────────────────
        snap = wait_snap(
            tf,
            lambda s: find_by_tag(s, BOARD) is not None,
            viewport=WIN,
            desc="the board container is painted",
        )
        assert find_by_tag(snap, BOARD) is not None, "J: the board is painted"
        tf.request("focus/set", {"tag": BOARD})
        assert_eq(
            tf.request("focus/get", {}).result.get("focused"),
            BOARD,
            "J: ★ ONE focus stop for the whole board — five cards are not five "
            "Tab stops, which is why the arrows have to be spatial",
        )
        assert_eq(inv(tf, "select", "latency"), "latency", "J: pick a movable card")
        placed = q(tf, "tiles")
        tf.key(path=BOARD, name="ArrowRight")
        assert_eq(
            q(tf, "current"),
            "loss",
            "J: a REAL key injection reached the same keymap the verb drives, "
            "so the platform path and the agent path are one keyboard model",
        )
        assert_eq(q(tf, "tiles"), placed, "J: and navigating still moved no card")

        # ── (K) the displacement is ANNOUNCED through a live region ──────────
        # ★ The expectation is DERIVED from the model's own reflow rather than
        # spelled out: the first draft hard-coded a sentence and failed, because
        # the sections above had legitimately rearranged the board. Walking a card
        # up until it lands on another one is state-independent, and asserting the
        # announcement against `last_reflow` is a stronger claim than a literal —
        # it says the sentence names EXACTLY the cards that moved.
        # A full-width card at row 0 is a wall nothing can pass, so the walk is
        # guaranteed to collide however the sections above left the board.
        inv(tf, "resize", "throughput,12,1")
        inv(tf, "move_to", "throughput,0,0")
        inv(tf, "resize", "topology,4,1")
        inv(tf, "move_to", "topology,0,9")
        assert_eq(inv(tf, "select", "topology"), "topology", "K: the tall card")
        pushed = ""
        for _ in range(12):
            assert chord(tf, "Shift+ArrowUp"), "K: walk it up"
            if q(tf, "last_reflow") != "clean":
                pushed = q(tf, "last_reflow")
                break
        assert pushed, (
            "K: walking a card up into a full-width one must displace it; "
            f"board is {q(tf, 'tiles')}"
        )
        said = q(tf, "announcement")
        assert "topology at column" in said, f"K: it states the card's slot: {said}"
        assert "pushed" in said, (
            "K: ★ and names what moved OUT OF THE WAY — the half a per-card value "
            f"cannot carry, because a displaced card is not the one the user is on: {said}"
        )
        for entry in pushed.split(", "):
            name = entry.split(":")[0]
            assert name in said, (
                f"K: {name} was displaced and the announcement does not name it: {said}"
            )

        access = tf.request("scene/access", {}).result
        region = access_node_by_tag(access, REFLOW)
        assert region is not None, "K: the live region is in the AT tree"
        assert_eq(
            region.get("live"),
            "polite",
            "K: ★ declared as `aria-live`, which is why it is READABLE at all. "
            "the toolkit's accessible announcement event is fired and leaves no trace, and "
            "no widget in the toolkit's widget module/src/widgets fires one anyway",
        )
        assert_eq(
            (region.get("value") or {}).get("text"),
            said,
            "K: and the region carries exactly the sentence the wire reports, so "
            "there is one announcement rather than two that could disagree",
        )
        assert_eq(
            region.get("role"),
            "status",
            "K: as a `status` region, the ARIA role for a polite consequence",
        )

        # ── (L) the AT tree names the current card as the active descendant ──
        board_node = access_node_by_tag(access, BOARD)
        assert board_node is not None, "L: the board is in the AT tree"
        assert_eq(
            len(board_node.get("children") or []),
            5,
            "L: every card is declared a child, which is what resolves the "
            "active descendant",
        )
        assert_eq(
            (access.get("focus") or {}).get("active_descendant"),
            "dashboard#card.topology",
            "L: ★ the roving current card reaches an AT as "
            "`aria-activedescendant`, so a screen reader says which card the "
            "keyboard will act on. The toolkit's interactive mode is private, so nothing "
            "there can tell an AT which subwindow the arrows would move",
        )
        selected = [
            node.get("tag")
            for node in access.get("nodes") or ()
            if node.get("selected") is True
        ]
        assert_eq(selected, ["dashboard#card.topology"], "L: exactly one current card")

        card = access_node_by_tag(access, "dashboard#card.topology")
        assert card is not None, "L: the card itself is in the tree"
        assert q(tf, "announcement").startswith("topology at column"), (
            "L: and the region's slot numbers are the SAME one-based lines the "
            f"per-card value uses ({card.get('value')}) — a user hearing "
            "'column 5' from one channel and 'column 4' from the other has no "
            "way to tell which is the board's"
        )

        # ── (M) a grip ring is painted, and only on the current card ─────────
        snap = wait_snap(
            tf,
            lambda s: find_by_tag(s, "dashboard#card.topology.handle.TopLeft") is not None,
            viewport=WIN,
            desc="the selected card paints its handle ring",
        )
        for name in HANDLES:
            assert find_by_tag(snap, f"dashboard#card.topology.handle.{name}") is not None, (
                f"M: the {name} grip is painted and addressable"
            )
        for other in ("throughput", "latency", "loss", "alarms"):
            assert find_by_tag(snap, f"dashboard#card.{other}.handle.TopLeft") is None, (
                f"M: ★ only the current card shows grips — {other} would make "
                "this a forty-grip board, and a card that showed no grip must "
                "not resize either, which is the fact the hit-test reads too"
            )
        assert_eq(q(tf, "violations"), 0, "M: with the invariant intact throughout")


if __name__ == "__main__":
    run_demo("r1609_tile_board_without_a_pointer", body)
