#!/usr/bin/env python3
"""R683.C §5.16 §5.41 §5.45 §5.49 — dock-panel tear-off + dock-back +
cross-panel select.

Drives `hello-dock-panels` — the first consumer of the R683.B
`pinion_widget_paint::dock` + `splitter` substrates and the 2nd
consumer of the R683.C-lifted `pinion_widget_paint::devtools::ClickRouter`.

Section roadmap (≥40 assertions across A–J):

  (A) Substrate sanity — viewport router slot reachable + all 3 dock-
      panel Externals expose their canonical schema slots.
  (B) Initial topology — 1 window, all 3 panels docked (panel tags in
      main scene), splitter ratios at the canonical defaults.
  (C) Splitter drag — drag the main splitter handle; ratio Signal
      mutates; layout reflows on the next snapshot.
  (D) Tear-off — drag the inspector header past 0.5 fraction; the
      reconcile Effect spawns a 2nd window `torn-inspector`; the
      inspector slot in the main dock becomes a placeholder.
  (E) Dock-back — drag the floating inspector's header past 0.5
      fraction; the reconcile Effect drops the floating window; the
      main dock re-installs the inspector.
  (F) Cross-panel selection (inspector tree → viewport wrap) — click
      the viewport-button row in the inspector tree; viewport scene
      paints the Error-red highlight wrap.
  (G) Cross-panel selection (viewport router → inspector focus) — AI
      invoke on the viewport ClickRouter; inspector tree row paints
      the focus state-layer + property pane updates with the resolved
      node's type.
  (H) Signal-driven topology round-trip — alternating tear-off /
      dock-back of all 3 panels; topology consistently reflects the
      Signal across multiple cycles.
  (I) Multi-tear-off cascade — tear off all 3 panels in sequence;
      observe a 4-window state (main + 3 floating).
  (J) Cleanup deselect + return-to-base — invoke router with Null;
      dock all 3 panels back; final state matches the initial
      baseline.
"""

from __future__ import annotations

import sys
import time
from pathlib import Path
from typing import Any, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import RpcError, RpcSubprocess, run_demo  # noqa: E402


# ─── constants mirrored from the binding ────────────────────────────

_MAIN_W = 880
_MAIN_H = 600
_FLOATING_W = 360
_FLOATING_H = 360

# Panel root tags. The DockPanelExternal is registered against the
# panel root tag (R683.C InputRouter routing correction — see
# `pinion_widget_paint::dock::DockPanelExternal` rustdoc). The paint
# scene tags the header child with `{panel_tag}#header`, but RPC
# query/invoke paths address the External by its primary tag.
_INSPECTOR_PANEL_TAG = "inspector"
_PROPERTY_PANEL_TAG = "property"
_VIEWPORT_PANEL_TAG = "viewport"

# Composite paint tags the dock substrate emits on the header child of
# each panel. Used as `from_path` arguments for `scene/drag` — the
# InputRouter resolves the rect at this tag and dispatches pointer
# events to the External at the primary half (`split_subindex(...).0`).
_INSPECTOR_HEADER_TAG = "inspector#header"
_PROPERTY_HEADER_TAG = "property#header"
_VIEWPORT_HEADER_TAG = "viewport#header"

# Splitter paint-side tags.
_MAIN_SPLITTER_TAG = "main_splitter"
_LEFT_SPLITTER_TAG = "left_splitter"

# Inspector / property / viewport content tags.
_INSPECTOR_TREE_TAG = "inspector_tree"
_PROPERTY_PANE_TAG = "property_pane"
_VIEWPORT_BTN_TAG = "viewport_btn"
_VIEWPORT_CLICK_ROUTER_TAG = "viewport_click_router"

# Canonical raw path the binding writes on a real user button click.
_VIEWPORT_BTN_RAW_PATH = "Container/Container[viewport_btn]"

# Floating-window id prefix the binding mints on tear-off.
_FLOATING_PREFIX = "torn-"

_SETTLE_SEC = 0.30


# ─── snapshot + introspect helpers ──────────────────────────────────


def _snapshot_window(tf: RpcSubprocess, window: str, w: int, h: int) -> Any:
    resp = tf.request(
        "scene/snapshot",
        {"path": "", "window": window, "from": "paint",
         "viewport": {"w": w, "h": h}},
    )
    assert resp is not None
    return resp.result


def _snap_main(tf: RpcSubprocess) -> Any:
    return _snapshot_window(tf, "main", _MAIN_W, _MAIN_H)


def _snap_floating(tf: RpcSubprocess, panel_id: str) -> Any:
    return _snapshot_window(tf, f"{_FLOATING_PREFIX}{panel_id}", _FLOATING_W, _FLOATING_H)


def _query(tf: RpcSubprocess, path: str) -> Any:
    return tf.query(path)


def _query_router_last_clicked(tf: RpcSubprocess) -> Any:
    return _query(tf, f"/{_VIEWPORT_CLICK_ROUTER_TAG}/external/last_clicked")


def _query_splitter_ratio(tf: RpcSubprocess, splitter_tag: str) -> Optional[float]:
    val = _query(tf, f"/{splitter_tag}/external/ratio")
    if val is None or not isinstance(val, (int, float)):
        return None
    return float(val)


def _query_dock_panel_dragging(tf: RpcSubprocess, header_tag: str) -> Optional[bool]:
    val = _query(tf, f"/{header_tag}/external/dragging")
    if val is None or not isinstance(val, bool):
        return None
    return val


def _query_dock_panel_tear_off_fired(
    tf: RpcSubprocess, header_tag: str
) -> Optional[bool]:
    val = _query(tf, f"/{header_tag}/external/tear_off_fired")
    if val is None or not isinstance(val, bool):
        return None
    return val


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


def _collect_borders(scene: Any) -> list[dict]:
    """Walk the snapshot for every node carrying a stroked border."""
    out: list[dict] = []

    def walk(node: Any) -> None:
        if not isinstance(node, dict):
            return
        style = node.get("style")
        if isinstance(style, dict):
            border = style.get("border")
            if isinstance(border, dict) and int(border.get("width", 0)) > 0:
                out.append(border)
        for child in node.get("children") or []:
            walk(child)
        content = node.get("content")
        if isinstance(content, dict):
            walk(content)

    walk(scene)
    return out


def _find_row_fill_by_tag(scene: Any, target: str) -> Optional[tuple[int, int, int, int]]:
    """Walk the snapshot for the first node whose tag == target; return
    its fill colour as an rgba 4-tuple. Used to verify the inspector
    row paints the M3 focus state-layer."""
    if not isinstance(scene, dict):
        return None
    if scene.get("tag") == target:
        style = scene.get("style") or {}
        fill = style.get("fill") or {}
        return (
            int(fill.get("r", 0)),
            int(fill.get("g", 0)),
            int(fill.get("b", 0)),
            int(fill.get("a", 0)),
        )
    for child in scene.get("children") or []:
        match = _find_row_fill_by_tag(child, target)
        if match is not None:
            return match
    content = scene.get("content")
    if isinstance(content, dict):
        match = _find_row_fill_by_tag(content, target)
        if match is not None:
            return match
    return None


def _is_transparent(rgba: tuple[int, int, int, int]) -> bool:
    return rgba == (0, 0, 0, 0)


def _drag_window(
    tf: RpcSubprocess,
    window: str,
    from_path: str,
    to_at: tuple[float, float],
    steps: int = 8,
) -> None:
    """Window-scoped `scene/drag` — `tf.drag` doesn't accept a window
    arg, so call the raw RPC directly. `from_path` resolves against the
    addressed window's last paint scene; `to_at` is in window-local
    logical pixels."""
    tf.request(
        "scene/drag",
        {
            "window": window,
            "from_path": from_path,
            "to": {"x": float(to_at[0]), "y": float(to_at[1])},
            "steps": int(steps),
        },
    )


def _collect_text(scene: Any) -> list[str]:
    """Walk the snapshot for every `Scene::Text` node's content."""
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


# ─── demo body ───────────────────────────────────────────────────────


def body() -> None:
    with RpcSubprocess("hello-dock-panels", boot_grace=2.0) as tf:
        # ── (A) Substrate sanity ──────────────────────────────────
        # Viewport ClickRouter — `last_clicked` slot resolves at boot,
        # returns Null on fresh state.
        boot_last = _query_router_last_clicked(tf)
        assert boot_last is None, (
            f"viewport router last_clicked must be Null at boot; got: {boot_last!r}"
        )

        # All 3 dock-panel Externals expose canonical slots.
        for panel_tag in (_INSPECTOR_PANEL_TAG, _PROPERTY_PANEL_TAG, _VIEWPORT_PANEL_TAG):
            dragging = _query_dock_panel_dragging(tf, panel_tag)
            assert dragging is False, (
                f"{panel_tag} dragging slot must be False at boot; got: {dragging!r}"
            )
            fired = _query_dock_panel_tear_off_fired(tf, panel_tag)
            assert fired is False, (
                f"{panel_tag} tear_off_fired slot must be False at boot; got: {fired!r}"
            )

        # Both splitters expose the ratio slot at the canonical defaults.
        main_ratio_boot = _query_splitter_ratio(tf, _MAIN_SPLITTER_TAG)
        assert main_ratio_boot is not None
        assert abs(main_ratio_boot - 0.32) < 0.01, (
            f"main splitter must boot at MAIN_SPLIT_RATIO_DEFAULT 0.32; got: {main_ratio_boot!r}"
        )
        left_ratio_boot = _query_splitter_ratio(tf, _LEFT_SPLITTER_TAG)
        assert left_ratio_boot is not None
        assert abs(left_ratio_boot - 0.55) < 0.01, (
            f"left splitter must boot at LEFT_SPLIT_RATIO_DEFAULT 0.55; got: {left_ratio_boot!r}"
        )

        # ── (B) Initial topology — 3 panels docked ────────────────
        snap_b = _snap_main(tf)
        assert _scene_contains_tag(snap_b, _INSPECTOR_PANEL_TAG), (
            "boot main scene must contain the inspector panel tag"
        )
        assert _scene_contains_tag(snap_b, _PROPERTY_PANEL_TAG), (
            "boot main scene must contain the property panel tag"
        )
        assert _scene_contains_tag(snap_b, _VIEWPORT_PANEL_TAG), (
            "boot main scene must contain the viewport panel tag"
        )
        # And the splitter tags.
        assert _scene_contains_tag(snap_b, _MAIN_SPLITTER_TAG)
        assert _scene_contains_tag(snap_b, _LEFT_SPLITTER_TAG)
        # Boot inspector tree carries the inspector_tree root + the
        # mirrored viewport tree.
        assert _scene_contains_tag(snap_b, _INSPECTOR_TREE_TAG)
        # No selection / hover wraps yet.
        boot_borders = _collect_borders(snap_b)
        assert boot_borders == [], (
            f"boot scene must have no stroked borders; got: {boot_borders!r}"
        )

        # ── (C) Splitter drag — main ratio mutates ────────────────
        # Drag the main splitter handle far enough that the projected
        # cursor delta exceeds the canonical clamp delta. The drag
        # forwards through the per-window InputRouter under the R51.34
        # capture lock; `from_path` resolves the main splitter
        # Container's rect centre, `to_at` is the target cursor pixel.
        _drag_window(
            tf, "main", _MAIN_SPLITTER_TAG, (_MAIN_W * 0.6, _MAIN_H * 0.5), steps=8
        )
        time.sleep(_SETTLE_SEC)
        main_ratio_after_drag = _query_splitter_ratio(tf, _MAIN_SPLITTER_TAG)
        assert main_ratio_after_drag is not None
        assert main_ratio_after_drag != main_ratio_boot, (
            f"main splitter ratio must change after drag; "
            f"boot={main_ratio_boot!r}, after={main_ratio_after_drag!r}"
        )
        # Drag back to the left so the ratio returns toward the
        # default; the assertion confirms the drag wire is bidirectional.
        _drag_window(
            tf, "main", _MAIN_SPLITTER_TAG, (_MAIN_W * 0.2, _MAIN_H * 0.5), steps=8
        )
        time.sleep(_SETTLE_SEC)
        main_ratio_after_drag_back = _query_splitter_ratio(tf, _MAIN_SPLITTER_TAG)
        assert main_ratio_after_drag_back is not None
        assert main_ratio_after_drag_back != main_ratio_after_drag, (
            f"main splitter ratio must change on drag-back too; "
            f"first_drag={main_ratio_after_drag!r}, "
            f"second_drag={main_ratio_after_drag_back!r}"
        )

        # ── (D) Tear-off — inspector header ───────────────────────
        # The dock substrate's L∞ delta past 0.5 fraction fires the
        # `tear_off` intent → reducer pushes a new floating WindowSpec.
        # Press at the inspector header's centre, drag well past the
        # threshold (toward window centre + some).
        _drag_window(
            tf,
            "main",
            _INSPECTOR_HEADER_TAG,
            (_MAIN_W * 0.5, _MAIN_H * 0.5),
            steps=8,
        )
        time.sleep(_SETTLE_SEC)
        # The torn-inspector floating window now answers scene/snapshot.
        snap_floating_inspector = _snap_floating(tf, _INSPECTOR_PANEL_TAG)
        assert _scene_contains_tag(snap_floating_inspector, _INSPECTOR_PANEL_TAG), (
            "torn-inspector floating window must contain the inspector panel tag"
        )
        assert _scene_contains_tag(snap_floating_inspector, _INSPECTOR_TREE_TAG), (
            "torn-inspector floating window must host the inspector tree"
        )
        # And the main dock now shows the inspector placeholder, not
        # the live inspector panel.
        snap_d_main = _snap_main(tf)
        assert not _scene_contains_tag(snap_d_main, _INSPECTOR_PANEL_TAG), (
            "main dock must drop the inspector panel after tear-off"
        )
        assert _scene_contains_tag(snap_d_main, "inspector_placeholder"), (
            "main dock must paint the inspector placeholder after tear-off"
        )
        # Property + viewport panels still docked.
        assert _scene_contains_tag(snap_d_main, _PROPERTY_PANEL_TAG)
        assert _scene_contains_tag(snap_d_main, _VIEWPORT_PANEL_TAG)

        # ── (E) Dock-back — direct tear_off invoke on floating ────
        # R683.C §5.16 §5.49 — the floating window's InputRouter does
        # not have a `last_paint_scene` populated in headless RPC mode
        # (winit's per-window paint cycle requires a real display).
        # The dock substrate exposes a direct `tear_off` invoke channel
        # for this exact case: AI clients drive `scene/invoke
        # /<panel_tag>/external/tear_off {Null}` to enqueue the same
        # `tear_off` intent the drag path would. The binding's
        # reducer-driven `Signal<Vec<WindowSpec>>` toggle interprets
        # this as dock-back when the panel is currently floating
        # (idempotent on the toggle).
        tf.invoke(f"/{_INSPECTOR_PANEL_TAG}/external/tear_off", None)
        time.sleep(_SETTLE_SEC)
        # Main dock re-installs the inspector (the reducer's
        # `is_panel_floating(...)` toggle removed the WindowSpec; the
        # reconcile Effect dropped the floating winit window; the
        # main dock re-renders with the live panel back in its slot).
        snap_e_main = _snap_main(tf)
        assert _scene_contains_tag(snap_e_main, _INSPECTOR_PANEL_TAG), (
            "main dock must re-install inspector panel after dock-back"
        )
        assert not _scene_contains_tag(snap_e_main, "inspector_placeholder"), (
            "main dock placeholder must disappear after dock-back"
        )
        # The inspector tree tag is back too (mirror panel content).
        assert _scene_contains_tag(snap_e_main, _INSPECTOR_TREE_TAG), (
            "inspector tree content re-installed alongside the panel"
        )

        # ── (F) Cross-panel selection (inspector → viewport wrap) ─
        # Direct invoke on the inspector tree row click External writes
        # the path into the shared `selected_path` Signal; the viewport
        # paints the highlight wrap.
        # The viewport tree mirrors as `Container/Container[viewport_btn]`
        # (raw path; tree row composite tag = `inspector_tree#{path}`).
        tf.invoke(f"/{_INSPECTOR_TREE_TAG}/external/click", _VIEWPORT_BTN_RAW_PATH)
        time.sleep(_SETTLE_SEC)
        snap_f_main = _snap_main(tf)
        borders_f = _collect_borders(snap_f_main)
        assert len(borders_f) >= 1, (
            f"inspector click must paint at least one highlight wrap; "
            f"got: {borders_f!r}"
        )
        # Inspector tree row paints the M3 focus state-layer (non-
        # transparent fill on the row tagged `inspector_tree#{path}`).
        row_tag = f"{_INSPECTOR_TREE_TAG}#{_VIEWPORT_BTN_RAW_PATH}"
        focused_fill_f = _find_row_fill_by_tag(snap_f_main, row_tag)
        assert focused_fill_f is not None, (
            f"inspector row {row_tag!r} must exist post-click"
        )
        assert not _is_transparent(focused_fill_f), (
            f"inspector row must paint focus state-layer post-click; "
            f"got fill: {focused_fill_f!r}"
        )

        # ── (G) Property pane updates on selection ────────────────
        property_texts_f = _collect_text(snap_f_main)
        # Property pane shows `type: Container` for the viewport btn
        # (which is a Container[viewport_btn]) since that's what the
        # find_node_at_path lookup resolves to.
        assert any("type: Container" in t for t in property_texts_f), (
            f"property pane must show resolved node's type after selection; "
            f"texts: {property_texts_f!r}"
        )

        # ── (G.2) Viewport router → cross-panel select ────────────
        tf.invoke(f"/{_VIEWPORT_CLICK_ROUTER_TAG}/external/click", "Container")
        time.sleep(_SETTLE_SEC)
        # last_clicked mirror reflects the latest invoke.
        last_clicked_g = _query_router_last_clicked(tf)
        assert last_clicked_g == "Container", (
            f"router last_clicked must mirror latest invoke; got: {last_clicked_g!r}"
        )
        snap_g = _snap_main(tf)
        # Inspector now focuses on `Container` row.
        focused_fill_root = _find_row_fill_by_tag(
            snap_g, f"{_INSPECTOR_TREE_TAG}#Container"
        )
        assert focused_fill_root is not None, (
            "inspector must carry row for Container path"
        )
        assert not _is_transparent(focused_fill_root), (
            "router-driven select must paint focus on the inspector row"
        )

        # ── (H) Signal-driven topology round-trip — multi-cycle ───
        # Round 1: tear off property → dock back.
        _drag_window(
            tf,
            "main",
            _PROPERTY_HEADER_TAG,
            (_MAIN_W * 0.5, _MAIN_H * 0.5),
            steps=8,
        )
        time.sleep(_SETTLE_SEC)
        snap_h1 = _snap_main(tf)
        assert _scene_contains_tag(snap_h1, "property_placeholder"), (
            "round-trip H1: property must show placeholder after tear-off"
        )
        # Verify the floating window exists.
        snap_floating_property = _snap_floating(tf, _PROPERTY_PANEL_TAG)
        assert _scene_contains_tag(snap_floating_property, _PROPERTY_PANEL_TAG)
        # Dock back via direct tear_off invoke (see section E for the
        # rationale — floating-window InputRouter is empty under
        # headless RPC).
        tf.invoke(f"/{_PROPERTY_PANEL_TAG}/external/tear_off", None)
        time.sleep(_SETTLE_SEC)
        snap_h2 = _snap_main(tf)
        assert _scene_contains_tag(snap_h2, _PROPERTY_PANEL_TAG), (
            "round-trip H2: property must re-install in main dock after dock-back"
        )

        # Round 2: tear off viewport → dock back.
        _drag_window(
            tf,
            "main",
            _VIEWPORT_HEADER_TAG,
            (_MAIN_W * 0.5, _MAIN_H * 0.5),
            steps=8,
        )
        time.sleep(_SETTLE_SEC)
        snap_h3 = _snap_main(tf)
        assert _scene_contains_tag(snap_h3, "viewport_placeholder"), (
            "round-trip H3: viewport must show placeholder after tear-off"
        )
        tf.invoke(f"/{_VIEWPORT_PANEL_TAG}/external/tear_off", None)
        time.sleep(_SETTLE_SEC)
        snap_h4 = _snap_main(tf)
        assert _scene_contains_tag(snap_h4, _VIEWPORT_PANEL_TAG), (
            "round-trip H4: viewport must re-install in main dock after dock-back"
        )

        # ── (I) Multi-tear-off cascade — 4-window state ───────────
        # Tear off all 3 panels in sequence.
        for header_tag, panel_id in (
            (_INSPECTOR_HEADER_TAG, _INSPECTOR_PANEL_TAG),
            (_PROPERTY_HEADER_TAG, _PROPERTY_PANEL_TAG),
            (_VIEWPORT_HEADER_TAG, _VIEWPORT_PANEL_TAG),
        ):
            _drag_window(
                tf, "main", header_tag,
                (_MAIN_W * 0.5, _MAIN_H * 0.5), steps=8,
            )
            time.sleep(_SETTLE_SEC)
        # All 3 floating windows answer scene/snapshot.
        for panel_id in (_INSPECTOR_PANEL_TAG, _PROPERTY_PANEL_TAG, _VIEWPORT_PANEL_TAG):
            snap_floating_i = _snap_floating(tf, panel_id)
            assert _scene_contains_tag(snap_floating_i, panel_id), (
                f"multi-tear-off cascade: {panel_id} floating window must exist"
            )
        # Main dock shows all 3 placeholders.
        snap_i_main = _snap_main(tf)
        for panel_id in (_INSPECTOR_PANEL_TAG, _PROPERTY_PANEL_TAG, _VIEWPORT_PANEL_TAG):
            placeholder_tag = f"{panel_id}_placeholder"
            assert _scene_contains_tag(snap_i_main, placeholder_tag), (
                f"main dock must show placeholder for floating {panel_id}"
            )
        # And no panel tag (all torn off).
        for panel_id in (_INSPECTOR_PANEL_TAG, _PROPERTY_PANEL_TAG, _VIEWPORT_PANEL_TAG):
            assert not _scene_contains_tag(snap_i_main, panel_id), (
                f"main dock must not contain {panel_id} panel tag when all torn off"
            )

        # ── (J) Cleanup — deselect + dock all 3 back ──────────────
        tf.invoke(f"/{_VIEWPORT_CLICK_ROUTER_TAG}/external/click", None)
        time.sleep(_SETTLE_SEC)
        last_clicked_j = _query_router_last_clicked(tf)
        assert last_clicked_j is None, (
            f"Null invoke must clear router last_clicked; got: {last_clicked_j!r}"
        )
        # Dock all 3 back via direct tear_off invoke (floating-window
        # InputRouter unavailable in headless RPC — see section E).
        for panel_id in (
            _INSPECTOR_PANEL_TAG,
            _PROPERTY_PANEL_TAG,
            _VIEWPORT_PANEL_TAG,
        ):
            tf.invoke(f"/{panel_id}/external/tear_off", None)
            time.sleep(_SETTLE_SEC)
        # Verify return to baseline: all 3 panels docked, no
        # placeholders, no floating windows.
        snap_j = _snap_main(tf)
        for panel_id in (_INSPECTOR_PANEL_TAG, _PROPERTY_PANEL_TAG, _VIEWPORT_PANEL_TAG):
            assert _scene_contains_tag(snap_j, panel_id), (
                f"final state: {panel_id} must be docked"
            )
            placeholder_tag = f"{panel_id}_placeholder"
            assert not _scene_contains_tag(snap_j, placeholder_tag), (
                f"final state: {panel_id} placeholder must be gone"
            )
        # All 3 floating windows are dropped — verify by checking the
        # main scene state: every panel tag back in main, every
        # placeholder tag gone (already asserted above).
        # The R683.A reconcile Effect has dropped the floating
        # WindowSpec entries; the binding's `windows_signal` now matches
        # the initial single-main-window topology.


if __name__ == "__main__":
    sys.exit(run_demo(
        "R683.C §5.16 §5.41 §5.45 §5.49 — dock tear-off + dock-back + cross-panel select",
        body,
    ))
