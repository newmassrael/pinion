#!/usr/bin/env python3
"""R1106 §5.51 §5.16 §5.41 PR-31 — editor ZONE-HONORING cross-window redock.

SCOPE — read this before the assertions. R1105 made `hello-dock-panels-editor`
float a panel into its own window and redock it; but a redock into main returned
the panel to its HOME slot (the `target`/`x_rel`/`y_rel` zone was recorded yet
ignored — exactly the flat `hello-dock-panels` limit, since the flat splitter has
fixed slots). R1106 honours the zone: because the editor has a `DockTopology` and
the torn panel stays in it as a placeholder LEAF, a redock into main RELOCATES the
panel AT the dropped zone via `DockReorganizer::apply_zone_redock` — the same
`dock_drop_zone_normalized` + `intent_for_zone` SSOT the in-window drag uses
(Centre = tabify, an edge = split-insert). This is the editor's distinguishing
value over the flat consumer (the flat demo CAN'T do it — no topology).

The DRIVER is the AI-primary `invoke("tear_off_redock_at", {window, target, x_rel,
y_rel})`. The proof is AI-observable scene-as-data: the live topology JSON
(`query("/dock_reorganize/external/topology")`) shows the panel's new neighbour,
and `query(".../last_outcome")` records the relocate (`"<source> -> <target>"`).

Section roadmap (>=30 assertions across A-E):

  (A) Boot — one window, the 5 panels docked; viewport sits beside properties (the
      inner_h split), and is NOT beside console.
  (B) Tear off viewport — two windows, viewport's slot shows a placeholder.
  (C) Zone redock — redock viewport onto CONSOLE's LEFT edge. The topology now puts
      viewport beside console (a split-insert), NOT beside properties; last_outcome
      reads "viewport -> console"; the floating window is gone; the real viewport
      content re-docked. THE HEADLINE: it landed at the zone, not its home slot.
  (D) Centre tabify — tear off outliner, redock onto PROPERTIES' centre. A tab well
      now stacks outliner + properties; last_outcome "outliner -> properties".
  (E) Integrity — all 5 panels survive every relocate; last_outcome is a read-only
      diagnostic.
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

_CONTENT = {
    _OUTLINER: "outliner_content_body",
    _VIEWPORT: "viewport_content_body",
    _PROPERTIES: "properties_content_body",
}


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


def _last_outcome(tf: RpcSubprocess) -> Any:
    return tf.query(f"{_REORG}/last_outcome")


def _is_leaf(node: Any, panel: str) -> bool:
    return isinstance(node, dict) and node.get("type") == "Leaf" and node.get("panel_id") == panel


def _siblings_in_split(node: Any, x: str, y: str) -> bool:
    """Are panels `x` and `y` the two leaves of a common divider anywhere?"""
    if not isinstance(node, dict) or node.get("type") != "Split":
        return False
    f, s = node.get("first"), node.get("second")
    if (_is_leaf(f, x) and _is_leaf(s, y)) or (_is_leaf(f, y) and _is_leaf(s, x)):
        return True
    return _siblings_in_split(f, x, y) or _siblings_in_split(s, x, y)


def _all_panels(node: Any) -> set[str]:
    """Every panel id reachable in the topology tree."""
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


def _tabs_wells(node: Any) -> list[set[str]]:
    """The panel set of every Tabs well in the tree."""
    if not isinstance(node, dict):
        return []
    kind = node.get("type")
    if kind == "Tabs":
        return [set(node.get("panels") or [])]
    if kind == "Split":
        return _tabs_wells(node.get("first")) + _tabs_wells(node.get("second"))
    return []


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


def _docked(scene: Any, panel: str) -> bool:
    return _scene_contains_tag(scene, _CONTENT[panel])


def _tear_off(tf: RpcSubprocess, panel: str) -> None:
    tf.invoke(f"/{panel}/external/tear_off", None)


def _redock(tf: RpcSubprocess, panel: str, window: str, *, target: str, x_rel: float, y_rel: float) -> None:
    tf.invoke(
        f"/{panel}/external/tear_off_redock_at",
        {"window": window, "target": target, "x_rel": x_rel, "y_rel": y_rel},
    )


def _section(label: str) -> None:
    print(f"[demo] -- {label}")


# ─── demo body ───────────────────────────────────────────────────────


def body() -> None:
    with RpcSubprocess("hello-dock-panels-editor", boot_grace=2.0) as tf:
        # ── (A) boot — viewport beside properties, not console ───────
        _section("A: boot — 5 panels docked, viewport|properties siblings")
        boot = _windows(tf)
        assert len(boot) == 1, f"A.1 one window at boot; got {boot!r}"
        assert boot[0].get("id") == _MAIN, f"A.2 boot window is main ({boot[0]!r})"
        root = _root(_topology(tf))
        assert root is not None, "A.3 topology readable as scene-as-data"
        assert _all_panels(root) == _ALL_PANELS, f"A.4 all 5 panels present ({_all_panels(root)})"
        assert _siblings_in_split(root, _VIEWPORT, _PROPERTIES), "A.5 viewport|properties are siblings"
        assert not _siblings_in_split(root, _VIEWPORT, _CONSOLE), "A.6 viewport not beside console yet"
        scene = _main_scene(tf)
        for panel in (_OUTLINER, _VIEWPORT, _PROPERTIES):
            assert _docked(scene, panel), f"A.7 {panel} content is docked"

        # ── (B) tear off viewport ────────────────────────────────────
        _section("B: tear off viewport")
        _tear_off(tf, _VIEWPORT)
        wait_until(lambda: _torn(_VIEWPORT) in _window_ids(tf), desc="B.1 viewport floats")
        assert len(_windows(tf)) == 2, "B.2 two windows after tear-off"
        scene_b = _main_scene(tf)
        assert _scene_contains_tag(scene_b, f"{_VIEWPORT}_placeholder"), "B.3 viewport slot is a placeholder"
        assert not _docked(scene_b, _VIEWPORT), "B.4 viewport content moved out of main"
        # The topology still holds viewport as a (placeholder) leaf beside properties.
        assert _siblings_in_split(_root(_topology(tf)), _VIEWPORT, _PROPERTIES), (
            "B.5 floating panel stays in the topology (placeholder leaf, still beside properties)"
        )

        # ── (C) zone redock — viewport onto console's LEFT edge ──────
        _section("C: redock viewport onto console's LEFT edge (relocate, not home)")
        _redock(tf, _VIEWPORT, _MAIN, target=_CONSOLE, x_rel=0.15, y_rel=0.5)
        wait_until(lambda: len(_windows(tf)) == 1, desc="C.1 viewport re-docks → one window")
        assert _torn(_VIEWPORT) not in _window_ids(tf), "C.2 the floating window is gone"
        root_c = _root(_topology(tf))
        assert _siblings_in_split(root_c, _VIEWPORT, _CONSOLE), (
            "C.3 ★viewport relocated BESIDE CONSOLE at the dropped edge (the headline)"
        )
        assert not _siblings_in_split(root_c, _VIEWPORT, _PROPERTIES), (
            "C.4 viewport left its home inner_h slot (NOT a home-slot return)"
        )
        assert _last_outcome(tf) == f"{_VIEWPORT} -> {_CONSOLE}", (
            f"C.5 last_outcome records the relocate ({_last_outcome(tf)!r})"
        )
        assert _all_panels(root_c) == _ALL_PANELS, "C.6 no panel lost in the relocate"
        scene_c = _main_scene(tf)
        assert _docked(scene_c, _VIEWPORT), "C.7 the real viewport content is back in the dock"
        assert not _scene_contains_tag(scene_c, f"{_VIEWPORT}_placeholder"), "C.8 no placeholder remains"

        # ── (D) centre tabify — outliner onto properties' centre ─────
        _section("D: redock outliner onto properties' CENTRE → tab well")
        _tear_off(tf, _OUTLINER)
        wait_until(lambda: _torn(_OUTLINER) in _window_ids(tf), desc="D.1 outliner floats")
        _redock(tf, _OUTLINER, _MAIN, target=_PROPERTIES, x_rel=0.5, y_rel=0.5)
        wait_until(lambda: len(_windows(tf)) == 1, desc="D.2 outliner re-docks → one window")
        root_d = _root(_topology(tf))
        wells = _tabs_wells(root_d)
        assert any({_OUTLINER, _PROPERTIES} <= w for w in wells), (
            f"D.3 a tab well stacks outliner + properties ({wells})"
        )
        assert _last_outcome(tf) == f"{_OUTLINER} -> {_PROPERTIES}", (
            f"D.4 last_outcome records the centre tabify ({_last_outcome(tf)!r})"
        )
        assert _all_panels(root_d) == _ALL_PANELS, "D.5 the tabify keeps every panel"

        # ── (E) integrity + read-only diagnostic ─────────────────────
        _section("E: integrity — panels survive, last_outcome read-only")
        assert _window_ids(tf) == {_MAIN}, "E.1 only main remains after every redock"
        assert _all_panels(_root(_topology(tf))) == _ALL_PANELS, "E.2 all 5 panels intact"
        try:
            tf.request("scene/intervene", {"path": f"{_REORG}/last_outcome", "value": "tamper"})
            raise AssertionError("E.3 intervening on last_outcome must error")
        except RpcError as exc:
            assert exc.code != 0, f"E.3 last_outcome intervene rejected (code {exc.code})"

        print("[demo] r1106_editor_zone_redock: all sections PASS (zone-honoring relocate)")


if __name__ == "__main__":
    sys.exit(run_demo("r1106_editor_zone_redock", body))
