#!/usr/bin/env python3
"""R1227 §5.38 §5.52 — node-graph COMMENT FRAMES.

A big graph needs grouping + labels to stay readable — the Blueprint / material-
editor "comment box". R1227 adds it as a new annotation entity (behind the nodes,
owning no graph semantics): frame the current node selection into a titled,
translucent rectangle. All AI-first over the §5.12 plane (§2 #2), no pixels:

  * `invoke add_frame` frames the selected nodes (their bbox + a margin), titled
    "Comment N", returning the new frame id; `remove_frame <id>` deletes it (the
    nodes stay). Both are ONE undoable step.
  * `query frame_count` / `frame_ids` / `frame.<id>.{title,x,y,w,h,contains}` read
    the annotation layer as data — `contains` is the CSV of node ids whose centre
    is inside the frame (the membership the paint + a11y share).
  * `intervene frame.<id>.title` renames it (undoable); the rect (x/y/w/h) is
    writable as of R1234 (see `r1234_node_frame_move_resize.py`) — v1 R1227
    sized a frame to its selection at creation and left the rect read-only.

Frames persist in the schema-4 [`SerializedGraph`] and lower to a labelled a11y
`group` per frame (unit-tested in the crate).

  (A) boot: 4 nodes, no frames.
  (B) frame the two left nodes (0,1): the rect encloses them, contains = "0,1".
  (C) rename the frame; one undo reverts it.
  (D) a second frame around ALL nodes; remove one, undo restores it.
  (E) persistence: the frames are in `query serialized`.
  (F) rejects: add_frame with no selection is Null; an unknown frame id is a
      benign no-op / error.

Run from the workspace root:
    cargo build -p hello-node-editor --release
    python3 tools/demos/r1227_node_comment_frames.py
"""

from __future__ import annotations

import re

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


def q(tf: RpcSubprocess, key: str) -> Any:
    return tf.query(f"/external/{key}")


def select(tf: RpcSubprocess, ids: list[int]) -> None:
    csv = ",".join(str(i) for i in ids)
    tf.intervene("/external/selected_ids", csv)
    wait_until(lambda: True if q(tf, "selected_ids") == csv else None,
               desc=f"selection = {csv}")


def add_frame(tf: RpcSubprocess) -> Any:
    return tf.invoke("/external/add_frame", None)


def undo(tf: RpcSubprocess) -> bool:
    return tf.invoke(f"{UNDO}/undo", None)


def undo_label(tf: RpcSubprocess) -> Any:
    return tf.query(f"{UNDO}/undo_label")


def body() -> None:
    with RpcSubprocess("hello-node-editor", boot_grace=1.5) as tf:
        # ── (A) boot ─────────────────────────────────────────────────
        assert_eq(q(tf, "node_count"), 4, "4 seed nodes")
        assert_eq(q(tf, "frame_count"), 0, "no frames at boot")
        assert_eq(q(tf, "frame_ids"), "", "empty frame enumeration")

        # ── (B) frame the two left-column nodes (0,1) ────────────────
        select(tf, [0, 1])
        fid = add_frame(tf)
        # R1596 — a frame IS a node (`NodeBody::Frame`, Blender's NODE_FRAME),
        # so it mints from the NODE counter: the seed graph holds four, and the
        # frame is the fifth thing in the tree.
        assert_eq(fid, 4, "the frame mints the next NODE id")
        assert_eq(q(tf, "frame_count"), 1, "one frame now")
        assert_eq(q(tf, "frame_ids"), str(fid), "frame id enumerated")
        assert_eq(q(tf, f"frame.{fid}.title"), "Comment 1", "auto title")
        assert_eq(q(tf, f"frame.{fid}.contains"), "0,1", "the two framed nodes are inside")
        # The rect encloses node 0 (Texture @ graph 40,70) with a margin.
        assert q(tf, f"frame.{fid}.x") <= 40, "left edge left of the framed nodes"
        assert q(tf, f"frame.{fid}.y") <= 70, "top edge above the framed nodes"
        assert q(tf, f"frame.{fid}.w") > 0 and q(tf, f"frame.{fid}.h") > 0, "positive extent"
        # Framing did NOT change the node selection (a separate axis).
        assert_eq(q(tf, "selected_ids"), "0,1", "node selection untouched by framing")

        # ── (C) rename the frame; one undo reverts it ────────────────
        tf.intervene(f"/external/frame.{fid}.title", "Inputs")
        wait_until(lambda: True if q(tf, f"frame.{fid}.title") == "Inputs" else None,
                   desc="frame renamed to Inputs")
        assert_eq(undo_label(tf), "Rename frame", "rename is its own undo step")
        assert_eq(undo(tf), True, "undo the rename")
        assert_eq(q(tf, f"frame.{fid}.title"), "Comment 1", "title reverted")

        # ── (D) a second frame around ALL nodes; remove + undo ───────
        select(tf, [0, 1, 2, 3])
        fid2 = add_frame(tf)
        assert fid2 > fid, f"a second frame mints past the first: {fid} -> {fid2}"
        assert_eq(q(tf, "frame_count"), 2, "two frames")
        assert_eq(q(tf, f"frame.{fid2}.contains"), "0,1,2,3", "the big frame holds all four")
        assert_eq(tf.invoke("/external/remove_frame", fid2), True, "remove the big frame")
        assert_eq(q(tf, "frame_count"), 1, "back to one frame")
        assert_eq(q(tf, "node_count"), 4, "removing a frame keeps the nodes")
        assert_eq(undo_label(tf), "Remove frame", "remove is its own undo step")
        assert_eq(undo(tf), True, "undo the remove")
        assert_eq(q(tf, "frame_count"), 2, "the frame is restored")

        # ── (E) persistence ──────────────────────────────────────────
        blob = q(tf, "serialized")
        assert "Comment 1" in blob, "the frame is in the serialized graph"
        # R1599 — the version is read off the blob rather than pinned to a
        # literal here. A demo asserting a NUMBER is a third source of truth for
        # it (the constant, a unit test, and this), and it goes stale on every
        # bump; what this section is actually about is that a frame ROUND-TRIPS,
        # so what it needs from the version is that one is present and stated.
        version = re.search(r'"schema_version":(\d+)', blob)
        assert version, f"the blob states its schema version: {blob[:120]}"
        assert int(version.group(1)) >= 8, (
            "and it is at least the version at which the blob became a Document"
        )

        # ── (F) rejects ──────────────────────────────────────────────
        # Frame nothing: add_frame with an empty selection returns Null.
        tf.intervene("/external/selected", None)  # clear the node selection
        wait_until(lambda: True if q(tf, "selected_ids") == "" else None,
                   desc="selection cleared")
        assert_eq(add_frame(tf), None, "framing an empty selection creates nothing")
        assert_eq(q(tf, "frame_count"), 2, "frame count unchanged")
        # The rect is writable as of R1234 (move / resize) — a non-Int value on a
        # rect field is a typed error; move / resize coverage lives in the R1234
        # demo.
        type_err = False
        try:
            tf.intervene(f"/external/frame.{fid}.x", "nope")
        except RpcError:
            type_err = True
        assert type_err, "a non-Int rect value is rejected"
        # An unknown frame id is a benign no-op / typed error.
        assert_eq(tf.invoke("/external/remove_frame", 99), False, "unknown frame id -> false")


if __name__ == "__main__":
    sys.exit(run_demo("R1227 node-graph comment frames", body))
