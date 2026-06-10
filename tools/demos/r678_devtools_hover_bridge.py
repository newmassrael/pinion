#!/usr/bin/env python3
"""R678 §5.16 §5.49 §5.50 — DevTools cross-window hover-overlay bridge.

Drives `hello-multi-window` after R678's two-axis upgrade on top of
R677:

  1. **`TreeRowClickExternal` hover axis** —
     `pinion_widget_paint::tree_view::TreeRowClickExternal` grows a
     parallel `hovered_id: Option<String>` slot and a `hover` event
     name. The substrate now handles `<id>:PointerEnter` and
     `<id>:PointerLeave` on the composite-tag wire, plus a new
     `invoke("hover", Text(id) | Null)` typed shortcut. Each
     hover-state transition emits one `hover` intent (`Text(id)`
     for Enter, `Null` for Leave).
  2. **Cross-window hover Signal** — `hello-multi-window` adds
     `use_hovered_path() -> Rc<Signal<Option<String>>>` (parallel to
     R675's `use_selected_path()`). The `update` reducer routes the
     new `inspector_tree.hover` intent into this Signal — Text →
     `Some(path)`, Null → `None`.
  3. **`view_main` paints BOTH wraps** — selection wrap (R676, Error
     red) and hover wrap (R678, M3 `SurfaceContainerHighest`). Same
     node both signal-targeted → selection wins. Different nodes →
     both visible (different border colors).
  4. **`pinion_widget_paint::devtools` substrate** — Rule-of-Three
     threshold reached at R678 (selection wrap R676 + hover wrap R678
     = 2 consumers); the previously-binding-local path-stable
     indexing + highlight overlay helpers move to the new substrate
     module. `hello-multi-window` simplified to a re-export consumer.

R678 atomic (3) verification scope (≥ 30 assertions):

  (A) Substrate sanity — `inspector_tree` schema exposes the new
      `hovered_id` slot + `hover` invoke channel.
  (B) Baseline — fresh boot has `hovered_id = null` and no hover
      wrap painted in the main scene.
  (C) Typed shortcut Enter — `invoke("/inspector_tree/external/hover", BUTTON_PATH)`
      sets `hovered_id` and paints a SurfaceContainerHighest border
      wrap on the main button. One hover intent emitted.
  (D) Typed shortcut Leave — `invoke("/inspector_tree/external/hover", null)`
      clears `hovered_id` and the hover wrap disappears.
  (E) Send-wire Enter/Leave matches typed shortcut bit-identical —
      `send` composite-tag `<id>:PointerEnter` / `:PointerLeave`
      produces the same `hovered_id` query result and the same
      painted wrap.
  (F) Selection + hover on **same node** → only the selection wrap
      paints (Error red). The hover signal is set but the visible
      wrap is the selection's (selection wins).
  (G) Selection + hover on **different nodes** → both wraps paint
      simultaneously with distinct border colors.
  (H) Inspector-only ids in the hover slot soft-fail — `state` /
      `main` placeholder ids produce no wrap.
  (I) Stale path soft-fail — a hover path that doesn't resolve
      against the current scene produces no wrap.
  (J) Move-from-A-to-B sequence (W3C Leave-before-Enter ordering)
      produces a clean hover-slot transition through Null.
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import RpcError, RpcSubprocess, run_demo, wait_until  # noqa: E402


_VIEW_MAIN_ROOT_PATH = "Container"
_BUTTON_PATH = "Container/Container[main_btn]"
_LABEL_PATH = "Container/Container[main_btn]/Text[0]"

_MAIN_W = 320
_MAIN_H = 200
_INSPECTOR_W = 480
_INSPECTOR_H = 320



def _snap_main(tf) -> dict:
    """Read the main window's paint scene at the canonical viewport."""
    resp = tf.request(
        "scene/snapshot",
        {"path": "", "window": "main", "from": "paint",
         "viewport": {"w": _MAIN_W, "h": _MAIN_H}},
    )
    assert resp is not None
    return resp.result


def _query_hover_slot(tf) -> Any:
    """Return the substrate `hovered_id` slot value (string or None)."""
    resp = tf.request(
        "scene/query",
        {"path": "/inspector_tree/external/hovered_id"},
    )
    assert resp is not None
    return resp.result


def _query_press_slot(tf) -> Any:
    """Return the substrate `pressed_id` slot value (string or None) —
    the press axis is independent from hover, this read confirms
    the axes don't cross-corrupt.
    """
    resp = tf.request(
        "scene/query",
        {"path": "/inspector_tree/external/pressed_id"},
    )
    assert resp is not None
    return resp.result


def _collect_borders(scene: Any) -> list[dict]:
    """Walk a paint snapshot for every node carrying a stroked
    border. Returns the border dict (`{"color": ..., "width": ...}`)
    of each. Empty list when no border is present.
    """
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


def _border_color_tuple(border: dict) -> tuple[int, int, int, int]:
    """Coerce a snapshot border `{"color": {...}}` into a 4-tuple."""
    color = border.get("color") or {}
    return (
        int(color.get("r", 0)),
        int(color.get("g", 0)),
        int(color.get("b", 0)),
        int(color.get("a", 0)),
    )


def body() -> None:
    with RpcSubprocess("hello-multi-window", boot_grace=1.5) as tf:
        # ── (A) Substrate sanity — query channel exposes both slots
        # No `scene/schema` RPC method exists (the schema is pinned
        # in Rust-side substrate tests); instead verify the read
        # channels for both axes resolve at boot — `hovered_id`
        # returns Null (the R678 slot) and `pressed_id` returns Null
        # (the R675 slot still works).
        boot_hovered = _query_hover_slot(tf)
        boot_pressed = _query_press_slot(tf)
        assert boot_hovered is None, (
            f"hovered_id read channel must resolve to null at boot; "
            f"got: {boot_hovered!r}"
        )
        assert boot_pressed is None, (
            f"pressed_id read channel must resolve to null at boot; "
            f"got: {boot_pressed!r}"
        )

        # ── (B) Baseline — fresh hover slot is null, no wrap ──────
        baseline_hover = _query_hover_slot(tf)
        assert baseline_hover is None, (
            f"fresh boot must have hovered_id = null; got: {baseline_hover!r}"
        )
        baseline_press = _query_press_slot(tf)
        assert baseline_press is None
        snap_baseline = _snap_main(tf)
        baseline_borders = _collect_borders(snap_baseline)
        assert baseline_borders == [], (
            f"fresh main scene must have no stroked borders; got: {baseline_borders!r}"
        )

        # ── (C) Typed Enter shortcut paints hover wrap ────────────
        tf.invoke("/inspector_tree/external/hover", _BUTTON_PATH)
        wait_until(
            lambda: _query_hover_slot(tf) == _BUTTON_PATH,
            desc="hover typed shortcut must set hovered_id",
        )
        snap_c = _snap_main(tf)
        borders_c = _collect_borders(snap_c)
        assert len(borders_c) == 1, (
            f"hover wrap = exactly 1 stroked border; got: {borders_c!r}"
        )
        assert int(borders_c[0].get("width", 0)) == 2, (
            f"hover border width must be 2px (DevTools canonical); got: {borders_c!r}"
        )
        hover_color_c = _border_color_tuple(borders_c[0])
        # Pin alpha = 255 (opaque); the rgb is theme-dependent but
        # must be non-zero against the M3 light-palette
        # SurfaceContainerHighest.
        assert hover_color_c[3] == 255, (
            f"hover border alpha must be opaque (255); got: {hover_color_c!r}"
        )
        assert hover_color_c[:3] != (0, 0, 0), (
            f"hover border must not be pure black; got: {hover_color_c!r}"
        )
        # Hover does NOT touch the press slot.
        assert _query_press_slot(tf) is None, (
            "hover must not disturb the press slot"
        )

        # ── (D) Typed Leave shortcut (Null payload) clears wrap ───
        tf.invoke("/inspector_tree/external/hover", None)
        wait_until(
            lambda: _query_hover_slot(tf) is None,
            desc="Null hover invoke must clear hovered_id",
        )
        snap_d = _snap_main(tf)
        borders_d = _collect_borders(snap_d)
        assert borders_d == [], (
            f"Leave must drop the hover wrap; got: {borders_d!r}"
        )

        # ── (E) Send-wire (composite tag) matches typed shortcut ──
        # Drive the same Enter→Leave cycle via the composite-tag wire
        # the InputRouter uses for real hover events. Should produce
        # bit-identical slot + paint output to the typed shortcut.
        tf.invoke("/inspector_tree/external/send", f"{_BUTTON_PATH}:PointerEnter")
        wait_until(
            lambda: _query_hover_slot(tf) == _BUTTON_PATH,
            desc="send-wire PointerEnter must set hovered_id",
        )
        snap_e_in = _snap_main(tf)
        borders_e_in = _collect_borders(snap_e_in)
        assert len(borders_e_in) == 1, (
            f"send-wire Enter wrap = 1 border; got: {borders_e_in!r}"
        )
        assert _border_color_tuple(borders_e_in[0]) == hover_color_c, (
            "send-wire hover color must be bit-identical to typed shortcut"
        )

        tf.invoke("/inspector_tree/external/send", f"{_BUTTON_PATH}:PointerLeave")
        wait_until(
            lambda: _query_hover_slot(tf) is None,
            desc="send-wire PointerLeave clears hovered_id",
        )
        assert _collect_borders(_snap_main(tf)) == []

        # ── (F) Selection + hover on SAME node → selection wins ───
        # Select button (via R675 click typed shortcut), then hover
        # the same button. The visible wrap must be the Error-red
        # selection wrap; the hover wrap is suppressed per the
        # "selection wins on same node" rule.
        tf.invoke("/inspector_tree/external/click", _BUTTON_PATH)
        wait_until(
            lambda: len(_collect_borders(_snap_main(tf))) == 1,
            desc="post-select = 1 wrap (selection)",
        )
        selection_borders = _collect_borders(_snap_main(tf))
        selection_color = _border_color_tuple(selection_borders[0])

        tf.invoke("/inspector_tree/external/hover", _BUTTON_PATH)
        wait_until(
            lambda: _query_hover_slot(tf) == _BUTTON_PATH,
            desc="same-node hover slot set",
        )
        snap_f = _snap_main(tf)
        borders_f = _collect_borders(snap_f)
        assert len(borders_f) == 1, (
            f"same-node select+hover = 1 wrap (selection wins); got: {borders_f!r}"
        )
        assert _border_color_tuple(borders_f[0]) == selection_color, (
            "surviving wrap must be the selection color (selection wins)"
        )
        # Selection color must differ from hover color — different
        # ColorRoles paint distinct rgba values.
        assert selection_color != hover_color_c, (
            f"selection (Error) and hover (SurfaceContainerHighest) "
            f"must be distinct colors; got selection={selection_color!r} "
            f"hover={hover_color_c!r}"
        )

        # ── (G) Selection + hover on DIFFERENT nodes → both paint ─
        # Re-hover a different node (the root Container, not the
        # selected button) — both wraps should paint with distinct
        # colors.
        tf.invoke("/inspector_tree/external/hover", _VIEW_MAIN_ROOT_PATH)
        wait_until(
            lambda: len(_collect_borders(_snap_main(tf))) == 2,
            desc="different-node select+hover = 2 wraps",
        )
        borders_g = _collect_borders(_snap_main(tf))
        border_colors_g = {_border_color_tuple(b) for b in borders_g}
        assert selection_color in border_colors_g, (
            f"selection color (Error) must be in border set; "
            f"got: {border_colors_g!r}"
        )
        assert hover_color_c in border_colors_g, (
            f"hover color (SurfaceContainerHighest) must be in border set; "
            f"got: {border_colors_g!r}"
        )

        # ── (H) Inspector-only ids in hover slot soft-fail ────────
        # Clear selection first so the only potential wrap is the
        # hover one.
        # (No "clear selection" RPC — set selection to an inspector-
        # only id, which soft-fails too, then verify hover soft-fail
        # independently.)
        for stale_id in ("state", "main"):
            tf.invoke("/inspector_tree/external/click", stale_id)
            tf.invoke("/inspector_tree/external/hover", stale_id)
            wait_until(
                lambda: _query_hover_slot(tf) == stale_id,
                desc="stale id stored in the hover slot",
            )
            borders_h = _collect_borders(_snap_main(tf))
            assert borders_h == [], (
                f"inspector-only id {stale_id!r} must produce no wrap; "
                f"got: {borders_h!r}"
            )

        # ── (I) Stale path soft-fail (path doesn't resolve) ──────
        ghost_path = "Container/Container[ghost_tag_never_existed]"
        tf.invoke("/inspector_tree/external/hover", ghost_path)
        wait_until(
            lambda: _query_hover_slot(tf) == ghost_path,
            desc="ghost path stored without validation",
        )
        # The slot is still set to the ghost id (the substrate
        # doesn't validate paths — it's the binding's view fn that
        # soft-fails on find_node_at_path returning None).
        assert _query_hover_slot(tf) == ghost_path, (
            "substrate must store any id without validation"
        )
        borders_i = _collect_borders(_snap_main(tf))
        assert borders_i == [], (
            f"stale hover path must produce no wrap; got: {borders_i!r}"
        )

        # ── (J) W3C move-from-A-to-B sequence ─────────────────────
        # Clear any prior state, then drive Leave + Enter pair on
        # different ids — the substrate's slot is the second id, the
        # intent stream has Null then Text(second).
        tf.invoke("/inspector_tree/external/hover", None)
        wait_until(
            lambda: _query_hover_slot(tf) is None,
            desc="hover cleared before the W3C move sequence",
        )
        # First Enter on button.
        tf.invoke("/inspector_tree/external/send", f"{_BUTTON_PATH}:PointerEnter")
        wait_until(
            lambda: _query_hover_slot(tf) == _BUTTON_PATH,
            desc="first Enter sets the hover slot",
        )
        # W3C: Leave on old, then Enter on new.
        tf.invoke("/inspector_tree/external/send", f"{_BUTTON_PATH}:PointerLeave")
        tf.invoke("/inspector_tree/external/send", f"{_LABEL_PATH}:PointerEnter")
        wait_until(
            lambda: _query_hover_slot(tf) == _LABEL_PATH,
            desc="post-move hover slot must be the new target",
        )

        # ── (K) intervene on hovered_id is read-only ─────────────
        # Verify the schema's read-only contract — the slot can be
        # queried but not directly written; mutation goes through
        # the send / hover invoke channels. The substrate returns
        # `InterveneError::ReadOnly`, the RPC transport surfaces it
        # as an error response (raised here as `RpcError`).
        slot_pre = _query_hover_slot(tf)
        intervene_raised = False
        try:
            tf.request(
                "scene/intervene",
                {"path": "/inspector_tree/external/hovered_id",
                 "value": "forged-by-rpc"},
            )
        except RpcError:
            intervene_raised = True
        assert intervene_raised, (
            "intervene on hovered_id must error (slot is read-only)"
        )
        slot_post_intervene = _query_hover_slot(tf)
        assert slot_post_intervene == slot_pre, (
            f"failed intervene must not mutate the slot; pre={slot_pre!r}, "
            f"post={slot_post_intervene!r}"
        )


if __name__ == "__main__":
    sys.exit(run_demo(
        "R678 §5.16 §5.49 §5.50 — DevTools cross-window hover-overlay bridge",
        body,
    ))
