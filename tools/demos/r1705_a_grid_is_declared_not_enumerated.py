#!/usr/bin/env python3
"""R1705 §5.32 §5.50 §2 #1 §2 #7 — **a canvas grid is a declaration, not a
thousand nodes**, checked in the scene, in the count and in the pixels.

# What this exists for

A person driving the node canvas reported two things:

    "the dots only go so far, so it doesn't feel infinite"
    "and zooming out is slow — is this the floor's quality?"

They were one fact. The framework had no repeating fill, so the screen built its
grid out of the only fill it had — a box per pip — and inherited both problems
from that. Measured before this round, on the running application:

* zooming out took the painted scene from **12,879 nodes to 95,131**, and a zoom
  step from **23 ms to 155 ms**. Each of those nodes was laid out, hit-tested,
  cached and published, for a 1 px dot.
* an enumerated lattice has to stop somewhere, so it was cut to the 6,400-unit
  world surface — and panning past that edge left the canvas blank.

The floor has neither problem because it has the primitive. Measured by building
a probe at 6.11.1 and running it offscreen, sweeping the zoom over a scene one
million units square: a tiled background brush costs **0.6–0.8 ms a frame and
does not move**; the hand-drawn variant stays under 2 ms while drawing 27,448
pips. Neither enumerates the scene, so neither can run out.

`BoxStyle::lattice` is that primitive, and this demo is the evidence it works.

# What it asserts

* **A** — the canvas DECLARES a lattice, and the declaration is the generator:
  pitch, dot size, phase and colour, enough for a client to place every dot.
* **B** — ★ the painted scene holds no pip. The node count is bounded as the
  zoom falls, where it used to grow sevenfold.
* **C** — the lattice is there at every pan, including far past the old world
  surface's edge, which is the "it doesn't feel infinite" half.
* **D** — ★ and it is actually DRAWN: the grid colour appears in the rendered
  pixels at the declared pitch. A node count going down is not a grid appearing,
  and the renderer is new code.

Run from the workspace root:
    python3 tools/demos/r1705_a_grid_is_declared_not_enumerated.py
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    png_pixel,
    read_png_rgba8,
    run_demo,
)

EXT = "/external"
CANVAS = "lab.canvas"

# ★ The ceiling B holds the painted scene to. Not a number picked to pass:
# measured at 2,025 nodes on the opening screen and 1,713 zoomed all the way
# out, against 12,879 and 95,131 before the lattice. A budget with headroom for
# the screen to grow, and two orders of magnitude below what it replaced.
NODE_CEILING = 4_000


def lattice_of(tf: RpcSubprocess) -> dict[str, Any] | None:
    """What the canvas box declares, read out of the PAINTED scene."""
    found: list[Any] = []

    def walk(node: Any) -> None:
        if isinstance(node, dict):
            if node.get("tag") == CANVAS:
                found.append((node.get("style") or {}).get("lattice"))
            for value in node.values():
                walk(value)
        elif isinstance(node, list):
            for child in node:
                walk(child)

    walk(tf.snapshot(source="paint"))
    return found[0] if found else None


def painted_nodes(tf: RpcSubprocess) -> int:
    total = 0

    def walk(node: Any) -> None:
        nonlocal total
        if isinstance(node, dict):
            total += 1
            for value in node.values():
                walk(value)
        elif isinstance(node, list):
            for child in node:
                walk(child)

    walk(tf.snapshot(source="paint"))
    return total


def body() -> None:
    checks = 0
    with RpcSubprocess("hello-node-lab") as tf:
        shot = abs_rects_of(tf.snapshot(source="paint"))
        canvas = shot[CANVAS]

        # ── A — the declaration is the generator ──────────────────────
        declared = lattice_of(tf)
        assert declared is not None, "the canvas declares no lattice at all"
        for key in ("pitch", "dot", "phase", "color"):
            assert key in declared, f"the lattice publishes no {key!r}: {declared}"
        assert declared["pitch"] > 0, f"a zero pitch never advances: {declared}"
        print(f"[A] the canvas declares {declared}")
        checks += 5

        # ── B — no pips in the scene, and the count does not grow ─────
        opening = painted_nodes(tf)
        assert opening <= NODE_CEILING, (
            f"the opening screen paints {opening} nodes, over the {NODE_CEILING} "
            f"ceiling — the grid is being enumerated again"
        )
        checks += 1
        worst = opening
        for _ in range(9):
            tf.invoke(f"{EXT}/zoom_by", "out")
            tf.tick(0.016)
            worst = max(worst, painted_nodes(tf))
        assert worst <= NODE_CEILING, (
            f"zooming out took the painted scene to {worst} nodes, over the "
            f"{NODE_CEILING} ceiling. This is the exact regression the round "
            f"repaired: before it, the same sweep reached 95,131"
        )
        print(f"[B] painted nodes: {opening} at the opening zoom, {worst} worst "
              f"across the whole zoom-out (ceiling {NODE_CEILING})")
        checks += 1

        # ★ The pitch tracks the zoom, so the declaration is live rather than a
        # constant that happens to be published.
        zoomed = lattice_of(tf)
        assert zoomed is not None
        assert zoomed["pitch"] < declared["pitch"], (
            f"the lattice's pitch did not follow the zoom out: "
            f"{declared['pitch']} -> {zoomed['pitch']}"
        )
        checks += 1

        # ── C — endless: still there past the old world edge ──────────
        tf.invoke(f"{EXT}/reset", "view")
        tf.tick(0.016)
        for _ in range(4):
            tf.drag(
                from_at=(canvas[0] + canvas[2] - 40, canvas[1] + canvas[3] - 40),
                to_at=(canvas[0] + 40, canvas[1] + canvas[3] - 40),
            )
        tf.tick(0.016)
        pan = tf.query(f"{EXT}/pan")
        far = lattice_of(tf)
        assert far is not None, (
            f"panned to {pan}, past the 6,400-unit world surface the pips used "
            f"to stop at, and the canvas declares no lattice — the grid ran out "
            f"again"
        )
        assert_eq(far["pitch"], declared["pitch"], "the pitch is unchanged by a pan")
        # ★ And the phase CARRIES the pan, which is what makes the grid travel
        # with the surface instead of being nailed to the window. Without this
        # the canvas reads as a viewport sliding over a static picture — the
        # thing the pips' own doc comment always said the phase was for, and
        # which nothing checked until a counterfactual asked.
        pan_xy = [int(part) for part in str(pan).split(",")]
        assert_eq(
            far["phase"],
            pan_xy,
            "the lattice's phase is the pan, so the grid moves with the graph",
        )
        print(f"[C] at pan {pan} the lattice is still {far['pitch']} px, phased {far['phase']}")
        checks += 3

    # ── D — and it is DRAWN ───────────────────────────────────────────
    with RpcSubprocess("hello-node-lab") as tf:
        shot = abs_rects_of(tf.snapshot(source="paint"))
        canvas = shot[CANVAS]
        declared = lattice_of(tf)
        assert declared is not None
        out = Path("/tmp/pinion-r1705-lattice.png")
        tf.request("scene/screenshot", {"path": "", "out_path": str(out)})
        assert out.exists(), "no screenshot"
        png = read_png_rgba8(out)
        want = (
            declared["color"]["r"],
            declared["color"]["g"],
            declared["color"]["b"],
        )
        pitch = declared["pitch"]

        # An empty stretch of canvas, below the graph and left of the gate panel.
        x0, y0 = canvas[0] + 40, canvas[1] + canvas[3] - 220
        hits = 0
        probes = 0
        for row in range(6):
            for col in range(6):
                # The lattice's phase is 0 here, so a dot sits where the pitch
                # says it does, measured from the canvas's own origin.
                px = canvas[0] + ((x0 - canvas[0]) // pitch + col) * pitch
                py = canvas[1] + ((y0 - canvas[1]) // pitch + row) * pitch
                probes += 1
                if png_pixel(png, px, py)[:3] == want:
                    hits += 1
        assert hits == probes, (
            f"only {hits} of {probes} lattice points carry the grid colour "
            f"{want} — the declaration is published and the renderer is not "
            f"drawing it"
        )
        print(f"[D] {hits}/{probes} probed lattice points are the grid colour in "
              f"the rendered pixels")
        checks += 1

        # ★ And the negative control: BETWEEN the dots is the ground, or the
        # check above would pass against a canvas painted entirely grid-colour.
        between = png_pixel(png, canvas[0] + 40 + pitch // 2, canvas[1] + canvas[3] - 220 + pitch // 2)
        assert between[:3] != want, (
            f"the gap between two dots is the grid colour too ({between}), so "
            f"the pixel check above proves nothing"
        )
        print(f"[D] and the gap between dots is {between[:3]}, not {want}")
        checks += 1

        # ── D2 — the person's own report, in pixels ───────────────────
        # Zoom all the way out AND pan far past the world surface the pips used
        # to stop at, then COUNT the grid pixels in an empty patch. A full
        # lattice puts one dot per pitch-square; anything less is the grid
        # running out, which is what "it doesn't feel infinite" was.
        for _ in range(9):
            tf.invoke(f"{EXT}/zoom_by", "out")
        for _ in range(4):
            tf.drag(
                from_at=(canvas[0] + canvas[2] - 40, canvas[1] + canvas[3] - 40),
                to_at=(canvas[0] + 40, canvas[1] + canvas[3] - 40),
            )
        tf.tick(0.016)
        far = lattice_of(tf)
        assert far is not None
        out2 = Path("/tmp/pinion-r1705-lattice-far.png")
        tf.request("scene/screenshot", {"path": "", "out_path": str(out2)})
        png2 = read_png_rgba8(out2)
        want2 = (far["color"]["r"], far["color"]["g"], far["color"]["b"])
        # ★ Every pixel of the patch, not a stride. A stride computed against
        # the wrong pitch is how the diagnostic for this very check reported
        # zero dots on a canvas that was full of them — the dots sit at
        # `phase mod pitch`, which was odd, and the probe sampled even columns.
        # Counting the patch cannot be wrong about the pitch it does not use.
        patch = 180
        px0, py0 = canvas[0] + 20, canvas[1] + canvas[3] - patch - 20
        hits2 = sum(
            1
            for yy in range(py0, py0 + patch)
            for xx in range(px0, px0 + patch)
            if png_pixel(png2, xx, yy)[:3] == want2
        )
        expected = (patch // far["pitch"]) ** 2
        assert hits2 >= expected * 8 // 10, (
            f"panned to {tf.query(f'{EXT}/pan')} at zoom {tf.query(f'{EXT}/zoom')}, "
            f"an empty {patch}x{patch} patch holds {hits2} grid pixels where a "
            f"full lattice at pitch {far['pitch']} would hold about {expected} — "
            f"the grid ran out, which is the report this round repaired"
        )
        print(
            f"[D] panned to {tf.query(f'{EXT}/pan')} at zoom "
            f"{tf.query(f'{EXT}/zoom')}: {hits2} grid pixels in a {patch}x{patch} "
            f"patch (a full lattice at pitch {far['pitch']} is about {expected})"
        )
        checks += 1

    print(f"[r1705] {checks} assertion point(s)")
    assert checks >= 14, f"{checks} < 14"


if __name__ == "__main__":
    run_demo("r1705 a grid is declared, not enumerated", body)
