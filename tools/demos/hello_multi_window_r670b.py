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
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_snap,
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
        snap_main = tf.snapshot(source="paint", viewport=(320, 200), window="main")
        assert (
            find_by_tag(snap_main, "main_btn") is not None
        ), f"main snapshot must contain main_btn: {snap_main!r}"
        # The main view does NOT contain the inspector tag.
        assert find_by_tag(snap_main, "inspector_tree") is None, (
            "main view must not leak inspector content"
        )

        # ── (3) Explicit {window: "inspector"} per-window scope ─────
        snap_inspector = tf.snapshot(
            source="paint", viewport=(280, 140), window="inspector"
        )
        # Inspector view carries the state-debug text tag.
        assert (
            find_by_tag(snap_inspector, "inspector_tree") is not None
        ), f"inspector snapshot must contain inspector_tree: {snap_inspector!r}"
        # Inspector view does NOT contain the main button tag.
        assert (
            find_by_tag(snap_inspector, "main_btn") is None
        ), "inspector view must not contain main button (different view_for_window branch)"

        # ── (4) Inspector reflects initial state ────────────────────
        # Fresh boot → ButtonState::Idle. Inspector text should
        # render "Idle".
        assert _walk_for_text(snap_inspector, "Idle"), (
            "inspector view must render initial state 'Idle'"
        )
        assert not _walk_for_text(snap_inspector, "Hover"), (
            "fresh-boot inspector must not yet show Hover"
        )

        # ── (5) Click main button → state flips → inspector mirrors ─
        # R672 §5.35 §5.41 — per-window InputRouter closes the
        # multi-window single-router race that made this assertion
        # flaky in R670.B → R671 (the inspector window's paint cycle
        # would overwrite the binding-wide `last_paint_scene` and the
        # `scene/click {window: "main"}` hit-test would miss). With
        # the per-window routers `dispatch_rpc_for_window` resolves
        # the addressed window's last paint scene before drain →
        # hit-test against main's tree → main_btn hit
        # deterministically.
        tf.click(path="main_btn")
        # Re-snapshot inspector — state should now be Hover (cursor
        # rests on button rect post-click).
        wait_snap(
            tf,
            lambda s: _walk_for_text(s, "Hover"),
            viewport=(280, 140),
            window="inspector",
            desc="after main click, inspector must mirror Hover state",
        )

        # ── (6) Ghost window id is rejected (R889) ───────────────────
        # Pre-R889 `AppShell::resolve_spec_id` silently aliased an
        # unknown id onto the primary — which also let bogus-window
        # WRITES hit the primary (set_fps 0 froze its game loop).
        # R889 deletes the alias: the dispatcher rejects the whole
        # request with `-32602 unknown_window` via the window-known
        # registry (`CoreShell::is_window_known`).
        try:
            tf.snapshot(
                source="paint",
                viewport=(320, 200),
                window="definitely_not_a_real_window",
            )
            raise AssertionError("ghost window id must be rejected, not aliased")
        except RpcError as err:
            assert (err.code, err.message) == (-32602, "unknown_window"), (
                f"ghost id rejection shape: {err.code} {err.message}"
            )
            assert err.data == "definitely_not_a_real_window", (
                "error data names the supplied id"
            )

        # ── (7) scene/key on inspector window is rejected gracefully ─
        # Inspector has no focusable widgets. A scene/key against the
        # inspector should not crash; the RPC dispatcher accepts the
        # frame (the path is valid) but the underlying apply_key
        # finds nothing to dispatch to.
        # We don't assert any state change — just that the binary
        # stays alive (a panic would crash the subprocess).
        try:
            tf.key(at=(100.0, 60.0), name="Space")
        except Exception:
            pass  # Best-effort — the main goal is no crash.

        # ── (8) Disable main button via scene/invoke ────────────────
        # scene/invoke /external/send Disable hits the single
        # ButtonExternal at the scene root. State should flip to
        # Disabled; inspector should mirror the new value.
        tf.invoke("/external/send", "Disable")
        wait_snap(
            tf,
            lambda s: _walk_for_text(s, "Disabled"),
            viewport=(280, 140),
            window="inspector",
            desc="inspector must mirror Disabled state after scene/invoke",
        )

        # ── (9) Re-enable via scene/invoke ──────────────────────────
        tf.invoke("/external/send", "Enable")
        # State is now Idle (Disabled → Idle on Enable per Button SCXML).
        snap_inspector_reenabled = wait_snap(
            tf,
            lambda s: _walk_for_text(s, "Idle"),
            viewport=(280, 140),
            window="inspector",
            desc="inspector must mirror Idle after re-enable",
        )
        assert not _walk_for_text(snap_inspector_reenabled, "Disabled"), (
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
        snap_main_final = tf.snapshot(
            source="paint", viewport=(320, 200), window="main"
        )
        assert find_by_tag(snap_main_final, "main_btn") is not None


if __name__ == "__main__":
    sys.exit(run_demo("hello-multi-window R670.B (multi-window first dogfood)", body))
