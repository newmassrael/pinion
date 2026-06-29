#!/usr/bin/env python3
"""R1155 §5.51 §2 #7 — the cross-window redock EDGE SNAP: an edge-flush slot (a
thin top toolbar pinned to the window's top border) catches a near-miss just
OUTSIDE the window edge, where strict containment resolves to None.

SCOPE — read this before the assertions. A torn-off panel redocks by mapping the
release cursor (desktop-absolute) against the host window's drop targets;
`scene/cross_window_drop` is the AI-introspection peer of that resolution
(§2 #7). Pre-R1155 the resolution was strict CONTAINMENT: the cursor had to land
INSIDE a drop target's rect. A toolbar is a ~48px strip flush against the
window's TOP border, so its band is thin AND a cursor a few px above the border
is out-of-window — strict containment returns None and the floater stays
floating instead of docking (the user's "the thin slot is uncatchable" report).

R1155 adds a second pass: when no host CONTAINS the cursor, snap to the nearest
drop target within EDGE_SNAP_MARGIN (24 logical px) and resolve there. It is
purely ADDITIVE — only a would-be None reaches the snap, so no in-rect
resolution changes. This demo drives `scene/cross_window_drop` at a vertical
sweep through and ABOVE the editor's toolbar and asserts:

  (A) boot — one 'main' window; the toolbar resolves at its interior.
  (B) EDGE SNAP — every cursor 1..24px ABOVE the window's top edge resolves to
      the toolbar. Strict containment can NEVER resolve an out-of-window cursor,
      so each of these is the R1155 snap (pre-R1155 = null = no dock).
  (C) MARGIN — a cursor >24px above the edge resolves to None (an intentional
      float in open space still floats).
  (D) EXACT INTACT — a cursor inside the toolbar resolves with its true interior
      position; a cursor inside the panel BELOW resolves to that panel, never
      snapped up (the additive snap never steals from a contained slot).

The RPC resolves against each window's DECLARED origin; the WM-placed 'main'
declares None -> (0,0), so the abs cursor is client-relative here. The live FEEL
— how easy the thin slot is to hit by hand — is HW-gated (a real desktop); this
pins what is observable as scene-as-data.
"""
from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import RpcSubprocess, run_demo  # noqa: E402

_MAIN = "main"
_X = 120.0  # within the full-width toolbar's x-range
_MARGIN = 24  # EDGE_SNAP_MARGIN (logical px) — keep in sync with input.rs


def _drop(tf: RpcSubprocess, x: float, y: float):
    resp = tf.request("scene/cross_window_drop", {"x": float(x), "y": float(y)})
    assert resp is not None and resp.result is not None, "scene/cross_window_drop must answer"
    return resp.result.get("drop")


def _tag(tf: RpcSubprocess, x: float, y: float):
    drop = _drop(tf, x, y)
    return drop.get("tag") if drop else None


def _section(label: str) -> None:
    print(f"[demo] -- {label}")


def body() -> None:
    with RpcSubprocess("hello-dock-panels-editor", boot_grace=2.0) as tf:
        _section("A: boot — one main window, toolbar present at its interior")
        wins = tf.request("scene/windows", {}).result.get("windows") or []
        assert len(wins) == 1, f"A.1 boot declares one window; got {wins!r}"
        assert wins[0].get("id") == _MAIN, f"A.2 the boot window is main; got {wins[0]!r}"
        assert _tag(tf, _X, 20) == "toolbar", "A.3 the toolbar resolves at its interior"

        _section("B: EDGE SNAP — cursors 1..24px ABOVE the window top edge dock to toolbar")
        # The window's client top edge is y=0; every y in [-24, -1] is OUTSIDE the
        # window. Strict containment can never resolve an out-of-window cursor, so
        # each resolve here is purely the R1155 edge snap (pre-R1155 = null).
        for y in range(-_MARGIN, 0):  # -24 .. -1 inclusive = 24 points
            tag = _tag(tf, _X, y)
            assert tag == "toolbar", (
                f"B y={y}: a near-miss {abs(y)}px above the edge must snap to the "
                f"toolbar; got {tag!r}"
            )

        _section("C: MARGIN — a cursor beyond 24px above the edge still floats (None)")
        for y in (-_MARGIN - 1, -_MARGIN - 2, -_MARGIN - 4, -_MARGIN - 8, -_MARGIN - 16, -_MARGIN - 28):
            tag = _tag(tf, _X, y)
            assert tag is None, (
                f"C y={y}: {abs(y)}px above the edge is beyond the {_MARGIN}px "
                f"margin -> float; got {tag!r}"
            )

        _section("D: EXACT INTACT — inside resolutions unchanged; the snap steals nothing")
        inside = _drop(tf, _X, 30)
        assert inside is not None and inside.get("tag") == "toolbar", "D.1 inside the toolbar resolves exactly"
        assert inside.get("y_rel", 0.0) > 0.0, f"D.2 the interior position is not snapped to an edge ({inside!r})"
        for y in (0, 8, 16, 24, 32, 40):
            assert _tag(tf, _X, y) == "toolbar", f"D.3 y={y} inside the toolbar resolves to toolbar"
        # A cursor INSIDE the panel below the toolbar resolves to THAT panel via the
        # exact pass — the additive snap never pulls a contained cursor up.
        below = _tag(tf, _X, 100)
        assert below is not None and below != "toolbar", (
            f"D.4 a cursor inside the panel below resolves to it, not the toolbar; got {below!r}"
        )

        print("[demo] r1155_edge_snap_redock: all sections PASS (cross-window redock edge snap)")


if __name__ == "__main__":
    sys.exit(run_demo("r1155_edge_snap_redock", body))
