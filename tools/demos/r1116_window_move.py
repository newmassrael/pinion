#!/usr/bin/env python3
"""R1116 §5.51 §5.16 §2 #7 PR-38 — drag a borderless floater by its title bar.

R1115 made a torn-off panel a BORDERLESS window (no OS title bar). A borderless
window has no OS chrome to drag, so pinion must own the move: dragging the
panel's own header (the title-bar replacement) relocates the window. This is a
DEDICATED window move, NOT a dock tear-off — a floating window has nothing to
"escape" (its cursor stays on its own title bar), so the move is a first-class
window relocation driven by `DockPanelExternal::with_floating_window` +
grab-offset follow, distinct from the docked tear-off/reorganize path.

BOTH chrome modes are framework-supported (the point of the `decorations`
toggle): a `decorations:true` floater is moved by the OS title bar (winit
`WindowEvent::Moved` → `windows_signal`, R1088); a `decorations:false` floater is
moved by this custom title-bar drag (R1116). The editor floats borderless (the
DCC/the DCC look); the main window stays decorated — both coexist here.

NOTE on the drive: `scene/drag` interpolates a window-LOCAL cursor in a FIXED
frame, while the live winit path reports the cursor relative to the moving
window. So the headless drive proves the BEHAVIOUR (the window moves on a header
drag, repeatably, and stays floating) but not the exact live grab-offset
magnitude — that is unit-tested (`r1116_sole_floater_header_drag_is_grab_offset
_window_move`) and live-verified on a real GPU host.

Section roadmap (>=30 assertions across A-F):
  (A) Boot — one decorated main window, panels docked.
  (B) Tear off viewport — a borderless floater (decorations:false) at a declared spot.
  (C) Drag its header — the window MOVES (position changes), it does NOT redock.
  (D) Drag again — repeatable; the panel content stays in the floater throughout.
  (E) Window move != tear-off — `detached` stays false; the floater window persists.
  (F) Both modes coexist — main stays decorated, the floater stays borderless.
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import RpcSubprocess, run_demo, wait_until  # noqa: E402

_VIEWPORT = "viewport"
_TORN = "torn-viewport"
_MAIN = "main"
_MAIN_W, _MAIN_H = 1200, 800


def _windows(tf: RpcSubprocess) -> dict[str, dict]:
    resp = tf.request("scene/windows", {})
    assert resp is not None and resp.result is not None, "scene/windows must answer"
    return {w["id"]: w for w in resp.result.get("windows", [])}


def _pos(tf: RpcSubprocess, wid: str) -> Any:
    w = _windows(tf).get(wid)
    return tuple(w["position"]) if w and w.get("position") else None


def _drag_header(tf: RpcSubprocess, frm=(45.0, 10.0), to=(245.0, 160.0), steps=6) -> None:
    """Drag the floater's header (title bar), scoped to ITS OWN router."""
    tf.request(
        "scene/drag",
        {"window": _TORN, "from": {"x": frm[0], "y": frm[1]},
         "to": {"x": to[0], "y": to[1]}, "steps": steps},
    )


def _main_scene(tf: RpcSubprocess) -> Any:
    return tf.snapshot(source="paint", viewport=(_MAIN_W, _MAIN_H), window=_MAIN)


def _floater_scene(tf: RpcSubprocess) -> Any:
    return tf.snapshot(source="paint", viewport=(360, 360), window=_TORN)


def _has_tag(scene: Any, tag: str) -> bool:
    if not isinstance(scene, dict):
        return False
    if scene.get("tag") == tag:
        return True
    for c in scene.get("children") or []:
        if _has_tag(c, tag):
            return True
    content = scene.get("content")
    return isinstance(content, dict) and _has_tag(content, tag)


def _section(label: str) -> None:
    print(f"[demo] -- {label}")


def body() -> None:
    with RpcSubprocess("hello-dock-panels-editor", boot_grace=2.0) as tf:
        # ── (A) boot ─────────────────────────────────────────────────
        _section("A: boot — one decorated main window")
        boot = _windows(tf)
        assert set(boot) == {_MAIN}, f"A.1 one window at boot; got {set(boot)!r}"
        assert boot[_MAIN]["decorations"] is True, "A.2 main is OS-decorated"
        assert boot[_MAIN].get("position") is None, "A.3 main is WM-placed"
        assert boot[_MAIN].get("declared_size") == [_MAIN_W, _MAIN_H], "A.4 main declares its size"

        # ── (B) tear off → borderless floater ────────────────────────
        _section("B: tear off viewport — a borderless floater")
        tf.invoke(f"/{_VIEWPORT}/external/tear_off", None)
        wait_until(lambda: _TORN in _windows(tf), desc="B.1 the floater window appears")
        w = _windows(tf)
        assert set(w) == {_MAIN, _TORN}, f"B.2 main + floater; got {set(w)!r}"
        assert w[_TORN]["decorations"] is False, "B.3 the floater is BORDERLESS (decorations:false)"
        start = _pos(tf, _TORN)
        assert start is not None, f"B.4 the floater opens at a declared position; got {start!r}"
        assert w[_MAIN]["decorations"] is True, "B.5 main stays decorated"
        assert _has_tag(_floater_scene(tf), f"{_VIEWPORT}#header"), "B.6 the floater paints a title-bar header"

        # ── (C) drag the header → window MOVES, no redock ────────────
        _section("C: drag the floater header — the window moves, does NOT redock")
        _drag_header(tf)
        wait_until(lambda: _pos(tf, _TORN) != start, desc="C.1 the floater position changes")
        moved1 = _pos(tf, _TORN)
        assert moved1 != start, f"C.2 the header drag moved the window ({start} -> {moved1})"
        assert _TORN in _windows(tf), "C.3 the window is still floating (a move, not a redock)"
        assert _pos(tf, _MAIN) is None, "C.4 main is untouched by the floater move"
        # The move is NOT a tear-off escape: detached stays false.
        assert tf.query(f"/{_VIEWPORT}/external/detached") is False, (
            "C.5 a window move does not set the tear-off `detached` latch"
        )

        # ── (D) repeatable; content stays in the floater ─────────────
        _section("D: drag again — repeatable, content stays in the floater")
        _drag_header(tf, frm=(40.0, 8.0), to=(120.0, 60.0))
        wait_until(lambda: _pos(tf, _TORN) != moved1, desc="D.1 a second drag moves it again")
        moved2 = _pos(tf, _TORN)
        assert moved2 != moved1, f"D.2 the window is movable repeatedly ({moved1} -> {moved2})"
        assert _TORN in _windows(tf), "D.3 still floating after the second move"
        # The real panel content lives in the floater (not back in main).
        assert _has_tag(_floater_scene(tf), f"{_VIEWPORT}_content_body"), (
            "D.4 the viewport's real content is in the floater"
        )
        assert _has_tag(_main_scene(tf), f"{_VIEWPORT}_placeholder"), (
            "D.5 main shows the viewport's placeholder while it floats"
        )

        # ── (E) window move is distinct from tear-off ────────────────
        _section("E: the move is a window relocation, not a tear-off")
        assert tf.query(f"/{_VIEWPORT}/external/detached") is False, "E.1 detached still false"
        assert tf.query(f"/{_VIEWPORT}/external/source_window") == _TORN, (
            "E.2 the drag's source window is the floater itself"
        )
        assert _TORN in _windows(tf), "E.3 the floater window was never removed by a move"
        assert _windows(tf)[_TORN]["decorations"] is False, "E.4 still borderless after moves"

        # ── (F) both chrome modes coexist ────────────────────────────
        _section("F: both modes supported — decorated main + borderless floater")
        w = _windows(tf)
        assert w[_MAIN]["decorations"] is True, "F.1 main: OS-decorated (OS-driven move, R1088)"
        assert w[_TORN]["decorations"] is False, "F.2 floater: borderless (custom title-bar move, R1116)"
        decorated = [k for k, v in w.items() if v["decorations"]]
        borderless = [k for k, v in w.items() if not v["decorations"]]
        assert decorated == [_MAIN] and borderless == [_TORN], (
            f"F.3 exactly one of each mode coexists (decorated={decorated}, borderless={borderless})"
        )
        # Redock (toggle) cleanly removes the floater — the move never corrupted it.
        tf.invoke(f"/{_VIEWPORT}/external/tear_off", None)
        wait_until(lambda: _TORN not in _windows(tf), desc="F.4 toggle redocks the moved floater")
        assert set(_windows(tf)) == {_MAIN}, "F.5 back to one window; the moved floater redocked cleanly"
        assert _has_tag(_main_scene(tf), f"{_VIEWPORT}_content_body"), "F.6 the viewport is docked again"

        print("[demo] r1116_window_move: all sections PASS (borderless title-bar window move, §2 #7)")


if __name__ == "__main__":
    sys.exit(run_demo("r1116_window_move", body))
