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
from pathlib import Path
from typing import Any, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import RpcError, RpcSubprocess, run_demo, wait_snap, wait_until  # noqa: E402


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

# (R685 §5.16) Splitter handle drag — the handle Container is
# intentionally **untagged** (composite-tag dispatch would route
# normalisation against the 4-px handle rect, breaking
# `SplitterExternal::pointer_move`'s splitter-wide cursor-fraction
# semantics). AI clients drive the drag with raw coordinates: query
# the splitter's rect via `scene/layout` + the live ratio via
# `<splitter>/external/ratio`, then compute the handle position as
# `splitter.x + splitter.w * ratio` and drag a few pixels past that.
def _find_node_by_tag(node, target):
    if not isinstance(node, dict):
        return None
    if node.get("tag") == target:
        return node
    for child in node.get("children") or []:
        r = _find_node_by_tag(child, target)
        if r is not None:
            return r
    return None


def _splitter_handle_rect(tf: "RpcSubprocess", splitter_tag: str, viewport_w: int) -> tuple:
    """Resolve the splitter's current handle rect (`(x, y, w, h)`).

    The handle Container is intentionally untagged (composite-tag
    dispatch would route via the 4-px handle rect for cursor
    normalisation, breaking SplitterExternal's splitter-wide
    cursor-fraction semantics). The handle is the middle child of
    the splitter Container, located between the two panel children;
    its rect is queryable through `scene/layout` by walking the
    Container's children list at index 1.
    """
    layout = tf.request("scene/layout", {"viewport": {"width": viewport_w, "height": _MAIN_H}})
    if layout is None:
        return (viewport_w * 0.5 - 2, _MAIN_H * 0.5 - 300, 4, 600)
    splitter = _find_node_by_tag(layout.result, splitter_tag)
    if splitter is None:
        return (viewport_w * 0.5 - 2, _MAIN_H * 0.5 - 300, 4, 600)
    children = splitter.get("children") or []
    # Splitter Container layout: [left_panel (tagged), handle (untagged),
    # right_panel (tagged)]. Pick the middle child for horizontal /
    # vertical orientation alike.
    if len(children) >= 3:
        handle = children[1]
        rect = handle.get("rect") or {}
        return (
            float(rect.get("x", 0)),
            float(rect.get("y", 0)),
            float(rect.get("w", 4)),
            float(rect.get("h", 600)),
        )
    sr = splitter.get("rect") or {}
    return (float(sr.get("x", 0)) + float(sr.get("w", viewport_w)) * 0.5 - 2,
            float(sr.get("y", 0)),
            4.0, float(sr.get("h", _MAIN_H)))


def _splitter_handle_x(tf: "RpcSubprocess", splitter_tag: str, viewport_w: int) -> float:
    """Handle main-axis centre x for horizontal splitters."""
    x, _, w, _ = _splitter_handle_rect(tf, splitter_tag, viewport_w)
    return x + w * 0.5

# Inspector / property / viewport content tags.
_INSPECTOR_TREE_TAG = "inspector_tree"
_PROPERTY_PANE_TAG = "property_pane"
_VIEWPORT_BTN_TAG = "viewport_btn"
_VIEWPORT_CLICK_ROUTER_TAG = "viewport_click_router"

# Canonical raw path the binding writes on a real user button click.
_VIEWPORT_BTN_RAW_PATH = "Container/Container[viewport_btn]"

# Floating-window id prefix the binding mints on tear-off.
_FLOATING_PREFIX = "torn-"


# ─── snapshot + introspect helpers ──────────────────────────────────


def _snapshot_window(tf: RpcSubprocess, window: str, w: int, h: int) -> Any:
    return tf.snapshot(source="paint", viewport=(w, h), window=window)


def _snap_main(tf: RpcSubprocess) -> Any:
    return _snapshot_window(tf, "main", _MAIN_W, _MAIN_H)


def _snap_floating(tf: RpcSubprocess, panel_id: str) -> Any:
    return _snapshot_window(tf, f"{_FLOATING_PREFIX}{panel_id}", _FLOATING_W, _FLOATING_H)


def _wait_floating(tf: RpcSubprocess, panel_id: str) -> Any:
    """Gate on the torn-off floating window becoming RPC-addressable.

    Window creation is genuinely asynchronous — the tear-off reducer
    commits inside the drag dispatch, but the reconcile Effect spawns
    the winit window afterwards. Poll a `{window: torn-<panel>}`-scoped
    snapshot until it stops raising (R883 zero-flake)."""
    def poll():
        try:
            return _snap_floating(tf, panel_id)
        except RpcError:
            return None

    return wait_until(
        poll, desc=f"floating window {_FLOATING_PREFIX}{panel_id} addressable"
    )


def _wait_main(tf: RpcSubprocess, predicate, desc: str) -> Any:
    """Poll the main window's paint snapshot until `predicate(snap)`
    holds; return the matching snapshot (R883 zero-flake gate)."""
    return wait_snap(
        tf, predicate, viewport=(_MAIN_W, _MAIN_H), window="main", desc=desc
    )


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
        # Press at the current handle x position, drag right to the
        # target — raw coordinates so the InputRouter dispatches via
        # splitter-tag fallback (handle is untagged for coord-frame
        # correctness, see _splitter_handle_x doc).
        handle_x = _splitter_handle_x(tf, _MAIN_SPLITTER_TAG, _MAIN_W)
        tf.request("scene/drag", {
            "window": "main",
            "from": {"x": handle_x, "y": _MAIN_H * 0.5},
            "to":   {"x": _MAIN_W * 0.6, "y": _MAIN_H * 0.5},
            "steps": 8,
        })
        wait_until(
            lambda: _query_splitter_ratio(tf, _MAIN_SPLITTER_TAG) != main_ratio_boot,
            desc="main splitter ratio changes after drag",
        )
        main_ratio_after_drag = _query_splitter_ratio(tf, _MAIN_SPLITTER_TAG)
        assert main_ratio_after_drag is not None
        assert main_ratio_after_drag != main_ratio_boot, (
            f"main splitter ratio must change after drag; "
            f"boot={main_ratio_boot!r}, after={main_ratio_after_drag!r}"
        )
        # Drag back to the left — re-query handle position because the
        # first drag moved it.
        handle_x = _splitter_handle_x(tf, _MAIN_SPLITTER_TAG, _MAIN_W)
        tf.request("scene/drag", {
            "window": "main",
            "from": {"x": handle_x, "y": _MAIN_H * 0.5},
            "to":   {"x": _MAIN_W * 0.2, "y": _MAIN_H * 0.5},
            "steps": 8,
        })
        wait_until(
            lambda: _query_splitter_ratio(tf, _MAIN_SPLITTER_TAG) != main_ratio_after_drag,
            desc="main splitter ratio changes on drag-back",
        )
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
        # The torn-inspector floating window now answers scene/snapshot
        # (gated — window creation is async, see _wait_floating).
        snap_floating_inspector = _wait_floating(tf, _INSPECTOR_PANEL_TAG)
        assert _scene_contains_tag(snap_floating_inspector, _INSPECTOR_PANEL_TAG), (
            "torn-inspector floating window must contain the inspector panel tag"
        )
        assert _scene_contains_tag(snap_floating_inspector, _INSPECTOR_TREE_TAG), (
            "torn-inspector floating window must host the inspector tree"
        )
        # And the main dock now shows the inspector placeholder, not
        # the live inspector panel.
        snap_d_main = _wait_main(
            tf,
            lambda s: _scene_contains_tag(s, "inspector_placeholder"),
            "main dock must paint the inspector placeholder after tear-off",
        )
        assert not _scene_contains_tag(snap_d_main, _INSPECTOR_PANEL_TAG), (
            "main dock must drop the inspector panel after tear-off"
        )
        # Property + viewport panels still docked.
        assert _scene_contains_tag(snap_d_main, _PROPERTY_PANEL_TAG)
        assert _scene_contains_tag(snap_d_main, _VIEWPORT_PANEL_TAG)

        # ── (E) Dock-back — drag-based (R685 §5.16 §5.49) ─────────
        # R683.C originally used a direct `scene/invoke
        # /<panel_tag>/external/tear_off {Null}` channel because the
        # floating window's InputRouter had no `last_paint_scene`
        # populated in headless RPC mode (winit's per-window paint
        # cycle requires a real display).
        #
        # R684 atomic 3 lands a post-dispatch first-paint finalize
        # hook in `pinion-shell::ShellCore::dispatch_rpc_inner` — when
        # an RPC frame addresses a window whose InputRouter has no
        # paint scene yet, the hook re-runs `compute_paint_scene_for_window`
        # + populates the router. This makes a `scene/snapshot` against
        # a never-painted floating window prime its hit-test
        # infrastructure so a subsequent `scene/drag` resolves.
        #
        # R685 atomic 3 converts this section to the drag-based path:
        # the substrate-canonical user gesture (drag the floating
        # header back past the 0.5 threshold) drives dock-back instead
        # of the bypass invoke channel.
        #
        # Step 1: prime the floating window's InputRouter by
        # snapshotting it. The R684 first-paint finalize hook
        # populates `last_paint_scene` so the subsequent drag
        # resolves the header tag against a real layout.
        _ = _snap_floating(tf, _INSPECTOR_PANEL_TAG)
        # Step 2: drag the floating panel's header past the 0.5
        # fraction of its 28-px height (= 14 px). Origin at the
        # composite header tag; target well past the threshold.
        _drag_window(
            tf,
            f"{_FLOATING_PREFIX}{_INSPECTOR_PANEL_TAG}",
            _INSPECTOR_HEADER_TAG,
            (200.0, 200.0),
            steps=4,
        )
        # Main dock re-installs the inspector (the reducer's
        # `is_panel_floating(...)` toggle removed the WindowSpec; the
        # reconcile Effect dropped the floating winit window; the
        # main dock re-renders with the live panel back in its slot).
        snap_e_main = _wait_main(
            tf,
            lambda s: _scene_contains_tag(s, _INSPECTOR_PANEL_TAG),
            "main dock must re-install inspector panel after dock-back",
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
        snap_f_main = _wait_main(
            tf,
            lambda s: len(_collect_borders(s)) >= 1,
            "inspector click must paint at least one highlight wrap",
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
        # last_clicked mirror reflects the latest invoke.
        wait_until(
            lambda: _query_router_last_clicked(tf) == "Container",
            desc="router last_clicked must mirror latest invoke",
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
        _wait_main(
            tf,
            lambda s: _scene_contains_tag(s, "property_placeholder"),
            "round-trip H1: property must show placeholder after tear-off",
        )
        # Verify the floating window exists (creation is async — gated).
        snap_floating_property = _wait_floating(tf, _PROPERTY_PANEL_TAG)
        assert _scene_contains_tag(snap_floating_property, _PROPERTY_PANEL_TAG)
        # Dock back via direct tear_off invoke (see section E for the
        # rationale — floating-window InputRouter is empty under
        # headless RPC).
        tf.invoke(f"/{_PROPERTY_PANEL_TAG}/external/tear_off", None)
        _wait_main(
            tf,
            lambda s: _scene_contains_tag(s, _PROPERTY_PANEL_TAG),
            "round-trip H2: property must re-install in main dock after dock-back",
        )

        # Round 2: tear off viewport → dock back.
        # (R1323) The release must land OFF the dragged panel. A panel released back
        # over its OWN slot snaps back — the ratified desktop-dock rule (R1110/R1162:
        # VS Code / Blender detach only once the drag leaves every dock zone), not a
        # tear-off. The viewport IS the centre panel here (x 284..880), so the old
        # "drag to the window centre" target landed inside the viewport itself; drop it
        # over the INSPECTOR (0..280) instead, which for this coordinator-less binding
        # is not a dock target either → float.
        _drag_window(
            tf,
            "main",
            _VIEWPORT_HEADER_TAG,
            (140.0, 160.0),
            steps=8,
        )
        _wait_main(
            tf,
            lambda s: _scene_contains_tag(s, "viewport_placeholder"),
            "round-trip H3: viewport must show placeholder after tear-off",
        )
        tf.invoke(f"/{_VIEWPORT_PANEL_TAG}/external/tear_off", None)
        _wait_main(
            tf,
            lambda s: _scene_contains_tag(s, _VIEWPORT_PANEL_TAG),
            "round-trip H4: viewport must re-install in main dock after dock-back",
        )

        # ── (I) Multi-tear-off cascade — 4-window state ───────────
        # Tear off all 3 panels in sequence. (R1323) Each release lands OFF the panel
        # being dragged (a self-release snaps back — R1110/R1162), so the two left-column
        # panels drop over the viewport's area and the viewport drops over the inspector's
        # (a torn slot's placeholder is not a dock target for this coordinator-less
        # binding either — it floats all the same).
        _VIEWPORT_AREA = (_MAIN_W * 0.5, _MAIN_H * 0.5)  # x 284..880 → inside viewport
        _INSPECTOR_AREA = (140.0, 160.0)  # x 0..280, y 0..328 → inside inspector
        for header_tag, panel_id, release_at in (
            (_INSPECTOR_HEADER_TAG, _INSPECTOR_PANEL_TAG, _VIEWPORT_AREA),
            (_PROPERTY_HEADER_TAG, _PROPERTY_PANEL_TAG, _VIEWPORT_AREA),
            (_VIEWPORT_HEADER_TAG, _VIEWPORT_PANEL_TAG, _INSPECTOR_AREA),
        ):
            _drag_window(tf, "main", header_tag, release_at, steps=8)
            _wait_main(
                tf,
                lambda s, p=panel_id: _scene_contains_tag(s, f"{p}_placeholder"),
                f"cascade: {panel_id} placeholder appears after tear-off",
            )
        # All 3 floating windows answer scene/snapshot (creation async — gated).
        for panel_id in (_INSPECTOR_PANEL_TAG, _PROPERTY_PANEL_TAG, _VIEWPORT_PANEL_TAG):
            snap_floating_i = _wait_floating(tf, panel_id)
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
        wait_until(
            lambda: _query_router_last_clicked(tf) is None,
            desc="Null invoke must clear router last_clicked",
        )
        # Dock all 3 back via direct tear_off invoke (floating-window
        # InputRouter unavailable in headless RPC — see section E).
        for panel_id in (
            _INSPECTOR_PANEL_TAG,
            _PROPERTY_PANEL_TAG,
            _VIEWPORT_PANEL_TAG,
        ):
            tf.invoke(f"/{panel_id}/external/tear_off", None)
            _wait_main(
                tf,
                lambda s, p=panel_id: _scene_contains_tag(s, p),
                f"{panel_id} re-docked into the main window",
            )
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
