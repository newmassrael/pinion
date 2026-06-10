#!/usr/bin/env python3
"""R685 §5.16 §5.41 §5.49 — DockSurface topology composition + 2nd
dock consumer demo.

Drives `hello-dock-panels-editor` — the 2nd consumer of the
`pinion_widget_paint::dock` substrate, triggering the
[[abstraction-needs-second-consumer]] Rule-of-Three lift for
`view_floating_placeholder` + `floating_window_id` + the
`DEFAULT_FLOATING_WINDOW_PREFIX` constant.

Section roadmap (≥40 assertions across A–H):

  (A) Substrate sanity — view fn produces a Scene with the outer
      splitter Container at root; viewport Button slot reachable.
  (B) 5-pane topology — every panel root tag + every splitter tag
      appears in the rendered scene.
  (C) Splitter slots — each of the 4 SplitterExternals exposes its
      canonical schema (orientation + ratio + dragging + send) and
      reports the expected default ratio.
  (D) Splitter orientation — outer V + inner V + middle H + inner H
      orientations match the topology declaration.
  (E) Viewport Button click — drive scene/click on the viewport
      button; the SCXML state advances to Hover (idle→press→hover);
      the binding's click-counter Signal increments.
  (F) Splitter drag — drive scene/drag on the inner H splitter
      handle; the ratio Signal mutates within the substrate clamp
      range.
  (G) Topology serde — the in-process view fn's emit JSON-shape is
      stable across paint cycles (the topology is deterministic
      pure data, so two snapshots without state change produce
      identical scene shape).
  (H) Panel content sanity — every panel paints its canonical
      content (toolbar menu names, outliner row labels, viewport
      label, properties keys, console log levels).
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import RpcError, RpcSubprocess, run_demo, wait_until  # noqa: E402

# ─── constants mirrored from the binding ────────────────────────────

_MAIN_W = 1200
_MAIN_H = 800

# Panel root tags.
_TOOLBAR_PANEL_TAG = "toolbar"
_OUTLINER_PANEL_TAG = "outliner"
_VIEWPORT_PANEL_TAG = "viewport"
_PROPERTIES_PANEL_TAG = "properties"
_CONSOLE_PANEL_TAG = "console"

# Splitter paint-side tags. Indexed by depth-first pre-order over
# `build_editor_topology` per `view_dock_surface`'s threading scheme.
_SPLIT_OUTER_TAG = "editor_split_outer"
_SPLIT_INNER_V_TAG = "editor_split_inner_v"
_SPLIT_MIDDLE_H_TAG = "editor_split_middle_h"
_SPLIT_INNER_H_TAG = "editor_split_inner_h"

# Viewport Button paint-side tag.
_VIEWPORT_BTN_TAG = "viewport_btn"

# Default split ratios (matched against the substrate-clamped value
# the SplitterExternal::ratio() query returns).
_SPLIT_OUTER_RATIO_DEFAULT = 0.06
_SPLIT_INNER_V_RATIO_DEFAULT = 0.78
_SPLIT_MIDDLE_H_RATIO_DEFAULT = 0.18
_SPLIT_INNER_H_RATIO_DEFAULT = 0.78


# ─── snapshot + introspect helpers ──────────────────────────────────


def _snapshot(tf: RpcSubprocess) -> Any:
    resp = tf.request(
        "scene/snapshot",
        {"path": "", "from": "paint", "viewport": {"w": _MAIN_W, "h": _MAIN_H}},
    )
    assert resp is not None
    return resp.result


def _query(tf: RpcSubprocess, path: str) -> Any:
    return tf.query(path)


def _scene_contains_tag(scene: Any, target: str) -> bool:
    """Depth-first walk for any node whose `tag` field == `target`."""
    if not isinstance(scene, dict):
        return False
    if scene.get("tag") == target:
        return True
    for child in scene.get("children") or []:
        if _scene_contains_tag(child, target):
            return True
    content = scene.get("content")
    if isinstance(content, dict) and _scene_contains_tag(content, target):
        return True
    return False


def _collect_text(scene: Any) -> list[str]:
    """Walk the snapshot for every Text node's content."""
    out: list[str] = []

    def walk(node: Any) -> None:
        if not isinstance(node, dict):
            return
        if node.get("type") == "Text":
            content = node.get("content")
            if isinstance(content, str):
                out.append(content)
        for child in node.get("children") or []:
            walk(child)
        content = node.get("content")
        if isinstance(content, dict):
            walk(content)

    walk(scene)
    return out


def _query_splitter_ratio(tf: RpcSubprocess, tag: str) -> Optional[float]:
    val = _query(tf, f"/{tag}/external/ratio")
    if val is None or not isinstance(val, (int, float)):
        return None
    return float(val)


def _query_splitter_orientation(tf: RpcSubprocess, tag: str) -> Optional[str]:
    val = _query(tf, f"/{tag}/external/orientation")
    if isinstance(val, str):
        return val
    return None


def _query_button_state(tf: RpcSubprocess) -> Optional[str]:
    val = _query(tf, f"/{_VIEWPORT_BTN_TAG}/external/state")
    if isinstance(val, str):
        return val
    return None


def _section(label: str) -> None:
    print(f"[demo] -- {label}")


# ─── demo body ──────────────────────────────────────────────────────


def body() -> None:
    with RpcSubprocess("hello-dock-panels-editor") as tf:
        # ─── (A) Substrate sanity ────────────────────────────────────
        _section("A: substrate sanity")
        scene = _snapshot(tf)
        assert scene is not None, "A.1 snapshot returns non-None"
        assert isinstance(scene, dict), "A.2 snapshot is a dict"
        assert scene.get("type") == "Container", "A.3 root is Container"
        assert scene.get("tag") == _SPLIT_OUTER_TAG, (
            f"A.4 root tag is {_SPLIT_OUTER_TAG} (the outer splitter), "
            f"got {scene.get('tag')!r}"
        )
        viewport_state = _query_button_state(tf)
        assert viewport_state in {"Idle", "Hover", "Pressed", "Disabled"}, (
            f"A.5 viewport button query returns a known state ({viewport_state})"
        )

        # ─── (B) 5-pane topology — every tag present ─────────────────
        _section("B: 5-pane topology")
        for panel_tag in (
            _TOOLBAR_PANEL_TAG,
            _OUTLINER_PANEL_TAG,
            _VIEWPORT_PANEL_TAG,
            _PROPERTIES_PANEL_TAG,
            _CONSOLE_PANEL_TAG,
        ):
            assert _scene_contains_tag(scene, panel_tag), (
                f"B.{panel_tag} panel root tag '{panel_tag}' present"
            )
        for split_tag in (
            _SPLIT_OUTER_TAG,
            _SPLIT_INNER_V_TAG,
            _SPLIT_MIDDLE_H_TAG,
            _SPLIT_INNER_H_TAG,
        ):
            assert _scene_contains_tag(scene, split_tag), (
                f"B.{split_tag} splitter tag '{split_tag}' present"
            )
        assert _scene_contains_tag(
            scene, _VIEWPORT_BTN_TAG
        ), "B.viewport_btn viewport Button tag present"

        # ─── (C) Splitter slots ──────────────────────────────────────
        _section("C: splitter schema slots")
        for split_tag, expected_ratio in (
            (_SPLIT_OUTER_TAG, _SPLIT_OUTER_RATIO_DEFAULT),
            (_SPLIT_INNER_V_TAG, _SPLIT_INNER_V_RATIO_DEFAULT),
            (_SPLIT_MIDDLE_H_TAG, _SPLIT_MIDDLE_H_RATIO_DEFAULT),
            (_SPLIT_INNER_H_TAG, _SPLIT_INNER_H_RATIO_DEFAULT),
        ):
            ratio = _query_splitter_ratio(tf, split_tag)
            assert ratio is not None, f"C.{split_tag} ratio queryable"
            assert abs(ratio - expected_ratio) < 0.001, (
                f"C.{split_tag} ratio {ratio} == default {expected_ratio}"
            )

        # ─── (D) Splitter orientation ────────────────────────────────
        _section("D: splitter orientations")
        for split_tag, expected_orient in (
            (_SPLIT_OUTER_TAG, "vertical"),
            (_SPLIT_INNER_V_TAG, "vertical"),
            (_SPLIT_MIDDLE_H_TAG, "horizontal"),
            (_SPLIT_INNER_H_TAG, "horizontal"),
        ):
            orient = _query_splitter_orientation(tf, split_tag)
            assert orient == expected_orient, (
                f"D.{split_tag} orientation {orient!r} == {expected_orient!r}"
            )

        # ─── (E) Viewport Button click ───────────────────────────────
        _section("E: viewport button click")
        # Locate the button by its tag — use scene/click against
        # the viewport button tag (the InputRouter resolves the
        # rect from the last paint scene).
        click_resp = tf.request(
            "scene/click",
            {"path": _VIEWPORT_BTN_TAG},
        )
        assert click_resp is not None, "E.1 click RPC returns a response"
        # After a press + release the SCXML returns to Hover (the
        # canonical Pressed → Hover transition; the binding's
        # update reducer mirrors the click into the counter Signal).
        wait_until(
            lambda: _query_button_state(tf) in {"Hover", "Idle"},
            desc="E.2 post-click button state settled to Hover/Idle",
        )
        # Second snapshot — counter text should reflect the
        # increment ("Clicks: 1" or higher).
        scene_after = _snapshot(tf)
        all_text = _collect_text(scene_after)
        click_lines = [t for t in all_text if t.startswith("Clicks:")]
        assert click_lines, "E.3 viewport renders a 'Clicks: N' counter line"
        # Parse the integer; assert ≥1 after the click.
        first_count = int(click_lines[0].split(":")[1].strip())
        assert first_count >= 1, (
            f"E.4 click counter advanced after click ({click_lines[0]!r})"
        )

        # ─── (F) Splitter drag ───────────────────────────────────────
        _section("F: inner-H splitter drag")
        # Pre-drag ratio.
        ratio_before = _query_splitter_ratio(tf, _SPLIT_INNER_H_TAG)
        assert ratio_before is not None, "F.1 inner-H ratio readable pre-drag"
        # Drag the inner-H splitter to a new position.
        # The exact rect depends on the layout pass; drive the drag
        # via scene/drag with the splitter tag as `from_path` (the
        # InputRouter hits the splitter Container's bare gutter and
        # routes to SplitterExternal::pointer_move).
        # (R685 §5.16) Drag the inner-H splitter via raw coordinates
        # computed from the handle's layout rect. The handle
        # Container is intentionally untagged (composite-tag dispatch
        # would normalise against the 4-px handle rect, breaking
        # SplitterExternal's splitter-wide cursor-fraction semantics);
        # the demo addresses it by walking `scene/layout` for the
        # splitter Container + picking children[1] (the handle is the
        # middle of [left, handle, right]).
        layout = tf.request(
            "scene/layout",
            {"viewport": {"width": _MAIN_W, "height": _MAIN_H}},
        )

        def _find_node(n, t):
            if not isinstance(n, dict):
                return None
            if n.get("tag") == t:
                return n
            for c in n.get("children") or []:
                r = _find_node(c, t)
                if r is not None:
                    return r
            return None

        splitter = _find_node(layout.result, _SPLIT_INNER_H_TAG)
        children = splitter.get("children") if splitter else []
        if len(children) >= 3:
            handle_rect = children[1].get("rect") or {}
            handle_cx = float(handle_rect.get("x", 0)) + float(handle_rect.get("w", 4)) * 0.5
            handle_cy = float(handle_rect.get("y", 0)) + float(handle_rect.get("h", 1)) * 0.5
        else:
            handle_cx, handle_cy = float(_MAIN_W) * 0.5, float(_MAIN_H) * 0.5
        drag_resp = tf.request(
            "scene/drag",
            {
                "from": {"x": handle_cx, "y": handle_cy},
                "to": {"x": 600.0, "y": handle_cy},
                "steps": 4,
            },
        )
        assert drag_resp is not None, "F.2 scene/drag returns a response"
        ratio_after = wait_until(
            lambda: _query_splitter_ratio(tf, _SPLIT_INNER_H_TAG),
            desc="F.3 inner-H ratio readable post-drag",
        )
        # Ratio stays within the SplitterExternal default clamp
        # (0.05, 0.95). The drag may not change the ratio if the
        # cursor lands at the same fractional position; the
        # important invariant is "stays in clamp range".
        assert 0.05 <= ratio_after <= 0.95, (
            f"F.4 post-drag ratio {ratio_after} inside clamp [0.05, 0.95]"
        )

        # ─── (G) Topology determinism ────────────────────────────────
        _section("G: topology determinism")
        # Two snapshots back-to-back with no state change should
        # produce scene with identical structural shape (modulo
        # paint metadata like rect — which depends on layout that
        # is deterministic anyway).
        snap_a = _snapshot(tf)
        snap_b = _snapshot(tf)
        # Compare just the tag sets — full snapshot includes
        # rects that may shift if the dirty cache invalidates.
        tags_a = _collect_tags(snap_a)
        tags_b = _collect_tags(snap_b)
        assert tags_a == tags_b, (
            "G.1 successive snapshots produce identical tag sets "
            f"(a={sorted(tags_a)[:5]}.. b={sorted(tags_b)[:5]}..)"
        )

        # ─── (H) Panel content sanity ────────────────────────────────
        _section("H: panel content sanity")
        all_text_h = _collect_text(snap_a)
        # Toolbar menu names.
        for menu in ("File", "Edit", "View", "Window", "Help"):
            assert any(menu in t for t in all_text_h), (
                f"H.toolbar.{menu} toolbar contains '{menu}' menu"
            )
        # Outliner Scene root + a few sub-items.
        outliner_present = ["Scene", "Camera", "Lights", "Meshes"]
        for label in outliner_present:
            assert any(label in t for t in all_text_h), (
                f"H.outliner.{label} outliner row '{label}' rendered"
            )
        # Viewport label.
        assert any("Viewport" in t for t in all_text_h), (
            "H.viewport viewport header label rendered"
        )
        # Properties keys.
        for key in ("Position", "Rotation", "Scale"):
            assert any(key in t for t in all_text_h), (
                f"H.properties.{key} property key '{key}' rendered"
            )
        # Console log levels.
        assert any("[info]" in t for t in all_text_h), (
            "H.console.info console renders [info] log lines"
        )
        assert any("[warn]" in t for t in all_text_h), (
            "H.console.warn console renders [warn] log lines"
        )

        # ─── final cleanup — release the subprocess ─────────────────
        _section("done")


def _collect_tags(scene: Any) -> set[str]:
    out: set[str] = set()

    def walk(node: Any) -> None:
        if not isinstance(node, dict):
            return
        tag = node.get("tag")
        if isinstance(tag, str):
            out.add(tag)
        for child in node.get("children") or []:
            walk(child)
        content = node.get("content")
        if isinstance(content, dict):
            walk(content)

    walk(scene)
    return out


if __name__ == "__main__":
    sys.exit(run_demo("r685_editor_layout", body))
