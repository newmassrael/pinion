#!/usr/bin/env python3
"""R1442 §5.38 §5.52 — a live topology view keeps its shape as the graph changes.

R1441 finished the layered solver inside `hello-node-editor`, deliberately
writing it against abstract vertices so a second consumer would make the crate
lift a file move. This is that consumer, and it needs something an editor never
did.

An editor lays out when a human presses a button. A **view** is handed a graph
with no coordinates and has to place it again every time the graph changes — and
a layout that only minimises crossings is free, on each of those passes, to swap
two nodes that had nothing to do with the change. The picture jumps for a reason
nothing on screen explains, and the viewer re-learns a drawing they had already
learned. The literature calls what is lost the *mental map* (Misue, Eades, Lai &
Sugiyama, JVLC 1995).

So `pinion_graph::Sugiyama` grew a second ordering — seed each column from the
PREVIOUS drawing's coordinates — and this demo drives both against the same
scripted incident:

* `stable` must never flip a pair the viewer has already seen;
* `fresh` demonstrably does, which is what makes the first claim worth anything.

**Neither ordering is free, and both costs are read here rather than asserted.**
`order_changes` is what a tidy drawing costs the viewer; `crossings` is what a
stable one costs the drawing. The two are queried for the same pass.

The strongest checks below do not trust `order_changes` at all: this script
records each column's SERVICE NAMES before a step and re-derives, on its own
side, whether any remembered pair came out reversed. A metric that agreed with
itself but not with the drawing would still be caught.

Qt reference: Qt ships no graph layout — `QGraphicsScene` draws what you
position, so a topology view there means Graphviz out of process (which has no
incremental mode at all) or a hand-rolled solver. ELK has the interactive
strategy; what is different here is that the ordering, its cost, and the wire's
route through the columns it crosses are all scene data over RPC (§2 #2, §2 #7),
so an agent verifies the drawing without a screenshot.

Run from the workspace root:
    cargo build -p hello-topology --release
    python3 tools/demos/r1442_live_topology.py
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

# The seed mesh, and the scripted incident the view replays.
SEED_SERVICES = ["gw-eu", "gw-us", "api", "auth", "search", "db", "warehouse"]
FEED_STEPS = 7


def q(tf: RpcSubprocess, path: str):
    return tf.query(f"/external/{path}")


def column(tf: RpcSubprocess, index: int) -> list[str]:
    """The service names in a column, top to bottom — what the viewer learns."""
    raw = tf.invoke("/external/column_order", str(index))
    return [name for name in raw.split(",") if name]


def columns(tf: RpcSubprocess) -> list[list[str]]:
    return [column(tf, i) for i in range(q(tf, "depth"))]


def flipped(before: list[list[str]], after: list[list[str]]) -> list[tuple[str, str]]:
    """Pairs that shared a column in BOTH drawings and came out reversed.

    Derived here from names alone, so it is an independent check on the view's
    own `order_changes` rather than a restatement of it.
    """
    place = lambda snap: {  # noqa: E731
        name: (col, row)
        for col, members in enumerate(snap)
        for row, name in enumerate(members)
    }
    was, now = place(before), place(after)
    out = []
    shared = sorted(set(was) & set(now))
    for i, a in enumerate(shared):
        for b in shared[i + 1 :]:
            if was[a][0] == was[b][0] and now[a][0] == now[b][0]:
                if (was[a][1] < was[b][1]) != (now[a][1] < now[b][1]):
                    out.append((a, b))
    return out


def wire(tf: RpcSubprocess, frm: str, to: str) -> list[tuple[int, int]]:
    raw = tf.invoke("/external/wire_points", f"{frm},{to}")
    return [tuple(int(v) for v in point.split(",")) for point in raw.split(";")]


def card(tf: RpcSubprocess, name: str) -> tuple[int, int]:
    return (
        tf.invoke("/external/node_x", name),
        tf.invoke("/external/node_y", name),
    )


def body() -> None:
    with RpcSubprocess("hello-topology", boot_grace=1.5) as tf:
        # ── (A) a graph that arrived with no coordinates is placed ───
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert find_by_tag(snap, VIEW) is not None, "the view is on screen"
        assert_eq(q(tf, "services"), len(SEED_SERVICES), "the seed mesh")
        assert_eq(q(tf, "dependencies"), 9, "and its dependencies")
        assert_eq(q(tf, "mode"), "stable", "a view defaults to keeping its shape")
        assert_eq(q(tf, "feed_remaining"), FEED_STEPS, "the incident is unplayed")
        assert_eq(
            q(tf, "service_names"),
            ",".join(SEED_SERVICES),
            "every service is named on the wire",
        )
        for name in SEED_SERVICES:
            assert find_by_tag(snap, f"topology.node.{name}") is not None, (
                f"{name} has a card"
            )
        # Data flows forward: a dependency's target is always further right.
        assert card(tf, "gw-eu")[0] < card(tf, "api")[0] < card(tf, "db")[0], (
            "the layering runs left to right"
        )
        assert_eq(q(tf, "depth"), 4, "four columns")

        # ── (B) ★ the long wire runs in the channel the layout reserved ──
        # `gw-eu -> warehouse` skips two columns. The solver gave it a slot in
        # each, and the view routes the polyline through them instead of cutting
        # a diagonal across whatever card is in the way.
        assert_eq(q(tf, "bends"), 2, "two columns are crossed, so two bends")
        assert_eq(q(tf, "inner_segments"), 1, "which is one inner segment...")
        assert_eq(q(tf, "straight_inner"), 1, "...and it is drawn straight")
        long_wire = wire(tf, "gw-eu", "warehouse")
        assert_eq(len(long_wire), 4, "start, two bends, end")
        assert_eq(
            long_wire[1][1],
            long_wire[2][1],
            "★ the middle run is level — the guarantee, seen as geometry",
        )
        gw_x, wh_x = card(tf, "gw-eu")[0], card(tf, "warehouse")[0]
        for bend in long_wire[1:3]:
            assert gw_x < bend[0] < wh_x, f"bend {bend} sits between the cards"
        # A one-column hop has nothing to route around.
        assert_eq(len(wire(tf, "gw-eu", "api")), 2, "a short dependency is a line")
        # And the wire is in the scene under a tag naming both ends.
        assert find_by_tag(snap, "topology.wire.gw-eu-warehouse") is not None

        # ── (C) ★ the whole incident, keeping the viewer's drawing ───
        # Every step is a real change of shape; not one may reverse a pair the
        # viewer has already seen. Checked BOTH ways: from the names on this
        # side, and against the metric the view publishes.
        seen = columns(tf)
        assert_eq(q(tf, "order_changes"), 0, "a first drawing preserves nothing yet")
        moved = False
        for step in range(FEED_STEPS):
            note = tf.invoke("/external/advance", None)
            assert isinstance(note, str) and note, f"step {step} describes itself"
            assert_eq(q(tf, "last_event"), note, "and the view says so")
            now = columns(tf)
            assert_eq(
                flipped(seen, now),
                [],
                f"★ step {step} ({note}) reordered nothing the viewer had learned",
            )
            assert_eq(
                q(tf, "order_changes"),
                0,
                f"and the published metric agrees at step {step}",
            )
            moved = moved or now != seen
            seen = now
        assert moved, (
            "★ the feed must actually have changed the drawing, or every "
            "assertion above is about a picture that never moved"
        )
        assert_eq(q(tf, "feed_remaining"), 0, "the incident played out")
        assert_action_refused(
            lambda: tf.invoke("/external/advance", None),
            saying="the scripted timeline has no further step",
        )
        # The graph really is different now.
        assert_eq(q(tf, "services"), len(SEED_SERVICES) + 1, "cache and gw-ap in, auth out")
        assert "auth" not in q(tf, "service_names"), "auth was retired"
        after = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert find_by_tag(after, "topology.node.cache") is not None, "cache appeared"
        assert find_by_tag(after, "topology.node.auth") is None, "auth's card is gone"

        # ── (D) ★ the counterfactual: fresh ordering DOES churn ──────
        # Same incident, same data, the other ordering. If this passed without
        # churn, section (C) would be crediting the seeded pass with a stability
        # the graph would have had anyway.
        tf.invoke("/external/reset", None)
        tf.intervene("/external/mode", "fresh")
        assert_eq(q(tf, "mode"), "fresh", "the ordering switched")
        assert_eq(q(tf, "services"), len(SEED_SERVICES), "and the mesh is back")

        seen = columns(tf)
        churn = 0
        reported = 0
        for step in range(FEED_STEPS):
            tf.invoke("/external/advance", None)
            now = columns(tf)
            churn += len(flipped(seen, now))
            reported += q(tf, "order_changes")
            seen = now
        assert churn > 0, (
            "★ a fresh relayout must reorder something across the incident — "
            "without it the stable run proves nothing"
        )
        assert reported > 0, f"and the view reports it: {reported}"

        # ── (E) the two orderings differ on IDENTICAL data ───────────
        # Switching mode re-places the same graph, so the difference cannot be
        # attributed to the graph having changed underneath.
        tf.invoke("/external/reset", None)
        tf.intervene("/external/mode", "stable")
        for _ in range(FEED_STEPS):
            tf.invoke("/external/advance", None)
        stable_columns = columns(tf)
        stable_crossings = q(tf, "crossings")
        tf.intervene("/external/mode", "fresh")
        fresh_columns = columns(tf)
        fresh_crossings = q(tf, "crossings")
        assert_eq(q(tf, "services"), len(SEED_SERVICES) + 1, "the graph did not change")
        assert stable_crossings >= fresh_crossings, (
            "★ stability is paid for in crossings: a seeded pass never chooses "
            f"the order, so it cannot beat a fresh one ({stable_crossings} < "
            f"{fresh_crossings})"
        )
        assert fresh_columns != stable_columns or stable_crossings == fresh_crossings, (
            "either the fresh pass re-ordered something, or it found nothing to "
            "improve — but it cannot claim a better count while drawing the same"
        )

        # ── (F) the topology verbs, and what they refuse ─────────────
        tf.intervene("/external/mode", "stable")
        before = q(tf, "services")
        assert isinstance(tf.invoke("/external/add_service", "billing"), str)
        assert_eq(q(tf, "services"), before + 1, "a service appeared")
        assert_action_refused(
            lambda: tf.invoke("/external/add_service", "billing"),
            saying='a service named "billing" is already in the topology',
        )
        assert isinstance(tf.invoke("/external/connect", "gw-us,billing"), str)
        # R1564 — these three were one indistinguishable frame; the first two
        # are a topology fact and the third is a malformed argument.
        assert_action_refused(
            lambda: tf.invoke("/external/connect", "gw-us,billing"),
            saying="a link that is already there",
        )
        assert_action_refused(
            lambda: tf.invoke("/external/connect", "gw-us,ghost"),
            saying="names a service that is not",
        )
        assert_action_refused(
            lambda: tf.invoke("/external/connect", "billing"),
            saying='malformed argument "billing"',
        )
        assert_eq(
            tf.invoke("/external/node_column", "billing"),
            tf.invoke("/external/node_column", "api"),
            "one hop downstream, like the other gateway consumers",
        )
        assert isinstance(tf.invoke("/external/disconnect", "gw-us,billing"), str)
        # R1564 — five refusals that arrived as one indistinguishable frame.
        # Every one is a different fact about the topology or the argument.
        assert_action_refused(
            lambda: tf.invoke("/external/disconnect", "gw-us,billing"),
            saying="no link gw-us -> billing in the topology",
        )
        assert isinstance(tf.invoke("/external/remove_service", "billing"), str)
        assert_action_refused(
            lambda: tf.invoke("/external/remove_service", "billing"),
            saying='no service named "billing"',
        )
        assert_action_refused(
            lambda: tf.invoke("/external/node_x", "billing"),
            saying='no service named "billing"',
        )
        assert_action_refused(
            lambda: tf.invoke("/external/column_order", "not-a-number"),
            saying='"not-a-number" is not a column index',
        )
        assert_action_refused(
            lambda: tf.invoke("/external/wire_points", "api,gw-eu"),
            saying="both exist but are not connected",
        )

        # Every published measurement is a READ — a client cannot assert a
        # crossing count the drawing does not have.
        for path in ("crossings", "order_changes", "depth", "bends", "straight_inner"):
            assert_rpc_error(
                lambda p=path: tf.intervene(f"/external/{p}", 0), data="ReadOnly"
            )
        assert_out_of_range(
            lambda: tf.intervene("/external/mode", "sideways"),
            saying='"sideways" is not a layout mode',
        )
        assert_eq(q(tf, "mode"), "stable", "and a rejected mode changes nothing")


if __name__ == "__main__":
    sys.exit(run_demo("r1442_live_topology", body))
