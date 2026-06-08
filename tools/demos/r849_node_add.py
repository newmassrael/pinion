#!/usr/bin/env python3
"""R849 §5.38 — node-graph node creation (palette + add_node).

Drives hello-node-editor over JSON-RPC. R838-R842 made the node graph
movable / connectable / deletable but a **closed set** — you could rearrange
and rewire the 4 seed nodes, never grow the graph. R849 makes it *authorable*:
an "add node" palette sidebar **and** an `add_node` RPC verb create new nodes
with fresh stable ids (monotonic, never reused), the foundation the self-hosted
blueprint / material-graph editor needs.

  (A) boot — the 4-node material graph (Texture/Color -> Multiply -> Output) +
      the palette sidebar of 5 kind cards.
  (B) RPC add_node — create by kind name; returns the new stable id; the node
      is addressable (title / ports / position) and selected; an unknown kind
      is rejected and changes nothing.
  (C) palette click — clicking a sidebar card creates that kind of node, which
      paints as a real card on the canvas.
  (D) the new node is a first-class citizen — connect it, move it, delete it;
      a deleted id is never reused (monotonic mint).
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    run_demo,
    wait_until,
)

EXAMPLE = "hello-node-editor"
G = "node_graph"
# PALETTE_W (132) + WIN_W (640), WIN_H (420).
WIN = (132 + 640, 420)
# Ids 0..3 are the seed nodes; the first minted id is 4.
FIRST_DYN = 4
NKINDS = 5  # Texture / Color / Multiply / Add / Output


def ncount(tf) -> int:
    return tf.query("/external/node_count")


def ecount(tf) -> int:
    return tf.query("/external/edge_count")


def node_ids(tf) -> list[int]:
    csv = tf.query("/external/node_ids")
    return [int(x) for x in csv.split(",")] if csv else []


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) boot ────────────────────────────────────────────────
        assert_eq(ncount(tf), 4, "boot: 4 seed nodes")
        assert_eq(ecount(tf), 3, "boot: 3 seed edges")
        assert_eq(tf.query("/external/node_ids"), "0,1,2,3", "boot: dense seed id space")
        assert_eq(tf.query("/external/node.2.title"), "Multiply", "seed node 2 is Multiply")
        # The palette sidebar painted its 5 kind cards.
        rects = abs_rects_of(tf.snapshot(source="paint", viewport=WIN))
        for idx in range(NKINDS):
            assert f"{G}#palette_{idx}" in rects, f"palette card {idx} present at boot"

        # ── (B) RPC add_node ────────────────────────────────────────
        new_id = tf.invoke("/external/add_node", "Multiply")
        assert_eq(new_id, FIRST_DYN, "add_node returns the first minted id")
        assert_eq(ncount(tf), 5, "RPC add_node grew the graph")
        assert_eq(tf.query(f"/external/node.{new_id}.title"), "Multiply", "new node title")
        assert_eq(tf.query(f"/external/node.{new_id}.inputs"), 2, "Multiply has 2 inputs")
        assert_eq(tf.query(f"/external/node.{new_id}.outputs"), 1, "Multiply has 1 output")
        assert_eq(tf.query("/external/selected"), new_id, "the new node is selected")
        assert new_id in node_ids(tf), "node_ids enumerates the new sparse id"
        # An unknown kind is rejected and changes nothing.
        rejected = False
        try:
            tf.invoke("/external/add_node", "Bogus")
        except RpcError:
            rejected = True
        assert rejected, "an unknown kind is rejected"
        assert_eq(ncount(tf), 5, "the rejected add_node changed nothing")

        # ── (C) palette click ───────────────────────────────────────
        tf.click(path=f"{G}#palette_0")  # the Texture card
        tex_id = wait_until(
            lambda: tf.query("/external/selected") if ncount(tf) == 6 else None,
            desc="a palette card click adds a node",
        )
        assert_eq(ncount(tf), 6, "palette click added a node")
        assert_eq(tf.query(f"/external/node.{tex_id}.title"), "Texture", "palette card 0 makes a Texture")
        assert_eq(tf.query(f"/external/node.{tex_id}.outputs"), 1, "Texture is a source (1 out, 0 in)")
        assert_eq(tf.query(f"/external/node.{tex_id}.inputs"), 0, "Texture has no inputs")

        # The new node paints as a real card on the canvas.
        def tex_card_rendered():
            r = abs_rects_of(tf.snapshot(source="paint", viewport=WIN))
            return r if f"{G}#node_{tex_id}" in r else None

        rects = wait_until(tex_card_rendered, desc="the added node paints a card")
        assert f"{G}#node_{tex_id}" in rects, "the added Texture node renders on the canvas"

        # ── (D) the new node is a first-class citizen ───────────────
        # Connect Texture(0).out0 -> the new Multiply(4).in0.
        assert_eq(tf.invoke("/external/add_edge", f"0,0,{new_id},0"), True, "wire into the new node")
        assert_eq(ecount(tf), 4, "the new edge landed")
        # Move it (intervene the canvas position; clamped, read back).
        tf.intervene(f"/external/node.{new_id}.x", 220)
        assert_eq(tf.query(f"/external/node.{new_id}.x"), 220, "the new node moves like any node")
        # Delete it — node + incident edge gone, survivors keep their ids.
        assert_eq(tf.invoke("/external/delete_node", new_id), True, "delete the new node")
        assert_eq(ncount(tf), 5, "delete removed the node")
        assert_eq(ecount(tf), 3, "delete removed its incident edge")
        assert new_id not in node_ids(tf), "the deleted id is gone from the id space"
        # Monotonic mint: a fresh add never reuses the deleted id.
        reborn = tf.invoke("/external/add_node", "Add")
        assert reborn > new_id, f"a minted id ({reborn}) never reuses a deleted one ({new_id})"
        assert_eq(tf.query(f"/external/node.{reborn}.title"), "Add", "the reborn node is the requested kind")
        assert_eq(ncount(tf), 6, "the reborn node grew the graph again")


if __name__ == "__main__":
    sys.exit(run_demo("R849 §5.38 — node-graph node creation (palette + add_node)", body))
