#!/usr/bin/env python3
"""R1094 §5.16 §5.41 §5.51 §2 #7 — the live cursor-following tear-off.

The consumer of the R1093 drag-cursor seam: a drag whose move escapes every
dock zone tears the panel into a live floating follower whose declared outer
position tracks the cursor. The widget (`DockPanelExternal`) reports a
SOURCE-window-logical cursor on every escaped move via the non-toggling
`tear_off_follow` intent; the binding reducer ensures the panel's floating
`WindowSpec` exists and writes its position = the main window's declared outer
origin + the cursor (the gap(b) desktop conversion). Architecture-A: the
declared position IS the SSOT the shell reconciles to the OS window — so the
follow is observable as scene-as-data over `scene/windows`, with NO dependence
on the floating winit window being snapshot-addressable (the headless limit
r683 hits). The live OS grab-move is the R1092.1-verified path (mutter honours
`set_outer_position` under a button grab) and is exercised on real hardware.

Drives `hello-dock-panels`. Each escape drives `scene/drag` from a panel
header to a point OUTSIDE the window (x past the 880px width) — over no tagged
region, so `resolve_drop_point` is None and the drag escapes the dock. The
per-move follow tracking + the restore/redock arms are unit-tested at the
widget + binding layer (an atomic `scene/drag` cannot observe between waypoints
nor drive escape-then-return in one gesture); this demo pins the headless-
observable end states.

Section roadmap (>=30 assertions across A-F):

  (A) Boot — `scene/windows` declares only "main" (WM-placed, null position);
      every panel reports detached=false + drag_cursor=null; the detached slot
      is in the introspect schema (discoverable).
  (B) Escape tears off at the cursor — drag the inspector header out of the
      window; `scene/windows` grows a `torn-inspector` whose declared position
      equals the escape cursor (main at the desktop origin), the inspector
      reports detached=true + drag_cursor=the cursor, and the main dock paints
      the placeholder (the inspector tree left the main window).
  (C) gap(b) desktop conversion — move "main" to a known origin via
      `scene/window_move`; a property escape then opens its floater at
      origin + cursor (the window-logical→desktop conversion, end to end).
  (D) Per-panel independence — inspector + property float at distinct declared
      positions; the viewport stays docked (detached=false, no torn-viewport).
  (E) Read-only — `scene/intervene` on detached is rejected; the value is
      untouched (a framework-owned, router-driven slot).
  (F) Drop-empty floats — the torn windows persist after the gesture ends
      (an escape-drop keeps the panel floating; no snap-back).
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import RpcError, RpcSubprocess, run_demo, wait_until  # noqa: E402

_MAIN_W = 880
_MAIN_H = 600

_INSPECTOR_PANEL_TAG = "inspector"
_PROPERTY_PANEL_TAG = "property"
_VIEWPORT_PANEL_TAG = "viewport"
_INSPECTOR_HEADER_TAG = "inspector#header"
_PROPERTY_HEADER_TAG = "property#header"
_INSPECTOR_TREE_TAG = "inspector_tree"

_FLOATING_PREFIX = "torn-"


def _floating_id(panel_id: str) -> str:
    return f"{_FLOATING_PREFIX}{panel_id}"


# ─── scene/windows helpers (the declared-topology SSOT) ──────────────


def _windows(tf: RpcSubprocess) -> list[dict]:
    resp = tf.request("scene/windows", {})
    assert resp is not None, "scene/windows returned no response"
    result = resp.result
    assert isinstance(result, dict), f"scene/windows result must be an object; got {result!r}"
    windows = result.get("windows")
    assert isinstance(windows, list), f"scene/windows.windows must be a list; got {windows!r}"
    return windows


def _window_by_id(tf: RpcSubprocess, window_id: str) -> Optional[dict]:
    for w in _windows(tf):
        if w.get("id") == window_id:
            return w
    return None


def _position_of(tf: RpcSubprocess, window_id: str) -> Any:
    w = _window_by_id(tf, window_id)
    assert w is not None, f"scene/windows must list {window_id!r}"
    return w.get("position")


def _wait_listed(tf: RpcSubprocess, window_id: str) -> dict:
    """Window creation is async (reconcile Effect commits after the follow
    reducer drains). Poll `scene/windows` until the id appears (zero-flake)."""
    return wait_until(
        lambda: _window_by_id(tf, window_id),
        desc=f"scene/windows lists {window_id}",
    )


# ─── panel-external introspection ────────────────────────────────────


def _detached(tf: RpcSubprocess, panel_tag: str) -> Any:
    return tf.query(f"/{panel_tag}/external/detached")


def _drag_cursor(tf: RpcSubprocess, panel_tag: str) -> Any:
    return tf.query(f"/{panel_tag}/external/drag_cursor")


def _scene_contains_tag(scene: Any, target: str) -> bool:
    if not isinstance(scene, dict):
        return False
    if scene.get("tag") == target:
        return True
    for child in scene.get("children") or []:
        if _scene_contains_tag(child, target):
            return True
    content = scene.get("content")
    return isinstance(content, dict) and _scene_contains_tag(content, target)


def _main_snapshot(tf: RpcSubprocess) -> Any:
    snap = tf.snapshot(source="paint", viewport=(_MAIN_W, _MAIN_H), window="main")
    assert snap is not None, "main snapshot must answer"
    return snap


def _drag_out(tf: RpcSubprocess, header_tag: str, to_at: tuple[float, float]) -> None:
    """Drive a tear-off escape: press the panel header, drag to a point
    OUTSIDE the window (over no tagged region → `resolve_drop_point` None →
    the drag escapes the dock), release there. The interpolated `steps`
    march from the header through the dock and out, so the last frames + the
    release fire the live `tear_off_follow`."""
    tf.request(
        "scene/drag",
        {
            "window": "main",
            "from_path": header_tag,
            "to": {"x": float(to_at[0]), "y": float(to_at[1])},
            "steps": 8,
        },
    )


def _assert_xy(label: str, value: Any, expected: tuple[int, int]) -> None:
    assert isinstance(value, list) and len(value) == 2, (
        f"{label} must be an [x, y] pair; got {value!r}"
    )
    assert int(value[0]) == expected[0] and int(value[1]) == expected[1], (
        f"{label} must equal {expected}; got {value!r}"
    )


# ─── demo body ───────────────────────────────────────────────────────


def body() -> None:
    # Escape targets — well past the 880px width, distinct per panel.
    insp_to = (1040.0, 200.0)
    prop_to = (1060.0, 360.0)
    main_origin = (300, 220)

    with RpcSubprocess("hello-dock-panels", boot_grace=2.0) as tf:
        # ── (A) boot — single WM-placed "main", panels docked ─────
        boot = _windows(tf)
        assert len(boot) == 1, f"boot must declare exactly one window; got {boot!r}"
        assert boot[0].get("id") == "main", f"boot window id must be 'main'; got {boot[0]!r}"
        assert boot[0].get("position") is None, (
            f"main is WM-placed at boot — position must be null; got {boot[0]!r}"
        )
        for panel in (_INSPECTOR_PANEL_TAG, _PROPERTY_PANEL_TAG, _VIEWPORT_PANEL_TAG):
            assert _detached(tf, panel) is False, (
                f"{panel} must report detached=false before any drag"
            )
            assert _drag_cursor(tf, panel) is None, (
                f"{panel} drag_cursor must be null before any drag"
            )
        # The main dock paints the inspector tree (panel is docked).
        assert _scene_contains_tag(_main_snapshot(tf), _INSPECTOR_TREE_TAG), (
            "boot main dock must contain the docked inspector tree"
        )

        # ── (B) escape tears off AT the cursor ────────────────────
        _drag_out(tf, _INSPECTOR_HEADER_TAG, insp_to)
        torn_inspector = _floating_id(_INSPECTOR_PANEL_TAG)
        entry = _wait_listed(tf, torn_inspector)
        assert len(_windows(tf)) == 2, "escape must add exactly one declared window"
        # Main at the desktop origin (null → (0,0)) ⇒ declared == cursor.
        _assert_xy(
            f"{torn_inspector} declared position",
            entry.get("position"),
            (int(insp_to[0]), int(insp_to[1])),
        )
        assert "floating" in (entry.get("title") or ""), (
            f"floating window title must mark it floating; got {entry!r}"
        )
        # The source panel reports the tear-off + the live cursor it went to.
        assert _detached(tf, _INSPECTOR_PANEL_TAG) is True, (
            "inspector must report detached=true after escaping the dock"
        )
        _assert_xy(
            "inspector drag_cursor",
            _drag_cursor(tf, _INSPECTOR_PANEL_TAG),
            (int(insp_to[0]), int(insp_to[1])),
        )
        # The main dock paints the placeholder now — the inspector tree left.
        assert not _scene_contains_tag(_main_snapshot(tf), _INSPECTOR_TREE_TAG), (
            "a torn-off inspector must paint the placeholder (tree gone from main)"
        )
        # The tear-off touched only the new spec — main stays WM-placed.
        assert _position_of(tf, "main") is None, "main position must stay null after a tear-off"

        # ── (C) gap(b) desktop conversion ─────────────────────────
        moved = tf.request(
            "scene/window_move",
            {"window_id": "main", "x": main_origin[0], "y": main_origin[1]},
        )
        assert moved is not None, "scene/window_move must answer"
        # The declared main origin took (the SSOT the follow reads).
        assert _position_of(tf, "main") == list(main_origin), (
            f"main declared position must be {list(main_origin)} after window_move; "
            f"got {_position_of(tf, 'main')!r}"
        )
        _drag_out(tf, _PROPERTY_HEADER_TAG, prop_to)
        torn_property = _floating_id(_PROPERTY_PANEL_TAG)
        prop_entry = _wait_listed(tf, torn_property)
        # Desktop = main origin + window-logical cursor.
        expected_prop = (main_origin[0] + int(prop_to[0]), main_origin[1] + int(prop_to[1]))
        _assert_xy(f"{torn_property} declared position", prop_entry.get("position"), expected_prop)
        assert _detached(tf, _PROPERTY_PANEL_TAG) is True, "property detached after escape"

        # ── (D) per-panel independence ────────────────────────────
        all_windows = _windows(tf)
        assert len(all_windows) == 3, f"main + 2 floaters expected; got {all_windows!r}"
        ids = [w.get("id") for w in all_windows]
        assert len(set(ids)) == len(ids), f"declared window ids must be unique; got {ids!r}"
        # Inspector floater unchanged by the property gesture.
        _assert_xy(
            f"{torn_inspector} unchanged",
            _position_of(tf, torn_inspector),
            (int(insp_to[0]), int(insp_to[1])),
        )
        assert tuple(_position_of(tf, torn_inspector)) != tuple(_position_of(tf, torn_property)), (
            "the two floaters must hold distinct declared positions"
        )
        # The viewport never escaped — still docked, no floater, not detached.
        assert _window_by_id(tf, _floating_id(_VIEWPORT_PANEL_TAG)) is None, (
            "the un-dragged viewport must have no floating window"
        )
        assert _detached(tf, _VIEWPORT_PANEL_TAG) is False, "the un-dragged viewport is not detached"

        # ── (E) detached is read-only (router-driven) ─────────────
        try:
            tf.request(
                "scene/intervene",
                {"path": f"/{_INSPECTOR_PANEL_TAG}/external/detached", "value": False},
            )
            raised = False
        except RpcError:
            raised = True
        assert raised, "intervene on detached must be rejected (read-only slot)"
        assert _detached(tf, _INSPECTOR_PANEL_TAG) is True, (
            "the rejected intervene must leave detached untouched"
        )

        # ── (F) drop-empty floats (persistence) ───────────────────
        # The gesture has ended; an escape-drop keeps the panel floating
        # (no snap-back). Both floaters are still declared.
        assert _window_by_id(tf, torn_inspector) is not None, "inspector stays floating post-drop"
        assert _window_by_id(tf, torn_property) is not None, "property stays floating post-drop"


if __name__ == "__main__":
    sys.exit(run_demo(
        "R1094 §5.16 §5.41 §5.51 §2 #7 PR-31 — live cursor-following tear-off (declared follow)",
        body,
    ))
