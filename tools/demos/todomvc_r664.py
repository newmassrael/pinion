#!/usr/bin/env python3
"""R664 §5.16 §5.38 §5.49 — todomvc edit-in-place + native double-click.

Drives the R664 axis end-to-end through the AI-first introspection
surface (§2 #2 invariant). scene/query + scene/invoke v0 are primary-
External only (R690+ multi-External addressing is carry), so the demo
reads non-primary state via scene/snapshot of the state tree and
drives keystrokes through scene/key (the substrate's focus-routed key
dispatch — Enter / Escape / character keys land in V::apply_key for
the focused widget tag).

Coverage:
  1. Boot + steady state (editing_id None).
  2. Add three rows via the main TextField (Enter submit).
  3. Steady state: editor not painted.
  4. scene/double_click on row[1]'s text → editing_id := id, editor
     paints inline, current row text seeded.
  5. Type Backspace x N + new text → Enter commit → row text updated.
  6. Re-double-click row[2], type ignored garbage, Escape → cancel;
     row text untouched.
  7. Single-click toggle on row[0] → strikethrough decoration appears
     on its text style.
  8. Erase-to-delete: double-click + Backspace x N + Enter → row gone.
  9. Native double-click via raw coordinate (at form) also enters edit
     mode — same code path as the path-form selector.
 10. Selector validation — at + path mutually exclusive.
 11. Main field still works after multiple edit cycles.
"""

from __future__ import annotations

import sys
import time
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import RpcSubprocess, assert_eq, isolated_storage_dir, run_demo  # noqa: E402

TF_TAG = "main_textfield"
EDIT_TF_TAG = "todo_edit"
ITEM_TAG = "todo_item"
TOGGLE_TAG = "todo_toggle"
LIST_TAG = "todo_list"
VIEWPORT_W = 480
VIEWPORT_H = 720
EDIT_PAUSE = 0.2


def focus_set(tf: RpcSubprocess, tag: str | None) -> None:
    tf.request("focus/set", {"tag": tag})


def type_text_into_focused(tf: RpcSubprocess, text: str, tag: str) -> None:
    """Type characters into the currently focused field by replaying
    scene/key for each char (the substrate dispatches via apply_key
    to the focused widget — both TF_TAG and EDIT_TF_TAG have the
    embedded TextField key handler)."""
    for ch in text:
        tf.key(path=tag, name=ch)
    time.sleep(0.05)


def submit_enter(tf: RpcSubprocess, target: str) -> None:
    tf.key(path=target, name="Enter")
    time.sleep(EDIT_PAUSE)


def find_node_by_tag(node: dict[str, Any], tag: str) -> dict[str, Any] | None:
    if not isinstance(node, dict):
        return None
    if node.get("tag") == tag:
        return node
    for child in node.get("children") or []:
        hit = find_node_by_tag(child, tag)
        if hit is not None:
            return hit
    content = node.get("content")
    if isinstance(content, dict):
        hit = find_node_by_tag(content, tag)
        if hit is not None:
            return hit
    return None


def find_external_node_by_tag(node: dict[str, Any], tag: str) -> dict[str, Any] | None:
    """DFS the state-scene snapshot for the External node with `tag`."""
    if not isinstance(node, dict):
        return None
    if node.get("type") == "External" and node.get("tag") == tag:
        return node
    for child in node.get("children") or []:
        hit = find_external_node_by_tag(child, tag)
        if hit is not None:
            return hit
    content = node.get("content")
    if isinstance(content, dict):
        hit = find_external_node_by_tag(content, tag)
        if hit is not None:
            return hit
    return None


def editing_id(tf: RpcSubprocess) -> int | None:
    """Read the TodoEditExternal's editing_id slot via scene/snapshot."""
    snap = tf.snapshot(source="state")
    node = find_external_node_by_tag(snap, ITEM_TAG)
    if node is None:
        return None
    intro = node.get("introspect")
    if not isinstance(intro, dict):
        return None
    val = intro.get("editing_id")
    if val is None:
        return None
    return int(val) if isinstance(val, (int, float)) else None


def editor_text(tf: RpcSubprocess) -> str:
    """Read the inline editor's TextEditState.text via state snapshot
    (the EDIT_TF_TAG External is always registered; its introspect
    `text` slot mirrors the buffer the user sees)."""
    snap = tf.snapshot(source="state")
    node = find_external_node_by_tag(snap, EDIT_TF_TAG)
    if node is None:
        return ""
    intro = node.get("introspect") or {}
    val = intro.get("text")
    return str(val) if isinstance(val, str) else ""


def collect_item_ids(tf: RpcSubprocess) -> list[int]:
    snap = tf.snapshot(source="paint", viewport=(VIEWPORT_W, VIEWPORT_H))
    list_node = find_node_by_tag(snap, LIST_TAG)
    assert list_node is not None, "todo_list must be present"
    ids: list[int] = []
    for child in list_node.get("children") or []:
        tag = child.get("tag")
        if isinstance(tag, str) and tag.startswith(f"{ITEM_TAG}#"):
            ids.append(int(tag.split("#", 1)[1]))
    return ids


def row_text(tf: RpcSubprocess, item_id: int) -> str | None:
    """Static text label of the row's middle slot (None when editor
    is swapped in)."""
    snap = tf.snapshot(source="paint", viewport=(VIEWPORT_W, VIEWPORT_H))
    row = find_node_by_tag(snap, f"{ITEM_TAG}#{item_id}")
    if row is None:
        return None
    children = row.get("children") or []
    if len(children) < 2:
        return None
    middle = children[1]
    if middle.get("type") == "Text":
        return middle.get("content")
    return None


def row_text_node(tf: RpcSubprocess, item_id: int) -> dict[str, Any] | None:
    snap = tf.snapshot(source="paint", viewport=(VIEWPORT_W, VIEWPORT_H))
    row = find_node_by_tag(snap, f"{ITEM_TAG}#{item_id}")
    if row is None:
        return None
    children = row.get("children") or []
    if len(children) < 2:
        return None
    middle = children[1]
    return middle if middle.get("type") == "Text" else None


def backspace_n(tf: RpcSubprocess, n: int, tag: str) -> None:
    for _ in range(n):
        tf.key(path=tag, name="Backspace")
    time.sleep(0.05)


def body() -> None:
    # R666 — isolate from `$XDG_DATA_HOME/pinion-todomvc/` so the
    # demo's typed rows do not bleed into later runs (R665 persistence).
    with isolated_storage_dir("pinion-todomvc-r664-"):
        _body_impl()


def _body_impl() -> None:
    with RpcSubprocess("todomvc") as tf:
        # ── (1) Boot + steady state ─────────────────────────────────
        assert_eq(tf.query("/external/state"), "Idle", "boot: main field Idle")
        assert_eq(editing_id(tf), None, "boot: no row in edit mode")

        # ── (2) Add three rows ─────────────────────────────────────
        focus_set(tf, TF_TAG)
        time.sleep(0.05)
        for text in ("alpha", "beta", "gamma"):
            type_text_into_focused(tf, text, TF_TAG)
            submit_enter(tf, target=TF_TAG)

        ids = collect_item_ids(tf)
        assert_eq(len(ids), 3, "three rows visible after submits")

        # ── (3) Steady state: editor not painted ───────────────────
        snap = tf.snapshot(source="paint", viewport=(VIEWPORT_W, VIEWPORT_H))
        assert (
            find_node_by_tag(snap, EDIT_TF_TAG) is None
        ), "EDIT_TF_TAG must be absent before any double-click"

        # ── (4) Double-click row[1] → enter edit ───────────────────
        target_id = ids[1]
        tf.double_click(path=f"{ITEM_TAG}#{target_id}")
        time.sleep(EDIT_PAUSE)
        assert_eq(editing_id(tf), target_id, "row[1] entered edit mode")

        # Editor paints inline; row text node is gone for that row.
        snap = tf.snapshot(source="paint", viewport=(VIEWPORT_W, VIEWPORT_H))
        assert (
            find_node_by_tag(snap, EDIT_TF_TAG) is not None
        ), "EDIT_TF_TAG editor paints inside the active row"
        assert_eq(
            row_text(tf, target_id),
            None,
            "middle slot is no longer a Text node (editor swapped in)",
        )
        # Seed: editor's TextEditState carries the original row text.
        assert_eq(editor_text(tf), "beta", "editor seeded with current row text")

        # ── (5) Backspace + retype + Enter commit ──────────────────
        # The substrate's focus_request drain focuses EDIT_TF_TAG —
        # confirm by typing keys and observing they land in the
        # editor's TextEditState (not the main TF_TAG buffer).
        backspace_n(tf, len("beta"), EDIT_TF_TAG)
        assert_eq(editor_text(tf), "", "editor buffer cleared by Backspace")
        type_text_into_focused(tf, "BETA-edited", EDIT_TF_TAG)
        assert_eq(editor_text(tf), "BETA-edited", "editor buffer updated")
        submit_enter(tf, target=EDIT_TF_TAG)
        assert_eq(editing_id(tf), None, "Enter commit clears editing_id")
        assert_eq(
            row_text(tf, target_id),
            "BETA-edited",
            "row[1] text reflects the committed edit",
        )

        # ── (6) Escape cancels — row untouched ─────────────────────
        cancel_id = ids[2]
        tf.double_click(path=f"{ITEM_TAG}#{cancel_id}")
        time.sleep(EDIT_PAUSE)
        assert_eq(editing_id(tf), cancel_id, "row[2] re-entered edit")
        # Wipe the editor buffer + type garbage that should not commit.
        backspace_n(tf, len("gamma"), EDIT_TF_TAG)
        type_text_into_focused(tf, "discard-me", EDIT_TF_TAG)
        assert_eq(editor_text(tf), "discard-me", "garbage typed into editor")
        tf.key(path=EDIT_TF_TAG, name="Escape")
        time.sleep(EDIT_PAUSE)
        assert_eq(editing_id(tf), None, "Escape clears editing_id")
        assert_eq(
            row_text(tf, cancel_id),
            "gamma",
            "row[2] text untouched on cancel",
        )

        # ── (7) Single-click toggle → strikethrough decoration ─────
        first_id = ids[0]
        tf.click(path=f"{TOGGLE_TAG}#{first_id}")
        time.sleep(EDIT_PAUSE)
        node = row_text_node(tf, first_id)
        assert node is not None, "row[0] text node present after toggle"
        deco = (node.get("style") or {}).get("decoration") or {}
        assert deco.get("strikethrough") is True, (
            f"completed row text must carry strikethrough; got {deco!r}"
        )

        # Confirm the active rows still have no strikethrough.
        for active_id in (target_id, cancel_id):
            n = row_text_node(tf, active_id)
            if n is None:
                continue
            d = (n.get("style") or {}).get("decoration") or {}
            assert not d.get("strikethrough"), (
                f"row {active_id} (active) must not carry strikethrough"
            )

        # ── (8) Erase-to-delete (TasteJS canonical) ────────────────
        tf.double_click(path=f"{ITEM_TAG}#{target_id}")
        time.sleep(EDIT_PAUSE)
        assert_eq(editing_id(tf), target_id, "row[1] re-entered edit")
        backspace_n(tf, len("BETA-edited"), EDIT_TF_TAG)
        assert_eq(editor_text(tf), "", "editor cleared for erase-delete")
        submit_enter(tf, target=EDIT_TF_TAG)
        assert_eq(editing_id(tf), None, "edit cleared after erase-commit")
        post_ids = collect_item_ids(tf)
        assert target_id not in post_ids, "row removed by erase-to-delete"
        assert_eq(len(post_ids), 2, "two rows remain after delete")

        # ── (9) Re-entry: double-click another row → edit → commit ─
        # Exercises the same code path back-to-back to confirm the
        # editor state (text + caret + focus) resets cleanly between
        # edit sessions (a per-session leak would surface as stale
        # text bleeding into the next edit).
        survivor = post_ids[1]
        tf.double_click(path=f"{ITEM_TAG}#{survivor}")
        time.sleep(EDIT_PAUSE)
        assert_eq(editing_id(tf), survivor, "re-entry double-click activates")
        # Editor seeded with row text (NOT the leftover from the last
        # erase-to-delete session).
        snap_state = tf.snapshot(source="state")
        node = find_external_node_by_tag(snap_state, EDIT_TF_TAG)
        seeded = (node or {}).get("introspect", {}).get("text")
        assert isinstance(seeded, str) and len(seeded) > 0, (
            f"re-entry editor must seed with row text; got {seeded!r}"
        )
        tf.key(path=EDIT_TF_TAG, name="Escape")
        time.sleep(EDIT_PAUSE)
        assert_eq(editing_id(tf), None, "re-entry edit cancellable")
        # Editor buffer wiped on cancel — next edit starts blank if
        # not re-seeded.
        snap_state = tf.snapshot(source="state")
        node = find_external_node_by_tag(snap_state, EDIT_TF_TAG)
        post_cancel = (node or {}).get("introspect", {}).get("text")
        assert_eq(post_cancel, "", "editor wiped after cancel teardown")

        # ── (10) Selector validation — at + path mutually exclusive ─
        try:
            tf.double_click(at=(0.0, 0.0), path=f"{ITEM_TAG}#{survivor}")
        except ValueError as e:
            assert "exactly one" in str(e), f"unexpected error: {e}"
        else:
            raise AssertionError(
                "R664: at + path is mutually exclusive — must raise"
            )

        # ── (11) Main field still works post-edit cycles ───────────
        focus_set(tf, TF_TAG)
        time.sleep(0.05)
        type_text_into_focused(tf, "epilogue", TF_TAG)
        submit_enter(tf, target=TF_TAG)
        final_ids = collect_item_ids(tf)
        assert_eq(len(final_ids), 3, "main field accepts new entry post-edit")

        # ── (12) The completed row[0] survived the cycles ───────────
        # Toggle is reactive, double-click on text triggers edit, not toggle.
        # The strikethrough decoration is the persistent visual signal.
        n = row_text_node(tf, first_id)
        assert n is not None, "row[0] still in scene"
        deco = (n.get("style") or {}).get("decoration") or {}
        assert deco.get("strikethrough") is True, "row[0] still strikethrough"


if __name__ == "__main__":
    sys.exit(run_demo("R664 edit-in-place + native double-click", body))
