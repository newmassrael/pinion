#!/usr/bin/env python3
"""R1102 §5.51 §2 #7 PR-33 — cross-window redock fires LIVE (mutation slice 2).

R1098 built the cross-window drop geometry, R1099 made it AI-observable as a
read (`scene/cross_window_drop`), R1100 gave `DockPanelExternal` a `over_window`
to consume + a `TEAR_OFF_REDOCK_AT` intent to emit, R1101 cleared the dispatch
SSOT. But the per-window `InputRouter` is cross-window-blind, so it always passed
`over_window: None` — the R1100 contract never fired on a live drag.

R1102 wires it: the shell (the sole holder of every window's geometry) resolves
the in-flight drag's abs cursor against the OTHER windows each move (source
window excluded — the F5 caller-owned exclusion the R1101 doc promised) and
stashes the result on the drag session, so the router fills `over_window` once
the cursor escapes this window's own drop targets (own-window first). The dragged
panel then emits the cross-window dock-at, recorded on the persistent
`query("redock_at")` diagnostic (the transient intent is drained into the
reducer; the diagnostic persists for an AI to observe).

The headline path: tear a panel into a real second window, then drag ANOTHER
panel out of main onto that floating window's region. The per-window router for
`main` resolves `None` there (the cursor is outside main); the shell maps it into
the floater and the panel redocks into THAT window — driven + observed headlessly
through `scene/drag` + `scene/query`, no cross-window pointer grab (HW-gated).

Section roadmap (>=30 assertions across A-F):

  (A) Boot — single WM-placed "main", three panels docked, every panel's
      redock_at null (no cross-window drag yet).
  (B) Tear-off — drag the inspector out; a second declared window appears at the
      escape position.
  (C) Cross-window redock (the headline) — drag the PROPERTY panel out of main
      onto the floating window's region; property's redock_at names the FLOATING
      window + the zone the cursor landed on (the live firing of the contract).
  (D) Same-window control — a drag that stays over main's own dock is a
      same-window reorganize: the dragged panel's redock_at stays null.
  (E) Read-only — `scene/intervene` on redock_at is rejected (router-driven).
  (F) The read peer still agrees — `scene/cross_window_drop` resolves the same
      floating window for the same abs cursor (the R1099 read + the R1102 write
      share one resolver).
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import RpcError, RpcSubprocess, run_demo, wait_until  # noqa: E402

_INSPECTOR = "inspector"
_PROPERTY = "property"
_VIEWPORT = "viewport"
_INSPECTOR_HEADER = "inspector#header"
_PROPERTY_HEADER = "property#header"
_VIEWPORT_HEADER = "viewport#header"
_FLOATING_PREFIX = "torn-"
_MAIN_W = 880
_MAIN_H = 600


# ─── helpers ─────────────────────────────────────────────────────────


def _windows(tf: RpcSubprocess) -> list[dict]:
    resp = tf.request("scene/windows", {})
    assert resp is not None and resp.result is not None, "scene/windows must answer"
    ws = resp.result.get("windows")
    assert isinstance(ws, list), f"scene/windows.windows must be a list; got {resp.result!r}"
    return ws


def _window_by_id(tf: RpcSubprocess, wid: str) -> Optional[dict]:
    return next((w for w in _windows(tf) if w.get("id") == wid), None)


def _layout(tf: RpcSubprocess) -> Any:
    resp = tf.request("scene/layout", {"viewport": {"width": _MAIN_W, "height": _MAIN_H}})
    assert resp is not None
    return resp.result


def _find_rect(layout: Any, tag: str) -> Optional[dict[str, float]]:
    def walk(node: Any) -> Optional[dict[str, Any]]:
        if not isinstance(node, dict):
            return None
        if node.get("tag") == tag:
            return node.get("rect")
        for child in node.get("children") or []:
            r = walk(child)
            if r is not None:
                return r
        content = node.get("content")
        if isinstance(content, dict):
            return walk(content)
        return None

    rect = walk(layout)
    if not isinstance(rect, dict):
        return None
    return {k: float(rect.get(k, 0)) for k in ("x", "y", "w", "h")}


def _center(rect: dict[str, float]) -> tuple[float, float]:
    return rect["x"] + rect["w"] * 0.5, rect["y"] + rect["h"] * 0.5


def _redock_at(tf: RpcSubprocess, panel: str) -> Any:
    return tf.query(f"/{panel}/external/redock_at")


def _detached(tf: RpcSubprocess, panel: str) -> Any:
    return tf.query(f"/{panel}/external/detached")


def _drag(tf: RpcSubprocess, window: str, from_path: str, to_at: tuple[float, float]) -> None:
    tf.request(
        "scene/drag",
        {
            "window": window,
            "from_path": from_path,
            "to": {"x": float(to_at[0]), "y": float(to_at[1])},
            "steps": 8,
        },
    )


def _resolve_cross(tf: RpcSubprocess, x: float, y: float) -> Any:
    resp = tf.request("scene/cross_window_drop", {"x": float(x), "y": float(y)})
    assert resp is not None and resp.result is not None, "cross_window_drop must answer"
    return resp.result.get("drop")


def _section(label: str) -> None:
    print(f"[demo] -- {label}")


# ─── demo body ───────────────────────────────────────────────────────


def body() -> None:
    insp_to = (1040.0, 200.0)  # well past the 880px main width → a clean escape

    with RpcSubprocess("hello-dock-panels", boot_grace=2.0) as tf:
        # ── (A) boot — one window, panels docked, no redock yet ──────
        _section("A: boot — single window, no cross-window redock yet")
        boot = _windows(tf)
        assert len(boot) == 1, f"A.1 boot declares one window; got {boot!r}"
        assert boot[0].get("id") == "main", f"A.2 the boot window is 'main'; got {boot[0]!r}"
        assert boot[0].get("position") is None, f"A.3 main is WM-placed (null position); {boot[0]!r}"
        for panel in (_INSPECTOR, _PROPERTY, _VIEWPORT):
            assert _redock_at(tf, panel) is None, f"A.4 {panel} redock_at null before any drag"
            assert _detached(tf, panel) is False, f"A.5 {panel} not detached at boot"

        # ── (B) tear off the inspector → a second declared window ────
        _section("B: tear-off creates a second declared window")
        _drag(tf, "main", _INSPECTOR_HEADER, insp_to)
        torn = f"{_FLOATING_PREFIX}{_INSPECTOR}"
        entry = wait_until(lambda: _window_by_id(tf, torn), desc="B.1 the floating window appears")
        assert len(_windows(tf)) == 2, "B.2 two declared windows after the tear-off"
        pos = entry.get("position")
        assert isinstance(pos, list) and len(pos) == 2, f"B.3 the floater has a position ({entry})"
        fx, fy = float(pos[0]), float(pos[1])
        assert (int(fx), int(fy)) == (int(insp_to[0]), int(insp_to[1])), (
            f"B.4 the floater sits at the escape cursor (main at origin) ({pos})"
        )
        assert _detached(tf, _INSPECTOR) is True, "B.5 the torn inspector reports detached"
        # The tear-off was a same-window escape (no other window existed yet), so
        # it floated — it did NOT cross-window redock.
        assert _redock_at(tf, _INSPECTOR) is None, "B.6 a tear-off-to-float is not a cross-window redock"

        # ── (C) cross-window redock — the headline ───────────────────
        _section("C: drag PROPERTY out of main onto the floating window → live redock")
        # Prime the floater's paint scene (R684 first-paint finalize) so its drop
        # targets resolve under the cross-window hit-test.
        size = entry.get("declared_size")
        fw, fh = (int(size[0]), int(size[1])) if isinstance(size, list) else (320, 480)
        tf.snapshot(source="paint", viewport=(fw, fh), window=torn)
        # A target a little inside the floater's region (its inspector panel fills
        # the window from its origin) — main is WM-placed at the origin, so the
        # main-local drag `to` IS the desktop-absolute point.
        target = (fx + 40.0, fy + 80.0)
        # Sanity (R1099 read peer): that abs cursor resolves the floater, not main.
        pre = wait_until(
            lambda: _resolve_cross(tf, target[0], target[1]),
            desc="C.1 the read peer resolves the floater for the target cursor",
        )
        assert pre["window"] == torn, f"C.2 the target abs cursor maps into the floater ({pre})"
        # Drag the property header from main out to that point. Early steps are
        # over main's own dock (same-window); the late steps + release escape main
        # (own resolve None) and map onto the floater (over_window = the floater).
        _drag(tf, "main", _PROPERTY_HEADER, target)
        observed = wait_until(
            lambda: _redock_at(tf, _PROPERTY),
            desc="C.3 property records a cross-window redock",
        )
        assert isinstance(observed, dict), f"C.4 redock_at is the dock-at payload ({observed})"
        assert observed["window"] == torn, (
            f"C.5 property redocked into the FLOATING window, not main ({observed})"
        )
        assert observed["panel"] == _PROPERTY, f"C.6 the redock names the property panel ({observed})"
        assert observed["target"] == _INSPECTOR, (
            f"C.7 the drop landed on the floater's inspector zone ({observed})"
        )
        assert 0.0 <= observed["x_rel"] <= 1.0 and 0.0 <= observed["y_rel"] <= 1.0, (
            f"C.8 the drop point is normalised over the zone ({observed})"
        )
        # The viewport, never dragged, recorded no cross-window redock.
        assert _redock_at(tf, _VIEWPORT) is None, "C.9 the un-dragged viewport has no redock_at"

        # ── (D) same-window control — a within-main drag never redocks ─
        _section("D: a drag that stays over main's own dock does NOT cross-window")
        vp = _find_rect(_layout(tf), _VIEWPORT)
        assert vp is not None, "D.1 the viewport still has a rect in main"
        # Drag the viewport header onto the viewport's OWN rect centre — a point
        # guaranteed to be over a main drop target the whole way, so the own
        # resolution is Some and over_window stays None (a same-window self-drop).
        within_main = _center(vp)
        _drag(tf, "main", _VIEWPORT_HEADER, within_main)
        assert _redock_at(tf, _VIEWPORT) is None, (
            "D.2 an own-window drop is a same-window reorganize, not a cross-window redock"
        )
        # Property's earlier cross-window redock is unaffected (it persists).
        assert isinstance(_redock_at(tf, _PROPERTY), dict), "D.3 property's redock_at still recorded"

        # ── (E) redock_at is read-only ───────────────────────────────
        _section("E: redock_at is a read-only diagnostic")
        try:
            tf.request("scene/intervene", {"path": f"/{_PROPERTY}/external/redock_at", "value": None})
            raise AssertionError("E.1 intervening on redock_at must error")
        except RpcError as exc:
            assert exc.code != 0, f"E.1 redock_at intervene rejected (code {exc.code})"

        # ── (F) the read peer agrees with the live write ─────────────
        _section("F: scene/cross_window_drop resolves the same floater")
        again = _resolve_cross(tf, target[0], target[1])
        assert again is not None and again["window"] == torn, (
            f"F.1 the read peer still resolves the floater ({again})"
        )
        assert again["tag"] == _INSPECTOR, f"F.2 the read peer's tag matches the redock target ({again})"

        print("[demo] r1102_cross_window_redock: all sections PASS")


if __name__ == "__main__":
    sys.exit(run_demo("r1102_cross_window_redock", body))
