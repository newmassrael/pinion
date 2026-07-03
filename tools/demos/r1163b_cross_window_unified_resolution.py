#!/usr/bin/env python3
"""R1163b §5.51 §2 #7 — cross-window dock resolution UNIFIED onto `resolve_drop`.

SCOPE — read this before the assertions. Through R1162 the SAME-window drag
(tab + panel header) was unified onto the discrete-target `resolve_drop` SSOT
(banded geometry: edge bands = split, a small centre square = tabify, the ring
between = FLOAT), but the CROSS-window path (the editor's `tear_off_redock_at`
reducer + the on-target preview) STILL used the legacy CONTINUOUS classifier
(`dock_drop_zone_normalized`, which maps EVERY in-panel point to an edge / centre —
no float dead-zone). Two geometries coexisted = a half-done unification: a true
cross-window floater could resolve differently from its preview, and a drop in a
panel's neutral ring DOCKED instead of floating.

R1163b routes the cross-window reducer through the ONE `resolve_drop` SSOT (the
same the same-window drag + the on-target preview use), so:

  * a Dock zone (edge / centre) relocates the panel AT that zone (unchanged), and
  * ★ the dead-zone RING now resolves to FLOAT — the panel STAYS FLOATING (the
    legacy continuous classifier had no dead zone, so this point used to dock).

The DRIVER is the AI-primary `invoke("tear_off_redock_at", {window, target, x_rel,
y_rel})`. The proof is AI-observable scene-as-data: `scene/windows` (still floating
vs re-docked), the live topology JSON (`query(".../topology")`, the new neighbour),
and `query(".../last_outcome")` (the relocate record).

Section roadmap (>=30 assertions across A-F):

  (A) Boot — one window, the 4 panels docked; viewport sits beside properties.
  (B) Tear off viewport — two windows, viewport's slot shows a placeholder.
  (C) ★ DEAD-ZONE redock — redock viewport onto console at x_rel=0.30 (the neutral
      ring between the 0.22 edge band and the 0.18 centre square). It RESOLVES TO
      FLOAT: viewport STAYS FLOATING, the topology is unchanged (still beside
      properties). THE HEADLINE — under the legacy continuous classifier this point
      docked; the unified banded `resolve_drop` floats it.
  (D) EDGE redock — redock the SAME viewport onto console's LEFT edge (x_rel=0.15).
      A Dock zone still relocates it beside console (a split-insert); last_outcome
      "viewport -> console". The Dock path is unchanged by the unification.
  (E) CENTRE tabify — tear off outliner, redock onto properties' centre. A tab well
      stacks outliner + properties; last_outcome "outliner -> properties".
  (F) Integrity — all 4 panels survive; last_outcome is a read-only diagnostic.
"""

from __future__ import annotations

import sys
import time
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import RpcError, RpcSubprocess, run_demo, wait_until  # noqa: E402

_OUTLINER = "outliner"
_VIEWPORT = "viewport"
_PROPERTIES = "properties"
_CONSOLE = "console"
_ALL_PANELS = {_OUTLINER, _VIEWPORT, _PROPERTIES, _CONSOLE}
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


def _redock(
    tf: RpcSubprocess, panel: str, window: str, *, target: str, x_rel: float, y_rel: float
) -> None:
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
        _section("A: boot — 4 panels docked, viewport|properties siblings")
        boot = _windows(tf)
        assert len(boot) == 1, f"A.1 one window at boot; got {boot!r}"
        assert boot[0].get("id") == _MAIN, f"A.2 boot window is main ({boot[0]!r})"
        root = _root(_topology(tf))
        assert root is not None, "A.3 topology readable as scene-as-data"
        assert _all_panels(root) == _ALL_PANELS, f"A.4 all 4 panels present ({_all_panels(root)})"
        assert _siblings_in_split(root, _VIEWPORT, _PROPERTIES), "A.5 viewport|properties siblings"
        assert not _siblings_in_split(root, _VIEWPORT, _CONSOLE), "A.6 viewport not beside console"
        scene = _main_scene(tf)
        for panel in (_OUTLINER, _VIEWPORT, _PROPERTIES):
            assert _docked(scene, panel), f"A.7 {panel} content is docked"

        # ── (B) tear off viewport ────────────────────────────────────
        _section("B: tear off viewport")
        _tear_off(tf, _VIEWPORT)
        wait_until(lambda: _torn(_VIEWPORT) in _window_ids(tf), desc="B.1 viewport floats")
        assert len(_windows(tf)) == 2, "B.2 two windows after tear-off"
        scene_b = _main_scene(tf)
        assert _scene_contains_tag(scene_b, f"{_VIEWPORT}_placeholder"), "B.3 viewport slot placeholder"
        assert not _docked(scene_b, _VIEWPORT), "B.4 viewport content moved out of main"
        assert _siblings_in_split(_root(_topology(tf)), _VIEWPORT, _PROPERTIES), (
            "B.5 floating panel stays in the topology (placeholder leaf, beside properties)"
        )

        # ── (C) ★ DEAD-ZONE redock — viewport onto console's neutral ring ──
        _section("C: ★ dead-zone redock (x_rel=0.30) → STAYS FLOATING (the headline)")
        _redock(tf, _VIEWPORT, _MAIN, target=_CONSOLE, x_rel=0.30, y_rel=0.5)
        # A Float resolution makes NO change — give the reducer a beat to settle,
        # then prove the panel did NOT re-dock (a non-event can't be `wait_until`'d;
        # the legacy continuous classifier WOULD have docked it here).
        time.sleep(0.3)
        assert _torn(_VIEWPORT) in _window_ids(tf), (
            "C.1 ★ a dead-zone drop FLOATS: viewport's window is still present"
        )
        assert len(_windows(tf)) == 2, "C.2 still two windows (no redock happened)"
        root_c = _root(_topology(tf))
        assert _siblings_in_split(root_c, _VIEWPORT, _PROPERTIES), (
            "C.3 topology UNCHANGED — viewport still in its home placeholder slot"
        )
        assert not _siblings_in_split(root_c, _VIEWPORT, _CONSOLE), (
            "C.4 the dead-zone did NOT dock viewport beside console"
        )
        assert _all_panels(root_c) == _ALL_PANELS, "C.5 no panel lost on the float"
        assert _scene_contains_tag(_main_scene(tf), f"{_VIEWPORT}_placeholder"), (
            "C.6 viewport's slot is still a placeholder (it never came home)"
        )

        # ── (D) EDGE redock — the SAME viewport onto console's LEFT edge ──
        _section("D: edge redock (x_rel=0.15) → relocates beside console (Dock unchanged)")
        _redock(tf, _VIEWPORT, _MAIN, target=_CONSOLE, x_rel=0.15, y_rel=0.5)
        wait_until(lambda: len(_windows(tf)) == 1, desc="D.1 viewport re-docks → one window")
        assert _torn(_VIEWPORT) not in _window_ids(tf), "D.2 the floating window is gone"
        root_d = _root(_topology(tf))
        assert _siblings_in_split(root_d, _VIEWPORT, _CONSOLE), (
            "D.3 a Dock edge still relocates viewport beside console"
        )
        assert not _siblings_in_split(root_d, _VIEWPORT, _PROPERTIES), (
            "D.4 viewport left its home slot (relocated, not home-returned)"
        )
        assert _last_outcome(tf) == f"{_VIEWPORT} -> {_CONSOLE}", (
            f"D.5 last_outcome records the relocate ({_last_outcome(tf)!r})"
        )
        assert _all_panels(root_d) == _ALL_PANELS, "D.6 no panel lost in the relocate"
        scene_d = _main_scene(tf)
        assert _docked(scene_d, _VIEWPORT), "D.7 the real viewport content is back in the dock"
        assert not _scene_contains_tag(scene_d, f"{_VIEWPORT}_placeholder"), "D.8 no placeholder"

        # ── (E) centre tabify — outliner onto properties' centre ─────
        _section("E: redock outliner onto properties' CENTRE → tab well (Dock unchanged)")
        _tear_off(tf, _OUTLINER)
        wait_until(lambda: _torn(_OUTLINER) in _window_ids(tf), desc="E.1 outliner floats")
        _redock(tf, _OUTLINER, _MAIN, target=_PROPERTIES, x_rel=0.5, y_rel=0.5)
        wait_until(lambda: len(_windows(tf)) == 1, desc="E.2 outliner re-docks → one window")
        root_e = _root(_topology(tf))
        wells = _tabs_wells(root_e)
        assert any({_OUTLINER, _PROPERTIES} <= w for w in wells), (
            f"E.3 a tab well stacks outliner + properties ({wells})"
        )
        assert _last_outcome(tf) == f"{_OUTLINER} -> {_PROPERTIES}", (
            f"E.4 last_outcome records the centre tabify ({_last_outcome(tf)!r})"
        )
        assert _all_panels(root_e) == _ALL_PANELS, "E.5 the tabify keeps every panel"

        # ── (F) integrity + read-only diagnostic ─────────────────────
        _section("F: integrity — panels survive, last_outcome read-only")
        assert _window_ids(tf) == {_MAIN}, "F.1 only main remains after every redock"
        assert _all_panels(_root(_topology(tf))) == _ALL_PANELS, "F.2 all 4 panels intact"
        try:
            tf.request("scene/intervene", {"path": f"{_REORG}/last_outcome", "value": "tamper"})
            raise AssertionError("F.3 intervening on last_outcome must error")
        except RpcError as exc:
            assert exc.code != 0, f"F.3 last_outcome intervene rejected (code {exc.code})"

        print("[demo] r1163b_cross_window_unified_resolution: all sections PASS (dead-zone floats)")


if __name__ == "__main__":
    sys.exit(run_demo("r1163b_cross_window_unified_resolution", body))
