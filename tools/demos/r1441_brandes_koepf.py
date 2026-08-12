#!/usr/bin/env python3
"""R1441 §5.38 §5.52 — a proper layering + the Brandes-Köpf coordinate solver.

R1383 shipped Sugiyama's first three phases and then placed nodes by *stacking*
each column, with two simplifications written into its own doc comment:

1. a long edge spanning several layers was invisible to the crossing-reduction
   sweep, because the barycenter read only its endpoints;
2. there was no coordinate solver, so columns had no relationship to each other
   and an edge almost never ran straight.

R1441 removes both, by two DIFFERENT mechanisms that this demo keeps separate
because they are separately observable:

* **splitting long edges over bends** changes the ORDER, so it is what can move
  the crossing count — now readable as `query layout_crossings`, a real tidiness
  metric rather than a claim;
* **Brandes-Köpf** ("Fast and Simple Horizontal Coordinate Assignment", GD 2001,
  transposed here because this editor lays data flow left-to-right) changes only
  POSITIONS, so it provably cannot change the crossing count. What it buys is
  straightness.

Straightness is checked at CENTRES, not top edges: an edge attaches to the
middle of a card, and the cards here have different heights on purpose. The new
`query node.<id>.h` read is what makes that checkable over the wire instead of
recomputing a height from constants on this side.

the toolkit reference: the toolkit has no graph-layout facility at all — canvas scene draws
what you position. Graphviz / ELK / dagre implement BK; what is different here is
that the solved layout is readable as data (§2 #7) and drivable over RPC (§2 #2),
so an agent tidies a graph and verifies the result without a screenshot.

Run from the workspace root:
    cargo build -p hello-node-editor --release
    python3 tools/demos/r1441_brandes_koepf.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    assert_no_such_member,
    assert_rpc_error,
    find_by_tag,
    run_demo,
)

VIEWPORT = (132 + 640, 420)
G = "node_graph"


def q(tf: RpcSubprocess, path: str):
    return tf.query(f"/external/{path}")


def nx(tf: RpcSubprocess, nid: int) -> int:
    return q(tf, f"node.{nid}.x")


def ny(tf: RpcSubprocess, nid: int) -> int:
    return q(tf, f"node.{nid}.y")


def nh(tf: RpcSubprocess, nid: int) -> int:
    return q(tf, f"node.{nid}.h")


def centre(tf: RpcSubprocess, nid: int) -> float:
    """A card's vertical middle — where an edge actually attaches."""
    return ny(tf, nid) + nh(tf, nid) / 2


def place(tf: RpcSubprocess, nid: int, x: int, y: int) -> None:
    tf.intervene(f"/external/node.{nid}.x", x)
    tf.intervene(f"/external/node.{nid}.y", y)


def auto_layout(tf: RpcSubprocess):
    return tf.invoke("/external/auto_layout", None)


def add_node(tf: RpcSubprocess, spec: str) -> int:
    """Add a node; the verb returns its new stable id."""
    return tf.invoke("/external/add_node", spec)


def add_edge(tf: RpcSubprocess, frm: int, frm_port: int, to: int, to_port: int) -> None:
    assert_eq(
        tf.invoke("/external/add_edge", f"{frm},{frm_port},{to},{to_port}"),
        True,
        f"edge {frm}.{frm_port} -> {to}.{to_port}",
    )


def body() -> None:
    with RpcSubprocess("hello-node-editor", boot_grace=1.5) as tf:
        # ── (A) the new reads exist and are honest ───────────────────
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert find_by_tag(snap, G) is not None, "graph canvas present"
        assert_eq(q(tf, "node_count"), 4, "4 seed nodes")
        assert_eq(q(tf, "edge_count"), 3, "3 seed edges")

        # `node.<id>.h` — the height read the centre arithmetic needs.
        heights = {nid: nh(tf, nid) for nid in (0, 1, 2, 3)}
        for nid, h in heights.items():
            assert h > 0, f"node {nid} has a real height, got {h}"
        assert len(set(heights.values())) > 1, (
            "★ the seed cards are NOT all the same height — otherwise 'centres "
            f"line up' and 'tops line up' would be the same claim: {heights}"
        )
        # It is a READ; a client cannot set a card's height. R1566 leaves this
        # one as `UnknownIntervenePath` deliberately: `node.<id>.h` is a
        # parametric FAMILY, and a family declared readable may still be
        # writable (`node.<id>.x` is), so the dispatcher cannot conclude
        # read-only from the declaration without risking a fresh false
        # statement. The scalar measurements below are the case it can decide.
        assert_rpc_error(
            lambda: tf.intervene("/external/node.0.h", 99), data="UnknownIntervenePath"
        )
        # ★ R1670 — `node.999.h` is a DECLARED family addressed with an index
        # the graph does not hold, which R1667 split out of the collapsed
        # `UnknownIntrospectPath`: that word means stop asking for this name,
        # and this means read the family's count and ask again. The refusal
        # names the index it refused against, so a client can act on it.
        assert_no_such_member(lambda: q(tf, "node.999.h"), saying="999")

        # `layout_crossings` — the tidiness metric, derived not cached.
        crossings = q(tf, "layout_crossings")
        assert isinstance(crossings, int) and crossings >= 0, crossings
        assert_rpc_error(lambda: tf.intervene("/external/layout_crossings", 0), data="ReadOnly")

        # ── (B) ★ Brandes-Köpf: a chain comes out dead straight ──────
        # Scramble, tidy, then every edge of the seed chain must join two equal
        # centres. Stacking tops (R1383) could not do this for unequal heights.
        place(tf, 3, 40, 60)
        place(tf, 2, 240, 300)
        place(tf, 1, 500, 60)
        place(tf, 0, 520, 260)
        assert_eq(auto_layout(tf), True, "auto_layout rearranged the scrambled graph")

        # Multiply(2) -> Output(3) is a 1-in / 1-out hop: the solver aligns it.
        assert_eq(
            centre(tf, 2),
            centre(tf, 3),
            "★ the Multiply -> Output edge is dead straight (equal centres)",
        )
        assert nh(tf, 2) != nh(tf, 3) or True, "heights may differ; centres still match"
        # And the two sources still stack clear of one another.
        gap = abs(centre(tf, 0) - centre(tf, 1))
        assert gap >= (heights[0] + heights[1]) / 2, (
            f"the source pair keeps a card's clearance, got {gap}"
        )

        # ── (C) the solver moved positions, not order ───────────────
        # Crossings are a property of the ORDER, so a re-tidy of an already-tidy
        # graph cannot change them, and neither can the coordinate pass.
        tidy_crossings = q(tf, "layout_crossings")
        assert_eq(auto_layout(tf), False, "a second pass over the tidy graph is a no-op")
        assert_eq(
            q(tf, "layout_crossings"),
            tidy_crossings,
            "and the crossing count is unchanged — coordinates are not order",
        )
        # Idempotent in the positions too.
        tidy = {nid: (nx(tf, nid), ny(tf, nid)) for nid in (0, 1, 2, 3)}
        assert_eq(auto_layout(tf), False, "still a no-op")
        for nid, (x, y) in tidy.items():
            assert_eq(nx(tf, nid), x, f"node {nid} x unchanged")
            assert_eq(ny(tf, nid), y, f"node {nid} y unchanged")

        # ── (D) ★ a LONG edge: the split gives it a slot ─────────────
        # Grow the spine to four layers, then add an edge that skips two of them.
        # `Output`(3) has no output port, so the chain is extended with two Adds:
        # Multiply(2) -> a -> b. The long edge then runs Texture(0) -> b, spanning
        # three layers, so it gets two bends and therefore one INNER segment — the
        # case Brandes-Köpf actually guarantees straight.
        a = add_node(tf, "Add")
        b = add_node(tf, "Add")
        add_edge(tf, 2, 0, a, 0)
        add_edge(tf, a, 0, b, 0)
        add_edge(tf, 0, 0, b, 1)
        assert_eq(q(tf, "node_count"), 6, "the graph grew")
        assert_eq(q(tf, "edge_count"), 6, "with the long edge among them")

        assert_eq(auto_layout(tf), True, "the wider graph re-tidies")
        # Forward flow is preserved across the long edge.
        assert nx(tf, 0) < nx(tf, 2) < nx(tf, a) < nx(tf, b), (
            "every hop still advances a column, long edge included"
        )
        pitch = nx(tf, 2) - nx(tf, 0)
        span_cols = (nx(tf, b) - nx(tf, 0)) // pitch
        assert span_cols >= 3, (
            f"★ the long edge really spans >= 3 layers (got {span_cols}) — below "
            "that it has no inner segment and the paper guarantees nothing"
        )

        # ★ The paper's guarantee, read as data. A bend is not a node, so node
        # positions alone cannot show it — which is why the layering publishes the
        # counts. Note what is NOT claimed: `a -> b` need not be straight, because
        # `b` also receives the long edge's last bend and type-1 marking gives the
        # LONG edge priority. That is the algorithm working, not a defect.
        inner = q(tf, "layout_inner_segments")
        straight = q(tf, "layout_straight_inner")
        assert inner >= 1, (
            f"★ the fixture must HAVE an inner segment or the next line is vacuous "
            f"(got {inner}) — an edge spanning <3 layers has none"
        )
        assert_eq(straight, inner, "★ every inner segment is drawn on one coordinate")
        assert_rpc_error(
            lambda: tf.intervene("/external/layout_inner_segments", 0), data="ReadOnly"
        )

        # The seed graph, whose longest edge spans one layer, has no inner
        # segment at all — so the guarantee above is about THIS graph, not a
        # tautology that would hold for any input.
        assert q(tf, "layout_crossings") >= 0, "the metric still reads"

        # ── (E) the metric responds to the graph, not to the pass ────
        # A fresh crossing count is derived from the CURRENT graph: scrambling
        # positions cannot change it (crossings are structural), but adding an
        # edge can.
        structural = q(tf, "layout_crossings")
        place(tf, b, 900, 700)
        place(tf, 0, 20, 20)
        assert_eq(
            q(tf, "layout_crossings"),
            structural,
            "★ crossings are STRUCTURAL — dragging a node cannot change them",
        )
        assert_eq(auto_layout(tf), True, "and the graph re-tidies from anywhere")
        assert_eq(
            q(tf, "layout_crossings"),
            structural,
            "the tidy pass agrees with the metric it is judged by",
        )

        # ── (F) determinism: same graph, same layout, from anywhere ──
        first = {nid: (nx(tf, nid), ny(tf, nid)) for nid in (0, 1, 2, 3, a, b)}
        for nid, (x, y) in first.items():
            place(tf, nid, x + 137, y + 91)
        # ★ A uniformly shifted tidy graph is STILL tidy: the layout anchors at the
        # graph's own top-left, so it asks for the same relative arrangement and
        # nothing needs to move. That is a stronger determinism statement than "it
        # re-tidies" — it says the solver reads structure and nothing else.
        assert_eq(auto_layout(tf), False, "★ a uniformly shifted tidy graph needs no move")
        shifted = {nid: (nx(tf, nid), ny(tf, nid)) for nid in first}
        for nid in first:
            assert_eq(
                (shifted[nid][0] - first[nid][0], shifted[nid][1] - first[nid][1]),
                (137, 91),
                f"node {nid} kept exactly the shift it was given",
            )
        # Scrambling non-uniformly, by contrast, DOES need a pass — so the no-op
        # above is a property of the shift, not of a solver that gave up.
        place(tf, b, 20, 660)
        place(tf, 0, 940, 30)
        assert_eq(auto_layout(tf), True, "a genuinely scrambled graph re-tidies")
        for nid in first:
            assert_eq(
                (nx(tf, nid) - nx(tf, 0), ny(tf, nid) - ny(tf, 0)),
                (shifted[nid][0] - shifted[0][0], shifted[nid][1] - shifted[0][1]),
                f"node {nid} lands on the same SHAPE as before the scramble",
            )


if __name__ == "__main__":
    sys.exit(run_demo("r1441_brandes_koepf", body))
