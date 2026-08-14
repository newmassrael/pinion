#!/usr/bin/env python3
"""R1182 §5.15 §5.28 §5.49 — node-editor edge-drag auto-pan.

Drives `hello-node-editor` via JSON-RPC. A node held against the canvas rim
auto-scrolls the viewport toward that edge every animation frame (the DCC /
the engine "drag-past-the-edge" convention), and the dragged node stays pinned
under the cursor as the world scrolls beneath it. The driver is a framework
`Tickable` (`AutoPan`, registered via `use_autopan` on the owner's animation
clock, exactly like the caret blink) — so the *logic* is headlessly
reproducible by advancing the clock (`tf.tick(dt)`), and the live
continuous-repaint comes free from the same non-at-rest path the caret uses.

★★★★★ R1688.1 — the paragraph that used to be here was WRONG, and it is worth
keeping the correction where the claim was rather than only at its call site.
It said the ride invariant is read "through back-to-back read-only queries (no
mutation between them, so no tick sneaks in)". Two `query` calls are two RPC
round-trips; the pan advances on WALL TIME (measured: +796 px across one second
of sleep, with `scene/set_fps 0` in force); and reading the two paths in the
other order changes the computed ride by 27 px. The assertions here are
directional / net against a seeded offset, and the ride is read from the PAINT
in a single call — see `painted_x` and section (C).

AI-first (§2 #2 / #7): the whole gesture is RPC-observable — `scene/drag`
holds a node mid-drag (`phase="begin"`), the animation clock advances via
`scene/animate`, and `query viewport.{x,y}` + `scene/snapshot` read the pan and
the node's ride, with no physical mouse and no wall-clock sleep.

  (A) boot + the R881/R882 drag-to-pan foundation still pans (middle + Space).
  (B) a node held at the RIGHT rim auto-pans +x and the node follows.
  (C) the node rides the viewport 1:1 (it does not MOVE ON SCREEN).
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
    abs_rects_of,
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


def painted_x(tf, nid: int = 0) -> int:
    """Where the node is PAINTED, from one call — see section (C).

    The left edge only: a card held against the canvas rim is clipped there, so
    its painted WIDTH changes as it rides while its position does not.
    """
    rects = abs_rects_of(tf.snapshot(source="paint", viewport=(PALETTE_W + CANVAS_W, CANVAS_H)))
    tag = f"{G}#node_{nid}"
    # ★ A named failure, not a `KeyError`. A node that does not ride the
    # viewport slides off the canvas and stops being painted at all — measured,
    # by making the follow run at half rate — and that is this section's own
    # defect arriving in the least readable possible form.
    assert tag in rects, (
        f"{tag} is not painted at all — a node held at the rim that does not "
        "ride the viewport leaves the canvas entirely"
    )
    return rects[tag][0]


# The editor's own auto-pan rate, in world px per second at a fully-pushed rim
# (`AUTOPAN_SPEED`). Read here because this file's tolerance is DERIVED from it.
AUTOPAN_SPEED = 900.0
# One frame at 60 Hz. The pan advances on the animation clock, and the clock is
# driven by REAL time — see `ride_slack`.
FRAME = 1.0 / 60.0


def ride_slack(zoom: float) -> int:
    """How far a pinned node's painted position may wander, in whole pixels.

    DERIVED from two measured facts, not fitted to make a red go away.

    **Rounding** — the painted position is `round(graph_x * zoom) - round(offset)`
    and `graph_x` is itself `round((offset + cursor) / zoom)`, so three roundings
    of which the first costs `zoom / 2` px because it happens in graph units:
    `zoom / 2 + 1`, which is 1 px at 100% and 2 px at 200%.

    **One frame of pan** — and this is the term that dominates. The auto-pan
    advances on the animation clock, and that clock runs on WALL TIME whatever
    this file does: measured on 2026-08-14, with the drag held at the rim and
    `scene/set_fps 0` in force, one read + one second of sleep + one read moved
    the viewport by **796 px**, and eleven back-to-back reads moved it 135. So a
    snapshot samples the ride at an uncontrolled moment and may catch it up to
    one frame's pan out of step: `AUTOPAN_SPEED * FRAME` = 15 px.

    That second term is a LIMIT OF THE INSTRUMENT, stated rather than hidden.
    It does not weaken what this section checks: a node that was not riding
    would be hundreds of pixels out within three ticks, not fifteen. Removing it
    needs `scene/set_fps 0` to actually freeze an animation that reports itself
    not-at-rest — see
    [[debt-fps-zero-does-not-freeze-a-not-at-rest-animation]].
    """
    return int(1 + zoom / 2 + AUTOPAN_SPEED * FRAME)


def settled_x(tf, zoom: float, label: str, tries: int = 12) -> int:
    """The painted x once two consecutive reads agree, within [`ride_slack`].

    `grab_to` marches the cursor in ten steps and returns as soon as the last
    one is sent; the node's own follow is applied on the next frame, so the
    first snapshot after it can still be a frame behind — measured at 11 px out
    on one run in ten, which is a settling artefact and not a ride defect.

    Failing to settle IS the ride defect, and this says so by name rather than
    by whatever the next assertion happens to compare.
    """
    slack = ride_slack(zoom)
    last = painted_x(tf)
    for _ in range(tries):
        now = painted_x(tf)
        if abs(now - last) <= slack:
            return now
        last = now
    raise AssertionError(
        f"{label}: the node never settled on screen — {tries} consecutive "
        f"snapshots disagreed by more than {slack} px, which is the ride "
        "itself being broken"
    )


def assert_pinned(tf, pinned: int, zoom: float, label: str) -> None:
    """The node has not moved on screen. Reads ONCE — see section (C)."""
    now = painted_x(tf)
    slack = ride_slack(zoom)
    assert abs(now - pinned) <= slack, (
        f"{label}: painted at {now}, was pinned at {pinned}, slack {slack} px "
        f"at {zoom:g}x"
    )


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
        vx1, nx1 = vq(tf, "x"), nx(tf, 0)
        tf.tick(0.05)
        vx2, nx2 = vq(tf, "x"), nx(tf, 0)
        assert vx2 > vx1, "C.1 a further tick keeps panning toward the edge"
        assert nx2 > nx1, "C.2 the node keeps following"
        # ★★★★★ R1688.1 — **the ride is read from the PAINT, in one call.**
        #
        # This used to subtract two separate `query` results and call the pair an
        # "atomic snapshot (no mutation between them, so no tick sneaks in)".
        # That sentence was false, and it went red in CI on 2026-08-14
        # (`offset 581.0 -> 584.0` against a tolerance of 2) while passing every
        # time locally. Measured while repairing it, with a drag held at the rim:
        #
        #   * two back-to-back `viewport.x` reads differ by ~1-39 px;
        #   * ONE read, one second of WALL CLOCK, one read: +796 px;
        #   * and reading node-then-viewport instead of viewport-then-node moves
        #     the computed "ride" by 27 px — the artefact IS the read order.
        #
        # So the pan advances with real time no matter what this file does
        # (`set_fps(0)` does not stop it either — see
        # [[debt-fps-zero-does-not-freeze-a-not-at-rest-animation]]), and any
        # invariant built from two reads of a moving thing is a race with a
        # tolerance in front of it.
        #
        # The paint answers both facts in ONE call: a node that rides the
        # viewport 1:1 does not MOVE ON SCREEN while the world scrolls under it,
        # which is also what "pinned under the cursor" means to the person doing
        # it. Measured stable across eight snapshots and five ticks. The 1 px is
        # two roundings (the graph position and the scroll offset are each
        # rounded to whole pixels), not slack for a race.
        pinned = settled_x(tf, 1.0, "C.3 baseline")
        for step in range(3):
            tf.tick(0.05)
            assert_pinned(tf, pinned, 1.0, f"C.3 the node stays pinned (tick {step})")
        assert vq(tf, "x") > vx2, "C.4 and the world really did keep moving"

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

        # ── (H) a marquee at the rim REACHES PAST IT (R1620 → R1627) ────
        #
        # R1182 asserted here that a marquee drag never auto-pans, and that was
        # a statement about what EXISTED rather than about what should: there
        # was no auto-scroll substrate, so a marquee could only ever select
        # what was already on screen. R1620 built one and it defaults on (a
        # region that says nothing still lets a drag reach past its edge), so
        # this section went red — and R1627 measured which of the two was
        # right rather than restoring the old number.
        #
        # It is the new behaviour, and the anchor is why: `use_marquee_rect`
        # holds the band in GRAPH units, so panning under a live marquee moves
        # the viewport and leaves the rectangle's corner where the user put it.
        # The rubber band therefore grows over ground that scrolled into view,
        # which is the whole point. Asserted the only way that means anything:
        # a node that starts OFF SCREEN ends up selected.
        reset_view(tf)
        tf.intervene("/external/selected", None)
        # One node off the right edge; the rest swept far away so nothing else
        # can be caught, and a background press can only arm a marquee.
        tf.intervene("/external/node.0.x", 700)
        tf.intervene("/external/node.0.y", 190)
        for i in range(1, 4):
            tf.intervene(f"/external/node.{i}.x", 2600 + i * 40)
            tf.intervene(f"/external/node.{i}.y", 2600)
        assert_eq(tf.query("/external/selected"), None, "H.0 nothing selected yet")
        tf.drag(from_at=canvas_at(40, 40), to_at=RIGHT_RIM, steps=10, phase="begin")
        vx_h0 = vq(tf, "x")
        for _ in range(6):
            tf.tick(0.1)
        vx_h1 = vq(tf, "x")
        assert vx_h1 > vx_h0, f"H.1 a marquee at the rim auto-pans (+x): {vx_h0} -> {vx_h1}"
        assert_eq(vq(tf, "y"), 0.0, "H.2 a horizontal rim leaves y alone")
        tf.drag(from_at=RIGHT_RIM, to_at=RIGHT_RIM, steps=1, phase="end")
        # THE POINT: the off-screen node is selected. Without the pan the band
        # could not have reached it, and if the anchor were in viewport units
        # the band would have slid off it instead.
        assert_eq(
            tf.query("/external/selected"),
            0,
            "H.3 the marquee caught a node that started off screen",
        )
        tf.intervene("/external/selected", None)

        # ── (I) at 2x zoom the auto-pan still works ──────────────────
        reset_view(tf)
        tf.intervene("/external/viewport.zoom", 2.0)
        tf.intervene("/external/node.0.x", 80)
        tf.intervene("/external/node.0.y", 90)
        grab_to(tf, RIGHT_RIM)
        vx_i0, n_i0 = vq(tf, "x"), nx(tf, 0)
        tf.tick(0.1)
        vx_i1, n_i1 = vq(tf, "x"), nx(tf, 0)
        assert vx_i1 > vx_i0, "I.1 auto-pan pans at 2x zoom"
        assert n_i1 > n_i0, "I.2 the node still follows at 2x zoom"
        # ★ R1688.1 — from the PAINT, for the reason section (C) sets out: the
        # ride cannot be measured by subtracting two reads of a thing that is
        # moving, at any zoom. This section carried the same false "atomic
        # snapshot" claim and the same tolerance, and it failed on the first
        # re-run after (C) was repaired — one flake per section, one cause.
        pinned_i = settled_x(tf, 2.0, "I.3 baseline")
        for step in range(3):
            tf.tick(0.05)
            assert_pinned(tf, pinned_i, 2.0, f"I.3 pinned at 2x (tick {step})")
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
