#!/usr/bin/env python3
"""R1592 §5.32 §5.38 §2 #2 — a shape selects in the caller's OWN coordinates.

R1591 gave the framework a region primitive and left it with one consumer, which
is a thing that drifts: `hello-node-editor` had held its own marquee hit test as
four inequalities since R880. Migrating it is what this round is, and the
migration found a real defect in R1591's own design — `Region` was welded to
`Rect`, an unsigned WINDOW-pixel type, while a node editor's marquee selects in
**graph units** that pan into negative coordinates. A shape predicate has no
business knowing what its numbers mean, so `Region` is now signed throughout and
`Region::covers_span` is its one `Rect`-free entry point.

What this script checks, and why each check discriminates:

* **The marquee still means what it meant.** Its "touching counts" rule (stated
  since R880, and Qt's rubber-band semantics) survives the migration: a card's
  far edge is passed as part of the card, which is now a sentence rather than an
  off-by-one in a filter. `r880_node_marquee_select.py` is the regression proof;
  this script re-drives the same rule through the new verbs.
* **PAST BLENDER: a lasso and a circle are the SAME call.** `NODE_OT_select_lasso`
  and `NODE_OT_select_circle` are two operators with two implementations there.
  Here both are one `Region` value handed to the one selection policy the
  pointer marquee uses, so a lasso and a rubber band cannot disagree about what
  "selected" means.
* **A lasso is not its bounding box.** A triangle is driven whose bounding
  rectangle holds a node the triangle does not, and the two answers are compared
  — without this the shape axis would be decorative.
* **It selects in GRAPH units, which is what makes it survive a pan.** The
  shapes are stated in the graph's own coordinates, read back from the model
  rather than from the screen, so no pixel geometry is involved at any point.
* **A degenerate shape is NAMED.** A two-vertex lasso is refused rather than
  answered with zero, so "your lasso was two points" and "the sweep took
  nothing" stay different facts — Blender's operators cannot report the
  difference and Qt's `items(QPolygonF, ..)` answers both with an empty list.

Run from the workspace root:
    cargo build -p hello-node-editor --release
    python3 tools/demos/r1592_a_shape_selects_in_graph_units.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    run_demo,
)

#: A widget's primary External is addressed by the framework path, not the tag.
EXT = "/external"


def q(tf: RpcSubprocess, path: str):
    return tf.query(f"{EXT}/{path}")


def inv(tf: RpcSubprocess, path: str, args):
    return tf.invoke(f"{EXT}/{path}", args)


def ids(tf: RpcSubprocess) -> list[int]:
    raw = str(q(tf, "selected_ids"))
    return sorted(int(n) for n in raw.split(",")) if raw else []


def node_ids(tf: RpcSubprocess) -> list[int]:
    raw = str(q(tf, "node_ids"))
    return [int(n) for n in raw.split(",")] if raw else []


def place(tf: RpcSubprocess, node: int) -> tuple[int, int]:
    """A node's origin in GRAPH units, read off the model.

    `detail.<field>` resolves against the SINGLE selected node (R916), so the
    selection is the addressing here — no pixel is consulted at any point.
    """
    tf.intervene(f"{EXT}/selected", node)
    return int(q(tf, "detail.x")), int(q(tf, "detail.y"))


def refused(tf: RpcSubprocess, path: str, args) -> str:
    try:
        inv(tf, path, args)
    except Exception as err:  # noqa: BLE001 — the message is the assertion
        return str(err)
    raise AssertionError(f"{path}({args!r}) was expected to be refused")


def body() -> None:
    with RpcSubprocess("hello-node-editor", boot_grace=1.5) as tf:
        for _ in range(3):
            tf.tick(0.016)

        # ── (A) where the nodes are, in the graph's own units ───────────────
        every = node_ids(tf)
        assert len(every) >= 4, f"A: the seeded graph: {every}"
        at = {node: place(tf, node) for node in every}
        assert_eq(q(tf, "node_count"), len(every), "A: and that is all of them")
        # Nothing here reads a pixel: these are model coordinates.
        assert any(x != y for (x, y) in at.values()), f"A: a real layout: {at}"

        # ── (B) a circle takes what it covers ───────────────────────────────
        target = every[0]
        (tx, ty) = at[target]
        took = int(inv(tf, "select_circle", f"{tx + 10},{ty + 10},40"))
        assert took >= 1, f"B: the disc took something: {took}"
        assert target in ids(tf), f"B: including the node it sits on: {ids(tf)}"
        assert_eq(took, len(ids(tf)), "B: and the reply counts the selection")

        # ── (C) a lasso is not its bounding box ─────────────────────────────
        # A right triangle at the graph origin whose legs are chosen from the
        # LAYOUT so the hypotenuse actually cuts it: half way along the range of
        # `x + y`, which puts at least one node on each side whatever the seed
        # looks like. A leg picked without looking would make this pass or fail
        # by luck.
        # A node origin is inside the triangle when `x + y <= leg`, so the leg
        # is placed in the WIDEST gap between two consecutive distinct sums —
        # the cut furthest from every node, whatever the seed looks like. A leg
        # picked without looking makes this pass or fail by luck: the first
        # draft used the midpoint of the whole range and landed on the near side
        # of every node.
        reach = sorted(x + y for (x, y) in at.values())
        gaps = [(reach[i + 1] - reach[i], i) for i in range(len(reach) - 1) if reach[i + 1] > reach[i]]
        assert gaps, f"C: the layout has to have a spread: {reach}"
        (_, at_index) = max(gaps)
        leg = (reach[at_index] + reach[at_index + 1]) // 2
        inv(tf, "select_lasso", f"0,0;{leg},0;0,{leg}")
        by_lasso = set(ids(tf))
        # The SAME corners as a rectangle — which is exactly the marquee, driven
        # through the very same call.
        inv(tf, "select_lasso", f"0,0;{leg},0;{leg},{leg};0,{leg}")
        by_box = set(ids(tf))
        assert by_box, f"C: the bounding rectangle takes something: {sorted(by_box)}"
        assert by_lasso < by_box, (
            f"C: PAST BLENDER — the triangle must take STRICTLY fewer than its "
            f"own bounding rectangle, or the shape is decorative: "
            f"{sorted(by_lasso)} vs {sorted(by_box)}"
        )
        assert by_lasso, f"C: and it must take something: {sorted(by_lasso)}"

        # ── (D) the rectangle form is the marquee, through the same call ────
        # A box around exactly one node, stated in graph units.
        (nx, ny) = at[target]
        box = f"{nx - 5},{ny - 5};{nx + 5},{ny - 5};{nx + 5},{ny + 5};{nx - 5},{ny + 5}"
        inv(tf, "select_lasso", box)
        near = ids(tf)
        assert target in near, f"D: the box over it takes it: {near}"

        # ── (E) a degenerate shape is named ─────────────────────────────────
        reason = refused(tf, "select_lasso", "0,0;10,10")
        assert "three vertices" in reason, f"E: {reason!r}"
        reason = refused(tf, "select_circle", f"{tx},{ty},0")
        assert "no pixels" in reason, f"E: {reason!r}"
        reason = refused(tf, "select_circle", "not,a,circle")
        assert "not a number" in reason, f"E: {reason!r}"
        reason = refused(tf, "select_lasso", "0,0;10;20,20")
        assert "malformed vertex" in reason, f"E: {reason!r}"
        # And the selection survived every refusal untouched.
        assert_eq(ids(tf), near, "E: a refused shape selects nothing and clears nothing")

        # ── (F) an empty answer is a different fact ─────────────────────────
        far = int(inv(tf, "select_circle", "-100000,-100000,5"))
        assert_eq(far, 0, "F: nothing is there, which is not a refusal")
        assert_eq(ids(tf), [], "F: and a plain sweep replaces the selection")


if __name__ == "__main__":
    run_demo("r1592_a_shape_selects_in_graph_units", body)
