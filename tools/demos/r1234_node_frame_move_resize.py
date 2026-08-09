#!/usr/bin/env python3
"""R1234 §5.38 §5.52 — comment-frame MOVE-WITH-CONTENTS + RESIZE.

R1227 shipped comment frames with a read-only rect (a frame sized to its
selection at creation). R1234 makes the rect writable over the §5.12 plane
(§2 #2, no pixels), with the two canonical visual script gestures:

  * `intervene frame.<id>.x` / `frame.<id>.y` MOVE the frame AND every node it
    currently contains by the same delta, as ONE undo step ("Move frame"). The
    contents come along — a node never slides out of the box it sits in, even
    when the move is clamped at the world edge (a rigid group clamp).
  * `intervene frame.<id>.w` / `frame.<id>.h` RESIZE the box (origin fixed, no
    node moves), clamped to a minimum so it can never collapse below its chrome.
    Which nodes it contains is recomputed lazily, so widening the frame swallows
    more nodes ("Resize frame", its own undo step).

  (A) boot: 4 nodes, no frames.
  (B) frame the two left nodes (0,1); contains = "0,1".
  (C) MOVE x: the frame + nodes 0,1 shift right by 60; node 2 (outside) does
      not. One "Move frame" undo reverts all; redo re-applies.
  (D) MOVE y: the frame + its contents shift down.
  (E) RESIZE w: the box grows to swallow the whole graph (contains -> 0,1,2,3);
      the node positions are unchanged. One "Resize frame" undo reverts it.
  (F) RESIZE clamp: shrinking below the chrome clamps to the minimum.
  (G) rigid clamp: pushing x past the world keeps every node on-surface and the
      frame->node offset intact.
  (H) rejects: a non-Int rect value and an unknown frame id are typed errors.

Run from the workspace root:
    cargo build -p hello-node-editor --release
    python3 tools/demos/r1234_node_frame_move_resize.py
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    assert_eq,
    run_demo,
    wait_until,
)

UNDO = "/node_undo/external"
# Mirrors the crate constants (FRAME_HEADER_H + FRAME_PAD; WORLD - NODE_W).
FRAME_MIN = 52
WORLD_MAX_NODE_X = 2048 - 130


def q(tf: RpcSubprocess, key: str) -> Any:
    return tf.query(f"/external/{key}")


def select(tf: RpcSubprocess, ids: list[int]) -> None:
    csv = ",".join(str(i) for i in ids)
    tf.intervene("/external/selected_ids", csv)
    wait_until(lambda: True if q(tf, "selected_ids") == csv else None,
               desc=f"selection = {csv}")


def set_rect(tf: RpcSubprocess, fid: int, field: str, value: int) -> None:
    tf.intervene(f"/external/frame.{fid}.{field}", value)
    wait_until(lambda: True if q(tf, f"frame.{fid}.{field}") == value else None,
               desc=f"frame.{fid}.{field} = {value}")


def undo(tf: RpcSubprocess) -> bool:
    return tf.invoke(f"{UNDO}/undo", None)


def redo(tf: RpcSubprocess) -> bool:
    return tf.invoke(f"{UNDO}/redo", None)


def undo_label(tf: RpcSubprocess) -> Any:
    return tf.query(f"{UNDO}/undo_label")


def body() -> None:
    with RpcSubprocess("hello-node-editor", boot_grace=1.5) as tf:
        # ── (A) boot ─────────────────────────────────────────────────
        assert_eq(q(tf, "node_count"), 4, "4 seed nodes")
        assert_eq(q(tf, "frame_count"), 0, "no frames at boot")

        # ── (B) frame the two left-column nodes (0,1) ────────────────
        select(tf, [0, 1])
        fid = tf.invoke("/external/add_frame", None)
        # R1596 — a frame IS a node (`NodeBody::Frame`, the DCC's NODE_FRAME),
        # so it mints from the NODE counter: the seed graph holds four, and the
        # frame is the fifth thing in the tree.
        assert_eq(fid, 4, "the frame mints the next NODE id")
        assert_eq(q(tf, f"frame.{fid}.contains"), "0,1", "the two framed nodes are inside")
        fx0 = q(tf, f"frame.{fid}.x")
        fy0 = q(tf, f"frame.{fid}.y")
        n0x, n1x, n2x = q(tf, "node.0.x"), q(tf, "node.1.x"), q(tf, "node.2.x")

        # ── (C) MOVE x: frame + contents shift, outsider does not ────
        set_rect(tf, fid, "x", fx0 + 60)
        assert_eq(q(tf, f"frame.{fid}.x"), fx0 + 60, "frame moved right by 60")
        assert_eq(q(tf, "node.0.x"), n0x + 60, "framed node 0 moved with the frame")
        assert_eq(q(tf, "node.1.x"), n1x + 60, "framed node 1 moved with the frame")
        assert_eq(q(tf, "node.2.x"), n2x, "node 2 (outside the frame) is untouched")
        assert_eq(undo_label(tf), "Move frame", "the move is a single labelled step")
        assert_eq(undo(tf), True, "one undo reverts the whole move")
        assert_eq(q(tf, f"frame.{fid}.x"), fx0, "frame restored")
        assert_eq(q(tf, "node.0.x"), n0x, "node 0 restored")
        assert_eq(q(tf, "node.1.x"), n1x, "node 1 restored")
        assert_eq(redo(tf), True, "redo the move")
        assert_eq(q(tf, "node.0.x"), n0x + 60, "node 0 re-moved by redo")
        # settle back to the un-moved baseline for the next section.
        assert_eq(undo(tf), True, "undo back to baseline")
        assert_eq(q(tf, f"frame.{fid}.x"), fx0, "frame back at origin")

        # ── (D) MOVE y: contents come along on the other axis ────────
        n0y = q(tf, "node.0.y")
        set_rect(tf, fid, "y", fy0 + 40)
        assert_eq(q(tf, f"frame.{fid}.y"), fy0 + 40, "frame moved down by 40")
        assert_eq(q(tf, "node.0.y"), n0y + 40, "member moved down with it")
        assert_eq(undo(tf), True, "undo the y-move")
        assert_eq(q(tf, "node.0.y"), n0y, "member y restored")

        # ── (E) RESIZE w: grow to swallow the whole graph ────────────
        n0x_b, n0y_b = q(tf, "node.0.x"), q(tf, "node.0.y")
        set_rect(tf, fid, "w", 800)
        assert_eq(q(tf, f"frame.{fid}.w"), 800, "width grew to 800")
        assert_eq(q(tf, "node.0.x"), n0x_b, "a resize does not drag node 0 (x)")
        assert_eq(q(tf, "node.0.y"), n0y_b, "a resize does not drag node 0 (y)")
        # ★R1596 — MEMBERSHIP DOES NOT MOVE WITH THE BOX. The editor re-derived
        # it from the rectangle on every read, so widening the frame silently
        # adopted the other two nodes and undoing the resize abandoned them
        # again -- what the frame SAID it held changed with nobody having
        # edited membership. It is `Node::parent` now (R1589, the DCC's model), and
        # joining is the explicit act `attach` performs.
        assert_eq(
            q(tf, f"frame.{fid}.contains"), "0,1",
            "a resize changes the box, not what the frame holds"
        )
        assert_eq(undo_label(tf), "Resize frame", "a resize is its own labelled step")
        assert_eq(undo(tf), True, "undo the resize")
        assert_eq(q(tf, f"frame.{fid}.contains"), "0,1", "and undoing it leaves them alone too")
        # The geometry DID cover them while the box was wide -- which is the
        # question a gesture asks, and `attach` is what turns it into membership.
        set_rect(tf, fid, "w", 800)
        select(tf, [2, 3])
        assert_eq(tf.invoke("/external/attach", None), "2,3", "attach names who joined")
        assert_eq(q(tf, f"frame.{fid}.contains"), "0,1,2,3", "and now it holds all four")
        select(tf, [2, 3])
        tf.invoke("/external/detach", None)
        assert_eq(undo(tf), True, "back to the resize baseline")
        assert_eq(undo(tf), True, "and past it")

        # ── (F) RESIZE clamp: never collapse below the chrome ────────
        tf.intervene(f"/external/frame.{fid}.w", 1)
        wait_until(lambda: True if q(tf, f"frame.{fid}.w") == FRAME_MIN else None,
                   desc="width clamped to the minimum")
        assert_eq(q(tf, f"frame.{fid}.w"), FRAME_MIN, "width clamped to the minimum")
        tf.intervene(f"/external/frame.{fid}.h", 0)
        wait_until(lambda: True if q(tf, f"frame.{fid}.h") == FRAME_MIN else None,
                   desc="height clamped to the minimum")
        assert_eq(q(tf, f"frame.{fid}.h"), FRAME_MIN, "height clamped to the minimum")

        # ── (G) rigid group clamp at the world edge ──────────────────
        # A fresh frame around {0,1}. R1596 — the first frame still HOLDS them
        # (membership is `Node::parent`, not a rectangle re-tested on every
        # read), so they must be detached before a second frame can take them:
        # `enframe` acts on the OUTERMOST of a selection.
        select(tf, [0, 1])
        tf.invoke("/external/detach", None)
        fid2 = tf.invoke("/external/add_frame", None)
        assert fid2 > fid, f"a second frame mints past the first: {fid} -> {fid2}"
        assert_eq(q(tf, f"frame.{fid2}.contains"), "0,1", "it holds nodes 0,1")
        rel = q(tf, f"frame.{fid2}.x") - q(tf, "node.0.x")
        tf.intervene(f"/external/frame.{fid2}.x", 1_000_000)
        wait_until(lambda: True if q(tf, "node.0.x") <= WORLD_MAX_NODE_X else None,
                   desc="node clamped onto the world surface")
        assert q(tf, "node.0.x") <= WORLD_MAX_NODE_X, "node 0 stayed on the world surface"
        assert q(tf, "node.1.x") <= WORLD_MAX_NODE_X, "node 1 stayed on the world surface"
        assert_eq(q(tf, f"frame.{fid2}.x") - q(tf, "node.0.x"), rel,
                  "the frame->member offset is preserved (rigid group move)")
        # R1240 — the frame's own RIGHT edge stays on-world (no FRAME_PAD overhang).
        assert q(tf, f"frame.{fid2}.x") + q(tf, f"frame.{fid2}.w") <= 2048, "frame right edge on-world"

        # ── (H) rejects ──────────────────────────────────────────────
        type_err = False
        try:
            tf.intervene(f"/external/frame.{fid}.w", "wide")
        except RpcError:
            type_err = True
        assert type_err, "a non-Int rect value is a typed error"
        unknown = False
        try:
            tf.intervene("/external/frame.99.x", 0)
        except RpcError:
            unknown = True
        assert unknown, "an unknown frame id is a typed error"


if __name__ == "__main__":
    sys.exit(run_demo("R1234 comment-frame move + resize", body))
