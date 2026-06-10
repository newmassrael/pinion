#!/usr/bin/env python3
"""R882 §5.35 §5.39 §5.49 — Space-hold left-drag pan (Figma / Photoshop hand tool).

Drives `hello-node-editor` via JSON-RPC. `scene/key {state: "down"|"up"}`
is the new winit `KeyboardInput` Pressed/Released RPC peer: the shell now
tracks held-key absolute state (the Space pan chord) out-of-band, exactly
like the R763 `scene/modifiers` cache. While Space is held, a LEFT press
enters the router's pan channel — the SAME R881 gesture engine the middle
button uses (pinned targets, DragLatch dead zone, two-stage wheel-dialect
dispatch), just opened from the left-drag channel. No widget sees the
press: no focus steal, no node select, no marquee. Release routing follows
the gesture in flight, so lifting Space mid-pan never strands it. The
legacy edgeless `scene/key` stays an atomic press that can never strand
the chord.

  (A) boot — viewport at origin, 100% zoom, empty selection.
  (B) Space-hold left-drag pans; the marquee that same drag would have
      swept never fires (selection untouched).
  (C) reverse chord-drag pans back and clamps at the world origin.
  (D) Space+click on a node is inert (no select, no focus-class probe).
  (E) Space up restores the normal left arc: the same sweep marquees,
      the same click selects, and nothing pans.
  (F) Ctrl+Space-drag reaches the canvas zoom chord (wheel-dialect reuse).
  (G) the legacy edgeless scene/key never arms the chord.
  (H) out-of-vocabulary state rejects with invalid_params; the chord
      still works after the rejection.

Scope note: gesture-capture across a chord lift (Space released
mid-pan keeps the pan) is pinned at the shell tier by unit test
(`r882_chord_lift_mid_gesture_release_still_resolves_in_pan_channel`)
— the atomic `scene/drag` wire cannot interleave a key edge inside a
drag, so it is not demoable here.

13 assertions (+8 observed-state wait_until gates).
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


def W(gx: float, gy: float) -> tuple[float, float]:
    """Graph units -> window-absolute logical px (zoom 1, pan 0)."""
    return (PALETTE_W + gx, gy)


def vq(tf, axis: str) -> float:
    return float(tf.query(f"/external/viewport.{axis}"))


def pan(tf) -> tuple[float, float]:
    return (vq(tf, "x"), vq(tf, "y"))


def ids(tf) -> str:
    return tf.query("/external/selected_ids")


def space(tf, state: str) -> None:
    """Drive the Space chord edge (the winit Pressed/Released peer)."""
    tf.key(at=(10.0, 10.0), name="Space", state=state)


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) boot: origin, 100% zoom, nothing selected ───────────
        assert_eq((*pan(tf), vq(tf, "zoom")), (0.0, 0.0, 1.0),
                  "boot viewport = origin @ 100%")
        assert_eq(ids(tf), "", "boot: empty selection")

        # ── (B) Space-hold left-drag pans, and never marquees ───────
        # The un-chorded version of this exact sweep is R880's marquee
        # gesture; with the chord held the press enters the pan channel
        # instead, so content follows the cursor and the selection
        # stays untouched.
        space(tf, "down")
        tf.drag(from_at=CENTER, to_at=(CENTER[0] - 80, CENTER[1] - 60))
        wait_until(lambda: vq(tf, "x") > 0, timeout=4.0, interval=0.03,
                   desc="chorded left drag panned the view")
        assert_eq(pan(tf), (80.0, 60.0), "content follows the cursor (x, y)")
        assert_eq(ids(tf), "", "the chorded drag swept no marquee")

        # ── (C) reverse chord-drag pans back, clamping at origin ────
        tf.drag(from_at=CENTER, to_at=(CENTER[0] + 200, CENTER[1] + 200))
        wait_until(lambda: vq(tf, "x") == 0.0, timeout=4.0, interval=0.03,
                   desc="reverse chord drag returned to the origin")
        assert_eq(pan(tf), (0.0, 0.0), "pan clamps at the world origin")

        # ── (D) Space+click on a node is inert ──────────────────────
        # Un-chorded, this click is the R879 single-select; chorded,
        # the press never reaches the node (release-in-place is the
        # left chord's inert Click verdict — Figma: Space+click does
        # nothing).
        tf.click(path=f"{G}#node_0")
        assert_eq(ids(tf), "", "Space+click selects nothing")
        assert_eq(pan(tf), (0.0, 0.0), "Space+click pans nothing")

        # ── (E) Space up restores the normal left arc ───────────────
        space(tf, "up")
        tf.click(path=f"{G}#node_0")
        wait_until(lambda: ids(tf) == "0", timeout=4.0, interval=0.03,
                   desc="un-chorded click selects node 0 again")
        tf.drag(from_at=W(20, 50), to_at=W(200, 260))
        wait_until(lambda: ids(tf) == "0,1", timeout=4.0, interval=0.03,
                   desc="un-chorded sweep marquees the pair again")
        assert_eq(pan(tf), (0.0, 0.0), "the un-chorded gestures panned nothing")
        tf.drag(from_at=W(610, 330), to_at=W(700, 400))  # empty sweep clears
        wait_until(lambda: ids(tf) == "", timeout=4.0, interval=0.03,
                   desc="empty sweep clears the selection")

        # ── (F) Ctrl+Space-drag = the canvas zoom chord ─────────────
        # The chord pan rides the wheel vocabulary (R881 two-stage
        # dispatch), so a held Ctrl reaches the canvas's R877
        # Ctrl-wheel zoom arm: 48 px of vertical drag = 48/16 lines =
        # ZOOM_STEP**3.
        space(tf, "down")
        tf.modifiers(ctrl=True)
        tf.drag(from_at=CENTER, to_at=(CENTER[0], CENTER[1] + 48))
        tf.modifiers()
        space(tf, "up")
        wait_until(lambda: vq(tf, "zoom") > 1.0, timeout=4.0, interval=0.03,
                   desc="ctrl+space drag zoomed in")
        zoom = vq(tf, "zoom")
        assert abs(zoom - ZOOM_STEP ** 3) < 1e-3, \
            f"48 px of chorded drag = three zoom steps, got {zoom}"
        tf.intervene("/external/viewport.zoom", 1.0)
        tf.intervene("/external/viewport.x", 0.0)
        tf.intervene("/external/viewport.y", 0.0)

        # ── (G) the legacy edgeless scene/key never arms the chord ──
        # An atomic press dispatches Space (a no-op for the canvas)
        # without touching the held-key cache, so the next left drag
        # is still a marquee — the pre-R882 wire is behaviourally
        # frozen.
        tf.key(at=(10.0, 10.0), name="Space")
        tf.drag(from_at=W(20, 50), to_at=W(200, 260))
        wait_until(lambda: ids(tf) == "0,1", timeout=4.0, interval=0.03,
                   desc="post-legacy-press sweep still marquees")
        assert_eq(pan(tf), (0.0, 0.0), "the legacy press armed no pan chord")

        # ── (H) out-of-vocabulary state rejects loudly ──────────────
        try:
            tf.request("scene/key", {
                "at": {"x": 10.0, "y": 10.0},
                "key": "Space",
                "state": "held",
            })
            raise AssertionError("state=held must reject")
        except RpcError as err:
            assert_eq(err.code, -32602, "out-of-vocabulary state = invalid_params")
        # The rejected request poisoned nothing: the chord still arms.
        space(tf, "down")
        tf.drag(from_at=CENTER, to_at=(CENTER[0] - 16, CENTER[1]))
        wait_until(lambda: vq(tf, "x") > 0, timeout=4.0, interval=0.03,
                   desc="chord pan still works after the rejection")
        assert_eq(pan(tf), (16.0, 0.0), "follow-up chord pan lands exactly")
        assert_eq(ids(tf), "0,1", "the chorded pan left the selection untouched")
        space(tf, "up")


if __name__ == "__main__":
    run_demo("R882 Space-hold left-drag pan", body)
