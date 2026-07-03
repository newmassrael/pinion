#!/usr/bin/env python3
"""R1124 §5.51 §2 #7 PR-33 — a floating panel dragged BACK over the main window by
its own title bar re-docks (the live pointer floater->main redock).

SCOPE — read this before the assertions. R1098-R1103 built the cross-window redock
as the AI-primary `invoke("tear_off_redock_at", ...)` channel; the LIVE pointer
gesture (drag the floating window's header back onto main and release over a dock
zone) was the parked carry — it fired only `window_move`, never a redock. The
cause was the router's own-window-first rule in `resolve_drag_targets`: a floater
follows the cursor, so at release the cursor is over the floater's OWN header /
content (both drop targets), and that `own_over` masked the cross-window redock the
shell had correctly resolved. R1124 makes a SELF-DROP (an own hit on the dragged
source's own subtree) yield to the cross-window redock, so the gesture executes.

The DRIVER is the live pointer path: `scene/drag` scoped to the FLOATING window
drives press-on-header -> move -> release exactly as a real mouse would (the same
`drain_drag_for_window` arc the shell runs for winit events). The floater follows
the cursor via the R1116 borderless `window_move`; the shell resolves the
cross-window drop against main each move and stashes it; the release reads it and,
because the own hit is a self-drop, redocks. The proof is an observable topology
change — the floating window is gone and the panel is re-hosted in main — not a
diagnostic.

  (A) Boot — one window, the 4 editor panels docked.
  (B) Tear off `properties` -> two declared windows, its slot a placeholder.
  (C) Drag the floater back by its header over main, release -> the floating window
      is gone, `properties` is re-docked in main, and its `redock_at` records the
      cross-window dock-at that fired (target window `main`).
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import RpcSubprocess, run_demo, wait_until  # noqa: E402

_PANEL = "properties"
_TORN = f"torn-{_PANEL}"
_MAIN = "main"
_MAIN_W = 1200
_MAIN_H = 800


def _windows(tf: RpcSubprocess) -> list[dict]:
    resp = tf.request("scene/windows", {})
    assert resp is not None and resp.result is not None, "scene/windows must answer"
    return resp.result.get("windows") or []


def _window_ids(tf: RpcSubprocess) -> set[str]:
    return {w.get("id") for w in _windows(tf)}


def _floater(tf: RpcSubprocess) -> Optional[dict]:
    return next((w for w in _windows(tf) if w.get("id") == _TORN), None)


def _redock_at(tf: RpcSubprocess) -> Any:
    return tf.query(f"/{_PANEL}/external/redock_at")


def _scene_has_tag(scene: Any, target: str) -> bool:
    if not isinstance(scene, dict):
        return False
    if scene.get("tag") == target:
        return True
    if any(_scene_has_tag(c, target) for c in scene.get("children") or []):
        return True
    return isinstance(scene.get("content"), dict) and _scene_has_tag(scene["content"], target)


def _section(label: str) -> None:
    print(f"[demo] -- {label}")


def body() -> None:
    with RpcSubprocess("hello-dock-panels-editor", boot_grace=2.0) as tf:
        # ── (A) boot — one window, all panels docked ─────────────────
        _section("A: boot — single window, properties docked")
        boot = _windows(tf)
        assert len(boot) == 1, f"A.1 boot declares one window; got {boot!r}"
        assert boot[0].get("id") == _MAIN, f"A.2 the boot window is 'main'; got {boot[0]!r}"
        main_scene = tf.snapshot(source="paint", viewport=(_MAIN_W, _MAIN_H), window=_MAIN)
        assert _scene_has_tag(main_scene, _PANEL), "A.3 properties is docked in main"

        # ── (B) tear off properties → a second window ────────────────
        _section("B: tear off properties (a floating window)")
        tf.invoke(f"/{_PANEL}/external/tear_off", None)
        entry = wait_until(lambda: _floater(tf), desc="B.1 the floating window appears")
        assert len(_windows(tf)) == 2, "B.2 two declared windows after the tear-off"
        assert entry.get("decorations") is False, "B.3 the floater is borderless (app-owned header)"
        pos = entry.get("position") or [0, 0]
        size = entry.get("declared_size") or [460, 380]
        fpx, fpy = float(pos[0]), float(pos[1])
        fw, fh = int(size[0]), int(size[1])
        # Prime both paint scenes so drop targets resolve under the hit-test.
        tf.snapshot(source="paint", viewport=(_MAIN_W, _MAIN_H), window=_MAIN)
        tf.snapshot(source="paint", viewport=(fw, fh), window=_TORN)
        assert _redock_at(tf) is None, "B.4 no cross-window redock recorded yet"

        # ── (C) drag the floater back by its header → redock ─────────
        _section("C: drag the floater's header back over main → redock")
        # Press on the floater's header strip and march LEFT so the cursor (and the
        # window_move-following floater) cross into main's interior; release there.
        # The release cursor is over the floater's OWN header (a self-drop) AND over
        # main's dock zone — R1124 lets the self-drop yield to the cross-window
        # redock. The starting header-abs (fpx + fw/2) is well right of main's
        # interior; marching left by > fw/2 lands the cursor inside main.
        tf.request("scene/drag", {
            "window": _TORN,
            "from": {"x": fw / 2.0, "y": 14.0},
            "to": {"x": fw / 2.0 - 200.0, "y": 14.0},
            "steps": 16,
        })
        wait_until(lambda: len(_windows(tf)) == 1, desc="C.1 the floating window is gone")
        assert _window_ids(tf) == {_MAIN}, "C.2 only main remains after the redock"
        redock = _redock_at(tf)
        assert isinstance(redock, dict), f"C.3 a cross-window redock fired ({redock})"
        assert redock["window"] == _MAIN, f"C.4 the redock targeted main ({redock})"
        assert redock["panel"] == _PANEL, f"C.5 the payload names the properties panel ({redock})"
        final = tf.snapshot(source="paint", viewport=(_MAIN_W, _MAIN_H), window=_MAIN)
        assert _scene_has_tag(final, _PANEL), "C.6 the real properties panel is re-docked in main"

        print("[demo] r1124_floater_drag_back_redock: all sections PASS (live floater→main redock)")


if __name__ == "__main__":
    sys.exit(run_demo("r1124_floater_drag_back_redock", body))
