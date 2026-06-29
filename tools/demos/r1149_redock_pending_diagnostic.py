#!/usr/bin/env python3
"""R1149 §5.51 §2 #7 — make a preview-vs-drop divergence RPC-DIAGNOSABLE.

The live user report: "there is a redock preview, but on release it does not dock
at the preview position — it docks elsewhere or floats." That class of bug was
NOT RPC-observable: the only redock read (`redock_at`) shows the COMPLETED drop,
never the IN-FLIGHT would-dock, so an AI could not compare "where the preview says
it will land" to "where release lands" from a held mid-drag — it had to eyeball
the live window.

R1149 adds `query("redock_pending")` = where a release RIGHT NOW would redock
(`{window, target, x_rel, y_rel}` or null = it would float), recorded on every
`drag_to_at` from the SAME `(over_window, over)` the release decides on. This demo
HOLDS a floater drag over main (`scene/drag phase:"begin"`), reads `redock_pending`
(the would-dock the AI can now see), then releases (`phase:"end"`) and reads
`redock_at` (what actually committed). They must AGREE — a divergence is the bug,
now caught from RPC instead of a screenshot.

  (A) Boot, tear off `properties` (a borderless floater, its slot a placeholder).
  (B) HELD drag the floater's header over main's interior; read `redock_pending`.
  (C) END the drag; read `redock_at`; assert it equals the held `redock_pending`.
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


def _floater(tf: RpcSubprocess) -> Optional[dict]:
    return next((w for w in _windows(tf) if w.get("id") == _TORN), None)


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
        _section("A: boot + tear off properties")
        tf.invoke(f"/{_PANEL}/external/tear_off", None)
        entry = wait_until(lambda: _floater(tf), desc="A.1 floater appears")
        pos = entry.get("position") or [0, 0]
        size = entry.get("declared_size") or [460, 380]
        fpx, fpy = float(pos[0]), float(pos[1])
        fw, fh = int(size[0]), int(size[1])
        tf.snapshot(source="paint", viewport=(_MAIN_W, _MAIN_H), window=_MAIN)
        tf.snapshot(source="paint", viewport=(fw, fh), window=_TORN)
        print(f"[demo] floater pos={pos} size={size}")

        _section("B: HELD drag the floater header over main; read redock_pending")
        # phase=begin: press on the header + march into main (the r1124 path that
        # redocks atomically), then HOLD. Read the would-dock + whether the redock
        # PREVIEW hint is painted into the floater (the affordance the user sees).
        held_to = (fw / 2.0 - 200.0, 14.0)
        tf.request("scene/drag", {
            "window": _TORN,
            "from": {"x": fw / 2.0, "y": 14.0},
            "to": {"x": held_to[0], "y": held_to[1]},
            "steps": 16,
            "phase": "begin",
        })
        # Re-prime both scenes so the resolution + preview are current. R1150:
        # the on-TARGET preview (`__xwin_drop_preview`) shows in main at the dock
        # zone (correct place); the R1137 on-FLOATER hint (`__xwin_redock_hint`)
        # was REMOVED (it sat at the static floater's spot — "preview here, docks
        # there").
        main_scene = tf.snapshot(source="paint", viewport=(_MAIN_W, _MAIN_H), window=_MAIN)
        floater_scene = tf.snapshot(source="paint", viewport=(fw, fh), window=_TORN)
        floater_hint_shown = _scene_has_tag(floater_scene, "__xwin_redock_hint")
        main_preview_shown = _scene_has_tag(main_scene, "__xwin_drop_preview")
        print(f"[demo] HELD: on-MAIN preview (at target) shown={main_preview_shown} "
              f"on-FLOATER hint shown={floater_hint_shown}")
        pending = tf.query(f"/{_PANEL}/external/redock_pending")
        detached = tf.query(f"/{_PANEL}/external/detached")
        drag_cursor = tf.query(f"/{_PANEL}/external/drag_cursor")
        wins_held = [w.get("id") for w in _windows(tf)]
        print(f"[demo] HELD: redock_pending={pending!r}")
        print(f"[demo] HELD: detached={detached!r} drag_cursor={drag_cursor!r} "
              f"windows={wins_held!r}")

        _section("C: END the drag; read redock_at; compare")
        tf.request("scene/drag", {
            "window": _TORN,
            "from": {"x": held_to[0], "y": held_to[1]},
            "to": {"x": held_to[0], "y": held_to[1]},
            "steps": 1,
            "phase": "end",
        })
        redock_at = tf.query(f"/{_PANEL}/external/redock_at")
        wins_after = [w.get("id") for w in _windows(tf)]
        print(f"[demo] AFTER RELEASE: redock_at={redock_at!r} windows={wins_after!r}")

        # The diagnosis (now RPC-observable): mid-hold the AI READS the would-dock.
        # R1150: the preview shows on the TARGET (main, at the dock zone), NOT on
        # the floater (the misplaced on-floater hint was removed).
        assert main_preview_shown, "B.1a the on-target preview is shown in main (at the dock zone)"
        assert not floater_hint_shown, "B.1b no on-floater redock hint (R1150 removed it)"
        assert isinstance(pending, dict), f"B.2 redock_pending is readable mid-hold ({pending!r})"
        assert pending.get("window") == _MAIN, f"B.3 the would-dock names main ({pending!r})"
        assert isinstance(redock_at, dict), f"C.1 a redock fired on release ({redock_at!r})"
        # The CORE diagnosability assertion: where the AI saw it would dock
        # (`redock_pending`, the in-flight read) EQUALS where the release docked
        # (`redock_at`), proving the resolution is consistent across the release —
        # so a real preview-vs-dock divergence would be CAUGHT here, from RPC, not
        # a screenshot. (The held read is the §2 #7 surface that was missing.)
        for field in ("window", "target", "x_rel", "y_rel"):
            assert pending.get(field) == redock_at.get(field), (
                f"C.2 redock_pending.{field}={pending.get(field)!r} must equal "
                f"redock_at.{field}={redock_at.get(field)!r} (preview==dock)"
            )
        assert wins_after == [_MAIN], f"C.3 the floater is gone (re-docked) ({wins_after!r})"
        print("[demo] r1149: redock_pending is RPC-readable + equals redock_at (preview==dock) — PASS")


if __name__ == "__main__":
    sys.exit(run_demo("r1149_redock_pending_diagnostic", body))
