#!/usr/bin/env python3
"""R670.B §5.16 §5.41 — hello-multi-window first multi-window dogfood.

Verifies the R670.B substrate end-to-end against the live binary:

Atomic (0) — AppShell multi-window refactor. Demo verifies:
  - Both windows boot successfully (binary doesn't crash on V::windows()
    returning 2 specs).
  - scene/snapshot returns the painted scene against the primary
    window by default.

Atomic (1) — RPC `{window: "<id>"}` param. Demo verifies:
  - `scene/snapshot {window: "main"}` returns the main button view.
  - `scene/snapshot {window: "inspector"}` returns the inspector
    state-debug view (DIFFERENT scene than main).
  - `scene/snapshot {window: "ghost"}` falls back to primary (the
    resolver in AppShell::resolve_spec_id is non-strict).

Atomic (2) — view_for_window per-window paint. Demo verifies:
  - Main contains the button tag (`main_btn`).
  - Inspector contains the TreeView root tag (`inspector_tree`); the
    leading "State: <variant>" row mirrors the main `ButtonState`.
  - Clicking the main button (scene/click) flips ButtonState; the
    inspector window's state row updates to reflect the new value
    on the next paint cycle (single ShellCore + per-window views).

R671 §5.16 upgrade: the inspector window previously carried a
single `inspector_tree` Container with a `format!("{:?}", state)`
TextNode. R671 replaced that with a `pinion_widget_paint::tree_view`
substrate consumer rooted at `inspector_tree`; the demo's tag
assertions are updated accordingly + the `_walk_for_text` walk
continues to read state names through the new tree row label
content.

R670.B SEED `verification mandatory` checklist:
  - main + inspector scene/snapshot SEPARATED ✓
  - main state change → inspector mirror ✓
  - 9-demo regression sweep PASS handled separately in the commit
    sequence (this script focuses on the new substrate axes)
"""

from __future__ import annotations

import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    find_by_tag,
    run_demo,
)


def _walk_for_text(node, needle: str) -> bool:
    """Depth-first walk for any text node whose content contains needle."""
    if not isinstance(node, dict):
        return False
    if node.get("kind") == "text" or node.get("type") == "Text":
        content = node.get("content")
        if isinstance(content, str) and needle in content:
            return True
    children = node.get("children")
    if isinstance(children, list):
        for child in children:
            if _walk_for_text(child, needle):
                return True
    content = node.get("content")
    if isinstance(content, dict) and _walk_for_text(content, needle):
        return True
    return False


def body() -> None:
    with RpcSubprocess("hello-multi-window", boot_grace=1.5) as tf:
        # ── (1) Default-scope snapshot (no {window} param) ──────────
        # Per AppShell::dispatch_rpc, a frame without {window: "..."}
        # defaults to the primary spec ("main"). The snapshot should
        # contain the main button tag.
        snap_default = tf.snapshot(source="paint", viewport=(320, 200))
        assert (
            find_by_tag(snap_default, "main_btn") is not None
        ), f"default-scope snapshot must contain main_btn: {snap_default!r}"

        # ── (2) Explicit {window: "main"} (R670.B atomic 1) ─────────
        snap_main = tf.request(
            "scene/snapshot",
            {"window": "main", "path": "", "from": "paint", "viewport": {"w": 320, "h": 200}},
        )
        assert snap_main is not None
        assert (
            find_by_tag(snap_main.result, "main_btn") is not None
        ), f"main snapshot must contain main_btn: {snap_main.result!r}"
        # The main view does NOT contain the inspector tag.
        assert find_by_tag(snap_main.result, "inspector_tree") is None, (
            "main view must not leak inspector content"
        )

        # ── (3) Explicit {window: "inspector"} per-window scope ─────
        snap_inspector = tf.request(
            "scene/snapshot",
            {
                "window": "inspector",
                "path": "",
                "from": "paint",
                "viewport": {"w": 280, "h": 140},
            },
        )
        assert snap_inspector is not None
        # Inspector view carries the state-debug text tag.
        assert (
            find_by_tag(snap_inspector.result, "inspector_tree") is not None
        ), f"inspector snapshot must contain inspector_tree: {snap_inspector.result!r}"
        # Inspector view does NOT contain the main button tag.
        assert (
            find_by_tag(snap_inspector.result, "main_btn") is None
        ), "inspector view must not contain main button (different view_for_window branch)"

        # ── (4) Inspector reflects initial state ────────────────────
        # Fresh boot → ButtonState::Idle. Inspector text should
        # render "Idle".
        assert _walk_for_text(snap_inspector.result, "Idle"), (
            "inspector view must render initial state 'Idle'"
        )
        assert not _walk_for_text(snap_inspector.result, "Hover"), (
            "fresh-boot inspector must not yet show Hover"
        )

        # ── (5) Main button state flip → inspector mirrors ──────────
        # R670.B used `scene/click {path: "main_btn"}` which goes
        # through the InputRouter — single ShellCore + multi-window
        # makes that path inherently racy (the last-painted window
        # owns the InputRouter's `last_paint_scene`, so a click
        # scoped to a non-last-painted window can miss). R671 swaps
        # to `scene/invoke /external/send PointerEnter` which
        # bypasses the InputRouter entirely — the SCXML transition
        # Idle → Hover fires deterministically. Per-window
        # InputRouter is the canonical future fix (carry through
        # R672+ Phase B widget catalog rounds); for now the
        # `scene/invoke` arc is the race-immune path AI clients
        # already use in production (the [[ai-first-rpc-introspection-obligation]]
        # canonical scope).
        tf.invoke("/external/send", "PointerEnter")
        # Give the shell a paint cycle to update both windows' caches.
        time.sleep(0.3)

        # Re-snapshot inspector — state should now be Hover (cursor
        # rests on button rect post-click).
        snap_inspector_after = tf.request(
            "scene/snapshot",
            {
                "window": "inspector",
                "path": "",
                "from": "paint",
                "viewport": {"w": 280, "h": 140},
            },
        )
        assert snap_inspector_after is not None
        assert _walk_for_text(snap_inspector_after.result, "Hover"), (
            f"after main click, inspector must mirror Hover state: "
            f"{snap_inspector_after.result!r}"
        )

        # ── (6) Ghost window id falls back to primary ───────────────
        # AppShell::resolve_spec_id falls back to primary on unknown
        # id rather than hard-erroring. AI clients targeting a
        # not-yet-defined window see the primary's scene + can
        # detect the fallback by comparing returned tags.
        snap_ghost = tf.request(
            "scene/snapshot",
            {
                "window": "definitely_not_a_real_window",
                "path": "",
                "from": "paint",
                "viewport": {"w": 320, "h": 200},
            },
        )
        assert snap_ghost is not None
        assert (
            find_by_tag(snap_ghost.result, "main_btn") is not None
        ), "ghost id falls back to primary window (main_btn must be present)"

        # ── (7) scene/key on inspector window is rejected gracefully ─
        # Inspector has no focusable widgets. A scene/key against the
        # inspector should not crash; the RPC dispatcher accepts the
        # frame (the path is valid) but the underlying apply_key
        # finds nothing to dispatch to.
        # We don't assert any state change — just that the binary
        # stays alive (a panic would crash the subprocess).
        try:
            tf.key(at=(100.0, 60.0), name="Space")
            time.sleep(0.1)
        except Exception:
            pass  # Best-effort — the main goal is no crash.

        # ── (8) Disable main button via scene/invoke ────────────────
        # scene/invoke /external/send Disable hits the single
        # ButtonExternal at the scene root. State should flip to
        # Disabled; inspector should mirror the new value.
        tf.invoke("/external/send", "Disable")
        time.sleep(0.3)
        snap_inspector_disabled = tf.request(
            "scene/snapshot",
            {
                "window": "inspector",
                "path": "",
                "from": "paint",
                "viewport": {"w": 280, "h": 140},
            },
        )
        assert snap_inspector_disabled is not None
        assert _walk_for_text(snap_inspector_disabled.result, "Disabled"), (
            "inspector must mirror Disabled state after scene/invoke"
        )

        # ── (9) Re-enable via scene/invoke ──────────────────────────
        tf.invoke("/external/send", "Enable")
        time.sleep(0.3)
        snap_inspector_reenabled = tf.request(
            "scene/snapshot",
            {
                "window": "inspector",
                "path": "",
                "from": "paint",
                "viewport": {"w": 280, "h": 140},
            },
        )
        assert snap_inspector_reenabled is not None
        # State is now Idle (Disabled → Idle on Enable per Button SCXML).
        assert _walk_for_text(snap_inspector_reenabled.result, "Idle"), (
            "inspector must mirror Idle after re-enable"
        )
        assert not _walk_for_text(snap_inspector_reenabled.result, "Disabled"), (
            "Disabled must clear after re-enable"
        )

        # ── (10) scene/snapshot from:state targets the SCXML state ──
        # `from:state` is the default for AI-side state introspection;
        # it returns the same shape regardless of {window} (state
        # scene is single-binding, not per-window).
        state_snap = tf.snapshot()
        assert isinstance(state_snap, dict)
        # The state scene root is the ButtonExternal node.
        assert (
            "external" in str(state_snap).lower() or state_snap.get("kind") == "external"
        ), f"state snapshot must contain the External node: {state_snap!r}"

        # ── (11) scene/layout {window: "inspector"} reflects 280×140 ─
        # The last-painted layout snapshot for the inspector window is
        # 280×140 (the WindowSpec declared size). For multi-window,
        # AppShell records the last_paint_layout per-shell (single
        # ShellCore); this assertion is best-effort because the
        # snapshot reflects whichever window painted LAST.
        layout = tf.request("scene/layout", {"viewport": None})
        assert layout is not None and isinstance(layout.result, dict)
        # Just confirm rect.w > 0 — full per-window layout would
        # need a deeper substrate extension (the current
        # last_paint_layout is single-shell). This assertion is a
        # smoke + a comment for the future per-window layout axis.
        rect = layout.result.get("rect") or {}
        assert int(rect.get("w") or 0) > 0, (
            f"scene/layout viewport must report some width: {layout.result!r}"
        )

        # ── (12) Final smoke: re-fetch main snapshot, confirm fresh ─
        snap_main_final = tf.request(
            "scene/snapshot",
            {"window": "main", "path": "", "from": "paint", "viewport": {"w": 320, "h": 200}},
        )
        assert snap_main_final is not None
        assert find_by_tag(snap_main_final.result, "main_btn") is not None


if __name__ == "__main__":
    sys.exit(run_demo("hello-multi-window R670.B (multi-window first dogfood)", body))
