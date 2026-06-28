#!/usr/bin/env python3
"""R1134 §5.51.1 — editor torn-slot FloatPolicy: collapse vs placeholder.

SCOPE — read this before the assertions. A live dock review asked for COLLAPSE
(VS Code / Blender: tearing a panel off makes its slot VANISH so neighbours
reclaim the space) alongside the existing PLACEHOLDER policy (the slot stays,
painting a "(torn off)" placeholder, no reflow) — and BOTH selectable. R1134 adds
`FloatPolicy{Placeholder,Collapse}` to the shared `DockReorganizer` (the R1112
`tabbing` precedent: one per-surface policy uniform across pointer + AI + cross-
window), runtime-togglable via the `set_float_policy` invoke + readable via
`query("float_policy")`. Under Collapse a float REMOVES the leaf
(`float_out_panel`, capturing the R1132 home anchor) and a dock-back RESTORES it
home (`restore_panel_home`); the external factory's (topology ∪ floating) union
keeps the floated panel's `DockPanelExternal` registered while its leaf is gone.

The DRIVER is the AI-primary `invoke`: `set_float_policy` on the coordinator +
`tear_off` (toggle) on the panel. The proof is AI-observable scene-as-data: the
live topology JSON shows viewport's leaf GONE under collapse (the reflow) vs
PRESENT under placeholder, and the main scene paints a placeholder only under
placeholder.

Section roadmap (>=30 assertions across A-F):

  (A) Boot — one window, 5 panels docked, the DEFAULT policy is placeholder
      (the coordinator + a panel both report it); viewport sits beside properties.
  (B) Toggle to collapse — set_float_policy("collapse"), read back on both the
      coordinator and the panel.
  (C) Collapse tear-off — tear off viewport: its leaf VANISHES from the topology
      (4 panels — the headline reflow), NO placeholder is painted, a floating
      window exists, the chart is Floating, the other 4 panels are intact.
  (D) Dock back home — tear off viewport again: it returns to the topology at its
      HOME anchor (5 panels, beside properties), the floating window is gone, the
      chart is Docked, still no placeholder.
  (E) Contrast placeholder — set_float_policy("placeholder"), tear off outliner:
      its leaf STAYS (5 panels, a placeholder IS painted), then dock it back.
  (F) Integrity — all 5 panels survive; float_policy is read-only to intervene.
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import RpcError, RpcSubprocess, run_demo, wait_until  # noqa: E402

_TOOLBAR = "toolbar"
_OUTLINER = "outliner"
_VIEWPORT = "viewport"
_PROPERTIES = "properties"
_CONSOLE = "console"
_ALL_PANELS = {_TOOLBAR, _OUTLINER, _VIEWPORT, _PROPERTIES, _CONSOLE}
_MAIN = "main"
_MAIN_W = 1200
_MAIN_H = 800
_REORG = "/dock_reorganize/external"


# ─── helpers ─────────────────────────────────────────────────────────


def _windows(tf: RpcSubprocess) -> list[dict]:
    resp = tf.request("scene/windows", {})
    assert resp is not None and resp.result is not None, "scene/windows must answer"
    return resp.result.get("windows") or []


def _window_ids(tf: RpcSubprocess) -> set[str]:
    return {w.get("id") for w in _windows(tf)}


def _torn(panel: str) -> str:
    return f"torn-{panel}"


def _topology(tf: RpcSubprocess) -> Any:
    return tf.query(f"{_REORG}/topology")


def _root(topo: Any) -> Any:
    return topo.get("root") if isinstance(topo, dict) else None


def _float_policy(tf: RpcSubprocess) -> Any:
    return tf.query(f"{_REORG}/float_policy")


def _panel_policy(tf: RpcSubprocess, panel: str) -> Any:
    return tf.query(f"/{panel}/external/float_policy")


def _lifecycle(tf: RpcSubprocess, panel: str) -> Any:
    return tf.query(f"/{panel}/external/lifecycle")


def _is_leaf(node: Any, panel: str) -> bool:
    return isinstance(node, dict) and node.get("type") == "Leaf" and node.get("panel_id") == panel


def _siblings_in_split(node: Any, x: str, y: str) -> bool:
    if not isinstance(node, dict) or node.get("type") != "Split":
        return False
    f, s = node.get("first"), node.get("second")
    if (_is_leaf(f, x) and _is_leaf(s, y)) or (_is_leaf(f, y) and _is_leaf(s, x)):
        return True
    return _siblings_in_split(f, x, y) or _siblings_in_split(s, x, y)


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


def _main_scene(tf: RpcSubprocess) -> Any:
    return tf.snapshot(source="paint", viewport=(_MAIN_W, _MAIN_H), window=_MAIN)


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


def _set_policy(tf: RpcSubprocess, policy: str) -> Any:
    return tf.invoke(f"{_REORG}/set_float_policy", policy)


def _tear_off(tf: RpcSubprocess, panel: str) -> None:
    tf.invoke(f"/{panel}/external/tear_off", None)


def _section(label: str) -> None:
    print(f"[demo] -- {label}")


# ─── demo body ───────────────────────────────────────────────────────


def body() -> None:
    with RpcSubprocess("hello-dock-panels-editor", boot_grace=2.0) as tf:
        # ── (A) boot — 5 panels, default policy = placeholder ────────
        _section("A: boot — 5 panels, default policy placeholder")
        boot = _windows(tf)
        assert len(boot) == 1, f"A.1 one window at boot; got {boot!r}"
        assert boot[0].get("id") == _MAIN, f"A.2 boot window is main ({boot[0]!r})"
        root = _root(_topology(tf))
        assert _all_panels(root) == _ALL_PANELS, f"A.3 all 5 panels present ({_all_panels(root)})"
        assert _float_policy(tf) == "placeholder", f"A.4 default policy is placeholder ({_float_policy(tf)!r})"
        assert _panel_policy(tf, _VIEWPORT) == "placeholder", "A.5 a panel reports the same surface policy"
        assert _siblings_in_split(root, _VIEWPORT, _PROPERTIES), "A.6 viewport|properties are siblings (home)"
        assert _lifecycle(tf, _VIEWPORT) == "Docked", "A.7 viewport starts docked"

        # ── (B) toggle to collapse ───────────────────────────────────
        _section("B: set_float_policy collapse")
        assert _set_policy(tf, "collapse") == "collapse", "B.1 the invoke echoes the applied policy"
        assert _float_policy(tf) == "collapse", "B.2 the coordinator reports collapse"
        assert _panel_policy(tf, _VIEWPORT) == "collapse", "B.3 every panel sees the surface policy"

        # ── (C) collapse tear-off — viewport's slot VANISHES ─────────
        _section("C: tear off viewport under collapse — slot reflows away")
        _tear_off(tf, _VIEWPORT)
        wait_until(lambda: _torn(_VIEWPORT) in _window_ids(tf), desc="C.1 viewport floats")
        assert len(_windows(tf)) == 2, "C.2 two windows after tear-off"
        root_c = _root(_topology(tf))
        assert _VIEWPORT not in _all_panels(root_c), "C.3 ★viewport's LEAF is gone from the topology (collapse)"
        assert _all_panels(root_c) == _ALL_PANELS - {_VIEWPORT}, f"C.4 the other 4 panels remain ({_all_panels(root_c)})"
        assert not _siblings_in_split(root_c, _VIEWPORT, _PROPERTIES), "C.5 viewport no longer occupies its slot"
        scene_c = _main_scene(tf)
        assert not _scene_contains_tag(scene_c, f"{_VIEWPORT}_placeholder"), (
            "C.6 ★NO placeholder is painted — the slot collapsed (vs the placeholder policy)"
        )
        assert _lifecycle(tf, _VIEWPORT) == "Floating", "C.7 the chart floated (the union'd external survives)"
        assert _panel_policy(tf, _VIEWPORT) == "collapse", "C.8 the floated panel's external is still registered (union)"

        # ── (D) dock back home — restored at the anchor ──────────────
        _section("D: tear off viewport again — dock back HOME")
        _tear_off(tf, _VIEWPORT)
        wait_until(lambda: len(_windows(tf)) == 1, desc="D.1 viewport docks back → one window")
        assert _torn(_VIEWPORT) not in _window_ids(tf), "D.2 the floating window is gone"
        root_d = _root(_topology(tf))
        assert _all_panels(root_d) == _ALL_PANELS, "D.3 all 5 panels back"
        assert _siblings_in_split(root_d, _VIEWPORT, _PROPERTIES), "D.4 ★viewport restored beside properties (home anchor)"
        assert _lifecycle(tf, _VIEWPORT) == "Docked", "D.5 the chart docked back"
        scene_d = _main_scene(tf)
        assert not _scene_contains_tag(scene_d, f"{_VIEWPORT}_placeholder"), "D.6 no placeholder lingering"

        # ── (E) contrast — placeholder keeps the slot ────────────────
        _section("E: placeholder policy keeps the slot (the other mode)")
        assert _set_policy(tf, "placeholder") == "placeholder", "E.1 toggle back to placeholder"
        assert _float_policy(tf) == "placeholder", "E.2 coordinator reports placeholder"
        _tear_off(tf, _OUTLINER)
        wait_until(lambda: _torn(_OUTLINER) in _window_ids(tf), desc="E.3 outliner floats")
        root_e = _root(_topology(tf))
        assert _OUTLINER in _all_panels(root_e), "E.4 ★placeholder KEEPS outliner's leaf in the topology"
        assert _all_panels(root_e) == _ALL_PANELS, "E.5 still all 5 panels (no reflow)"
        scene_e = _main_scene(tf)
        assert _scene_contains_tag(scene_e, f"{_OUTLINER}_placeholder"), "E.6 a placeholder IS painted (placeholder policy)"
        _tear_off(tf, _OUTLINER)  # dock back
        wait_until(lambda: len(_windows(tf)) == 1, desc="E.7 outliner docks back")
        assert _lifecycle(tf, _OUTLINER) == "Docked", "E.8 outliner docked back"

        # ── (F) integrity + read-only diagnostic ─────────────────────
        _section("F: integrity — panels survive, float_policy read-only")
        assert _window_ids(tf) == {_MAIN}, "F.1 only main remains"
        assert _all_panels(_root(_topology(tf))) == _ALL_PANELS, "F.2 all 5 panels intact"
        try:
            tf.request("scene/intervene", {"path": f"{_REORG}/float_policy", "value": "collapse"})
            raise AssertionError("F.3 intervening on float_policy must error (set via invoke)")
        except RpcError as exc:
            assert exc.code != 0, f"F.3 float_policy intervene rejected (code {exc.code})"

        print("[demo] r1134_dock_collapse: all sections PASS (collapse vs placeholder)")


if __name__ == "__main__":
    sys.exit(run_demo("r1134_dock_collapse", body))
