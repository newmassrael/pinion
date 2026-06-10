#!/usr/bin/env python3
"""R879 §5.38 §5.40 §5.52 — node-editor multi-selection model.

Drives `hello-node-editor` via JSON-RPC. The single-node selection
generalises to a set (the marquee substrate): plain click replaces,
`Ctrl`+click toggles membership, `Shift`+click adds (an unordered graph has
no range — the Unreal convention). The set is a first-class group for every
verb: `Delete` removes the whole set + incident edges as ONE undo step,
arrow nudges move the group as one coalescing step, and grabbing a selected
node drags the whole selection rigidly. AI-first: `query selected_ids` /
`intervene selected_ids` (CSV write twin); `query selected` stays the
exact-one answer. Modifiers ride the R763 out-of-band `scene/modifiers`
channel onto the R781 third wire segment.

  (A) boot — empty selection, both wire forms agree.
  (B) plain click selects one; Ctrl+click toggles a second in and out.
  (C) Shift+click adds; plain click collapses back to a single.
  (D) Delete on a 2-node selection = one undo step restoring both + edges.
  (E) arrow nudge moves the whole set; the burst is one undo step.
  (F) dragging a selected member moves the group rigidly (one undo step).
  (G) dragging an unselected node moves only it; the selection holds.
  (H) intervene selected_ids: CSV write, strict unknown-id reject, "" clear.
  (I) F2 on a multi-selection refuses (no unambiguous rename target).
  (J) the status line surfaces the set size.

>= 30 assertions.
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
VIEWPORT = (772, 420)
NUDGE = 12  # NUDGE_STEP


def ids(tf) -> str:
    return tf.query("/external/selected_ids")


def pos(tf, i: int) -> tuple[int, int]:
    return (tf.query(f"/external/node.{i}.x"), tf.query(f"/external/node.{i}.y"))


def node_tag(i: int) -> str:
    return f"{G}#node_{i}"


def click_node(tf, i: int, *, ctrl: bool = False, shift: bool = False) -> None:
    if ctrl or shift:
        tf.modifiers(ctrl=ctrl, shift=shift)
    tf.click(path=node_tag(i))
    if ctrl or shift:
        tf.modifiers()


def texts_of(snap) -> list[str]:
    """Every Scene::Text content under the paint snapshot."""
    out: list[str] = []

    def walk(node) -> None:
        if isinstance(node, dict):
            content = node.get("content")
            if isinstance(content, str):
                out.append(content)
            for v in node.values():
                walk(v)
        elif isinstance(node, list):
            for v in node:
                walk(v)

    walk(snap)
    return out


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) boot ────────────────────────────────────────────────
        assert_eq(tf.query("/external/selected"), None, "boot: no single selection")
        assert_eq(ids(tf), "", "boot: empty selected_ids")

        # ── (B) plain click + Ctrl-toggle ───────────────────────────
        click_node(tf, 0)
        wait_until(lambda: ids(tf) == "0", timeout=4.0, interval=0.03,
                   desc="plain click selects node 0")
        assert_eq(tf.query("/external/selected"), 0, "exactly-one reads as Int")
        click_node(tf, 2, ctrl=True)
        wait_until(lambda: ids(tf) == "0,2", timeout=4.0, interval=0.03,
                   desc="Ctrl+click toggles node 2 in")
        assert_eq(tf.query("/external/selected"), None,
                  "a multi-selection has no single `selected`")
        click_node(tf, 0, ctrl=True)
        wait_until(lambda: ids(tf) == "2", timeout=4.0, interval=0.03,
                   desc="Ctrl+click toggles node 0 back out")

        # ── (C) Shift adds; plain collapses ─────────────────────────
        click_node(tf, 1, shift=True)
        wait_until(lambda: ids(tf) == "1,2", timeout=4.0, interval=0.03,
                   desc="Shift+click adds node 1")
        click_node(tf, 1, shift=True)
        assert_eq(ids(tf), "1,2", "Shift re-add is idempotent")
        click_node(tf, 3)
        wait_until(lambda: ids(tf) == "3", timeout=4.0, interval=0.03,
                   desc="plain click collapses to a single")

        # ── (D) multi-delete = one undo step ────────────────────────
        tf.intervene("/external/selected_ids", "0,2")
        assert_eq(ids(tf), "0,2", "the CSV write twin selects the pair")
        n_before = tf.query("/external/node_count")
        e_before = tf.query("/external/edge_count")
        assert_eq(tf.invoke("/external/delete_selected", None), True, "delete the set")
        assert_eq(tf.query("/external/node_count"), n_before - 2, "both nodes gone")
        assert_eq(tf.query("/external/edge_count"), 0,
                  "every edge incident to the pair went with them")
        assert_eq(tf.query("/node_undo/external/undo_label"), "Delete 2 nodes",
                  "one labelled journal entry for the whole group")
        assert_eq(tf.invoke("/node_undo/external/undo", None), True, "undo the delete")
        assert_eq(tf.query("/external/node_count"), n_before, "undo restores both nodes")
        assert_eq(tf.query("/external/edge_count"), e_before, "and every incident edge")
        assert_eq(ids(tf), "0,2", "and the selection")

        # ── (E) group nudge = one coalescing undo step ──────────────
        p0, p2 = pos(tf, 0), pos(tf, 2)
        tf.request("focus/set", {"tag": G})
        tf.key(path=G, name="ArrowRight")
        tf.key(path=G, name="ArrowRight")
        wait_until(lambda: pos(tf, 0)[0] == p0[0] + 2 * NUDGE, timeout=4.0, interval=0.03,
                   desc="two nudges move member 0")
        assert_eq(pos(tf, 2)[0], p2[0] + 2 * NUDGE, "member 2 moved with it")
        assert_eq(tf.query("/node_undo/external/undo_label"), "Move 2 nodes",
                  "the group nudge journals as a multi-move")
        assert_eq(tf.invoke("/node_undo/external/undo", None), True, "one undo")
        assert_eq(pos(tf, 0), list(p0) if isinstance(p0, list) else p0,
                  "one undo restores member 0 across the burst")
        assert_eq(pos(tf, 2), p2, "and member 2")

        # ── (F) dragging a selected member moves the group ──────────
        p0, p2 = pos(tf, 0), pos(tf, 2)
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        x, y, w, h = abs_rects_of(snap)[node_tag(0)]
        tf.drag(from_at=(x + w / 2, y + h / 2), to_at=(x + w / 2 + 60, y + h / 2 + 25))
        wait_until(lambda: pos(tf, 0) != p0, timeout=4.0, interval=0.03,
                   desc="the drag moved the grabbed member")
        d0 = (pos(tf, 0)[0] - p0[0], pos(tf, 0)[1] - p0[1])
        d2 = (pos(tf, 2)[0] - p2[0], pos(tf, 2)[1] - p2[1])
        assert_eq(d2, d0, "the unselected-axis member moved by the same delta (rigid)")
        assert_eq(ids(tf), "0,2", "a group drag never collapses the selection")
        assert_eq(tf.query("/node_undo/external/undo_label"), "Move 2 nodes",
                  "the whole group drag is ONE journal entry")
        assert_eq(tf.invoke("/node_undo/external/undo", None), True, "undo the drag")
        assert_eq(pos(tf, 0), p0, "undo restores the grabbed member")
        assert_eq(pos(tf, 2), p2, "and the co-dragged member")

        # ── (G) dragging an unselected node moves only it ───────────
        p1, p0 = pos(tf, 1), pos(tf, 0)
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        x, y, w, h = abs_rects_of(snap)[node_tag(1)]
        tf.drag(from_at=(x + w / 2, y + h / 2), to_at=(x + w / 2 + 40, y + h / 2))
        wait_until(lambda: pos(tf, 1) != p1, timeout=4.0, interval=0.03,
                   desc="the unselected node dragged")
        assert_eq(pos(tf, 0), p0, "selection members stayed put")
        assert_eq(ids(tf), "0,2", "the selection itself is untouched")
        assert_eq(tf.query("/node_undo/external/undo_label"), "Move node",
                  "a single-node drag keeps the single label")

        # ── (H) intervene selected_ids strictness ───────────────────
        try:
            tf.intervene("/external/selected_ids", "0,99")
            raise AssertionError("an unknown member must reject the whole write")
        except RpcError:
            pass
        assert_eq(ids(tf), "0,2", "the rejected write changed nothing")
        tf.intervene("/external/selected_ids", "")
        assert_eq(ids(tf), "", "an empty CSV clears the selection")

        # ── (I) F2 on a multi-selection refuses ─────────────────────
        tf.intervene("/external/selected_ids", "1,3")
        assert_eq(tf.invoke("/external/begin_rename", None), False,
                  "no unambiguous rename target in a multi-selection")
        assert_eq(tf.query("/external/renaming"), None, "nothing armed")

        # ── (J) the status line surfaces the set size ───────────────
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        status = [t for t in texts_of(snap) if "selected:" in t]
        assert status and "2 nodes" in status[0], \
            f"status line reports the set size, got {status}"


if __name__ == "__main__":
    sys.exit(run_demo("r879_node_multi_select", body))
