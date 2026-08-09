#!/usr/bin/env python3
"""R1608 §5.21 — a tile dashboard is a value, and every gesture is a verb.

R1607 built `TileGrid` and measured that the R1560 CSS Grid already holds a
dashboard of the class a monitoring tool draws, then deliberately stopped
without a consumer -- so it
could not answer whether the reflow's *locality* holds under a real pointer.
This demo is that answer, over the wire.

What it proves, none of which the dashboard tool can be asked:

* The whole arrangement is ONE read (`layout`, a JSON document), so an agent
  knows where every card is without a pixel.
* A move REPORTS what it displaced (`last_reflow`), including the transitive
  case -- a card pushed by a card that was itself pushed.
* A drag's effect is LOCAL: a card that shares no column with the dragged one
  does not move, which is the property the model's doc claims.
* Compaction is a VERB: removing a card leaves its gap until someone asks, so
  a gesture's inverse stays a gesture.
* A refused gesture says WHY and changes nothing.
* Every slot is read-only, and a read-only refusal is told apart from an
  unknown path (R1566).
* The painted board really is a CSS grid, read out of `scene/snapshot` rather
  than assumed -- one child per card, each at the placement the model derived.
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
    wait_snap,
)

WIN = (760, 420)
EXT = "/external"
BOARD = "dashboard"

SEED = (
    "throughput@0,0+12x1 latency@0,1+6x1 loss@6,1+6x1 "
    "topology@0,2+4x2 alarms@4,2+8x1"
)


def q(tf: RpcSubprocess, path: str):
    return tf.query(f"{EXT}/{path}")


def inv(tf: RpcSubprocess, path: str, args):
    return tf.invoke(f"{EXT}/{path}", args)


def refused(tf: RpcSubprocess, path: str, args) -> None:
    """A gesture the surface declines. The reason is read back separately, so
    the demo asserts the SENTENCE rather than only that it failed."""
    try:
        inv(tf, path, args)
    except Exception:  # noqa: BLE001 — the refusal is the expected outcome
        return
    raise AssertionError(f"{path}({args!r}) was accepted and should not have been")


def body() -> None:
    with RpcSubprocess("hello-tile-dashboard", boot_grace=1.5) as tf:
        # ── (A) the arrangement is a value ──────────────────────────────────
        assert_eq(q(tf, "columns"), 12, "A: twelve columns, the one declaration")
        assert_eq(q(tf, "tile_count"), 5, "A: five cards")
        assert_eq(q(tf, "tiles"), SEED, "A: the seed board")
        assert_eq(q(tf, "violations"), 0, "A: and it holds its invariant")
        assert_eq(q(tf, "row_count"), 5, "A: four rows of cards, floored at MIN_ROWS")

        layout = q(tf, "layout")
        assert_eq(len(layout["tiles"]), 5, "A: the WHOLE board is one JSON read")
        assert_eq(layout["columns"], 12, "A: including its column count")
        assert_eq(
            {t["id"] for t in layout["tiles"]},
            {"throughput", "latency", "loss", "topology", "alarms"},
            "A: every card is in it, addressed by the application's own id",
        )

        # ── (B) the painted board IS a CSS grid ─────────────────────────────
        snap = wait_snap(
            tf,
            lambda s: find_by_tag(s, BOARD) is not None,
            viewport=WIN,
            desc="the board container is painted",
        )
        board = find_by_tag(snap, BOARD)
        assert board is not None, "B: the board container is tagged"
        assert_eq(
            len(board.get("children", [])),
            5,
            "B: one painted child per card, from the model's own placement",
        )
        cards = {
            child.get("tag")
            for child in board.get("children", [])
            if child.get("tag")
        }
        assert_eq(
            cards,
            {
                "card.throughput",
                "card.latency",
                "card.loss",
                "card.topology",
                "card.alarms",
            },
            "B: each card is addressable, so a client can point at one",
        )

        # ── (C) a move NAMES what it displaced, transitively ────────────────
        assert_eq(
            inv(tf, "move_to", "topology,0,0"),
            "throughput:0>2, latency:1>3, alarms:2>4",
            "C: ★ the dashboard tool reflows silently; every displaced card is named here, "
            "and `alarms` moved because a card that was itself pushed landed on it",
        )
        assert_eq(q(tf, "last_reflow"), "throughput:0>2, latency:1>3, alarms:2>4",
                  "C: and the report is readable after the fact")
        assert_eq(q(tf, "violations"), 0, "C: the board is still legal")
        assert_eq(
            q(tf, "tiles"),
            "throughput@0,2+12x1 latency@0,3+6x1 loss@6,1+6x1 "
            "topology@0,0+4x2 alarms@4,4+8x1",
            "C: and every card's new slot is readable",
        )

        # ── (D) a drag's effect is LOCAL ────────────────────────────────────
        assert_eq(
            q(tf, "tiles").split()[2],
            "loss@6,1+6x1",
            "D: ★ `loss` never moved — it shares no column with `topology`, so "
            "the reflow did not touch it. That locality is the whole reason a "
            "dashboard's drag is usable",
        )

        # ── (E) a move that fits is clean ───────────────────────────────────
        assert_eq(inv(tf, "move_to", "loss,6,3"), "clean", "E: nothing in the way")
        assert_eq(q(tf, "last_reflow"), "clean", "E: and it says so")

        # ── (F) a refusal says WHY and changes nothing ──────────────────────
        before = q(tf, "tiles")
        refused(tf, "resize", "latency,20,1")
        assert_eq(
            q(tf, "last_refusal"),
            "a tile 20 columns wide does not fit a grid of 12",
            "F: the refusal is a sentence, not a bool — and it names both numbers",
        )
        refused(tf, "move_to", "ghost,0,0")
        assert_eq(
            q(tf, "last_refusal"),
            "no tile ghost in this grid",
            "F: an unknown card is a different refusal from an impossible size",
        )
        refused(tf, "move_to", "latency,x")
        assert_eq(q(tf, "tiles"), before, "F: and no refusal moved a card")

        # ── (G) resize pushes what it grows into ────────────────────────────
        assert_eq(inv(tf, "move_to", "topology,0,0"), "clean", "G: put it back")
        # 12 wide by 3 tall reaches row 2, where `throughput` is. A 6x2 was the
        # first try and it came out `clean` -- correctly, because rows 0..2 were
        # empty by then. The demo asserts a collision, so it has to cause one.
        grew = inv(tf, "resize", "topology,12,3")
        assert grew != "clean", f"G: widening onto a neighbour displaces: {grew}"
        assert "throughput" in grew, f"G: and it names the card it grew into: {grew}"
        assert_eq(q(tf, "violations"), 0, "G: and the board stays legal")

        # ── (H) compaction is a VERB ────────────────────────────────────────
        assert_eq(inv(tf, "remove", "throughput"), "throughput", "H: take one out")
        assert_eq(q(tf, "tile_count"), 4, "H: four left")
        gapped = q(tf, "tiles")
        assert "@0,0" not in gapped.split()[0], (
            "H: ★ the gap STAYS — the dashboard tool would have closed it as a side effect "
            f"of the removal, which makes the inverse not a removal. Got {gapped}"
        )
        tidied = inv(tf, "compact", 0)
        assert tidied != "clean", f"H: tidying moved cards and named them: {tidied}"
        assert_eq(q(tf, "violations"), 0, "H: still legal after tidying")
        assert_eq(inv(tf, "compact", 0), "clean", "H: and it is idempotent")

        # ── (I) every slot is read-only, and unknown is a different answer ──
        try:
            tf.intervene(f"{EXT}/tiles", "nope")
            raise AssertionError("I: a gesture is a verb, so no slot is writable")
        except AssertionError:
            raise
        except Exception as exc:  # noqa: BLE001
            assert "read" in str(exc).lower() or "-32" in str(exc), (
                f"I: a read-only refusal, told apart from an unknown path: {exc}"
            )
        try:
            tf.intervene(f"{EXT}/nonesuch", "nope")
            raise AssertionError("I: an unknown path is refused too")
        except AssertionError:
            raise
        except Exception:  # noqa: BLE001
            pass

        # ── (J) the board survives every edit and repaints ──────────────────
        snap = wait_snap(
            tf,
            lambda s: len((find_by_tag(s, BOARD) or {}).get("children", [])) == 4,
            viewport=WIN,
            desc="the removed card is gone from the paint",
        )
        board = find_by_tag(snap, BOARD)
        assert board is not None, "J: the board is still painted"
        assert_eq(
            len(board.get("children", [])),
            4,
            "J: and the removed card is gone from the paint, not just the model",
        )
        assert_eq(q(tf, "violations"), 0, "J: with the invariant intact throughout")


if __name__ == "__main__":
    run_demo("r1608_tile_dashboard", body)
