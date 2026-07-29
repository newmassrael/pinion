#!/usr/bin/env python3
"""R881 §5.35 §5.49 — middle-button drag-to-pan (Blender / Unreal idiom).

Drives `hello-node-editor` via JSON-RPC. `scene/drag {button: "middle"}` is
the new §2 #2 peer of a physical middle-button drag: the router opens a
middle gesture at the press (pan targets pinned — the hovered External and
the deepest scrollable), the `DragLatch` dead zone splits click from pan,
and each latched move dispatches the `last - current` cursor delta through
the SAME two-stage wheel routing R877 built (External offer first, scroll
fallback second). Content follows the cursor — the grab convention. The
canvas binding contributes ZERO pan code: plain deltas fall through to the
world `ScrollNode`, and a held `Ctrl` reaches the canvas's existing
`Ctrl`-wheel zoom arm (the Blender chord, free via vocabulary reuse). A
press-release in place resolves to the middle-*click* (the X11 PRIMARY
paste funnel — now fired at release, never after a pan).

  (A) boot — viewport at origin, 100% zoom.
  (B) middle-drag pans; content follows the cursor on both axes.
  (C) reverse middle-drag pans back and clamps at the world origin.
  (D) middle press-release in place: no pan, no selection side effect.
  (E) Ctrl+middle-drag zooms via the canvas's wheel chord arm.
  (F) middle-drag over the palette strip pans nothing.
  (G) out-of-vocabulary button name rejects with invalid_params.

10 assertions (+4 observed-state wait_until gates).
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    assert_eq,
    run_demo,
    wait_until,
)

EXAMPLE = "hello-node-editor"
G = "node_graph"
PALETTE_W = 132
CANVAS_W, CANVAS_H = 640, 420
ZOOM_STEP = 1.2
CENTER = (PALETTE_W + CANVAS_W / 2, CANVAS_H / 2)


def vq(tf, axis: str) -> float:
    return float(tf.query(f"/external/viewport.{axis}"))


def pan(tf) -> tuple[float, float]:
    return (vq(tf, "x"), vq(tf, "y"))


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) boot: viewport at origin, 100% zoom ─────────────────
        assert_eq((*pan(tf), vq(tf, "zoom")), (0.0, 0.0, 1.0),
                  "boot viewport = origin @ 100%")

        # ── (B) middle-drag pans; content follows the cursor ────────
        # Cursor moves (-80, -60); the world under it follows, so the
        # viewport scrolls (+80, +60) — at 100% zoom, 1 px = 1 unit.
        tf.drag(from_at=CENTER, to_at=(CENTER[0] - 80, CENTER[1] - 60),
                button="middle")
        wait_until(lambda: vq(tf, "x") > 0, timeout=4.0, interval=0.03,
                   desc="middle drag panned the view")
        assert_eq(pan(tf), (80.0, 60.0), "content follows the cursor (x, y)")

        # ── (C) reverse drag pans back, clamping at the origin ──────
        tf.drag(from_at=CENTER, to_at=(CENTER[0] + 200, CENTER[1] + 200),
                button="middle")
        wait_until(lambda: vq(tf, "x") == 0.0, timeout=4.0, interval=0.03,
                   desc="reverse middle drag returned to the origin")
        assert_eq(pan(tf), (0.0, 0.0), "pan clamps at the world origin")

        # ── (D) middle click-in-place: a click, never a pan ─────────
        # steps=0 collapses to press/release at one point — inside the
        # DragLatch dead zone, so the router resolves Click (the paste
        # funnel; a no-op for the canvas) and nothing pans or selects.
        sel_before = tf.query("/external/selected_ids")
        tf.drag(from_at=CENTER, to_at=CENTER, steps=0, button="middle")
        assert_eq(pan(tf), (0.0, 0.0), "click-in-place pans nothing")
        assert_eq(tf.query("/external/selected_ids"), sel_before,
                  "middle click never mutates the selection")

        # ── (E) Ctrl+middle-drag = the canvas zoom chord ────────────
        # The pan rides the wheel vocabulary, so a held Ctrl reaches
        # the canvas's R877 Ctrl-wheel arm: 48 px of vertical middle
        # drag = 48/16 wheel lines = ZOOM_STEP**3, anchored per-move.
        tf.modifiers(ctrl=True)
        tf.drag(from_at=CENTER, to_at=(CENTER[0], CENTER[1] + 48),
                button="middle")
        tf.modifiers()
        wait_until(lambda: vq(tf, "zoom") > 1.0, timeout=4.0, interval=0.03,
                   desc="ctrl+middle drag zoomed in")
        zoom = vq(tf, "zoom")
        assert abs(zoom - ZOOM_STEP ** 3) < 1e-3, \
            f"48 px of chorded drag = three zoom steps, got {zoom}"
        tf.intervene("/external/viewport.zoom", 1.0)
        tf.intervene("/external/viewport.x", 0.0)
        tf.intervene("/external/viewport.y", 0.0)

        # ── (F) middle-drag over the palette pans nothing ───────────
        # The press pins targets at the press point: the palette strip
        # sits outside the world ScrollNode and the canvas declines
        # out-of-rect offers (R799 bounds guard), so the gesture has no
        # pan target — viewport stays put.
        tf.drag(from_at=(PALETTE_W / 2, CANVAS_H / 2),
                to_at=(PALETTE_W / 2, CANVAS_H / 2 - 100), button="middle")
        assert_eq((*pan(tf), vq(tf, "zoom")), (0.0, 0.0, 1.0),
                  "a palette middle drag neither pans nor zooms")

        # ── (G) unknown button rejects loudly ───────────────────────
        try:
            tf.request("scene/drag", {
                "from": {"x": 200.0, "y": 200.0},
                "to": {"x": 300.0, "y": 300.0},
                "button": "right",
            })
            raise AssertionError("button=right must reject")
        except RpcError as err:
            assert_eq(err.code, -32602, "out-of-vocabulary button = invalid_params")

        # The rejected request enqueued nothing — the canvas is still
        # at the origin and a follow-up middle pan works normally.
        assert_eq(pan(tf), (0.0, 0.0), "rejected drag injected no input")
        tf.drag(from_at=CENTER, to_at=(CENTER[0] - 16, CENTER[1]),
                button="middle")
        wait_until(lambda: vq(tf, "x") > 0, timeout=4.0, interval=0.03,
                   desc="middle pan still works after the rejection")
        assert_eq(pan(tf), (16.0, 0.0), "follow-up middle pan lands exactly")


if __name__ == "__main__":
    sys.exit(run_demo("R881 middle-button drag-to-pan", body))
