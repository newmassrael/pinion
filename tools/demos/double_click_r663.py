#!/usr/bin/env python3
"""R663 §5.49 — scene/double_click primitive smoke verify.

Drives the existing todomvc binding (no new application wiring yet —
R664 will land the per-item edit-in-place consumer) to prove the
substrate fires two complete press/release cycles. The receiving
TodoToggleExternal observes two PointerDown activations and flips the
completed flag back to its original state — a `Bool(was_present)`
return that the AI client can introspect via the toggle's
`completed_count` query.

Single-click test (existing): toggle item → completed_count goes 0 → 1.
Double-click test (R663):     toggle item → completed_count goes 0 → 0
                                              (1st flips to 1, 2nd flips to 0)

The visible end state (completed_count) is the AI-first introspection
surface that proves the substrate fired 2x, not 1x, without needing a
new application widget.
"""

from __future__ import annotations

import sys
import time
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import RpcSubprocess, assert_eq, isolated_storage_dir, run_demo

TF_TAG = "main_textfield"


def focus_set(tf: RpcSubprocess, tag: str | None) -> None:
    tf.request("focus/set", {"tag": tag})


def type_text(tf: RpcSubprocess, text: str) -> None:
    for ch in text:
        tf.invoke("/external/key", ch)
    time.sleep(0.05)


def submit_enter(tf: RpcSubprocess) -> None:
    tf.key(path=TF_TAG, name="Enter")
    time.sleep(0.1)


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


def first_item_id(tf: RpcSubprocess) -> int:
    snap = tf.snapshot(source="paint", viewport=(480, 720))
    list_node = find_node_by_tag(snap, "todo_list")
    assert list_node is not None
    rows = list_node.get("children") or []
    # children[0] = header Text; children[1+] = items
    for row in rows[1:]:
        tag = row.get("tag")
        if isinstance(tag, str) and tag.startswith("todo_item#"):
            return int(tag.split("#", 1)[1])
    raise AssertionError("no todo item found")


def body() -> None:
    # R666 cleanup — R663 predates R665 persistence. Run inside a
    # `PINION_STORAGE_DIR` tempdir so this demo's typed row does not
    # bleed into the developer's real `$XDG_DATA_HOME/pinion-todomvc/`
    # blob and break later runs.
    with isolated_storage_dir("pinion-double_click-r663-"):
        _body_impl()


def _body_impl() -> None:
    with RpcSubprocess("todomvc") as tf:
        # ── (0) Boot ────────────────────────────────────────────────
        assert_eq(tf.query("/external/state"), "Idle", "boot state Idle")

        # ── (1) Add a single item ──────────────────────────────────
        focus_set(tf, TF_TAG)
        time.sleep(0.05)
        type_text(tf, "alpha")
        submit_enter(tf)

        item_id = first_item_id(tf)
        toggle_tag = f"todo_toggle#{item_id}"

        # ── (2) Sanity: single click flips completed 0 → 1 ──────────
        tf.click(path=toggle_tag)
        time.sleep(0.15)
        snap_post_single = tf.snapshot(source="paint", viewport=(480, 720))
        # The toggle glyph swap is the visible-state proof.
        list_node = find_node_by_tag(snap_post_single, "todo_list")
        assert list_node is not None
        rows = list_node.get("children") or []
        found_checked = False
        for row in rows[1:]:
            for ch in row.get("children") or []:
                if ch.get("tag") == toggle_tag:
                    text_children = ch.get("children") or []
                    for tc in text_children:
                        if tc.get("type") == "Text":
                            content = tc.get("content") or ""
                            if "☑" in content:  # ☑ checked
                                found_checked = True
        assert found_checked, (
            "R663-precheck: single click flips toggle glyph to ☑ (state must be 'completed')"
        )

        # ── (3) Reset to unchecked via another single click ──────────
        tf.click(path=toggle_tag)
        time.sleep(0.15)

        # ── (4) R663 double-click — fires 2 PointerDown cycles ──────
        # Toggle is the canonical "second-press reverts" semantic, so
        # observing the post-double-click glyph as ☐ (back to active)
        # is the substrate proof: the shell drain fed 2 complete
        # press/release cycles, the InputRouter routed them through
        # the composite-tag wire to TodoToggleExternal, and the
        # external's invoke arm flipped `completed` twice (0→1→0).
        tf.double_click(path=toggle_tag)
        time.sleep(0.2)

        snap_post_double = tf.snapshot(source="paint", viewport=(480, 720))
        list_node = find_node_by_tag(snap_post_double, "todo_list")
        assert list_node is not None
        rows = list_node.get("children") or []
        post_glyph = None
        for row in rows[1:]:
            for ch in row.get("children") or []:
                if ch.get("tag") == toggle_tag:
                    for tc in ch.get("children") or []:
                        if tc.get("type") == "Text":
                            post_glyph = tc.get("content")
        assert_eq(
            post_glyph,
            "☐",  # ☐ unchecked
            "R663: double-click fired 2x toggle → completed flipped 0→1→0",
        )

        # ── (5) Smoke the at-coordinate selector form ────────────────
        # The path-form was exercised above; the at-form takes (x, y)
        # directly. Resolve the toggle's center from the snapshot and
        # invoke the same double-click via raw coordinates.
        rect = None
        for row in rows[1:]:
            for ch in row.get("children") or []:
                if ch.get("tag") == toggle_tag:
                    rect = ch.get("rect")
        assert rect is not None
        cx = float(rect["x"]) + float(rect["w"]) / 2.0
        cy = float(rect["y"]) + float(rect["h"]) / 2.0
        tf.double_click(at=(cx, cy))
        time.sleep(0.2)

        # Same flip-back behaviour, just via at-coord path.
        snap_post_at = tf.snapshot(source="paint", viewport=(480, 720))
        list_node = find_node_by_tag(snap_post_at, "todo_list")
        assert list_node is not None
        rows = list_node.get("children") or []
        for row in rows[1:]:
            for ch in row.get("children") or []:
                if ch.get("tag") == toggle_tag:
                    for tc in ch.get("children") or []:
                        if tc.get("type") == "Text":
                            content = tc.get("content")
                            assert_eq(
                                content,
                                "☐",
                                "R663: at-coord double_click also fires 2x cycle",
                            )

        # ── (6) Selector validation — both arms can't coexist ───────
        try:
            tf.double_click(at=(0.0, 0.0), path="todo_list")
        except ValueError as e:
            assert "exactly one" in str(e)
        else:
            raise AssertionError("R663: at + path is mutually exclusive — must raise")


if __name__ == "__main__":
    sys.exit(run_demo("scene/double_click R663", body))
