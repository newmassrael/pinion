#!/usr/bin/env python3
"""R1411 §5.51 §5.52 — drag a palette card onto the canvas to INSTANTIATE its
node AT THE DROP POINT.

The signature editor object-creation gesture (the engine / the DCC / any DCC): grab
a node TYPE from the palette/toolbox and drop it onto the graph, and a new node
of that kind appears WHERE YOU DROPPED IT. Before R1411 the palette was
click-only (R849): a click added the node at a fixed spawn point, never at a
chosen position; the palette cards were not drag sources at all (`begin_drag`
armed only wire drags off a pin). R1411 makes a palette card a drag source that
commits at the drop point, coexisting with the click-to-add-at-spawn — the two
are mutually exclusive (the router's `!became_drag` gate), so no double-add.

The gesture reuses TWO already-built substrates without one line of new
framework wiring:
  * the R742/R1093 drag session (press -> march -> release, the source resolves
    the drop under the absolute cursor), and
  * the R1113 drag-image FOLLOWER — the translucent chip that floats under the
    cursor showing what is being dragged. It surfaces the drag payload's Text
    value as its label automatically, so a palette drag carrying the node TITLE
    gets a "Scalar"/"Lerp" follower chip for free (section B snapshots it).

Every half is AI-observable + AI-drivable over the §5.12 RPC plane (§2 #2):
`scene/drag` runs the real capture arc, `scene/snapshot` records the held
mid-drag follower, and `query node.<id>.x/y` reads where the drop landed.

The seed graph is `Texture x Color -> Multiply -> Output` (nodes 0/1/2/3); the
palette kinds are Texture(0) Color(1) Multiply(2) Add(3) Output(4) Scalar(5)
Lerp(6). The canvas is the 640x420 region right of the 132px palette strip, so
window x maps to graph x as `x - 132` at zoom 1 / no pan (the R1220 projection).

  (A) boot taxonomy — 4 nodes / 3 edges / no follower chip at rest.
  (B) HELD palette drag — the R1113 follower chip appears under the cursor with
      the node TITLE, and nothing is created yet (the drag is held, not dropped).
  (C) RELEASE — EXACTLY ONE node is instantiated (not two: the no-double-add
      proof through the real router) AT the drop graph point; ONE undo removes it.
  (D) a SECOND drop at a DIFFERENT window point lands at a DIFFERENT graph point
      (it is the drop point, not a fixed spawn).
  (E) a palette CLICK still adds at the SPAWN point (coexistence) — a distinct
      position from any drop.
  (F) a palette drag released OFF the canvas instantiates nothing (the gate).

Run from the workspace root:
    cargo build -p hello-node-editor --release
    python3 tools/demos/r1411_node_palette_drag_instantiate.py
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

VIEWPORT = (132 + 640, 420)
G = "node_graph"
DRAG_IMAGE = "ai-overlay/drag-image"
UNDO = "/node_undo/external"

# Two clearly-empty canvas drop targets (below / between the seed nodes, whose
# ys are all <= 210). Window (x, y) -> graph (x - 132, y) at zoom 1 / no pan.
DROP1_WIN = (432.0, 340.0)
DROP1_GRAPH = (300, 340)
DROP2_WIN = (392.0, 380.0)
DROP2_GRAPH = (260, 380)


def node_count(tf) -> int:
    return tf.query("/external/node_count")


def edge_count(tf) -> int:
    return tf.query("/external/edge_count")


def selected(tf):
    return tf.query("/external/selected")


def node_pos(tf, nid) -> tuple[int, int]:
    return (tf.query(f"/external/node.{nid}.x"), tf.query(f"/external/node.{nid}.y"))


def node_title(tf, nid) -> str:
    return tf.query(f"/external/node.{nid}.title")


def undo(tf) -> bool:
    return tf.invoke(f"{UNDO}/undo", None)


def chip_labels(node) -> list[str]:
    """The Text `content`s directly inside the drag-image chip container."""
    return [
        c.get("content")
        for c in (node.get("children") or [])
        if isinstance(c.get("content"), str)
    ]


def body() -> None:
    with RpcSubprocess("hello-node-editor", boot_grace=1.5) as tf:
        # ── (A) boot taxonomy ────────────────────────────────────────
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert find_by_tag(snap, G) is not None, "graph canvas present"
        assert find_by_tag(snap, f"{G}#palette_5") is not None, "the Scalar palette card paints"
        assert find_by_tag(snap, f"{G}#palette_6") is not None, "the Lerp palette card paints"
        assert_eq(node_count(tf), 4, "4 seed nodes")
        assert_eq(edge_count(tf), 3, "3 seed edges")
        assert find_by_tag(snap, DRAG_IMAGE) is None, "no follower chip at rest"

        # ── (B) HELD palette drag: the R1113 follower chip ───────────
        # Press the Scalar card, march onto empty canvas, and HOLD (phase=begin).
        tf.drag(from_path=f"{G}#palette_5", to_at=DROP1_WIN, steps=10, phase="begin")
        held = tf.snapshot(source="paint", viewport=VIEWPORT)
        chip = find_by_tag(held, DRAG_IMAGE)
        assert chip is not None, "the R1113 drag-image follower appears mid palette-drag"
        assert "Scalar" in chip_labels(chip), (
            f"the follower chip shows the dragged node TITLE, got {chip_labels(chip)}"
        )
        # The drag is HELD, not dropped — nothing has been created yet (an atomic
        # drag would have released + committed; this is the §2 #2 mid-drag view).
        assert_eq(node_count(tf), 4, "a held palette drag has instantiated nothing yet")

        # ── (C) RELEASE: exactly ONE node, AT the drop point ─────────
        tf.drag(from_at=DROP1_WIN, to_at=DROP1_WIN, steps=0, phase="end")
        wait_until(lambda: node_count(tf) == 5, timeout=4.0,
                   desc="the release instantiated the dropped node")
        # The headline invariant: ONE node, not two — the drop commit and the
        # would-be trailing click are mutually exclusive (router `!became_drag`).
        assert_eq(node_count(tf), 5, "EXACTLY one node added (no double-add)")
        assert_eq(edge_count(tf), 3, "a palette drop wires nothing (unlike a pin drop)")
        new_id = selected(tf)
        assert isinstance(new_id, int), "the dropped node is selected (its id reads back)"
        assert_eq(node_title(tf, new_id), "Scalar", "the dropped kind is the pressed card")
        assert_eq(node_pos(tf, new_id), DROP1_GRAPH,
                  "the node lands at the DROP point, not the fixed spawn point")
        # The follower chip is gone once the drag released.
        after = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert find_by_tag(after, DRAG_IMAGE) is None, "the follower clears after the release"
        # ONE reversible step removes it.
        assert_eq(tf.query(f"{UNDO}/undo_label"), "Add Scalar", "one labelled undo step")
        assert_eq(undo(tf), True, "undo the drop")
        assert_eq(node_count(tf), 4, "one undo removes the dropped node")

        # ── (D) a SECOND drop lands at a DIFFERENT graph point ───────
        tf.drag(from_path=f"{G}#palette_6", to_at=DROP2_WIN, steps=10)  # Lerp
        wait_until(lambda: node_count(tf) == 5, timeout=4.0,
                   desc="the second drop instantiated its node")
        d2 = selected(tf)
        assert_eq(node_title(tf, d2), "Lerp", "the second drop is the Lerp card")
        assert_eq(node_pos(tf, d2), DROP2_GRAPH, "it lands at ITS drop point")
        assert node_pos(tf, d2) != DROP1_GRAPH, "a different drop point yields a different position"
        assert_eq(undo(tf), True, "undo the second drop")
        assert_eq(node_count(tf), 4, "back to the seed graph")

        # ── (E) a palette CLICK still adds at the SPAWN point ────────
        # A press-release IN PLACE is the R849 click-to-add — untouched by R1411.
        tf.click(path=f"{G}#palette_5")
        wait_until(lambda: node_count(tf) == 5, timeout=4.0, desc="the click added a node")
        clicked = selected(tf)
        assert_eq(node_title(tf, clicked), "Scalar", "the clicked kind")
        cx, cy = node_pos(tf, clicked)
        # The spawn point is the fixed SPAWN_X/SPAWN_Y (300, 44) projection + a
        # small fan-out cascade — its y is far above any drop y used here, so a
        # click is unmistakably NOT a drop.
        assert cy < 300, f"a click lands at the spawn point (y={cy}), not a drop point"
        assert (cx, cy) != DROP1_GRAPH, "the click position is not the drop position"
        assert_eq(undo(tf), True, "undo the click add")
        assert_eq(node_count(tf), 4, "seed graph restored")

        # ── (F) a palette drag released OFF the canvas creates nothing ─
        # March off the bottom edge (y > 420): the release is over no region, so
        # the drop-point gate rejects it AND the moved gesture fires no click.
        tf.drag(from_path=f"{G}#palette_5", to_at=(432.0, 600.0), steps=10)
        # Nothing to wait for — assert the graph is unchanged after the arc.
        assert_eq(node_count(tf), 4, "a drag released off the canvas instantiates nothing")
        assert find_by_tag(tf.snapshot(source="paint", viewport=VIEWPORT), DRAG_IMAGE) is None, (
            "no follower lingers after the off-canvas release"
        )


if __name__ == "__main__":
    sys.exit(run_demo("R1411 node palette drag-to-instantiate", body))
