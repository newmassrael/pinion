#!/usr/bin/env python3
"""R1173 §5.16 §2 #7 — per-panel PAINT chrome via the walker style customizer.

SCOPE — read this before the assertions. The dock WALKER
(`view_dock_surface_styled`) takes a per-panel style customizer, so a host can
compose panel chrome PER PANEL — a taller header, a suppressed header, a
non-receiving drop target — while every other panel keeps the default. R1173
introduced this seam; it was first demonstrated on the editor's toolbar.

R1206 made the toolbar a fixed app-frame OUTSIDE the dock (it is no longer a dock
panel), so this demo was REPOINTED (the user's directive) onto a real workspace
panel: the editor composes its PROPERTIES panel as a "pinned inspector" with a
TALLER header via the customizer, while the outliner / viewport / console keep
the `DockPanelStyle::m3_default` 28px header. The properties panel stays a fully
normal, movable, receiving dock panel — ONLY its header height is customized — so
this is the pure per-panel PAINT-customizer seam as observable data (§2 #7). The
per-panel INPUT lock (`with_movable` / `with_floatable`, the retired toolbar's
other axes) stays unit-tested in `pinion-widget-paint` — locking a shared editor
panel would break the many demos that drag / tear it off.

  (A) Boot — the 4-panel workspace (toolbar is a fixed frame, not a dock panel).
  (B) The PROPERTIES header is TALLER than the default (the customizer fired).
  (C) The customizer is PER-PANEL — outliner / viewport / console keep the 28px
      default header; only properties differs.
  (D) Properties is otherwise a NORMAL dock panel — it HAS a header (a drag
      handle) and content, and it is a live leaf in the topology.
  (E) Determinism.
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import RpcSubprocess, run_demo  # noqa: E402

_MAIN = "main"
_MAIN_W = 1200
_MAIN_H = 800
_REORG = "/dock_reorganize/external"

_OUTLINER = "outliner"
_VIEWPORT = "viewport"
_PROPERTIES = "properties"
_CONSOLE = "console"
_WORKSPACE = {_OUTLINER, _VIEWPORT, _PROPERTIES, _CONSOLE}

# Mirrored from the editor binding: the default `DockPanelStyle::m3_default`
# header (28px) vs the properties "pinned inspector" customized header (40px).
_DEFAULT_HEADER_PX = 28
_INSPECTOR_HEADER_PX = 40


def _layout(tf: RpcSubprocess) -> Any:
    resp = tf.request("scene/layout", {"viewport": {"width": _MAIN_W, "height": _MAIN_H}})
    assert resp is not None and resp.result is not None, "scene/layout must answer"
    return resp.result


def _find(scene: Any, tag: str) -> Optional[dict]:
    if not isinstance(scene, dict):
        return None
    if scene.get("tag") == tag:
        return scene
    for c in scene.get("children") or []:
        hit = _find(c, tag)
        if hit is not None:
            return hit
    content = scene.get("content")
    return _find(content, tag) if isinstance(content, dict) else None


def _header_h(layout: Any, panel: str) -> int:
    node = _find(layout, f"{panel}#header")
    assert node is not None, f"{panel}#header must be present"
    rect = node.get("rect") or {}
    return int(rect.get("h", 0))


def _panel_set(tf: RpcSubprocess) -> set[str]:
    def walk(n: Any) -> set[str]:
        if not isinstance(n, dict):
            return set()
        k = n.get("type")
        if k == "Leaf":
            return {n.get("panel_id")}
        if k == "Tabs":
            return set(n.get("panels") or [])
        if k == "Split":
            return walk(n.get("first")) | walk(n.get("second"))
        return set()

    return walk(tf.query(f"{_REORG}/topology").get("root"))


def _section(label: str) -> None:
    print(f"[demo] -- {label}")


def body() -> None:
    with RpcSubprocess("hello-dock-panels-editor", boot_grace=2.0) as tf:
        # ── (A) boot ─────────────────────────────────────────────────
        _section("A: boot — the 4-panel workspace (toolbar is a fixed frame)")
        wins = tf.request("scene/windows", {}).result.get("windows") or []
        assert len(wins) == 1, f"A.1 one window; got {wins!r}"
        assert wins[0].get("id") == _MAIN, f"A.2 the window is main ({wins[0]!r})"
        assert _panel_set(tf) == _WORKSPACE, f"A.3 the workspace is 4 panels ({_panel_set(tf)})"
        assert "toolbar" not in _panel_set(tf), "A.4 the toolbar is not a dock panel"

        # ── (B) the properties header is taller (customizer fired) ───
        _section("B: the PROPERTIES header is TALLER than the default")
        layout = _layout(tf)
        props_h = _header_h(layout, _PROPERTIES)
        assert props_h == _INSPECTOR_HEADER_PX, (
            f"B.1 ★the properties header is the customized {_INSPECTOR_HEADER_PX}px (got {props_h})"
        )
        assert props_h > _DEFAULT_HEADER_PX, f"B.2 taller than the {_DEFAULT_HEADER_PX}px default"

        # ── (C) the customizer is PER-PANEL ─────────────────────────
        _section("C: the customizer is per-panel — only properties differs")
        for panel in (_OUTLINER, _VIEWPORT, _CONSOLE):
            h = _header_h(layout, panel)
            assert h == _DEFAULT_HEADER_PX, (
                f"C.{panel} ★{panel} keeps the {_DEFAULT_HEADER_PX}px default header (got {h})"
            )
            assert h < props_h, f"C.{panel}.lt {panel} header ({h}) < properties ({props_h})"

        # ── (D) properties is otherwise a NORMAL dock panel ─────────
        _section("D: properties is a normal movable/receiving panel — only its header is taller")
        assert _find(layout, f"{_PROPERTIES}#header") is not None, "D.1 properties HAS a header (a drag handle)"
        assert _find(layout, f"{_PROPERTIES}#content") is not None, "D.2 properties has a content region"
        assert _PROPERTIES in _panel_set(tf), "D.3 properties is a live leaf in the topology"
        # A dockable panel — it is NOT the retired non-receiving toolbar.
        assert _find(layout, _PROPERTIES) is not None, "D.4 the properties panel root is present"

        # ── (E) determinism ─────────────────────────────────────────
        _section("E: determinism")
        l2 = _layout(tf)
        assert _header_h(l2, _PROPERTIES) == props_h, "E.1 back-to-back properties header height is stable"
        assert _panel_set(tf) == _panel_set(tf), "E.2 back-to-back topology reads are identical"

        print("[demo] r1173_per_panel_style_customizer: all sections PASS (per-panel header customizer)")


if __name__ == "__main__":
    sys.exit(run_demo("r1173_per_panel_style_customizer", body))
