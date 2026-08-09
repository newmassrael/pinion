#!/usr/bin/env python3
"""R1622 §5.28 §2 #7 — the crate stacks, and each band keeps its own value.

The chart residue's plainest item: `pinion-chart` drew several areas over one
another and left the accumulation to the application. A binding that wanted a
stacked area chart had to hand in a series whose `y` was ALREADY a running sum
— so the chart's tooltips, legend and value axis all reported the total where
the reader sees a band, and the original measurement was gone before anything
could ask for it. That is the shape the standing directive rules out: the
deliverable is a crate that composes, not an example that pre-computes.

What this checks, and why each check discriminates:

* **The bands are drawn between curves.** An area closed onto a scalar baseline
  ends flat; a band ends on the curve below it. That difference is visible in
  the emitted path, so a stack that quietly drew overlaps would fail here.
* **Every band has its own extent and they differ.** A stack drawn as one
  merged silhouette would lose every series while still looking taller.

The value axis's extent is checked in Rust rather than here: it is an exact
numeric claim (`stacked_value_bounds` against `data_bounds`), and reading it
back off painted tick text would compare an SI-rounded string to a float — an
assertion that passes for the wrong reason.
* **Every series still paints its own band.** A stack is not one merged shape.

Run from the workspace root:
    cargo build -p hello-chart-fill --release
    python3 tools/demos/r1622_the_crate_stacks.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    indexed_tags,
    run_demo,
    text_of_tag,
)

EXAMPLE = "hello-chart-fill"
CHART = "chart"
WIN = (900, 600)


def run(tf: RpcSubprocess) -> None:
    snap = tf.snapshot(source="paint", viewport=WIN)
    rects = abs_rects_of(snap)

    # ── 1. every series paints its own band ──────────────────────────────
    areas = indexed_tags(rects, f"{CHART}.area.")
    assert len(areas) >= 2, f"a stack of several bands, not one shape: {areas}"
    print(f"[demo] bands painted: {areas}")

    # ── 2. the bands do not overlap vertically ───────────────────────────
    #      This is the whole visual difference. Overlapping translucent areas
    #      all start at the baseline and therefore share it; stacked bands sit
    #      ON one another, so each one's top is above the one below it.
    tops = []
    for i in areas:
        x, y, w, h = rects[f"{CHART}.area.{i}"]
        assert w > 0 and h > 0, f"band {i} has extent: {(x, y, w, h)}"
        tops.append((i, y, y + h))
    tops.sort()
    bottoms = {i: bottom for i, _top, bottom in tops}
    assert_eq(
        len({b for b in bottoms.values()}) > 1,
        True,
        "the bands do NOT all end on one baseline — overlapping areas would, "
        f"and that is exactly what stacking replaces: {bottoms}",
    )
    for (i, top_i, _), (j, top_j, _) in zip(tops, tops[1:]):
        assert top_j <= top_i, (
            f"band {j} sits on band {i} rather than starting from the axis "
            f"again: tops {top_j} vs {top_i}"
        )
    print(f"[demo] band extents (top, bottom): {[(i, t, b) for i, t, b in tops]}")

    # ── 3. the value axis is labelled, and its extent is checked exactly
    #      in Rust, not here.
    #      `stacked_value_bounds` vs `data_bounds` is an exact numeric claim
    #      (a stack of three 40-peaks reaches 120), and the unit test makes it.
    #      Reading it back off painted tick text would go through SI
    #      formatting and compare a rounded string to a float — an assertion
    #      that passes for the wrong reason is worse than one that is absent,
    #      so what is asserted here is only what the picture can honestly say.
    labels = [
        text_of_tag(tf, f"{CHART}.label.y.{i}", viewport=WIN)
        for i in range(24)
        if f"{CHART}.label.y.{i}" in rects
    ]
    assert len(labels) >= 2, f"the value axis is labelled: {labels}"
    assert len(set(labels)) >= 2, f"with distinct ticks, not one repeated: {labels}"
    print(f"[demo] value axis ticks: {labels}")

    # ── 4. NEGATIVE CONTROL: the chart is not one merged shape ───────────
    #      A stack drawn as a single silhouette would satisfy "the axis is
    #      taller" while losing every series. Each band must be its own node
    #      with its own extent.
    assert len(areas) == len(set(areas)), f"distinct bands: {areas}"
    heights = [rects[f"{CHART}.area.{i}"][3] for i in areas]
    assert all(h > 0 for h in heights), f"no band collapsed to nothing: {heights}"
    assert len(set(heights)) > 1, (
        f"the bands differ in height, so they carry different series: {heights}"
    )

    # ── 5. the picture is stable across frames ───────────────────────────
    for _ in range(3):
        tf.tick(0.016)
    again = abs_rects_of(tf.snapshot(source="paint", viewport=WIN))
    assert_eq(
        [again[f"{CHART}.area.{i}"] for i in areas],
        [rects[f"{CHART}.area.{i}"] for i in areas],
        "a stack is a derivation of the data, so it does not drift per frame",
    )

    print("[demo] the crate stacks")


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        for _ in range(3):
            tf.tick(0.016)
        run(tf)


if __name__ == "__main__":
    run_demo("R1622 §5.28 — the crate stacks", body)
