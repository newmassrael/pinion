#!/usr/bin/env python3
"""R1135 §5.51.1 — editor GUI float-policy toggle button.

SCOPE — read this before the assertions. R1134 added the `FloatPolicy`
collapse|placeholder torn-slot policy + the AI/RPC `set_float_policy` invoke, but
a HUMAN using the editor could only change it over RPC. R1135 adds a toolbar
TOGGLE BUTTON (`float_policy_btn`, a second `ButtonExternal`): clicking it flips
the shared `DockReorganizer`'s policy, and the reactive label repaints to the new
mode ("torn slot: placeholder" <-> "torn slot: collapse"). The policy lives in a
reactive `Signal` (R1135), so the GUI click + the AI invoke drive ONE SSOT and
stay consistent.

The DRIVER is a real pointer click (`scene/click {path: "float_policy_btn"}`) — the
same arc a mouse fires. The proof is AI-observable: `query("float_policy")` flips,
the toolbar label text flips, and a subsequent tear-off COLLAPSES the slot (the
GUI-set policy drives the dock behaviour) vs KEEPS it under placeholder.

Section roadmap (>=30 assertions across A-E):

  (A) Boot — toolbar shows the toggle button + the "placeholder" label; policy is
      placeholder.
  (B) Click toggle -> collapse — policy flips, the label repaints to "collapse".
  (C) Collapse behaviour — tear off viewport: its slot VANISHES (the GUI-set policy
      drives the collapse), then dock it back home.
  (D) Click toggle -> placeholder — policy flips back, label repaints; a tear-off
      now KEEPS the slot (placeholder), then dock back.
  (E) Integrity — all 4 panels survive; the button is read-only-introspect clean.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import RpcSubprocess, run_demo, wait_until  # noqa: E402

_VIEWPORT = "viewport"
_OUTLINER = "outliner"
_PROPERTIES = "properties"
_ALL_PANELS = {_OUTLINER, _VIEWPORT, _PROPERTIES, "console"}
_MAIN = "main"
_MAIN_W = 1200
_MAIN_H = 800
_REORG = "/dock_reorganize/external"
_POLICY_BTN = "float_policy_btn"
_PLACEHOLDER_LABEL = "torn slot: placeholder"
_COLLAPSE_LABEL = "torn slot: collapse"


# ─── helpers ─────────────────────────────────────────────────────────


def _windows(tf: RpcSubprocess) -> list[dict]:
    resp = tf.request("scene/windows", {})
    assert resp is not None and resp.result is not None, "scene/windows must answer"
    return resp.result.get("windows") or []


def _window_ids(tf: RpcSubprocess) -> set[str]:
    return {w.get("id") for w in _windows(tf)}


def _torn(panel: str) -> str:
    return f"torn-{panel}"


def _float_policy(tf: RpcSubprocess) -> Any:
    return tf.query(f"{_REORG}/float_policy")


def _topology_root(tf: RpcSubprocess) -> Any:
    topo = tf.query(f"{_REORG}/topology")
    return topo.get("root") if isinstance(topo, dict) else None


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


def _scene_has_tag(scene: Any, target: str) -> bool:
    if not isinstance(scene, dict):
        return False
    if scene.get("tag") == target:
        return True
    for child in scene.get("children") or []:
        if _scene_has_tag(child, target):
            return True
    content = scene.get("content")
    return isinstance(content, dict) and _scene_has_tag(content, target)


def _scene_has_text(scene: Any, text: str) -> bool:
    # Robust to the exact TextNode JSON shape: search the serialized scene.
    return text in json.dumps(scene)


def _click_toggle(tf: RpcSubprocess) -> None:
    tf.click(path=_POLICY_BTN)


def _tear_off(tf: RpcSubprocess, panel: str) -> None:
    tf.invoke(f"/{panel}/external/tear_off", None)


def _section(label: str) -> None:
    print(f"[demo] -- {label}")


# ─── demo body ───────────────────────────────────────────────────────


def body() -> None:
    with RpcSubprocess("hello-dock-panels-editor", boot_grace=2.0) as tf:
        # ── (A) boot — toggle button + placeholder label ─────────────
        _section("A: boot — toolbar toggle button, placeholder label")
        assert len(_windows(tf)) == 1, "A.1 one window at boot"
        assert _float_policy(tf) == "placeholder", f"A.2 default policy placeholder ({_float_policy(tf)!r})"
        scene = _main_scene(tf)
        assert _scene_has_tag(scene, _POLICY_BTN), "A.3 the toggle button is painted in the toolbar"
        assert _scene_has_text(scene, _PLACEHOLDER_LABEL), "A.4 label shows the current placeholder mode"
        assert not _scene_has_text(scene, _COLLAPSE_LABEL), "A.5 label is NOT collapse yet"
        assert _all_panels(_topology_root(tf)) == _ALL_PANELS, "A.6 all 4 panels docked"

        # ── (B) click toggle -> collapse ─────────────────────────────
        _section("B: click the toggle -> collapse, label repaints")
        _click_toggle(tf)
        wait_until(lambda: _float_policy(tf) == "collapse", desc="B.1 GUI click flips policy to collapse")
        scene_b = _main_scene(tf)
        assert _scene_has_text(scene_b, _COLLAPSE_LABEL), "B.2 ★label repainted to collapse (reactive Signal)"
        assert not _scene_has_text(scene_b, _PLACEHOLDER_LABEL), "B.3 the placeholder label is gone"

        # ── (C) collapse behaviour from the GUI-set policy ───────────
        _section("C: tear off viewport — the GUI-set collapse reflows the slot")
        _tear_off(tf, _VIEWPORT)
        wait_until(lambda: _torn(_VIEWPORT) in _window_ids(tf), desc="C.1 viewport floats")
        root_c = _topology_root(tf)
        assert _VIEWPORT not in _all_panels(root_c), "C.2 ★viewport's slot collapsed (GUI policy drove it)"
        assert _all_panels(root_c) == _ALL_PANELS - {_VIEWPORT}, "C.3 the other 3 reclaim the space"
        scene_c = _main_scene(tf)
        assert not _scene_has_tag(scene_c, f"{_VIEWPORT}_placeholder"), "C.4 no placeholder painted under collapse"
        _tear_off(tf, _VIEWPORT)  # dock back home
        wait_until(lambda: len(_windows(tf)) == 1, desc="C.5 viewport docks back")
        assert _all_panels(_topology_root(tf)) == _ALL_PANELS, "C.6 viewport restored home (4 panels)"

        # ── (D) click toggle -> placeholder ──────────────────────────
        _section("D: click toggle -> placeholder, the slot is kept on tear-off")
        _click_toggle(tf)
        wait_until(lambda: _float_policy(tf) == "placeholder", desc="D.1 GUI click flips back to placeholder")
        scene_d = _main_scene(tf)
        assert _scene_has_text(scene_d, _PLACEHOLDER_LABEL), "D.2 label repainted to placeholder"
        _tear_off(tf, _OUTLINER)
        wait_until(lambda: _torn(_OUTLINER) in _window_ids(tf), desc="D.3 outliner floats")
        root_d = _topology_root(tf)
        assert _OUTLINER in _all_panels(root_d), "D.4 ★placeholder KEEPS outliner's leaf (no reflow)"
        scene_d2 = _main_scene(tf)
        assert _scene_has_tag(scene_d2, f"{_OUTLINER}_placeholder"), "D.5 a placeholder IS painted"
        _tear_off(tf, _OUTLINER)  # dock back
        wait_until(lambda: len(_windows(tf)) == 1, desc="D.6 outliner docks back")

        # ── (E) integrity ───────────────────────────────────────────
        _section("E: integrity — panels survive, policy back to placeholder")
        assert _window_ids(tf) == {_MAIN}, "E.1 only main remains"
        assert _all_panels(_topology_root(tf)) == _ALL_PANELS, "E.2 all 4 panels intact"
        assert _float_policy(tf) == "placeholder", "E.3 policy ended at placeholder (two flips)"

        print("[demo] r1135_policy_toggle: all sections PASS (GUI toggle flips collapse|placeholder)")


if __name__ == "__main__":
    sys.exit(run_demo("r1135_policy_toggle", body))
