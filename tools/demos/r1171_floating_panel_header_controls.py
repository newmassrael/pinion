#!/usr/bin/env python3
"""R1171 §5.16 §2 #7 — a torn-off panel's window controls (minimize / maximize /
close) live IN the panel HEADER, and the close docks the panel BACK.

SCOPE — read this before the assertions. R1170 first added the controls as a SHELL
chrome OVERLAY on the borderless floater. But that overlay is a separate LAYER from
the panel's own header (which is already the title bar + redock drag handle), so the
binding had to manually dimension-match the overlay to the header (height, then
colour) — the user caught it: "치수를 맞춰야 한다는 것 자체가 잘못된 설계 아니야?".
R1171 is the redesign: the controls are rendered IN the panel header
(`view_window_controls` → `view_dock_panel_with_actions`), one title bar, auto-sized
by the layout — and the shell's controls-only overlay is RETIRED. The shell keeps
only the close SEAM (`window_close_requested`) + tag routing (`try_chrome_press`).

This pins the §2 #7 scene-as-data: the floating window's header carries the three
control hit tags (and NO shell chrome STRIP / grip), the main window has none, and
the controls are DESCENDANTS of the header (controls-in-header, not an overlay). The
press ACTION (min/max/close) is winit-path → HW-gated; the close → dock-back is
unit-pinned (`r1171_floating_panel_header_*`).
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import RpcSubprocess, run_demo, wait_until  # noqa: E402

_MAIN = "main"
_PROPERTIES = "properties"
_CONSOLE = "console"
_ALL_PANELS = {"outliner", "viewport", _PROPERTIES, _CONSOLE}
_REORG = "/dock_reorganize/external"

_STRIP = "ai-overlay/window-chrome"
_CLOSE = "ai-overlay/window-chrome#close"
_MINIMIZE = "ai-overlay/window-chrome#minimize"
_MAXIMIZE = "ai-overlay/window-chrome#maximize"
_GRIP = "ai-overlay/window-chrome#grip"


def _torn(panel: str) -> str:
    return f"torn-{panel}"


def _windows(tf: RpcSubprocess) -> list[dict]:
    return tf.request("scene/windows", {}).result.get("windows") or []


def _window_ids(tf: RpcSubprocess) -> set[str]:
    return {w.get("id") for w in _windows(tf)}


def _scene(tf: RpcSubprocess, window: str) -> Any:
    size = (1200, 800) if window == _MAIN else (460, 380)
    result = tf.snapshot(source="paint", window=window, viewport=size)
    assert result is not None, f"snapshot {window} must answer"
    return result


def _find(scene: Any, tag: str) -> Any:
    if isinstance(scene, dict):
        if scene.get("tag") == tag:
            return scene
        for c in scene.get("children") or []:
            hit = _find(c, tag)
            if hit is not None:
                return hit
        if isinstance(scene.get("content"), dict):
            return _find(scene["content"], tag)
    return None


def _has(scene: Any, tag: str) -> bool:
    return _find(scene, tag) is not None


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


def _tear_off(tf: RpcSubprocess, panel: str) -> None:
    tf.invoke(f"/{panel}/external/tear_off", None)


def _section(label: str) -> None:
    print(f"[demo] -- {label}")


def body() -> None:
    with RpcSubprocess("hello-dock-panels-editor", boot_grace=2.0) as tf:
        # ── (A) boot — main window, no client controls ───────────────
        _section("A: boot — main window, OS-decorated (no client controls)")
        boot = _windows(tf)
        assert len(boot) == 1, f"A.1 one window at boot ({boot!r})"
        assert boot[0].get("id") == _MAIN, f"A.2 boot window is main ({boot[0]!r})"
        main = _scene(tf, _MAIN)
        assert not _has(main, _CLOSE), "A.3 main has NO client close control (OS decorations)"
        assert not _has(main, _STRIP), "A.4 main has NO shell chrome strip"
        assert not _has(main, _GRIP), "A.5 main has NO client grip"
        assert _panel_set(tf) == _ALL_PANELS, f"A.6 all 4 panels present ({_panel_set(tf)})"

        # ── (B) tear off properties → its HEADER carries the controls ─
        _section("B: tear off properties → header has min/max/close, no overlay strip")
        _tear_off(tf, _PROPERTIES)
        wait_until(lambda: _torn(_PROPERTIES) in _window_ids(tf), desc="B.1 properties floats")
        assert len(_windows(tf)) == 2, "B.2 two windows after the tear-off"
        torn = _scene(tf, _torn(_PROPERTIES))
        assert _has(torn, _CLOSE), "B.3 ★the floating window has a CLOSE control"
        assert _has(torn, _MINIMIZE), "B.4 ★the floating window has a MINIMIZE control"
        assert _has(torn, _MAXIMIZE), "B.5 ★the floating window has a MAXIMIZE control"
        assert not _has(torn, _GRIP), "B.6 NO grip (the header itself is the redock drag handle)"
        assert not _has(torn, _STRIP), (
            "B.7 ★NO shell chrome STRIP — the controls are in the header, not a shell overlay"
        )

        # ── (C) the controls are DESCENDANTS of the panel header ─────
        _section("C: the controls live INSIDE the panel header (controls-in-header)")
        header = _find(torn, f"{_PROPERTIES}#header")
        assert header is not None, "C.1 the floating panel has a header"
        for tag in (_MINIMIZE, _MAXIMIZE, _CLOSE):
            assert _find(header, tag) is not None, f"C.2 {tag} is INSIDE the header (not an overlay)"
        close = _find(torn, _CLOSE)["rect"]
        maxi = _find(torn, _MAXIMIZE)["rect"]
        mini = _find(torn, _MINIMIZE)["rect"]
        assert close["x"] > maxi["x"] > mini["x"], (
            f"C.3 order min<max<close left-to-right ({mini['x']},{maxi['x']},{close['x']})"
        )
        assert close["x"] == maxi["x"] + maxi["w"], "C.4 close abuts maximize (contiguous cluster)"
        assert maxi["x"] == mini["x"] + mini["w"], "C.5 maximize abuts minimize (contiguous cluster)"
        assert mini["h"] == maxi["h"] == close["h"], "C.6 the controls share a row height"
        assert close["w"] > 0 and close["h"] > 0, "C.7 the close control has a real hit rect"

        # ── (D) a SECOND torn panel also gets header controls ────────
        _section("D: a second torn panel (console) also gets header controls")
        _tear_off(tf, _CONSOLE)
        wait_until(lambda: _torn(_CONSOLE) in _window_ids(tf), desc="D.1 console floats")
        assert len(_windows(tf)) == 3, "D.2 three windows now"
        torn_c = _scene(tf, _torn(_CONSOLE))
        assert _has(torn_c, _CLOSE), "D.3 console's floating window has a close control"
        assert _has(torn_c, _MINIMIZE), "D.4 console's floating window has a minimize control"
        assert _has(torn_c, _MAXIMIZE), "D.5 console's floating window has a maximize control"
        assert not _has(torn_c, _STRIP), "D.6 console's floating window has no shell strip"

        # ── (E) the main window STILL has no client controls ─────────
        _section("E: the main window keeps OS decorations throughout")
        main2 = _scene(tf, _MAIN)
        assert not _has(main2, _CLOSE), "E.1 main still has no client close control"
        assert not _has(main2, _MINIMIZE), "E.2 main still has no client minimize control"
        assert not _has(main2, _STRIP), "E.3 main still has no shell chrome strip"

        # ── (F) integrity — every panel survives the tear-offs ───────
        _section("F: integrity — panels conserved, reads deterministic")
        assert _panel_set(tf) == _ALL_PANELS, "F.1 all 4 panels intact after tear-offs"
        assert _window_ids(tf) == {_MAIN, _torn(_PROPERTIES), _torn(_CONSOLE)}, (
            f"F.2 exactly main + the two torn windows ({_window_ids(tf)})"
        )
        a = tf.query(f"{_REORG}/topology")
        b = tf.query(f"{_REORG}/topology")
        assert a == b, "F.3 back-to-back topology reads are identical"

        print("[demo] r1171_floating_panel_header_controls: all sections PASS (controls-in-header)")


if __name__ == "__main__":
    sys.exit(run_demo("r1171_floating_panel_header_controls", body))
