#!/usr/bin/env python3
"""R1173 §5.16 §2 #7 — FREELY COMPOSED per-panel chrome: a headerless locked toolbar.

SCOPE — read this before the assertions. pinion's per-panel policies are ORTHOGONAL
axes that a pro dock (Qt ADS, VS Code) lets the host compose freely: a panel can be
headerless (no title-bar handle — the menu strip IS the content), non-receiving
(`DockPanelStyle.drop_target=false` — refuses an incoming dock), and non-movable
(`DockPanelExternal.with_movable(false)` — opens no drag) all at once, while every
other panel keeps the full set. R1173 adds the missing seam: the dock WALKER
(`view_dock_surface_styled`) takes a per-panel style customizer, so the editor wires
`show_header`/`drop_target` PER PANEL (the R1172 move/float policy was already
per-panel on the input side). The editor composes its `toolbar` = headerless +
non-receiving + non-movable.

This pins the composition as §2 #7 scene-as-data: the toolbar has NO `toolbar#header`
tag while every other panel HAS its `{panel}#header` (the per-panel show_header wired
through the walker), and a movable panel DROPPED ONTO the toolbar is REFUSED (the
toolbar's `drop_target=false` is not a router drop target, so the panel floats instead
of docking into the toolbar). The live drag FEEL is HW-gated; this drives it via
`scene/drag` + observes the refusal as data.
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import RpcSubprocess, run_demo  # noqa: E402

_MAIN = "main"
_TOOLBAR = "toolbar"
_OUTLINER = "outliner"
_VIEWPORT = "viewport"
_PROPERTIES = "properties"
_CONSOLE = "console"
_HEADERFUL = {_OUTLINER, _VIEWPORT, _PROPERTIES, _CONSOLE}
_REORG = "/dock_reorganize/external"


def _scene(tf: RpcSubprocess, window: str) -> Any:
    result = tf.snapshot(source="paint", window=window, viewport=(1280, 800))
    assert result is not None, f"snapshot {window} must answer"
    return result.get("scene") if isinstance(result, dict) and "scene" in result else result


def _find(scene: Any, tag: str) -> Any:
    if not isinstance(scene, dict):
        return None
    if scene.get("tag") == tag:
        return scene
    for c in scene.get("children") or []:
        hit = _find(c, tag)
        if hit is not None:
            return hit
    if "content" in scene:
        return _find(scene["content"], tag)
    return None


def _has(scene: Any, tag: str) -> bool:
    return _find(scene, tag) is not None


def _windows(tf: RpcSubprocess) -> list[dict]:
    return tf.request("scene/windows", {}).result.get("windows") or []


def _window_ids(tf: RpcSubprocess) -> set[str]:
    return {w.get("id") for w in _windows(tf)}


def _torn(panel: str) -> str:
    return f"torn-{panel}"


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


def _tab_groups(tf: RpcSubprocess) -> list[set[str]]:
    """Every Tabs group in the topology, as sets of co-tabbed panel ids."""
    groups: list[set[str]] = []

    def walk(n: Any) -> None:
        if not isinstance(n, dict):
            return
        k = n.get("type")
        if k == "Tabs":
            groups.append(set(n.get("panels") or []))
        if k == "Split":
            walk(n.get("first"))
            walk(n.get("second"))

    walk(tf.query(f"{_REORG}/topology").get("root"))
    return groups


def _toolbar_tab_siblings(tf: RpcSubprocess) -> set[str]:
    """Panels the toolbar shares a Tabs group with (empty = lone leaf, never tabbed)."""
    sibs: set[str] = set()
    for g in _tab_groups(tf):
        if _TOOLBAR in g:
            sibs |= g - {_TOOLBAR}
    return sibs


def _center(rect: dict) -> tuple[float, float]:
    return rect["x"] + rect["w"] / 2.0, rect["y"] + rect["h"] / 2.0


def _section(label: str) -> None:
    print(f"[demo] -- {label}")


def body() -> None:
    with RpcSubprocess("hello-dock-panels-editor", boot_grace=2.0) as tf:
        scene = _scene(tf, _MAIN)

        # ── (A) the toolbar is HEADERLESS; every other panel keeps its header ─
        _section("A: per-panel show_header wired through the walker (toolbar headerless)")
        assert _has(scene, _TOOLBAR), "A.1 the toolbar panel root is present (tag 'toolbar')"
        assert not _has(scene, f"{_TOOLBAR}#header"), "A.2 ★the toolbar has NO header (headerless)"
        for panel in _HEADERFUL:
            assert _has(scene, f"{panel}#header"), f"A.3 {panel} KEEPS its header (per-panel, not global)"
        assert _panel_set(tf) == {_TOOLBAR, *_HEADERFUL}, "A.4 all 5 panels present"

        # ── (B) the toolbar REFUSES to host a dock (drop_target=false) ───────
        _section("B: drop_target=false wired through the walker — the toolbar hosts no tab")
        assert _toolbar_tab_siblings(tf) == set(), "B.1 the toolbar is a lone leaf at boot (no tabs)"
        toolbar_rect = _find(scene, _TOOLBAR)["rect"]
        cx, cy = _center(toolbar_rect)
        # Drag a MOVABLE panel's header squarely onto the toolbar. A receiving panel
        # would tab it in (section C proves the gesture docks); the toolbar is not a
        # router drop target, so it NEVER hosts the tab — the drop resolves to the
        # window's outer edge instead, never docking INTO the toolbar.
        tf.request(
            "scene/drag",
            {"window": _MAIN, "from_path": f"{_CONSOLE}#header", "to": {"x": cx, "y": cy}, "steps": 8},
        )
        assert _toolbar_tab_siblings(tf) == set(), "B.2 ★the toolbar hosted NO tab (it refused the dock)"
        assert {_TOOLBAR, _CONSOLE} not in _tab_groups(tf), "B.3 ★console never tabbed INTO the toolbar"
        assert _torn(_CONSOLE) not in _window_ids(tf), "B.4 console did not float (it found a valid edge)"
        assert _CONSOLE in _panel_set(tf), "B.5 console stayed in the main dock (outer edge, not the toolbar)"

        # ── (C) control — a RECEIVING inner panel DOES accept the same tab ───
        _section("C: control — the same centre-drop tabs INTO a receiving inner panel")
        scene_c = _scene(tf, _MAIN)
        vx, vy = _center(_find(scene_c, _VIEWPORT)["rect"])
        tf.request(
            "scene/drag",
            {"window": _MAIN, "from_path": f"{_PROPERTIES}#header", "to": {"x": vx, "y": vy}, "steps": 8},
        )
        assert {_VIEWPORT, _PROPERTIES} in _tab_groups(tf), (
            "C.1 ★properties TABBED into the receiving viewport — the gesture docks when ALLOWED, "
            "so the toolbar's refusal in B is the drop_target policy, not an inert gesture"
        )

        # ── (D) orthogonal composition — three locks hold at once ────────────
        _section("D: the toolbar is headerless + non-receiving + non-movable, all at once")
        scene2 = _scene(tf, _MAIN)
        assert not _has(scene2, f"{_TOOLBAR}#header"), "D.1 still headerless after the drops"
        assert tf.query(f"/{_TOOLBAR}/external/movable") is False, "D.2 non-movable (R1172 input policy)"
        assert tf.query(f"/{_TOOLBAR}/external/dragging") is False, "D.3 the toolbar never entered a drag"
        assert _toolbar_tab_siblings(tf) == set(), "D.4 still hosts no tab (non-receiving holds)"
        assert _TOOLBAR in _panel_set(tf), "D.5 the locked toolbar never left the dock"
        # A normal panel has the full set: header present, movable=true.
        assert _has(scene2, f"{_OUTLINER}#header"), "D.6 a normal panel keeps its header"
        assert tf.query(f"/{_OUTLINER}/external/movable") is True, "D.7 a normal panel is movable"

        # ── (E) integrity — deterministic reads ──────────────────────────────
        _section("E: integrity — panels conserved, reads deterministic")
        assert _TOOLBAR in _panel_set(tf), "E.1 the toolbar is intact"
        a = tf.query(f"{_REORG}/topology")
        b = tf.query(f"{_REORG}/topology")
        assert a == b, "E.2 back-to-back topology reads are identical"

        print("[demo] r1173_composed_locked_toolbar: all sections PASS (freely composed lock)")


if __name__ == "__main__":
    sys.exit(run_demo("r1173_composed_locked_toolbar", body))
