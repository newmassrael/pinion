#!/usr/bin/env python3
"""R1591 §5.32 §2 #2 §2 #7 — a region of the painted surface is a VALUE.

Selecting by dragging a shape over what is drawn is one gesture every canvas
has: a node editor's marquee and lasso, a timeline's range, a chart's brush, a
diagram editor's rubber band. The framework had exactly one of the shapes —
`scene/locate_region` took a rectangle — and exactly one of the two things you
can mean by "covered": *touches*.

R1590 measured the absence from the other end, against the DCC's
`NODE_OT_select_circle` and `NODE_OT_select_lasso`, and recorded that those are
NOT node-graph capabilities: they test a region against `node->runtime->draw_bounds`,
the DRAWN rectangle, which is a question for the layer that knows what was
painted where. This demo is that layer answering it, and the proof is the
composition: a node editor's lasso select is `scene/locate_region` plus the
application's own tag→node mapping, with no node-graph code involved at all.

What this script checks, and why each check discriminates:

* **The rectangle still means what it meant.** Every assertion the rect form
  made still holds; the general form is asked for the same rectangle and shown
  to answer identically.
* **PAST the DCC / the toolkit (1): three shapes are one question.** the DCC answers them
  with three operators; the toolkit with three `items()` overloads. Here `shape` is a
  parameter of one method, so a client learns one call.
* **PAST the toolkit (2): the fit is per query.** the toolkit takes it from
  `rubberBandSelectionMode`, a VIEW property — so two selections
  in one view cannot mean different things, and nothing records which mode a
  given selection used. Here it is an argument AND it is repeated in the answer.
* **PAST the toolkit (3): the region can be asked from outside the process.** A
  painter path is an opaque mutable object that can only be built in-process,
  so a toolkit application can be asked "what is under this lasso" over a wire. This
  script has no pointer at all.
* **A lasso is not its bounding box.** A triangular lasso is driven over a
  canvas whose nodes are on a diagonal; the bounding rectangle holds both and
  the lasso holds one. Without this the whole shape axis would be decorative.
* **A degenerate shape is NAMED.** A two-point lasso is refused rather than
  answered with an empty list — the toolkit's `items(polygon F, ..)` returns a list,
  which is the same value it returns for an empty surface.
* **The painted frame is what a canvas means.** `from: "paint"` asks the frame
  that is on screen; the STATE tree of a view-fn binding carries no geometry, so
  a region query against it answers with the zero rect for every node. Both are
  driven, and the difference is asserted rather than assumed.

Run from the workspace root:
    cargo build -p hello-node-groups --release
    python3 tools/demos/r1591_a_region_is_a_value.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    run_demo,
)

#: A widget's primary External is addressed by the framework path, not the tag.
EXT = "/external"

#: `hello-node-groups`'s seed, mirrored rather than imported.
BASE, BLEND, LEVEL, MIX, FADE, OUT = 0, 1, 2, 3, 4, 5

#: The viewport every paint-scene read in this script uses.
VIEW = (1180, 760)


def inv(tf: RpcSubprocess, path: str, args):
    return tf.invoke(f"{EXT}/{path}", args)


def cards(tf: RpcSubprocess) -> dict[int, tuple[int, int, int, int]]:
    """Every node card's window-absolute rect, from the PAINT scene."""
    rects = abs_rects_of(tf.snapshot(source="paint", viewport=VIEW))
    out: dict[int, tuple[int, int, int, int]] = {}
    for tag, rect in rects.items():
        if tag.startswith("nodegroups.node.") and tag.count(".") == 2:
            out[int(tag.rsplit(".", 1)[1])] = rect
    return out


def nodes_in(reply) -> list[int]:
    """The node ids a region answer names — the application's own mapping."""
    found = []
    for path in reply["paths"]:
        leaf = path.rsplit("/", 1)[-1]
        if leaf.startswith("nodegroups.node.") and leaf.count(".") == 2:
            found.append(int(leaf.rsplit(".", 1)[1]))
    return sorted(found)


def refused(tf: RpcSubprocess, **params) -> str:
    try:
        tf.locate_region(**params)
    except Exception as err:  # noqa: BLE001 — the message is the assertion
        return str(err)
    raise AssertionError(f"locate_region({params!r}) was expected to be refused")


def body() -> None:
    with RpcSubprocess("hello-node-groups", boot_grace=1.5) as tf:
        for _ in range(3):
            tf.tick(0.016)

        # ── (A) where the cards actually are ────────────────────────────────
        placed = cards(tf)
        assert_eq(len(placed), 6, "A: the seeded material, painted")
        for node in (BASE, MIX, OUT):
            assert node in placed, f"A: node {node} is painted"

        # ── (B) the rectangle form is unchanged, and now says what it did ───
        (bx, by, bw, bh) = placed[BASE]
        over_base = tf.locate_region(
            x=bx, y=by, w=bw, h=bh, source="paint", viewport=VIEW
        )
        assert_eq(nodes_in(over_base), [BASE], "B: the rect over one card")
        assert_eq(over_base["shape"], "rect", "B: and the answer repeats the shape")
        assert_eq(
            over_base["fit"],
            "intersects",
            "B: PAST QT — the mode is part of the answer. Qt takes it from "
            "QGraphicsView::rubberBandSelectionMode, a VIEW property that "
            "nothing records per selection",
        )
        # The default is what it always was, so a caller written before R1591
        # means exactly what it meant.
        plain = tf.request(
            "scene/locate_region",
            {"x": bx, "y": by, "w": bw, "h": bh, "from": "paint",
             "viewport": {"w": VIEW[0], "h": VIEW[1]}},
        )
        assert plain is not None
        assert_eq(plain.result["paths"], over_base["paths"], "B: named or not, one answer")

        # ── (C) the fit is a per-query argument ─────────────────────────────
        # A rectangle covering only the left half of the Base card.
        half = tf.locate_region(
            x=bx, y=by, w=max(bw // 2, 1), h=bh, source="paint", viewport=VIEW
        )
        assert_eq(nodes_in(half), [BASE], "C: it touches the card")
        strict = tf.locate_region(
            x=bx, y=by, w=max(bw // 2, 1), h=bh, fit="contains",
            source="paint", viewport=VIEW,
        )
        assert_eq(nodes_in(strict), [], "C: and does not contain it")
        assert_eq(strict["fit"], "contains", "C: which the answer states")

        # ── (D) a circle, which the DCC needs a second operator for ─────────
        cx, cy = bx + bw // 2, by + bh // 2
        disc = tf.locate_region(
            shape="circle", cx=cx, cy=cy, r=6, source="paint", viewport=VIEW
        )
        assert_eq(nodes_in(disc), [BASE], "D: a small disc inside the card")
        assert_eq(disc["shape"], "circle")
        far = tf.locate_region(
            shape="circle", cx=cx, cy=cy, r=6, fit="contains",
            source="paint", viewport=VIEW,
        )
        assert_eq(nodes_in(far), [], "D: which it does not swallow")

        # ── (E) a lasso is not its bounding box ─────────────────────────────
        # A right triangle at the top-left whose legs span every card, so its
        # BOUNDING RECT holds all of them — and whose hypotenuse cuts the
        # canvas, so the triangle itself does not. Sized from the painted
        # geometry rather than hard-coded, so a layout change cannot make this
        # pass by accident.
        span_x = max(r[0] + r[2] for r in placed.values()) + 20
        span_y = max(r[1] + r[3] for r in placed.values()) + 20
        triangle = [[0, 0], [span_x, 0], [0, span_y]]
        nearest = min(placed, key=lambda n: placed[n][0] + placed[n][1])
        farthest = max(placed, key=lambda n: placed[n][0] + placed[n][1])
        lasso = tf.locate_region(
            shape="lasso", points=triangle, source="paint", viewport=VIEW
        )
        assert_eq(lasso["shape"], "lasso")
        box = tf.locate_region(
            x=0, y=0, w=span_x, h=span_y, source="paint", viewport=VIEW
        )
        inside_lasso = set(nodes_in(lasso))
        inside_box = set(nodes_in(box))
        assert_eq(
            sorted(inside_box),
            sorted(placed),
            "E: the bounding rectangle really does hold every card",
        )
        assert inside_lasso < inside_box, (
            f"E: the triangle must select STRICTLY fewer cards than its own "
            f"bounding rectangle, or the shape axis is decorative: "
            f"{sorted(inside_lasso)} vs {sorted(inside_box)}"
        )
        assert nearest in inside_lasso, f"E: the near card is inside: {lasso}"
        assert farthest not in inside_lasso, (
            f"E: and the far one is past the hypotenuse — which the bounding "
            f"rectangle cannot express: node {farthest}"
        )

        # ── (F) a degenerate shape is named, not answered with nothing ──────
        reason = refused(
            tf, shape="lasso", points=[[0, 0], [10, 10]], source="paint", viewport=VIEW
        )
        assert "three vertices" in reason, f"F: {reason!r}"
        reason = refused(tf, x=0, y=0, w=0, h=10, source="paint", viewport=VIEW)
        assert "no pixels" in reason, f"F: {reason!r}"
        reason = refused(tf, shape="hexagon", x=0, y=0, w=1, h=1, source="paint")
        assert "not" in reason and "hexagon" in reason, f"F: {reason!r}"
        reason = refused(tf, x=0, y=0, w=1, h=1, fit="loosely", source="paint")
        assert "loosely" in reason, f"F: {reason!r}"
        # An empty ANSWER is still reachable, and now means only itself.
        empty = tf.locate_region(
            shape="circle", cx=5000, cy=5000, r=3, source="paint", viewport=VIEW
        )
        assert_eq(empty["paths"], [], "F: nothing is there, which is a different fact")

        # ── (G) the painted frame is what a canvas means ────────────────────
        state = tf.locate_region(
            x=bx, y=by, w=bw, h=bh, source="state", viewport=VIEW
        )
        assert_eq(
            nodes_in(state),
            [],
            "G: a view-fn binding's STATE tree carries no geometry, so the same "
            "rectangle finds no card there. Both scenes are askable and the "
            "caller says which — the two-scene basis scene/snapshot uses",
        )
        assert_eq(state["shape"], "rect", "G: and the answer still says what was asked")

        # ── (H) the composition R1590 named ─────────────────────────────────
        # A node editor's lasso select is this method plus the application's own
        # tag→node mapping. Nothing in `pinion-node-graph` took part; the result
        # is then handed to the SELECTION, which is the editor's.
        picked = nodes_in(lasso)
        inv(tf, "select", ",".join(str(n) for n in picked))
        assert_eq(
            [int(n) for n in str(tf.query(f"{EXT}/selection")).split(",")],
            picked,
            "H: the region named the nodes and the editor holds the selection",
        )
        # And R1590's growth takes it from there, which is the whole point of
        # the split: geometry answers WHICH, the graph answers WHAT ELSE.
        grown = str(inv(tf, "grow", "downstream:transitive"))
        assert "added:" in grown, f"H: {grown!r}"


if __name__ == "__main__":
    run_demo("r1591_a_region_is_a_value", body)
