#!/usr/bin/env python3
"""R1205 §5.51 §5.39 §2 #7 — the DOCK SURFACE RECT is the one dock-area SSOT.

SCOPE — read this before the assertions. R1202/R1203 taught the outer-dock band
and the redock preview where the dock area sits by stamping a per-window chrome-
height SCALAR (`dock_area_top_inset` / `inset_below_chrome`) — a TOP-only inset
that a fixed toolbar / menu / status bar (a future 2nd consumer) would be blind
to. R1205 retires the scalar for a RECT: the dock walker wraps its whole
workspace subtree in a `DOCK_SURFACE_TAG` container, and its laid-out rect IS the
dock area (`Scene::dock_surface_rect`). Wherever the composing view places that
surface — below a client-side chrome strip, a toolbar, inside a split — the
layout engine carries every inset for free, and the same-window OUTER band + the
cross-window redock preview both read the ONE rect, agreeing with zero wiring.

The editor main is OS-decorated (no chrome yet — that is R1205 item #4), so here
the dock surface == the whole window; this demo pins that the surface tag is
present, IS the dock area (encloses every panel), drives the same-window outer
band, and stays STABLE as reorganizes happen inside it. The chrome-inset case
(surface below a control strip) is pinned by the shell unit tests
(`r1205_outer_redock_preview_lands_on_the_dock_surface`) since the editor floater
path is HW-gated.

  (A) Boot — 5 panels, one main window.
  (B) The DOCK_SURFACE node is present in scene/layout and fills the workspace
      (== the window content rect, no chrome).
  (C) The surface IS the dock area: every panel's rect nests inside it.
  (D) The same-window OUTER band is measured against the surface — a bottom-edge
      drag previews the outer sentinel and lands the outliner full-width.
  (E) The surface rect is UNCHANGED by the reorganize — the workspace boundary is
      stable; the outer dock happened INSIDE the surface (non-tautological: the
      topology changed, the surface rect did not).
  (F) The right edge works too (the surface drives both axes).
  (G) Integrity + determinism.
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import RpcSubprocess, run_demo, wait_until  # noqa: E402

_MAIN = "main"
_MAIN_W = 1200
_MAIN_H = 800
_REORG = "/dock_reorganize/external"

_TOOLBAR = "toolbar"
_OUTLINER = "outliner"
_VIEWPORT = "viewport"
_PROPERTIES = "properties"
_CONSOLE = "console"
_ALL_PANELS = {_TOOLBAR, _OUTLINER, _VIEWPORT, _PROPERTIES, _CONSOLE}


# ─── helpers ─────────────────────────────────────────────────────────


def _is_outer(tag: Any) -> bool:
    return isinstance(tag, str) and tag.endswith("outer-dock-zone")


def _is_dock_surface(tag: Any) -> bool:
    # DOCK_SURFACE_TAG is a NUL-sentinel + "dock-surface"; match by suffix so the
    # demo never has to embed a NUL byte.
    return isinstance(tag, str) and tag.endswith("dock-surface")


def _topology(tf: RpcSubprocess) -> Any:
    return tf.query(f"{_REORG}/topology")


def _all_panels(node: Any) -> set[str]:
    if not isinstance(node, dict):
        return set()
    kind = node.get("type")
    if kind == "Leaf":
        return {node.get("panel_id")}
    if kind == "Tabs":
        return set(node.get("panels") or [])
    if kind == "Split":
        return _all_panels(node.get("first")) | _all_panels(node.get("second"))
    return set()


def _panel_set(tf: RpcSubprocess) -> set[str]:
    return _all_panels(_topology(tf).get("root"))


def _window_ids(tf: RpcSubprocess) -> set[str]:
    return {w.get("id") for w in (tf.request("scene/windows", {}).result.get("windows") or [])}


def _layout(tf: RpcSubprocess) -> Any:
    resp = tf.request("scene/layout", {"viewport": {"width": _MAIN_W, "height": _MAIN_H}})
    assert resp is not None and resp.result is not None, "scene/layout must answer"
    return resp.result


def _find_rect_by(layout: Any, pred: Any) -> Optional[dict[str, float]]:
    def walk(node: Any) -> Optional[dict[str, Any]]:
        if not isinstance(node, dict):
            return None
        if pred(node.get("tag")):
            r = node.get("rect")
            if isinstance(r, dict) and r.get("w", 0) > 0:
                return r
        for child in node.get("children") or []:
            found = walk(child)
            if found is not None:
                return found
        content = node.get("content")
        return walk(content) if isinstance(content, dict) else None

    rect = walk(layout)
    if not isinstance(rect, dict):
        return None
    return {k: float(rect.get(k, 0)) for k in ("x", "y", "w", "h")}


def _find_rect(layout: Any, tag: str) -> Optional[dict[str, float]]:
    return _find_rect_by(layout, lambda t: t == tag)


def _rect(tf: RpcSubprocess, tag: str) -> dict[str, float]:
    rect = _find_rect(_layout(tf), tag)
    assert rect is not None, f"{tag} rect must be present in scene/layout"
    return rect


def _surface_rect(tf: RpcSubprocess) -> dict[str, float]:
    rect = _find_rect_by(_layout(tf), _is_dock_surface)
    assert rect is not None, "the DOCK_SURFACE node must be present in scene/layout"
    return rect


def _nested(inner: dict[str, float], outer: dict[str, float], slack: float = 2.0) -> bool:
    return (
        inner["x"] >= outer["x"] - slack
        and inner["y"] >= outer["y"] - slack
        and inner["x"] + inner["w"] <= outer["x"] + outer["w"] + slack
        and inner["y"] + inner["h"] <= outer["y"] + outer["h"] + slack
    )


def _drop_preview(tf: RpcSubprocess, panel: str) -> Any:
    return tf.query(f"/{panel}/external/drop_preview")


def _section(label: str) -> None:
    print(f"[demo] -- {label}")


# ─── demo body ───────────────────────────────────────────────────────


def body() -> None:
    with RpcSubprocess("hello-dock-panels-editor", boot_grace=2.0) as tf:
        # ── (A) boot ─────────────────────────────────────────────────
        _section("A: boot — 5 panels, one main window")
        wins = tf.request("scene/windows", {}).result.get("windows") or []
        assert len(wins) == 1, f"A.1 one window at boot; got {wins!r}"
        assert wins[0].get("id") == _MAIN, f"A.2 the window is main ({wins[0]!r})"
        assert _panel_set(tf) == _ALL_PANELS, f"A.3 all 5 panels present ({_panel_set(tf)})"

        # ── (B) the DOCK_SURFACE node is the workspace rect ──────────
        _section("B: the DOCK_SURFACE node is present and fills the workspace")
        surf = _surface_rect(tf)
        assert surf["x"] == 0 and surf["y"] == 0, f"B.1 ★surface origin is the window top-left ({surf!r})"
        assert surf["w"] >= _MAIN_W - 2, f"B.2 ★surface fills the window width (w={surf['w']})"
        assert surf["h"] >= _MAIN_H - 2, f"B.3 ★surface fills the window height (h={surf['h']})"
        assert surf["w"] <= _MAIN_W + 2 and surf["h"] <= _MAIN_H + 2, (
            f"B.4 and no more than the window (no chrome inset here) ({surf!r})"
        )

        # ── (C) the surface IS the dock area (encloses every panel) ──
        _section("C: every panel nests inside the dock surface")
        layout = _layout(tf)
        for panel in _ALL_PANELS:
            prect = _find_rect(layout, panel)
            assert prect is not None, f"C.1 {panel} has a rect"
            assert _nested(prect, surf), f"C.2 ★{panel} nests inside the dock surface ({prect!r} ⊄ {surf!r})"

        # ── (D) the outer band is measured against the surface ───────
        _section("D: same-window OUTER dock (bottom) previews the sentinel + lands full-width")
        tf.drag(from_path=f"{_OUTLINER}#header", to_at=(600.0, 790.0), phase="begin")
        prev = _drop_preview(tf, _OUTLINER)
        assert isinstance(prev, dict), f"D.1 a held drag in the bottom band has a preview ({prev!r})"
        assert _is_outer(prev.get("target")), f"D.2 ★the outer sentinel — the band read the surface ({prev.get('target')!r})"
        assert prev.get("zone") == "Bottom", f"D.3 the previewed edge is Bottom ({prev.get('zone')!r})"
        tf.drag(from_path=f"{_OUTLINER}#header", to_at=(600.0, 790.0), phase="end")
        wait_until(
            lambda: _rect(tf, _OUTLINER)["w"] >= _MAIN_W - 8,
            desc="D.4 the outliner now spans the full width",
        )
        row = _rect(tf, _OUTLINER)
        assert row["w"] >= _MAIN_W - 8, f"D.5 ★it spans the full width (w={row['w']})"
        assert row["y"] >= _MAIN_H * 0.5, f"D.6 it is the BOTTOM row (y={row['y']})"
        assert _panel_set(tf) == _ALL_PANELS, "D.7 the outer dock is a move — no panel lost"

        # ── (E) the surface rect is UNCHANGED by the reorganize ──────
        _section("E: the workspace boundary is stable — the reorganize happened INSIDE the surface")
        surf_after = _surface_rect(tf)
        assert surf_after == surf, f"E.1 ★the dock surface rect is unchanged ({surf_after!r} != {surf!r})"
        # Non-tautological: the topology DID change (outliner is now full-width) —
        # only the surface boundary held.
        row2 = _rect(tf, _OUTLINER)
        assert row2["w"] >= _MAIN_W - 8, "E.2 the topology changed (outliner full-width) though the surface did not"
        assert _nested(row2, surf_after), "E.3 the new full-width row still nests in the (stable) surface"

        # ── (F) the right edge works too (both axes off one rect) ────
        _section("F: OUTER dock the RIGHT edge → a right column, surface still stable")
        tf.drag(from_path=f"{_OUTLINER}#header", to_at=(1190.0, 400.0), phase="begin")
        dprev = _drop_preview(tf, _OUTLINER)
        assert isinstance(dprev, dict), f"F.1 a held drag in the right band has a preview ({dprev!r})"
        assert _is_outer(dprev.get("target")), f"F.2 ★the right edge previews the outer sentinel too ({dprev!r})"
        assert dprev.get("zone") == "Right", f"F.3 the previewed edge is Right ({dprev.get('zone')!r})"
        tf.drag(from_path=f"{_OUTLINER}#header", to_at=(1190.0, 400.0), phase="end")
        wait_until(
            lambda: _rect(tf, _OUTLINER)["h"] >= 600,
            desc="F.4 the outliner now spans the full height (a right column)",
        )
        col = _rect(tf, _OUTLINER)
        assert col["x"] + col["w"] >= _MAIN_W - 8, f"F.5 it is flush right ({col!r})"
        assert col["h"] >= 600, f"F.6 ★it spans the full height (h={col['h']})"
        assert _surface_rect(tf) == surf, "F.7 ★the dock surface rect is STILL unchanged across both docks"

        # ── (G) integrity + determinism ──────────────────────────────
        _section("G: integrity — panels survive, reads deterministic")
        assert _window_ids(tf) == {_MAIN}, "G.1 only main remains (no spurious floater)"
        assert _panel_set(tf) == _ALL_PANELS, "G.2 all 5 panels intact after every move"
        assert _surface_rect(tf) == _surface_rect(tf), "G.3 back-to-back surface reads are identical"
        assert _topology(tf) == _topology(tf), "G.4 back-to-back topology reads are identical"

        print("[demo] r1205_dock_surface_rect: all sections PASS (dock surface rect is the one dock-area SSOT)")


if __name__ == "__main__":
    sys.exit(run_demo("r1205_dock_surface_rect", body))
