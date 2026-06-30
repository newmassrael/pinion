#!/usr/bin/env python3
"""R1172 §5.16 §2 #7 — per-panel MOVE / FLOAT policy: a LOCKED toolbar.

SCOPE — read this before the assertions. A pro dock (Qt ADS, VS Code) can mark a
panel non-movable (a fixed toolbar / status bar that the user cannot drag, reorder,
or tear off) and non-floatable (dockable but never torn off). pinion had only
`DockPanelStyle.drop_target` (opt out of RECEIVING a dock); R1172 adds the move-OUT
side: `DockPanelExternal::with_movable(false)` makes `begin_drag` return `None` (no
drag session opens at all), and `with_floatable(false)` makes a drag that escapes
every dock zone SNAP BACK instead of floating. The editor locks its `toolbar`.

This pins the §2 #7 scene-as-data: the toolbar reports `movable=false` and a drag of
its header opens no session (no tear-off, topology unchanged), while every other
panel is freely movable + floatable (a control drag floats one). The live drag FEEL
is HW-gated; this drives it via `scene/drag` + observes the result as data.
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import RpcError, RpcSubprocess, run_demo, wait_until  # noqa: E402

_MAIN = "main"
_TOOLBAR = "toolbar"
_OUTLINER = "outliner"
_VIEWPORT = "viewport"
_PROPERTIES = "properties"
_CONSOLE = "console"
_ALL_PANELS = {_TOOLBAR, _OUTLINER, _VIEWPORT, _PROPERTIES, _CONSOLE}
_MOVABLE = {_OUTLINER, _VIEWPORT, _PROPERTIES, _CONSOLE}
_REORG = "/dock_reorganize/external"


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


def _movable(tf: RpcSubprocess, panel: str) -> Any:
    return tf.query(f"/{panel}/external/movable")


def _floatable(tf: RpcSubprocess, panel: str) -> Any:
    return tf.query(f"/{panel}/external/floatable")


def _drag_header_out(tf: RpcSubprocess, panel: str) -> None:
    # Drag the panel's header far past the window edge → an escaped (Float) drag.
    tf.request(
        "scene/drag",
        {"window": _MAIN, "from_path": f"{panel}#header", "to": {"x": 2000.0, "y": 400.0}, "steps": 8},
    )


def _section(label: str) -> None:
    print(f"[demo] -- {label}")


def body() -> None:
    with RpcSubprocess("hello-dock-panels-editor", boot_grace=2.0) as tf:
        # ── (A) boot — the toolbar is locked, the rest are movable ────
        _section("A: boot — the toolbar reports movable=false, others movable=true")
        wins = _windows(tf)
        assert len(wins) == 1, f"A.1 one window at boot ({wins!r})"
        assert wins[0].get("id") == _MAIN, f"A.2 boot window is main ({wins[0]!r})"
        assert _panel_set(tf) == _ALL_PANELS, f"A.3 all 5 panels present ({_panel_set(tf)})"
        assert _movable(tf, _TOOLBAR) is False, "A.4 ★the toolbar is NON-movable (locked)"
        for panel in _MOVABLE:
            assert _movable(tf, panel) is True, f"A.5 {panel} is movable"
        # Every panel is floatable by default (the editor only locks MOVE on the toolbar).
        for panel in _MOVABLE:
            assert _floatable(tf, panel) is True, f"A.6 {panel} is floatable"
        assert _floatable(tf, _TOOLBAR) is True, "A.7 the toolbar's floatable default is true (moot)"
        assert _window_ids(tf) == {_MAIN}, "A.8 only the main window at boot"

        # ── (B) the locked toolbar is inert — no gesture, still docked ─
        _section("B: the locked toolbar is inert (movable=false, no drag possible)")
        # The toolbar is movable=false (A.4) AND headerless (R1173 — the menu strip
        # is the content, no title-bar handle), so there is nothing to grab and the
        # policy denies a drag regardless. Its drag diagnostics stay at rest.
        assert tf.query(f"/{_TOOLBAR}/external/dragging") is False, "B.1 the toolbar never entered a drag"
        assert tf.query(f"/{_TOOLBAR}/external/tear_off_fired") is False, "B.2 no tear-off fired"
        assert tf.query(f"/{_TOOLBAR}/external/detached") is False, "B.3 the toolbar never detached"
        assert _TOOLBAR in _panel_set(tf), "B.4 the toolbar is docked"
        assert _window_ids(tf) == {_MAIN}, "B.5 no floating window"

        # ── (C) control — a MOVABLE panel DOES tear off ──────────────
        _section("C: control — a movable panel (console) DOES float on the same drag")
        _drag_header_out(tf, _CONSOLE)
        wait_until(lambda: _torn(_CONSOLE) in _window_ids(tf), desc="C.1 console floats (control)")
        assert _torn(_CONSOLE) in _window_ids(tf), "C.2 ★console tore off (the drag mechanism works)"
        assert len(_windows(tf)) == 2, "C.3 a new floating window appeared"
        assert tf.query(f"/{_CONSOLE}/external/tear_off_fired") is True, "C.4 console's tear-off fired"

        # ── (D) the policy is read-only introspection (§2 #7) ────────
        _section("D: the move/float policy is read-only scene-as-data")
        assert _movable(tf, _TOOLBAR) is False, "D.1 toolbar movable=false (stable)"
        assert _movable(tf, _CONSOLE) is True, "D.2 console movable=true"
        assert _floatable(tf, _TOOLBAR) is True, "D.3 toolbar floatable default true (moot — never moves)"
        assert _movable(tf, _OUTLINER) is True, "D.4 outliner movable=true"
        assert _movable(tf, _VIEWPORT) is True, "D.5 viewport movable=true"
        assert _movable(tf, _PROPERTIES) is True, "D.6 properties movable=true"
        assert _floatable(tf, _CONSOLE) is True, "D.7 console floatable=true"
        assert _floatable(tf, _OUTLINER) is True, "D.8 outliner floatable=true"
        # The policy is read-only scene-as-data — an AI cannot mutate it via intervene.
        try:
            tf.request("scene/intervene", {"path": f"/{_TOOLBAR}/external/movable", "value": True})
            raise AssertionError("D.9 intervening on the movable policy must error")
        except RpcError as exc:
            assert exc.code != 0, f"D.9 movable is read-only (intervene rejected, code {exc.code})"

        # ── (E) integrity — every panel survives ─────────────────────
        _section("E: integrity — panels conserved, reads deterministic")
        assert _panel_set(tf) == _ALL_PANELS, "E.1 all 5 panels intact"
        assert _TOOLBAR in _panel_set(tf), "E.2 the locked toolbar never left the dock"
        a = tf.query(f"{_REORG}/topology")
        b = tf.query(f"{_REORG}/topology")
        assert a == b, "E.3 back-to-back topology reads are identical"

        print("[demo] r1172_locked_panel_policy: all sections PASS (locked toolbar)")


if __name__ == "__main__":
    sys.exit(run_demo("r1172_locked_panel_policy", body))
