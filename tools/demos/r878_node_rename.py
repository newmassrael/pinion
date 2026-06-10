#!/usr/bin/env python3
"""R878 §5.38 §5.40 §5.52 — node-editor inline rename.

Drives `hello-node-editor` via JSON-RPC. The authorable graph gains its
last missing verb: double-click a node card (or `F2` on the selection, or
`invoke begin_rename`) opens ONE shared inline `TextFieldExternal` over the
node's title (the R790 todomvc `EDIT_TF` modal-member shape). `Enter`
commits, `Escape` cancels, a click-away commits through the R793 blur
intent. A committed rename journals an undoable `RenameNodeCmd`; the
AI-first write-twin is `intervene node.<id>.title` (`query renaming` is the
in-flight read; `query node.<id>.title` reads the result back).

  (A) boot — no rename in flight; the shared field is unpainted.
  (B) double-click a node card opens the editor seeded with the title.
  (C) typing + Enter commits; the title and the status line update.
  (D) Ctrl+Z undoes the rename, Ctrl+Shift+Z redoes it.
  (E) Escape cancels without touching the title.
  (F) a click-away commits (the R793 blur intent).
  (G) F2 renames the selection (keyboard parity).
  (H) `invoke begin_rename` / `query renaming` — the RPC pair.
  (I) `intervene node.<id>.title` — the undoable write-twin (trim,
      empty-rejection, type mismatch).
  (J) renaming migrates: double-click another node commits the first.
  (K) the editor works on a zoomed canvas (the R877 viewport).
  (L) save / load round-trips the renamed title.

>= 30 assertions.
"""

from __future__ import annotations

import sys
import time
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
RENAME = "node_rename"
VIEWPORT = (772, 420)


def renaming(tf):
    return tf.query("/external/renaming")


def title(tf, node_id: int):
    return tf.query(f"/external/node.{node_id}.title")


def editor_text(tf):
    return tf.query(f"/{RENAME}/external/text")


def field_painted(tf) -> bool:
    snap = tf.snapshot(source="paint", viewport=VIEWPORT)
    return RENAME in abs_rects_of(snap)


def wait_renaming(tf, want, desc: str) -> None:
    wait_until(lambda: renaming(tf) == want, timeout=4.0, interval=0.03, desc=desc)


def begin_via_double_click(tf, node_id: int) -> None:
    tf.double_click(path=f"{G}#node_{node_id}")
    wait_renaming(tf, node_id, f"dblclick arms the rename of node {node_id}")


def retype(tf, new_text: str) -> None:
    """Erase the seeded title (caret parks at the end) and type a new one."""
    current = editor_text(tf)
    for _ in range(len(current)):
        tf.key(path=RENAME, name="Backspace")
    tf.text(new_text, path=RENAME)


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) boot — idle ─────────────────────────────────────────
        assert_eq(renaming(tf), None, "boot: no rename in flight")
        assert_eq(title(tf, 2), "Multiply", "seed title intact")
        assert not field_painted(tf), "the shared field is unpainted while idle"

        # ── (B) double-click opens the seeded editor ────────────────
        begin_via_double_click(tf, 2)
        wait_until(lambda: field_painted(tf), timeout=4.0, interval=0.03,
                   desc="the shared field paints over the title")
        assert_eq(editor_text(tf), "Multiply", "editor seeded with the current title")
        assert_eq(tf.request("focus/get").result.get("focused"), RENAME,
                  "the field owns keyboard focus")

        # ── (C) typing + Enter commits ──────────────────────────────
        retype(tf, "Mix")
        wait_until(lambda: editor_text(tf) == "Mix", timeout=4.0, interval=0.03,
                   desc="keystrokes reach the shared field")
        tf.key(path=RENAME, name="Enter")
        wait_renaming(tf, None, "Enter leaves rename mode")
        assert_eq(title(tf, 2), "Mix", "the commit renamed the node")
        assert not field_painted(tf), "the field unpaints after the commit"
        assert_eq(tf.request("focus/get").result.get("focused"), G,
                  "focus returns to the canvas")

        # ── (D) the rename is one undo step ─────────────────────────
        assert_eq(tf.query("/node_undo/external/undo_label"), "Rename node",
                  "the rename journaled undoably")
        tf.modifiers(ctrl=True)
        tf.key(path=G, name="z")
        tf.modifiers()
        wait_until(lambda: title(tf, 2) == "Multiply", timeout=4.0, interval=0.03,
                   desc="Ctrl+Z restores the old title")
        tf.modifiers(ctrl=True, shift=True)
        tf.key(path=G, name="z")
        tf.modifiers()
        wait_until(lambda: title(tf, 2) == "Mix", timeout=4.0, interval=0.03,
                   desc="Ctrl+Shift+Z re-applies the rename")

        # ── (E) Escape cancels ──────────────────────────────────────
        begin_via_double_click(tf, 0)
        retype(tf, "Scrap")
        tf.key(path=RENAME, name="Escape")
        wait_renaming(tf, None, "Escape leaves rename mode")
        assert_eq(title(tf, 0), "Texture", "a cancel never touches the title")

        # ── (F) a click-away commits (R793 blur intent) ─────────────
        begin_via_double_click(tf, 0)
        retype(tf, "Albedo")
        wait_until(lambda: editor_text(tf) == "Albedo", timeout=4.0, interval=0.03,
                   desc="typed the blur-commit candidate")
        # Click empty canvas: focus leaves the field -> blur -> commit.
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        gx, gy, gw, gh = abs_rects_of(snap)[G]
        tf.click(at=(gx + gw - 12, gy + gh - 12))
        wait_renaming(tf, None, "the blur committed and left rename mode")
        assert_eq(title(tf, 0), "Albedo", "the click-away committed the typed title")

        # ── (G) F2 renames the selection ────────────────────────────
        tf.request("focus/set", {"tag": G})
        tf.intervene("/external/selected", 1)
        tf.key(path=G, name="F2")
        wait_renaming(tf, 1, "F2 arms the rename of the selected node")
        assert_eq(editor_text(tf), "Color", "seeded from the selection")
        tf.key(path=RENAME, name="Escape")
        wait_renaming(tf, None, "cleanup: Escape")

        # ── (H) the RPC pair: begin_rename / renaming ───────────────
        assert_eq(tf.invoke("/external/begin_rename", 3), True, "begin_rename by id")
        assert_eq(renaming(tf), 3, "query renaming reports the in-flight target")
        assert_eq(tf.invoke("/external/begin_rename", 99), False,
                  "an unknown id is rejected (graph unchanged)")
        assert_eq(renaming(tf), 3, "the rejected begin did not steal the rename")
        tf.key(path=RENAME, name="Escape")
        wait_renaming(tf, None, "cleanup: Escape")
        tf.intervene("/external/selected", None)
        assert_eq(tf.invoke("/external/begin_rename", None), False,
                  "Null with no selection is a no-op")

        # ── (I) intervene node.<id>.title — the undoable write-twin ─
        tf.intervene("/external/node.3.title", "  Sum  ")
        assert_eq(title(tf, 3), "Sum", "the RPC rename applies trimmed")
        assert_eq(tf.query("/node_undo/external/undo_label"), "Rename node",
                  "the RPC rename journals the same undo step")
        try:
            tf.intervene("/external/node.3.title", "   ")
            raise AssertionError("an empty title must be rejected")
        except RpcError:
            pass
        assert_eq(title(tf, 3), "Sum", "the rejected write left the title alone")
        try:
            tf.intervene("/external/node.3.title", 7)
            raise AssertionError("a non-Text title must be a type mismatch")
        except RpcError:
            pass

        # ── (J) migration: dblclick another node commits the first ──
        begin_via_double_click(tf, 2)
        retype(tf, "Blend")
        wait_until(lambda: editor_text(tf) == "Blend", timeout=4.0, interval=0.03,
                   desc="typed the migration candidate")
        begin_via_double_click(tf, 1)
        assert_eq(title(tf, 2), "Blend", "opening node 1's editor committed node 2's text")
        assert_eq(editor_text(tf), "Color", "the editor reseeded from the new target")
        tf.key(path=RENAME, name="Escape")
        wait_renaming(tf, None, "cleanup: Escape")

        # ── (K) the editor works on a zoomed canvas (R877) ──────────
        tf.intervene("/external/viewport.zoom", 2.0)
        assert_eq(tf.query("/external/viewport.zoom"), 2.0, "canvas at 200%")
        tf.invoke("/external/frame_all", None)
        time.sleep(0.1)
        begin_via_double_click(tf, 1)
        assert field_painted(tf), "the field paints inside the zoomed header"
        retype(tf, "Tint")
        tf.key(path=RENAME, name="Enter")
        wait_renaming(tf, None, "zoomed commit leaves rename mode")
        assert_eq(title(tf, 1), "Tint", "a rename at 200% commits identically")
        tf.intervene("/external/viewport.zoom", 1.0)

        # ── (L) save / load round-trips the renamed titles ──────────
        assert_eq(tf.invoke("/external/save", None), True, "save snapshots the graph")
        tf.intervene("/external/node.1.title", "Scratch")
        assert_eq(title(tf, 1), "Scratch", "post-save scratch rename applied")
        assert_eq(tf.invoke("/external/load", None), True, "load restores the snapshot")
        assert_eq(title(tf, 1), "Tint", "the persisted title round-trips")
        assert_eq(title(tf, 2), "Blend", "every committed rename persisted")
        assert_eq(renaming(tf), None, "no rename in flight after the load")


if __name__ == "__main__":
    sys.exit(run_demo("r878_node_rename", body))
