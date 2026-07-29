#!/usr/bin/env python3
"""R1182 §5.15 §5.28 §5.49 — node-editor edge-drag auto-pan.

Drives `hello-node-editor` via JSON-RPC. A node held against the canvas rim
auto-scrolls the viewport toward that edge every animation frame (the DCC /
Unreal "drag-past-the-edge" convention), and the dragged node stays pinned
under the cursor as the world scrolls beneath it. The driver is a framework
`Tickable` (`AutoPan`, registered via `use_autopan` on the owner's animation
clock, exactly like the caret blink) — so the *logic* is headlessly
reproducible by advancing the clock (`tf.tick(dt)`), and the live
continuous-repaint comes free from the same non-at-rest path the caret uses.

Because the headless backend ticks animations on every paint (each drag-march
frame that lands in the rim already advances the pan), the assertions here are
directional / net (against a seeded offset) and the ride invariant is read
through back-to-back read-only queries (no mutation between them, so no tick
sneaks in) rather than exact per-tick deltas.

AI-first (§2 #2 / #7): the whole gesture is RPC-observable — `scene/drag`
holds a node mid-drag (`phase="begin"`), the animation clock advances via
`scene/animate`, and `query viewport.{x,y}` + `query node.<id>.{x,y}` read the
pan and the node's ride, with no physical mouse and no wall-clock sleep.

  (A) boot + the R881/R882 drag-to-pan foundation still pans (middle + Space).
  (B) a node held at the RIGHT rim auto-pans +x and the node follows.
  (C) the node rides the viewport 1:1 (node.x - viewport.x is invariant).
  (D) release stops the auto-pan (the driver goes at-rest; node frozen too).
  (E) a node held at the CENTRE does not auto-pan (no rim = at rest).
  (F) each rim pans the correct axis / direction (left / top / bottom).
  (G) ticking with no drag in flight never pans.
  (H) a background (marquee) drag at the rim does not auto-pan (nodes only).
  (I) at 2x zoom the auto-pan still pans and the node still follows.
  (J) the auto-pan clamps at the world edge (viewport + node both stop).

>= 30 assertions.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    run_demo,
    wait_until,
)

EXAMPLE = "hello-node-editor"
G = "node_graph"
PALETTE_W = 132
CANVAS_W, CANVAS_H = 640, 420
WORLD = 2048


def vq(tf, axis: str) -> float:
    return float(tf.query(f"/external/viewport.{axis}"))


def viewport(tf) -> tuple[float, float, float]:
    return (vq(tf, "x"), vq(tf, "y"), vq(tf, "zoom"))


def canvas_at(cx: float, cy: float) -> tuple[float, float]:
    return (PALETTE_W + cx, cy)


def nx(tf, nid: int) -> int:
    return tf.query(f"/external/node.{nid}.x")


def ny(tf, nid: int) -> int:
    return tf.query(f"/external/node.{nid}.y")


def reset_view(tf) -> None:
    tf.intervene("/external/viewport.zoom", 1.0)
    tf.intervene("/external/viewport.x", 0.0)
    tf.intervene("/external/viewport.y", 0.0)


# Rim probe points (well inside each margin) + a dead-centre control.
RIGHT_RIM = canvas_at(CANVAS_W - 8, CANVAS_H / 2)   # (764, 210)
LEFT_RIM = canvas_at(8, CANVAS_H / 2)               # (140, 210)
TOP_RIM = canvas_at(CANVAS_W / 2, 8)                # (452, 8)
BOTTOM_RIM = canvas_at(CANVAS_W / 2, CANVAS_H - 8)  # (452, 412)
CENTER = canvas_at(CANVAS_W / 2, CANVAS_H / 2)      # (452, 210)


def grab_to(tf, dst: tuple[float, float]) -> None:
    """Press node 0, march to `dst`, and HOLD (no release)."""
    tf.drag(from_path=f"{G}#node_0", to_at=dst, steps=10, phase="begin")


def release(tf, at: tuple[float, float]) -> None:
    tf.drag(from_at=at, to_at=at, steps=1, phase="end")


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) boot + the drag-to-pan foundation (R881/R882) ────────
        assert_eq(viewport(tf), (0.0, 0.0, 1.0), "A.1 boot viewport = origin @ 100%")
        assert_eq(tf.query("/external/node_count"), 4, "A.2 seed graph intact")
        reset_view(tf)
        tf.drag(from_at=CENTER, to_at=(CENTER[0] - 90, CENTER[1] - 60),
                steps=8, button="middle")
        assert vq(tf, "x") != 0.0 or vq(tf, "y") != 0.0, \
            "A.3 middle-drag pans (R881 foundation)"
        reset_view(tf)
        tf.key(name="Space", state="down", at=CENTER)
        tf.drag(from_at=CENTER, to_at=(CENTER[0] - 70, CENTER[1] - 50), steps=8)
        tf.key(name="Space", state="up")
        assert vq(tf, "x") != 0.0 or vq(tf, "y") != 0.0, \
            "A.4 Space+left-drag pans (R882 foundation)"
        reset_view(tf)

        # ── (B) a node held at the RIGHT rim auto-pans +x, node follows ─
        tf.intervene("/external/node.0.x", 120)
        tf.intervene("/external/node.0.y", 180)
        grab_to(tf, RIGHT_RIM)
        ny0 = ny(tf, 0)   # the node's y after it followed the cursor to the rim
        tf.tick(0.1)
        assert vq(tf, "x") > 0.0, "B.1 the right rim auto-pans +x"
        assert_eq(vq(tf, "y"), 0.0, "B.2 a horizontal rim leaves the viewport y alone")
        assert nx(tf, 0) > 120, "B.3 the node followed into the revealed area"
        assert_eq(vq(tf, "zoom"), 1.0, "B.4 the pan does not change zoom")
        assert_eq(ny(tf, 0), ny0, "B.5 an x-only auto-pan leaves the node's y put")

        # ── (C) the node rides the viewport 1:1 across ticks ─────────
        tf.tick(0.05)
        vx1, nx1 = vq(tf, "x"), nx(tf, 0)   # back-to-back reads = atomic snapshot
        ride1 = nx1 - vx1
        tf.tick(0.05)
        vx2, nx2 = vq(tf, "x"), nx(tf, 0)
        ride2 = nx2 - vx2
        assert vx2 > vx1, "C.1 a further tick keeps panning toward the edge"
        assert nx2 > nx1, "C.2 the node keeps following"
        assert abs(ride2 - ride1) <= 2, \
            f"C.3 the node rides the viewport 1:1 (offset {ride1} -> {ride2})"
        tf.tick(0.05)
        vx3, nx3 = vq(tf, "x"), nx(tf, 0)
        assert abs((nx3 - vx3) - ride1) <= 2, "C.4 the ride offset stays invariant"

        # ── (D) release stops the auto-pan (driver + node at-rest) ───
        release(tf, RIGHT_RIM)
        v_rest, n_rest = vq(tf, "x"), nx(tf, 0)
        tf.tick(0.2)
        assert_eq(vq(tf, "x"), v_rest, "D.1 no auto-pan after the drag is released")
        assert_eq(nx(tf, 0), n_rest, "D.2 the node is frozen after release")
        tf.tick(0.2)
        assert_eq(vq(tf, "x"), v_rest, "D.3 still at rest on a second idle tick")

        # ── (E) a node held at the CENTRE does not auto-pan ──────────
        reset_view(tf)
        tf.intervene("/external/node.0.x", 260)
        tf.intervene("/external/node.0.y", 190)
        grab_to(tf, CENTER)
        tf.tick(0.2)
        assert_eq((vq(tf, "x"), vq(tf, "y")), (0.0, 0.0),
                  "E.1 a centred hold never auto-pans (no rim)")
        assert nx(tf, 0) != 260, "E.2 the node still followed the cursor to the centre"
        release(tf, CENTER)

        # ── (F) each rim pans the correct axis / direction ───────────
        # LEFT rim → pan -x. Seed a positive x so a decrease is observable.
        reset_view(tf)
        tf.intervene("/external/viewport.x", 800.0)
        tf.intervene("/external/node.0.x", 860)
        tf.intervene("/external/node.0.y", 190)
        grab_to(tf, LEFT_RIM)
        tf.tick(0.1)
        assert vq(tf, "x") < 800.0, "F.1 the left rim pans -x"
        release(tf, LEFT_RIM)
        # TOP rim → pan -y.
        reset_view(tf)
        tf.intervene("/external/viewport.y", 800.0)
        tf.intervene("/external/node.0.x", 300)
        tf.intervene("/external/node.0.y", 860)
        grab_to(tf, TOP_RIM)
        tf.tick(0.1)
        assert vq(tf, "y") < 800.0, "F.2 the top rim pans -y"
        assert_eq(vq(tf, "x"), 0.0, "F.3 a vertical rim leaves x alone")
        release(tf, TOP_RIM)
        # BOTTOM rim → pan +y.
        reset_view(tf)
        tf.intervene("/external/node.0.x", 300)
        tf.intervene("/external/node.0.y", 120)
        grab_to(tf, BOTTOM_RIM)
        tf.tick(0.1)
        assert vq(tf, "y") > 0.0, "F.4 the bottom rim pans +y"
        assert_eq(vq(tf, "x"), 0.0, "F.5 the bottom rim leaves x alone")
        release(tf, BOTTOM_RIM)

        # ── (G) ticking with no drag in flight never pans ────────────
        reset_view(tf)
        tf.intervene("/external/viewport.x", 150.0)
        tf.intervene("/external/viewport.y", 90.0)
        tf.tick(0.3)
        assert_eq((vq(tf, "x"), vq(tf, "y")), (150.0, 90.0), "G.1 an idle tick never pans")
        assert_eq(vq(tf, "zoom"), 1.0, "G.2 an idle tick never zooms")

        # ── (H) a background (marquee) drag at the rim does not auto-pan ─
        reset_view(tf)
        tf.intervene("/external/selected", None)
        # Sweep every seed node far off-screen so the canvas is provably empty,
        # then a background press at the rim can only arm a marquee, not a grab.
        for i in range(4):
            tf.intervene(f"/external/node.{i}.x", 1600 + i * 40)
            tf.intervene(f"/external/node.{i}.y", 1600)
        tf.drag(from_at=canvas_at(40, 40), to_at=RIGHT_RIM, steps=10, phase="begin")
        tf.tick(0.2)
        assert_eq(vq(tf, "x"), 0.0, "H.1 a marquee drag at the rim never auto-pans")
        assert_eq(vq(tf, "y"), 0.0, "H.2 a marquee drag never pans y either")
        release(tf, RIGHT_RIM)
        tf.intervene("/external/selected", None)

        # ── (I) at 2x zoom the auto-pan still works ──────────────────
        reset_view(tf)
        tf.intervene("/external/viewport.zoom", 2.0)
        tf.intervene("/external/node.0.x", 80)
        tf.intervene("/external/node.0.y", 90)
        grab_to(tf, RIGHT_RIM)
        vx_i0, n_i0 = vq(tf, "x"), nx(tf, 0)   # atomic snapshot (no mutation between)
        ride_i0 = n_i0 - vx_i0
        tf.tick(0.1)
        vx_i1, n_i1 = vq(tf, "x"), nx(tf, 0)
        assert vx_i1 > vx_i0, "I.1 auto-pan pans at 2x zoom"
        assert n_i1 > n_i0, "I.2 the node still follows at 2x zoom"
        # The 1:1 ride holds at zoom != 1 too (viewport.x is graph units, so the
        # divide-by-zoom that matters for staying-under-cursor is exercised).
        assert abs((n_i1 - vx_i1) - ride_i0) <= 2, "I.3 node rides 1:1 in graph units at 2x"
        assert_eq(vq(tf, "zoom"), 2.0, "I.4 the zoom is unchanged by the pan")
        release(tf, RIGHT_RIM)

        # ── (J) the auto-pan clamps at the world edge ────────────────
        reset_view(tf)
        tf.intervene("/external/viewport.x", 1.0e9)
        tf.intervene("/external/viewport.y", 1.0e9)
        mx, my = vq(tf, "x"), vq(tf, "y")
        assert mx < WORLD and my < WORLD, "J.1 a huge pan clamped to the world extent"
        tf.intervene("/external/node.0.x", int(mx) + 150)
        tf.intervene("/external/node.0.y", int(my) + 100)
        grab_to(tf, BOTTOM_RIM)
        for _ in range(20):
            tf.tick(0.1)
        assert_eq(vq(tf, "y"), my, "J.2 the viewport y stays clamped at the world edge")
        assert_eq(vq(tf, "x"), mx, "J.3 the bottom rim leaves x at its clamp")
        assert nx(tf, 0) <= WORLD and ny(tf, 0) <= WORLD, "J.4 the dragged node clamps in-world"
        release(tf, BOTTOM_RIM)

        # ── (K) clean origin ─────────────────────────────────────────
        reset_view(tf)
        assert_eq(viewport(tf), (0.0, 0.0, 1.0), "K.1 viewport resets clean")


if __name__ == "__main__":
    sys.exit(run_demo("r1182_node_edge_autopan", body))
