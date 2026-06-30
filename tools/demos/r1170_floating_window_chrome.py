#!/usr/bin/env python3
"""R1170 §5.16 §5.39 §2 #7 — a torn-off panel's floating window gets client-side
window CONTROLS (minimize / maximize / close), and its close docks the panel BACK
instead of quitting the app.

SCOPE — read this before the assertions. A torn-off dock panel opens as a
BORDERLESS window (`decorations:false`, R1115) so its custom header can drive the
redock drag (an OS title bar would do OS-move and break redock). But R1121 deferred
the actual window controls, so a torn-off panel had NO visible way to be minimized,
maximized, or closed — the user's "왜 undock했을때 vscode처럼 최소화, 최대화,
exit가 없어?". R1170 closes it:

  * The editor's `window_chrome(torn-{panel})` returns a CONTROLS-ONLY
    `WindowChromeStyle` — the three buttons but NO grip (the panel header stays the
    redock drag handle) and NO content inset (the buttons OVERLAY the header).
  * Close routes through the new `WidgetView::window_close_requested` shell seam: a
    torn window's close docks the panel BACK (drops its `WindowSpec`), only the
    PRIMARY window's close exits the app. (min / max are the shell's per-window
    `set_minimized` / `set_maximized`.)

This pins the §2 #7 scene-as-data: the floating window paints the three control
buttons, no grip, and the main window keeps its OS decorations (no client chrome).
The close ACTION (dock-back) is unit-pinned (`r1170_torn_window_*`); the live
min/max/close FEEL is HW-gated (a real `:0` press on the buttons).
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
_ALL_PANELS = {"toolbar", "outliner", "viewport", _PROPERTIES, _CONSOLE}
_REORG = "/dock_reorganize/external"

_STRIP = "ai-overlay/window-chrome"
_CLOSE = "ai-overlay/window-chrome#close"
_MINIMIZE = "ai-overlay/window-chrome#minimize"
_MAXIMIZE = "ai-overlay/window-chrome#maximize"
_GRIP = "ai-overlay/window-chrome#grip"


# ─── helpers ─────────────────────────────────────────────────────────


def _torn(panel: str) -> str:
    return f"torn-{panel}"


def _windows(tf: RpcSubprocess) -> list[dict]:
    return tf.request("scene/windows", {}).result.get("windows") or []


def _window_ids(tf: RpcSubprocess) -> set[str]:
    return {w.get("id") for w in _windows(tf)}


def _scene(tf: RpcSubprocess, window: str) -> Any:
    # R883.1 — scope the paint snapshot to a named window via the `window=` param
    # (NOT a `/window[id]` path, which only resolves the primary). Each window
    # paints at its own size; a torn-off floater is 460x380 (floating_window_spec).
    size = (1200, 800) if window == _MAIN else (460, 380)
    result = tf.snapshot(source="paint", window=window, viewport=size)
    assert result is not None, f"snapshot {window} must answer"
    return result


def _find(scene: Any, tag: str) -> Any:
    """The first node carrying `tag` (with a non-zero rect), or None."""
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


# ─── demo body ───────────────────────────────────────────────────────


def body() -> None:
    with RpcSubprocess("hello-dock-panels-editor", boot_grace=2.0) as tf:
        # ── (A) boot — one main window, no client chrome on it ───────
        _section("A: boot — main window, OS-decorated (no client chrome)")
        boot = _windows(tf)
        assert len(boot) == 1, f"A.1 one window at boot ({boot!r})"
        assert boot[0].get("id") == _MAIN, f"A.2 boot window is main ({boot[0]!r})"
        main = _scene(tf, _MAIN)
        assert not _has(main, _CLOSE), "A.3 main has NO client close button (OS decorations)"
        assert not _has(main, _GRIP), "A.4 main has NO client grip (OS title bar)"
        assert _panel_set(tf) == _ALL_PANELS, f"A.5 all 5 panels present ({_panel_set(tf)})"

        # ── (B) tear off properties → its floating window has CONTROLS ─
        _section("B: tear off properties → floating window gets min/max/close")
        _tear_off(tf, _PROPERTIES)
        wait_until(lambda: _torn(_PROPERTIES) in _window_ids(tf), desc="B.1 properties floats")
        assert len(_windows(tf)) == 2, "B.2 two windows after the tear-off"
        torn = _scene(tf, _torn(_PROPERTIES))
        assert _has(torn, _CLOSE), "B.3 ★the floating window has a CLOSE button"
        assert _has(torn, _MINIMIZE), "B.4 ★the floating window has a MINIMIZE button"
        assert _has(torn, _MAXIMIZE), "B.5 ★the floating window has a MAXIMIZE button"
        assert not _has(torn, _GRIP), (
            "B.6 ★controls-only: NO grip (the panel header owns the redock drag)"
        )
        assert _has(torn, _STRIP), "B.7 the floating window carries the chrome strip container"
        assert not _has(main, _STRIP), "B.8 the main window carries NO chrome strip"

        # ── (C) the buttons anchor at the top-right, in order ────────
        _section("C: the controls anchor top-right (close rightmost)")
        close = _find(torn, _CLOSE)["rect"]
        maxi = _find(torn, _MAXIMIZE)["rect"]
        mini = _find(torn, _MINIMIZE)["rect"]
        assert close["y"] == 0, f"C.1 close sits at the top edge ({close!r})"
        assert close["x"] > maxi["x"], f"C.2 close is right of maximize ({close} vs {maxi})"
        assert maxi["x"] > mini["x"], f"C.3 maximize is right of minimize ({maxi} vs {mini})"
        assert close["w"] > 0 and close["h"] > 0, "C.4 the close button has a real hit rect"
        assert mini["w"] == maxi["w"] == close["w"], "C.5 the three buttons are equal width"
        assert mini["h"] == maxi["h"] == close["h"], "C.6 the three buttons are equal height"
        assert close["x"] == maxi["x"] + maxi["w"], "C.7 close abuts maximize (no gap/overlap)"
        assert maxi["x"] == mini["x"] + mini["w"], "C.8 maximize abuts minimize (contiguous cluster)"

        # ── (D) a SECOND torn panel also gets controls (uniform) ─────
        _section("D: a second torn panel (console) also gets controls")
        _tear_off(tf, _CONSOLE)
        wait_until(lambda: _torn(_CONSOLE) in _window_ids(tf), desc="D.1 console floats")
        assert len(_windows(tf)) == 3, "D.2 three windows now"
        torn_c = _scene(tf, _torn(_CONSOLE))
        assert _has(torn_c, _CLOSE), "D.3 console's floating window has a close button"
        assert _has(torn_c, _MINIMIZE), "D.4 console's floating window has a minimize button"
        assert _has(torn_c, _MAXIMIZE), "D.5 console's floating window has a maximize button"
        assert not _has(torn_c, _GRIP), "D.6 console's floating window has no grip"

        # ── (E) the main window STILL has no client chrome ───────────
        _section("E: the main window keeps OS decorations throughout")
        main2 = _scene(tf, _MAIN)
        assert not _has(main2, _CLOSE), "E.1 main still has no client close button"
        assert not _has(main2, _MINIMIZE), "E.2 main still has no client minimize button"
        assert not _has(main2, _GRIP), "E.3 main still has no client grip"

        # ── (F) integrity — every panel survives the tear-offs ───────
        _section("F: integrity — panels conserved, reads deterministic")
        assert _panel_set(tf) == _ALL_PANELS, "F.1 all 5 panels intact after tear-offs"
        assert _window_ids(tf) == {_MAIN, _torn(_PROPERTIES), _torn(_CONSOLE)}, (
            f"F.2 exactly main + the two torn windows ({_window_ids(tf)})"
        )
        a = tf.query(f"{_REORG}/topology")
        b = tf.query(f"{_REORG}/topology")
        assert a == b, "F.3 back-to-back topology reads are identical"

        print("[demo] r1170_floating_window_chrome: all sections PASS (floating window controls)")


if __name__ == "__main__":
    sys.exit(run_demo("r1170_floating_window_chrome", body))
