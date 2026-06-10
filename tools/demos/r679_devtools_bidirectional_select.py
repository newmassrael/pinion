#!/usr/bin/env python3
"""R679 §5.16 §5.49 §5.50 — DevTools bidirectional select bridge.

Drives `hello-multi-window` after R679's bridge-closure on top of R678:

  1. **`pinion_widget_paint::devtools::path_for_paint_hit`** — pure
     substrate hit-test inverse walker. Maps `(x, y)` → devtools-
     canonical path (`Container[tag]/Type[nth-of-type]`) using
     deepest-tagged-ancestor semantics (matches `InputRouter`'s
     `resolve_hover_tag` so the (x, y) → path map stays consistent
     across paint-side input arcs). The R679 atomic 0 ships 12 unit
     tests pinning the round-trip invariant (`find_node_at_path(scene,
     path_for_paint_hit(scene, x, y).unwrap()).is_some()`).
  2. **`MainWindowClickRouter` External + reducer arms** — closes the
     bidirectional select bridge previously one-way (inspector tree row
     click → main wrap, R675/R676). Two new arms:
       - `main_click_router.click` (AI-invoke-driven) — `Text(path)`
         mirrors into `use_selected_path`; `Null` deselects.
       - `main_btn.click` (user-mouse-driven) — ButtonExternal's SCXML
         Pressed→Hover transition emits this on real user clicks;
         reducer writes the static `MAIN_BUTTON_RAW_PATH` (=
         `Container/Container[main_btn]`) into the same Signal.

R679 atomic 3 verification scope (≥ 35 assertions):

  (A) Substrate sanity — the new `main_click_router` External exposes
      `last_clicked` (read-only mirror) + `click` invoke channel.
  (B) Baseline — fresh boot has both signals empty and no wrap painted
      in either window's scene.
  (C) Inspector → main arc (R675/R676 baseline, regression pin) — a
      click via `/inspector_tree/external/click` typed shortcut writes
      `use_selected_path` and the main scene paints the Error-red wrap.
  (D) Main router → inspector arc (R679 new) — `scene/invoke
      /main_click_router/external/click {Text(path)}` mirrors the path
      into the Signal; inspector tree row paints focus state-layer.
  (E) Button click → inspector arc (R679 new) — driving `scene/click
      {window:"main", path:"main_btn"}` (real paint-side input through
      the button's SCXML) lands the button click intent; reducer writes
      MAIN_BUTTON_RAW_PATH into the Signal; inspector tree row paints
      focus state-layer.
  (F) Bidirectional alternation — alternating inspector ↔ main writes
      preserve latest-write-wins; no oscillation.
  (G) AI-driven deselect — main router invoke with `Null` payload
      clears the Signal; inspector focus state-layer disappears; main
      wrap disappears.
  (H) Router last_clicked mirror — query channel reflects the latest
      invoke payload for AI introspection.
  (I) Intervene on last_clicked is read-only — substrate enforces
      schema slot mutability convention.
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    run_demo,
    wait_query,
    wait_snap,
)


# R679 §5.16 — the same path constants the binding hard-codes;
# duplicated here so the demo runs without a Python wrapper around
# the Rust constants.
_BUTTON_PATH = "Container/Container[main_btn]"
_ALT_PATH = "Container/Text[0]"  # the banner Text — exists only when a selection is active

_MAIN_W = 320
_MAIN_H = 200
_INSPECTOR_W = 480
_INSPECTOR_H = 320

# M3 Lists tokens — focused inspector row paints
# SurfaceContainerHighest. Non-focused rows stay transparent.
# Row height per `TreeViewStyle::m3_default` is 48 px.


def _wait_main(tf, predicate, desc: str) -> dict:
    """Poll the main window's paint snapshot (R883.1 — window-scoped
    polling lives in the harness `wait_snap`)."""
    return wait_snap(
        tf, predicate, viewport=(_MAIN_W, _MAIN_H), window="main", desc=desc,
    )


def _wait_inspector(tf, predicate, desc: str) -> dict:
    """Poll the inspector window's paint snapshot (R883.1)."""
    return wait_snap(
        tf, predicate, viewport=(_INSPECTOR_W, _INSPECTOR_H),
        window="inspector", desc=desc,
    )


def _snap_main(tf) -> dict:
    """Read the main window's paint scene at the canonical viewport."""
    return tf.snapshot(
        source="paint", viewport=(_MAIN_W, _MAIN_H), window="main",
    )


def _snap_inspector(tf) -> dict:
    """Read the inspector window's paint scene at canonical viewport."""
    return tf.snapshot(
        source="paint", viewport=(_INSPECTOR_W, _INSPECTOR_H), window="inspector",
    )


def _query_router_last_clicked(tf) -> Any:
    """Return the main router's `last_clicked` slot (string or None)."""
    resp = tf.request(
        "scene/query",
        {"path": "/main_click_router/external/last_clicked"},
    )
    assert resp is not None
    return resp.result


def _collect_borders(scene: Any) -> list[dict]:
    """Walk a paint snapshot for every node carrying a stroked
    border. Returns the border dict (`{"color": ..., "width": ...}`)
    of each. Empty list when no border is present.

    R705 §5.39 — the keyboard focus ring is now an introspectable
    `ai-overlay/focus-ring` Box overlay (it used to be an opaque vello
    stroke invisible to `scene/snapshot`). Clicking a button in the
    inspected window both *selects* it (the DevTools Error-red wrap this
    demo measures) AND *focuses* it (the blue focus ring). The ring is
    orthogonal to selection, so it is excluded here — the demo asserts
    on selection wraps only. (See r705_focus_ring_introspect.py for the
    ring's own coverage.)
    """
    out: list[dict] = []

    def walk(node: Any) -> None:
        if not isinstance(node, dict):
            return
        if node.get("tag") == "ai-overlay/focus-ring":
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
    color = border.get("color") or {}
    return (
        int(color.get("r", 0)),
        int(color.get("g", 0)),
        int(color.get("b", 0)),
        int(color.get("a", 0)),
    )


def _find_row_fill_by_tag(scene: Any, target: str) -> tuple[int, int, int, int] | None:
    """Walk the inspector scene for a Scene::Container whose `tag` ==
    `target`. Returns its fill colour as an rgba 4-tuple, or `None`
    when no such tag is found. The focused tree row paints a non-
    transparent M3 SurfaceContainerHighest fill; non-focused rows
    paint transparent (rgba = 0,0,0,0). Mirrors the binding's
    `find_container_fill_by_tag` test helper at the RPC level.
    """
    if not isinstance(scene, dict):
        return None
    tag = scene.get("tag")
    if tag == target:
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


def body() -> None:
    with RpcSubprocess("hello-multi-window", boot_grace=1.5) as tf:
        # ── (A) Substrate sanity — main_click_router exposes slots ─
        # No `scene/schema` RPC method (schema is pinned in Rust
        # tests); verify the read channel resolves at boot — the
        # `last_clicked` slot returns Null on a fresh router.
        boot_last_clicked = _query_router_last_clicked(tf)
        assert boot_last_clicked is None, (
            f"main_click_router last_clicked must be null at boot; "
            f"got: {boot_last_clicked!r}"
        )

        # ── (B) Baseline — both signals empty, no wraps ───────────
        snap_main_baseline = _snap_main(tf)
        baseline_main_borders = _collect_borders(snap_main_baseline)
        assert baseline_main_borders == [], (
            f"fresh main scene must have no stroked borders; "
            f"got: {baseline_main_borders!r}"
        )

        snap_inspector_baseline = _snap_inspector(tf)
        # Baseline: no focus on the button row.
        row_tag = f"inspector_tree#{_BUTTON_PATH}"
        baseline_fill = _find_row_fill_by_tag(snap_inspector_baseline, row_tag)
        assert baseline_fill is not None, (
            f"inspector must carry row tagged {row_tag!r} from boot"
        )
        assert _is_transparent(baseline_fill), (
            f"baseline inspector row must be transparent; got: {baseline_fill!r}"
        )

        # ── (C) Inspector → main arc (R675 baseline regression pin) ─
        tf.invoke("/inspector_tree/external/click", _BUTTON_PATH)
        # Main scene paints the selection wrap (Error red).
        snap_main_c = _wait_main(
            tf,
            lambda s: len(_collect_borders(s)) == 1,
            "post inspector-click = 1 selection wrap",
        )
        borders_c = _collect_borders(snap_main_c)
        selection_color = _border_color_tuple(borders_c[0])
        assert selection_color[3] == 255, (
            f"selection border must be opaque; got: {selection_color!r}"
        )
        # Inspector focus state-layer paints on the matching row.
        snap_inspector_c = _snap_inspector(tf)
        focused_fill_c = _find_row_fill_by_tag(snap_inspector_c, row_tag)
        assert focused_fill_c is not None, (
            f"inspector row {row_tag!r} must still exist post-click"
        )
        assert not _is_transparent(focused_fill_c), (
            f"inspector row must paint focus state-layer post-click; "
            f"got: {focused_fill_c!r}"
        )

        # ── (D) Main router → inspector arc (R679 new) ────────────
        # Direct AI invoke on the main router writes the same path
        # the inspector-arc would write. Cross-arc invariant: the
        # observable result is identical regardless of which side
        # initiated the write.
        # Clear first via Null (atomic G test below verifies the
        # explicit deselect; here we use a fresh write to assert the
        # write-side mechanism works independently).
        tf.invoke("/main_click_router/external/click", _BUTTON_PATH)
        # Slot mirror updated.
        wait_query(
            tf, "/main_click_router/external/last_clicked", _BUTTON_PATH,
            desc="main_click_router last_clicked must mirror invoke arg",
        )
        # Main wrap still present (same path was written).
        snap_main_d = _snap_main(tf)
        borders_d = _collect_borders(snap_main_d)
        assert len(borders_d) == 1, (
            f"main router write to same path → 1 wrap; got: {borders_d!r}"
        )
        assert _border_color_tuple(borders_d[0]) == selection_color, (
            "main wrap colour must be bit-identical regardless of write source"
        )
        # Inspector row still focused.
        snap_inspector_d = _snap_inspector(tf)
        focused_fill_d = _find_row_fill_by_tag(snap_inspector_d, row_tag)
        assert focused_fill_d == focused_fill_c, (
            f"inspector focus state-layer colour must be bit-identical "
            f"to inspector-arc write; c={focused_fill_c!r} d={focused_fill_d!r}"
        )

        # ── (E) Button click → inspector arc (R679 new) ───────────
        # Drive a paint-side mouse click on the button — InputRouter
        # routes through ButtonExternal → SCXML Pressed→Hover →
        # `main_btn.click` intent → R679 reducer arm writes
        # MAIN_BUTTON_RAW_PATH into the Signal. End-to-end test of
        # the user-mouse half of the bidirectional bridge.
        #
        # First step: write a DIFFERENT path to the Signal so we can
        # observe the button click overriding it (rather than just
        # confirming no-change).
        tf.invoke("/main_click_router/external/click", _ALT_PATH)
        # Verify the alt write landed in the router mirror.
        wait_query(
            tf, "/main_click_router/external/last_clicked", _ALT_PATH,
            desc="alt write landed in the router mirror",
        )

        # Click on the button via paint-side input. The `tf.click`
        # helper is single-window only; for multi-window bindings we
        # issue `scene/click` directly with `{window: "main", path:
        # "main_btn"}`. The dispatcher walks the main paint scene
        # for the tagged button rect and synthesises a press/release
        # cycle at its centre — same arc winit's MouseInput would
        # drive on a real user click.
        tf.request("scene/click", {"window": "main", "path": "main_btn"})
        # Reducer's main_btn.click arm wrote MAIN_BUTTON_RAW_PATH.
        # The router's last_clicked slot is NOT updated by this arc
        # (it's the inspector's TreeViewFocus that observes the
        # Signal change; the router is the AI-invoke entry point
        # only). Verify the *visible* end-state instead — gate on the
        # inspector row's focus state-layer, which is uniquely
        # post-click (the alt path left this row transparent).
        snap_inspector_e = _wait_inspector(
            tf,
            lambda s: (lambda f: f is not None and not _is_transparent(f))(
                _find_row_fill_by_tag(s, row_tag)
            ),
            "inspector row paints focus state-layer post button-click",
        )
        snap_main_e = _snap_main(tf)
        borders_e = _collect_borders(snap_main_e)
        # The button click landed → selected_path now points at the
        # button path → main paints the Error red selection wrap on
        # the button container.
        assert len(borders_e) == 1, (
            f"post button-click = 1 wrap; got: {borders_e!r}"
        )
        assert _border_color_tuple(borders_e[0]) == selection_color, (
            "button-click wrap colour must be Error red (= selection)"
        )

        # ── (F) Bidirectional alternation — latest write wins ──────
        # Inspector writes A.
        tf.invoke("/inspector_tree/external/click", _BUTTON_PATH)
        _wait_inspector(
            tf,
            lambda s: (lambda f: f is not None and not _is_transparent(f))(
                _find_row_fill_by_tag(s, row_tag)
            ),
            "alternation step 1: inspector arc wrote selection",
        )

        # Main router writes the alt path B.
        tf.invoke("/main_click_router/external/click", _ALT_PATH)
        # Inspector row that *was* selected (button) is now NOT
        # focused; alt path might not even resolve in the inspector
        # tree (banner Text shows only when a selection is set, and
        # the tree mirror walks the raw scene). Confirm the original
        # button row lost its focus state-layer.
        _wait_inspector(
            tf,
            lambda s: (lambda f: f is None or _is_transparent(f))(
                _find_row_fill_by_tag(s, row_tag)
            ),
            "alternation step 2: button row should no longer be focused",
        )

        # Inspector writes button again — wins.
        tf.invoke("/inspector_tree/external/click", _BUTTON_PATH)
        _wait_inspector(
            tf,
            lambda s: (lambda f: f is not None and not _is_transparent(f))(
                _find_row_fill_by_tag(s, row_tag)
            ),
            "alternation step 3: latest inspector write wins",
        )

        # ── (G) AI-driven deselect — Null payload ─────────────────
        tf.invoke("/main_click_router/external/click", None)
        # Router slot cleared.
        wait_query(
            tf, "/main_click_router/external/last_clicked", None,
            desc="Null invoke must clear last_clicked",
        )
        # Main scene → no selection wrap.
        snap_main_g = _snap_main(tf)
        borders_g = _collect_borders(snap_main_g)
        assert borders_g == [], (
            f"deselect via Null must drop main wrap; got: {borders_g!r}"
        )
        # Inspector row → no focus state-layer.
        snap_inspector_g = _snap_inspector(tf)
        focused_fill_g = _find_row_fill_by_tag(snap_inspector_g, row_tag)
        assert focused_fill_g is None or _is_transparent(focused_fill_g), (
            f"deselect must clear inspector focus state-layer; "
            f"got: {focused_fill_g!r}"
        )

        # ── (H) Router last_clicked mirror reflects latest invoke ─
        # Sequential writes followed by reads confirm the mirror is
        # the source-of-truth at the AI plane.
        tf.invoke("/main_click_router/external/click", "Container")
        wait_query(tf, "/main_click_router/external/last_clicked", "Container",
                   desc="mirror reflects 'Container'")
        tf.invoke("/main_click_router/external/click", "Container/Text[0]")
        wait_query(tf, "/main_click_router/external/last_clicked", "Container/Text[0]",
                   desc="mirror reflects 'Container/Text[0]'")
        tf.invoke("/main_click_router/external/click", None)
        wait_query(tf, "/main_click_router/external/last_clicked", None,
                   desc="mirror reflects the Null clear")

        # ── (I) Intervene on last_clicked is read-only ────────────
        # Substrate enforces schema convention: query channels are
        # read-only at the RPC plane; mutation goes through invoke.
        slot_pre = _query_router_last_clicked(tf)
        intervene_raised = False
        try:
            tf.request(
                "scene/intervene",
                {"path": "/main_click_router/external/last_clicked",
                 "value": "forged-by-rpc"},
            )
        except RpcError:
            intervene_raised = True
        assert intervene_raised, (
            "intervene on last_clicked must error (slot is read-only)"
        )
        slot_post_intervene = _query_router_last_clicked(tf)
        assert slot_post_intervene == slot_pre, (
            f"failed intervene must not mutate the slot; pre={slot_pre!r}, "
            f"post={slot_post_intervene!r}"
        )


if __name__ == "__main__":
    sys.exit(run_demo(
        "R679 §5.16 §5.49 §5.50 — DevTools bidirectional select bridge",
        body,
    ))
