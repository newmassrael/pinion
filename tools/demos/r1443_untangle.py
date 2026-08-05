#!/usr/bin/env python3
"""R1443 §5.38 §5.52 — a live topology view sheds a tangle without losing its shape.

R1442 gave the view two orderings and published what each costs: `stable` never
reorders a pair the viewer has learned, `fresh` re-minimises crossings and churns
the drawing to do it. Its own metrics then showed up the hole it shipped with. A
stable view keeps every crossing its changes introduce, for ever — a seeded pass
does not choose an order, so it cannot unpick one — and the only relief on offer
threw the whole learned picture away.

R1443 is the missing move: order by the seed as before, then exchange adjacent
vertices for exactly as long as an exchange strictly removes a crossing
(Gansner, Koutsofios, North & Vo's `transpose`, TSE 1993). Every departure from
the remembered drawing has to pay for itself in crossings, so it reports a small
`order_changes` instead of either extreme.

It arrives as two things, because they differ in kind:

* `untangle` — a **verb**. Tidy what is on screen now; `mode` is untouched, so
  the next change behaves exactly as it would have.
* `settled` — a **policy**. Draw every change that way and the tangle never
  accumulates in the first place.

What this script checks, over real RPC and never on trust:

* the stable feed really does leave a tangle (without this everything else is
  vacuous, so it is asserted first);
* untangling relieves it, and the pairs it moved are re-derived HERE from
  service names rather than read off the view's own `order_changes`;
* on ONE tangled drawing, untangling reaches the same crossing count as a fresh
  pass while moving a quarter as much of the drawing;
* the verb leaves `mode` alone — the property that makes it a verb;
* and untangling an untangled drawing is a no-op, which is what makes the
  policy safe to leave switched on.

Qt reference: Qt ships no graph layout at all, so there is nothing to be at
parity with; Graphviz `dot` has no incremental mode, and ELK's interactive
strategy holds the order absolutely, as R1442's `stable` does. Publishing the
trade as scene data for the same pass (§2 #2, §2 #7) is what lets an agent pick
between them without a screenshot.

Run from the workspace root:
    cargo build -p hello-topology --release
    python3 tools/demos/r1443_untangle.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    assert_action_refused,
    assert_out_of_range,
    assert_rpc_error,
    find_by_tag,
    run_demo,
)

VIEWPORT = (880, 520)
VIEW = "topology"
FEED_STEPS = 7
# The step of the scripted incident at which the stable drawing is most tangled
# — `api` has just stopped talking to the database directly, which crosses the
# cache in front of it.
MOST_TANGLED_AFTER = 2


def q(tf: RpcSubprocess, path: str):
    return tf.query(f"/external/{path}")


def column(tf: RpcSubprocess, index: int) -> list[str]:
    raw = tf.invoke("/external/column_order", str(index))
    return [name for name in raw.split(",") if name]


def columns(tf: RpcSubprocess) -> list[list[str]]:
    return [column(tf, i) for i in range(q(tf, "depth"))]


def placement(tf: RpcSubprocess) -> dict[str, tuple[int, int]]:
    """Every service's `(column, y)`, read one service at a time over RPC.

    The y is the card's own top edge, not a row index: columns are drawn on ONE
    shared vertical axis, so comparing services in different columns needs the
    coordinate rather than a position within a list.
    """
    names = [n for n in q(tf, "service_names").split(",") if n]
    return {
        name: (
            tf.invoke("/external/node_column", name),
            tf.invoke("/external/node_y", name),
        )
        for name in names
    }


def flipped(
    before: dict[str, tuple[int, int]], after: dict[str, tuple[int, int]]
) -> list[tuple[str, str]]:
    """Pairs that shared a column in BOTH drawings and came out reversed.

    The strict question — pairs the viewer actually saw stacked. Derived from
    names on this side, so every claim below about what a pass cost the viewer is
    independent of the view's own `order_changes`.
    """
    out = []
    shared = sorted(set(before) & set(after))
    for i, a in enumerate(shared):
        for b in shared[i + 1 :]:
            if before[a][0] == before[b][0] and after[a][0] == after[b][0]:
                if (before[a][1] < before[b][1]) != (after[a][1] < after[b][1]):
                    out.append((a, b))
    return out


def reordered(
    before: dict[str, tuple[int, int]], after: dict[str, tuple[int, int]]
) -> list[tuple[str, str]]:
    """What `order_changes` itself counts, re-derived from names.

    A wider question than `flipped`, and R1443 is where the two come apart. The
    free axis is global, so a pair counts when both were on screen before and
    share a column NOW, drawn in the opposite vertical order to the remembered
    one — whether or not they used to share a column. A service that changes
    column while overtaking another is counted here and not by `flipped`: nobody
    ever saw those two stacked, but the viewer did see one above the other.
    """
    out = []
    shared = sorted(set(before) & set(after))
    for i, a in enumerate(shared):
        for b in shared[i + 1 :]:
            if after[a][0] == after[b][0] and (
                (before[a][1] > before[b][1]) != (after[a][1] > after[b][1])
            ):
                out.append((a, b))
    return out


def replay(tf: RpcSubprocess, steps: int, mode: str = "stable") -> None:
    """Start the incident over and play `steps` of it in `mode`.

    The mode is chosen BEFORE the reset: a reset re-places from nothing, so the
    drawing it produces is the same whichever ordering is selected, and the
    replay therefore starts from an identical picture every time.
    """
    tf.intervene("/external/mode", mode)
    tf.invoke("/external/reset", None)
    for _ in range(steps):
        tf.invoke("/external/advance", None)


def body() -> None:
    with RpcSubprocess("hello-topology", boot_grace=1.5) as tf:
        # ── (A) ★ the stable feed accumulates a tangle it cannot shed ─
        # Asserted before anything else: if the view ended tidy, every claim in
        # this script about relieving a tangle would pass on an empty case.
        assert find_by_tag(tf.snapshot(source="paint", viewport=VIEWPORT), VIEW)
        assert_eq(q(tf, "mode"), "stable", "a view defaults to keeping its shape")
        assert_eq(q(tf, "crossings"), 0, "and the first drawing is tidy")
        for step in range(FEED_STEPS):
            tf.invoke("/external/advance", None)
            assert_eq(
                q(tf, "order_changes"),
                0,
                f"step {step} moved nothing the viewer had learned",
            )
        tangled = q(tf, "crossings")
        assert tangled > 0, (
            "★ the stable feed ended tidy, so there is nothing here to untangle "
            "and the rest of this script proves nothing"
        )

        # ── (B) ★ the verb relieves it, and says what that cost ──────
        before = placement(tf)
        note = tf.invoke("/external/untangle", None)
        assert isinstance(note, str) and "untangled" in note, f"the verb reports: {note}"
        assert_eq(q(tf, "last_event"), note, "and the view says so on screen")
        relieved = q(tf, "crossings")
        assert relieved < tangled, f"★ the tangle came out: {tangled} -> {relieved}"
        after = placement(tf)
        moved = flipped(before, after)
        assert moved, "and it cost something — a free repair would be suspicious"
        counted = reordered(before, after)
        assert_eq(
            len(counted),
            q(tf, "order_changes"),
            f"★ the published cost agrees with the names: {counted}",
        )
        for pair in moved:
            assert pair in counted, f"{pair} moved and was not counted"
        assert str(tangled) in note and str(relieved) in note, (
            f"the verb's own note carries both counts: {note}"
        )

        # ── (C) ★ it is a VERB, not a policy: `mode` is untouched ────
        # This is the whole distinction. Reaching for `fresh` to tidy up also
        # changes what every LATER change will do, and switching back does not
        # restore the drawing discarded on the way.
        assert_eq(q(tf, "mode"), "stable", "★ untangling did not adopt a new ordering")
        settled_columns = columns(tf)
        assert_action_refused(
            lambda: tf.invoke("/external/advance", None),
            saying="the scripted timeline has no further step",
        )
        assert_eq(
            columns(tf), settled_columns, "a refused step leaves the drawing alone"
        )

        # ── (D) ★ untangling an untangled drawing is a no-op ─────────
        # What makes the same pass safe as a standing policy: it is a fixed point
        # once there is nothing left to buy.
        again = tf.invoke("/external/untangle", None)
        assert_eq(columns(tf), settled_columns, "★ a second untangle moved nothing")
        assert_eq(q(tf, "order_changes"), 0, "and reports that it moved nothing")
        assert_eq(q(tf, "crossings"), relieved, "the count is unchanged too")
        assert isinstance(again, str), "and the verb still answers"

        # ── (E) ★ the comparison, on ONE drawing ─────────────────────
        # Replay to the most tangled point of the incident, then relieve it two
        # ways from that same drawing: the exchange, and a fresh pass. Both are
        # driven over RPC and both are measured from names here.
        replay(tf, MOST_TANGLED_AFTER)
        worst = q(tf, "crossings")
        assert worst >= tangled, f"this is the tangled point of the feed: {worst}"
        start = placement(tf)
        tf.invoke("/external/untangle", None)
        exchange_cost = len(flipped(start, placement(tf)))
        exchange_crossings = q(tf, "crossings")

        replay(tf, MOST_TANGLED_AFTER)
        assert_eq(placement(tf), start, "the replay reproduced the same drawing")
        tf.intervene("/external/mode", "fresh")
        fresh_cost = len(flipped(start, placement(tf)))
        fresh_crossings = q(tf, "crossings")

        assert_eq(
            exchange_crossings,
            fresh_crossings,
            "★ the exchange reached the same tidiness a fresh pass did",
        )
        assert exchange_cost * 2 <= fresh_cost, (
            "★ and paid a fraction of the drawing for it: the exchange moved "
            f"{exchange_cost} remembered pair(s), the fresh pass {fresh_cost}"
        )
        assert exchange_cost > 0, "while genuinely having moved something"

        # ── (F) ★ the same pass as a POLICY, over the whole incident ──
        # A view drawn `settled` never accumulates the tangle at all, and every
        # step that does move a pair bought a crossing with it.
        replay(tf, 0, mode="settled")
        assert_eq(q(tf, "mode"), "settled", "the third ordering is selectable")
        seen = placement(tf)
        churn = 0
        wider_somewhere = False
        for step in range(FEED_STEPS):
            tf.invoke("/external/advance", None)
            now = placement(tf)
            step_cost = len(flipped(seen, now))
            counted = reordered(seen, now)
            assert_eq(
                len(counted),
                q(tf, "order_changes"),
                f"step {step}: the metric and the names agree",
            )
            wider_somewhere = wider_somewhere or len(counted) > step_cost
            churn += step_cost
            seen = now
        assert wider_somewhere, (
            "★ no step separated the two questions, so the wider one is being "
            "checked against a case that never arises"
        )
        settled_end = q(tf, "crossings")
        assert settled_end < tangled, (
            "★ settled never accumulated the tangle a stable view ended with: "
            f"{settled_end} against {tangled}"
        )

        replay(tf, 0, mode="fresh")
        seen = placement(tf)
        fresh_churn = 0
        for _ in range(FEED_STEPS):
            tf.invoke("/external/advance", None)
            now = placement(tf)
            fresh_churn += len(flipped(seen, now))
            seen = now
        assert churn < fresh_churn, (
            "★ and moved less of the drawing across the incident than a fresh "
            f"feed: {churn} pair-moves against {fresh_churn}"
        )

        # ── (G) the surface: what it accepts and what it refuses ─────
        tf.intervene("/external/mode", "stable")
        assert_eq(q(tf, "mode"), "stable")
        assert_out_of_range(
            lambda: tf.intervene("/external/mode", "untangle"),
            saying='"untangle" is not a layout mode',
        )
        assert_eq(q(tf, "mode"), "stable", "a rejected mode changes nothing")
        # `untangle` is a verb, not a writable measurement.
        assert_rpc_error(
            lambda: tf.intervene("/external/untangle", "yes"),
            data="UnknownIntervenePath",
        )
        assert_rpc_error(lambda: tf.invoke("/external/untangled", None), data="UnknownInvokePath")
        # The measurements it moves stay read-only.
        for path in ("crossings", "order_changes", "depth"):
            assert_rpc_error(
                lambda p=path: tf.intervene(f"/external/{p}", 0), data="ReadOnly"
            )


if __name__ == "__main__":
    sys.exit(run_demo("r1443_untangle", body))
