#!/usr/bin/env python3
"""R1206 §5.16 §5.51 §2 #7 — the toolbar is a FIXED app-frame, out of the dock.

SCOPE — read this before the assertions. Before R1206 the editor's toolbar was a
LOCKED dock LEAF inside the topology (R1172: headerless + non-receiving +
non-movable). But "fixed" was input-policy only — the toolbar still lived in the
DockTopology, so a full-span OUTER dock (`dock_panel_outer` → `split_root`)
wrapped the WHOLE root INCLUDING the toolbar, and a panel docked full-span-right
spanned UP past the fixed toolbar (the R1205 cost-no-object audit's structural
finding).

R1206 adopts the canonical 3-layer pro-tool structure — shell chrome / fixed
app-frame / dockable workspace: the toolbar LEFT the topology and is composed as
a fixed-height strip ABOVE the dock surface (`view_toolbar_frame`), so the
reorganizer manages ONLY the workspace. An OUTER dock now splits just the
workspace (below the toolbar), never reaching it. Built on the R1205 surface-rect
SSOT (`Scene::dock_surface_rect`): the dock area is the workspace region below the
toolbar, and the outer-dock band + redock preview measure against it for free.

  (A) Boot — one window; the WORKSPACE is 4 dock panels (toolbar is NOT one).
  (B) The toolbar is a fixed frame at the TOP (y=0); the dock surface sits BELOW
      it (its y clears the toolbar height).
  (C) The toolbar is NOT in the reorganizer topology (a fixed frame, not a panel).
  (D) Every workspace panel nests inside the dock surface (below the toolbar).
  (E) OUTER-dock a panel to the RIGHT → it becomes a right column that stays
      BELOW the toolbar (does NOT span up past it — the R1206 fix), and the dock
      surface rect is UNCHANGED (the workspace boundary held below the toolbar).
  (F) A BOTTOM outer dock likewise lands inside the workspace.
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
_WORKSPACE_PANELS = {_OUTLINER, _VIEWPORT, _PROPERTIES, _CONSOLE}


# ─── helpers ─────────────────────────────────────────────────────────


def _is_outer(tag: Any) -> bool:
    return isinstance(tag, str) and tag.endswith("outer-dock-zone")


def _is_dock_surface(tag: Any) -> bool:
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
        _section("A: boot — one window; the WORKSPACE is 4 dock panels (no toolbar)")
        wins = tf.request("scene/windows", {}).result.get("windows") or []
        assert len(wins) == 1, f"A.1 one window at boot; got {wins!r}"
        assert wins[0].get("id") == _MAIN, f"A.2 the window is main ({wins[0]!r})"
        assert _panel_set(tf) == _WORKSPACE_PANELS, f"A.3 workspace = 4 panels ({_panel_set(tf)})"

        # ── (B) toolbar frame ABOVE the dock surface ─────────────────
        _section("B: the toolbar is a fixed frame at the top; the dock surface is BELOW it")
        toolbar = _rect(tf, _TOOLBAR)
        surf = _surface_rect(tf)
        assert toolbar["y"] == 0, f"B.1 the toolbar frame is at the window top (y={toolbar['y']})"
        assert toolbar["w"] >= _MAIN_W - 2, f"B.2 it spans the full width (w={toolbar['w']})"
        assert 0 < toolbar["h"] <= 96, f"B.3 a slim fixed strip (h={toolbar['h']})"
        assert surf["y"] >= toolbar["h"] - 2, (
            f"B.4 ★the dock surface starts BELOW the toolbar (surf.y={surf['y']} >= tb.h={toolbar['h']})"
        )
        assert surf["x"] == 0 and surf["w"] >= _MAIN_W - 2, f"B.5 the surface spans full width ({surf!r})"
        # The toolbar frame and the dock surface do not overlap vertically.
        assert toolbar["y"] + toolbar["h"] <= surf["y"] + 2, "B.6 toolbar and surface are stacked, not overlapping"

        # ── (C) toolbar is NOT a dock panel ──────────────────────────
        _section("C: the toolbar is NOT in the reorganizer topology (a fixed frame, not a panel)")
        assert _TOOLBAR not in _panel_set(tf), "C.1 ★the toolbar is not a dock panel in the topology"
        assert len(_panel_set(tf)) == 4, f"C.2 exactly 4 dockable panels ({_panel_set(tf)})"

        # ── (D) every workspace panel nests in the dock surface ──────
        _section("D: every workspace panel nests inside the dock surface (below the toolbar)")
        layout = _layout(tf)
        for panel in _WORKSPACE_PANELS:
            prect = _find_rect(layout, panel)
            assert prect is not None, f"D.1 {panel} has a rect"
            assert prect["y"] >= toolbar["h"] - 2, f"D.2 ★{panel} sits below the toolbar (y={prect['y']})"
            assert _nested(prect, surf), f"D.3 {panel} nests inside the dock surface ({prect!r} ⊄ {surf!r})"

        # ── (E) OUTER dock RIGHT stays below the toolbar ─────────────
        _section("E: OUTER-dock outliner RIGHT → a right column BELOW the toolbar; surface stable")
        tf.drag(from_path=f"{_OUTLINER}#header", to_at=(1190.0, 400.0), phase="begin")
        prev = _drop_preview(tf, _OUTLINER)
        assert isinstance(prev, dict), f"E.1 a held drag in the right band has a preview ({prev!r})"
        assert _is_outer(prev.get("target")), f"E.2 the right edge previews the outer sentinel ({prev!r})"
        assert prev.get("zone") == "Right", f"E.3 the previewed edge is Right ({prev.get('zone')!r})"
        tf.drag(from_path=f"{_OUTLINER}#header", to_at=(1190.0, 400.0), phase="end")
        wait_until(
            lambda: _rect(tf, _OUTLINER)["x"] + _rect(tf, _OUTLINER)["w"] >= _MAIN_W - 8,
            desc="E.4 the outliner is now the flush-right column",
        )
        col = _rect(tf, _OUTLINER)
        assert col["x"] + col["w"] >= _MAIN_W - 8, f"E.5 flush right ({col!r})"
        assert col["y"] >= toolbar["h"] - 2, (
            f"E.6 ★the right column STAYS BELOW the toolbar (y={col['y']}), not spanning up past it"
        )
        assert col["y"] + col["h"] <= _MAIN_H + 2, f"E.7 and within the window ({col!r})"
        assert _surface_rect(tf) == surf, "E.8 ★the dock surface rect is unchanged (workspace boundary held)"
        assert _panel_set(tf) == _WORKSPACE_PANELS, "E.9 no panel lost — still 4"
        assert _TOOLBAR not in _panel_set(tf), "E.10 the toolbar is still not a dock panel after the reorg"

        # ── (F) OUTER dock BOTTOM lands inside the workspace ─────────
        _section("F: OUTER-dock outliner BOTTOM → a full-width bottom row inside the workspace")
        tf.drag(from_path=f"{_OUTLINER}#header", to_at=(600.0, 790.0), phase="begin")
        bprev = _drop_preview(tf, _OUTLINER)
        assert isinstance(bprev, dict) and _is_outer(bprev.get("target")), f"F.1 bottom band previews outer ({bprev!r})"
        assert bprev.get("zone") == "Bottom", f"F.2 the previewed edge is Bottom ({bprev.get('zone')!r})"
        tf.drag(from_path=f"{_OUTLINER}#header", to_at=(600.0, 790.0), phase="end")
        wait_until(
            lambda: _rect(tf, _OUTLINER)["w"] >= _MAIN_W - 8,
            desc="F.3 the outliner is now the full-width bottom row",
        )
        row = _rect(tf, _OUTLINER)
        assert row["w"] >= _MAIN_W - 8, f"F.4 full width ({row!r})"
        assert row["y"] >= toolbar["h"] - 2, f"F.5 ★still below the toolbar (y={row['y']})"
        assert _surface_rect(tf) == surf, "F.6 the dock surface rect is STILL unchanged across both docks"

        # ── (G) integrity + determinism ─────────────────────────────
        _section("G: integrity — panels survive, toolbar fixed, reads deterministic")
        assert _window_ids(tf) == {_MAIN}, "G.1 only main remains (no spurious floater)"
        assert _panel_set(tf) == _WORKSPACE_PANELS, "G.2 all 4 workspace panels intact"
        assert _rect(tf, _TOOLBAR)["y"] == 0, "G.3 the toolbar frame stayed pinned at the top through every dock"
        assert _surface_rect(tf) == _surface_rect(tf), "G.4 back-to-back surface reads are identical"
        assert _topology(tf) == _topology(tf), "G.5 back-to-back topology reads are identical"

        print("[demo] r1206_toolbar_fixed_frame: all sections PASS (toolbar is a fixed app-frame)")


if __name__ == "__main__":
    sys.exit(run_demo("r1206_toolbar_fixed_frame", body))
