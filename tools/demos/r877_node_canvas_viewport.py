#!/usr/bin/env python3
"""R877 §5.15 §5.45 §5.49 — node-editor canvas pan / zoom (viewport).

Drives `hello-node-editor` via JSON-RPC. The canvas becomes a real editor
viewport: pan = a `ScrollAxis::Both` scroll over a fixed 2048-unit world
(plain wheel pans through the router's native scroll dispatch — the canvas
binding contributes zero pan code), zoom = a shared Signal the view projects
every world coordinate through. `Ctrl`+wheel zooms anchored at the cursor via
the R877 `External::wheel` forwarding leg (the router offers the wheel to the
hovered External before the scroll fallback; the canvas consumes modified
wheels and declines plain ones). `Shift`+wheel pans horizontally,
`Ctrl`+`=`/`-`/`0` step/reset, `f` frames the graph.

AI-first (§2 #2 / #7): `query`/`intervene viewport.{x,y,zoom}` (pan in
zoom-independent graph units) + `invoke frame_all`; modifiers ride the R763
out-of-band `scene/modifiers` channel, so an injected Ctrl-wheel zoom needs no
physical keyboard.

  (A) boot — viewport at origin, 100% zoom; seed graph intact.
  (B) plain wheel pans (vertical + horizontal hardware deltas).
  (C) Shift+wheel pans horizontally (vertical notches drive x).
  (D) Ctrl+wheel zooms, anchored at the cursor (the graph point under the
      cursor stays put).
  (E) viewport.zoom intervene clamps to [0.5, 4.0].
  (F) viewport.x/y intervene round-trips in graph units.
  (G) frame_all fits the seed bbox; every node tag paints on-canvas.
  (H) keyboard: Ctrl+= steps in, Ctrl+0 resets, f frames.
  (I) node drag at 2x zoom moves in graph units (screen px / zoom).
  (J) selection still works on a panned + zoomed canvas.
  (K) wheel over the palette does not pan the canvas.
  (L) add_node spawns inside the panned view.

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
# Window = palette strip (132) + canvas (640 x 420).
VIEWPORT = (772, 420)
PALETTE_W = 132
CANVAS_W, CANVAS_H = 640, 420
WORLD = 2048
ZOOM_STEP = 1.2
LINE_PX = 16.0  # W3C wheel line height the framework scales Lines by


def vq(tf, axis: str) -> float:
    return float(tf.query(f"/external/viewport.{axis}"))


def viewport(tf) -> tuple[float, float, float]:
    return (vq(tf, "x"), vq(tf, "y"), vq(tf, "zoom"))


def canvas_at(cx: float, cy: float) -> tuple[float, float]:
    """Window coords of a canvas-relative point."""
    return (PALETTE_W + cx, cy)


def graph_under(tf, cx: float, cy: float) -> tuple[float, float]:
    """The graph point under canvas px (cx, cy) at the current viewport."""
    x, y, zoom = viewport(tf)
    return (x + cx / zoom, y + cy / zoom)


def _focus_graph(tf) -> None:
    tf.request("focus/set", {"tag": G})
    wait_until(
        lambda: tf.request("focus/get").result.get("focused") == G,
        timeout=4.0,
        interval=0.03,
        desc="graph owns keyboard focus",
    )


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) boot: viewport at origin, 100% zoom ─────────────────
        assert_eq(viewport(tf), (0.0, 0.0, 1.0), "boot viewport = origin @ 100%")
        assert_eq(tf.query("/external/node_count"), 4, "seed graph intact")
        # The three viewport slots are queryable floats from frame one.
        for axis, want in (("x", 0.0), ("y", 0.0), ("zoom", 1.0)):
            v = vq(tf, axis)
            assert isinstance(v, float) and v == want, f"viewport.{axis} boots {want}, got {v}"

        # ── (B) plain wheel pans (native scroll dispatch) ───────────
        center = canvas_at(CANVAS_W / 2, CANVAS_H / 2)
        tf.wheel(at=center, lines=(0.0, 2.0))
        wait_until(lambda: vq(tf, "y") > 0, timeout=4.0, interval=0.03,
                   desc="wheel-down panned the view down")
        assert_eq(vq(tf, "y"), 2 * LINE_PX, "2 notches = 32 graph units at 100%")
        assert_eq(vq(tf, "x"), 0.0, "vertical wheel leaves x alone")
        tf.wheel(at=center, lines=(1.5, 0.0))
        wait_until(lambda: vq(tf, "x") > 0, timeout=4.0, interval=0.03,
                   desc="horizontal hardware delta pans x")
        assert_eq(vq(tf, "x"), 1.5 * LINE_PX, "trackpad dx pans horizontally")
        tf.intervene("/external/viewport.x", 0.0)
        tf.intervene("/external/viewport.y", 0.0)

        # ── (C) Shift+wheel pans horizontally ───────────────────────
        tf.modifiers(shift=True)
        tf.wheel(at=center, lines=(0.0, 3.0))
        tf.modifiers()
        wait_until(lambda: vq(tf, "x") > 0, timeout=4.0, interval=0.03,
                   desc="shift-wheel pans x")
        assert_eq(vq(tf, "x"), 3 * LINE_PX, "vertical notches drive the x offset")
        assert_eq(vq(tf, "y"), 0.0, "shift-wheel leaves y alone")
        tf.intervene("/external/viewport.x", 0.0)

        # ── (D) Ctrl+wheel zooms anchored at the cursor ─────────────
        anchor_canvas = (CANVAS_W * 0.25, CANVAS_H * 0.25)
        before = graph_under(tf, *anchor_canvas)
        tf.modifiers(ctrl=True)
        tf.wheel(at=canvas_at(*anchor_canvas), lines=(0.0, -1.0))
        tf.modifiers()
        wait_until(lambda: vq(tf, "zoom") > 1.0, timeout=4.0, interval=0.03,
                   desc="ctrl-wheel zoomed in")
        zoom = vq(tf, "zoom")
        assert abs(zoom - ZOOM_STEP) < 1e-6, f"one notch = one ZOOM_STEP, got {zoom}"
        after = graph_under(tf, *anchor_canvas)
        assert abs(after[0] - before[0]) < 1.5 and abs(after[1] - before[1]) < 1.5, \
            f"the graph point under the cursor is pinned: {before} -> {after}"

        # ── (E) zoom intervene clamps ───────────────────────────────
        tf.intervene("/external/viewport.zoom", 99.0)
        assert_eq(vq(tf, "zoom"), 4.0, "zoom clamps at 400%")
        tf.intervene("/external/viewport.zoom", 0.01)
        assert_eq(vq(tf, "zoom"), 0.5, "zoom clamps at 50%")

        # ── (F) pan intervene round-trips in graph units ────────────
        tf.intervene("/external/viewport.zoom", 2.0)
        tf.intervene("/external/viewport.x", 100.0)
        tf.intervene("/external/viewport.y", 50.0)
        assert_eq(viewport(tf), (100.0, 50.0, 2.0), "pan round-trips zoom-independent")
        tf.intervene("/external/viewport.x", 1.0e9)
        assert vq(tf, "x") < WORLD, "a huge pan clamps to the world extent"

        # ── (G) frame_all fits the seed bbox ────────────────────────
        assert_eq(tf.invoke("/external/frame_all", None), True, "frame_all on seed graph")
        zoom = vq(tf, "zoom")
        assert 1.0 < zoom < 1.2, f"fit zoom in the expected band, got {zoom}"
        wait_until(
            lambda: all(
                f"{G}#node_{i}" in abs_rects_of(tf.snapshot(source="paint", viewport=VIEWPORT))
                for i in range(4)
            ),
            timeout=4.0,
            interval=0.05,
            desc="all four seed nodes paint after framing",
        )
        rects = abs_rects_of(tf.snapshot(source="paint", viewport=VIEWPORT))
        for i in range(4):
            x, y, w, h = rects[f"{G}#node_{i}"]
            assert x >= PALETTE_W and x + w <= VIEWPORT[0], f"node {i} inside the canvas x-band"
            assert y >= 0 and y + h <= VIEWPORT[1], f"node {i} inside the canvas y-band"

        # ── (H) keyboard zoom: Ctrl+= in, Ctrl+0 reset, f frames ────
        _focus_graph(tf)
        tf.intervene("/external/viewport.zoom", 1.0)
        tf.modifiers(ctrl=True)
        tf.key(path=G, name="=")
        tf.modifiers()
        wait_until(lambda: vq(tf, "zoom") > 1.0, timeout=4.0, interval=0.03,
                   desc="Ctrl+= zoomed in")
        assert abs(vq(tf, "zoom") - ZOOM_STEP) < 1e-6, "Ctrl+= is one step"
        tf.modifiers(ctrl=True)
        tf.key(path=G, name="0")
        tf.modifiers()
        wait_until(lambda: abs(vq(tf, "zoom") - 1.0) < 1e-9, timeout=4.0, interval=0.03,
                   desc="Ctrl+0 resets to 100%")
        tf.key(path=G, name="f")
        wait_until(lambda: abs(vq(tf, "zoom") - 1.0) > 1e-3, timeout=4.0, interval=0.03,
                   desc="f frames the graph")

        # ── (I) node drag at 2x zoom moves in graph units ───────────
        tf.intervene("/external/viewport.zoom", 2.0)
        tf.intervene("/external/viewport.x", 0.0)
        tf.intervene("/external/viewport.y", 0.0)
        tf.intervene("/external/node.0.x", 40)
        tf.intervene("/external/node.0.y", 70)
        rects = abs_rects_of(tf.snapshot(source="paint", viewport=VIEWPORT))
        nx, ny, nw, nh = rects[f"{G}#node_0"]
        # Grab the header centre, drag +80 screen px right.
        grab = (nx + nw / 2.0, ny + 12.0)
        tf.drag(from_at=grab, to_at=(grab[0] + 80.0, grab[1]), steps=8)
        wait_until(lambda: tf.query("/external/node.0.x") == 80, timeout=4.0, interval=0.03,
                   desc="80 screen px / 2x zoom = 40 graph units")
        assert_eq(tf.query("/external/node.0.y"), 70, "a horizontal drag leaves y alone")
        tf.intervene("/external/node.0.x", 40)

        # ── (J) selection still works panned + zoomed ───────────────
        tf.click(path=f"{G}#node_2")
        wait_until(lambda: tf.query("/external/selected") == 2, timeout=4.0,
                   interval=0.03, desc="click selects node 2 on a zoomed canvas")
        tf.intervene("/external/selected", None)

        # ── (K) wheel over the palette does not pan the canvas ──────
        pan_before = (vq(tf, "x"), vq(tf, "y"))
        tf.wheel(at=(PALETTE_W / 2, CANVAS_H / 2), lines=(0.0, 3.0))
        assert_eq((vq(tf, "x"), vq(tf, "y")), pan_before,
                  "a palette wheel neither pans nor zooms the canvas")

        # ── (L) add_node spawns inside the panned view ──────────────
        tf.intervene("/external/viewport.zoom", 1.0)
        tf.intervene("/external/viewport.x", 900.0)
        tf.intervene("/external/viewport.y", 700.0)
        new_id = tf.invoke("/external/add_node", "Color")
        sx = tf.query(f"/external/node.{new_id}.x")
        sy = tf.query(f"/external/node.{new_id}.y")
        assert sx >= 900 and sy >= 700, f"spawn follows the viewport, got ({sx}, {sy})"


if __name__ == "__main__":
    sys.exit(run_demo("r877_node_canvas_viewport", body))
