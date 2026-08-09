#!/usr/bin/env python3
"""R1625 §5.28 §2 #7 — a smooth line says what it invented.

A chart that joins its samples with straight lines draws only values the data
contains. A spline does not: a cubic through a plateau followed by a rise dips
BELOW the plateau, so a chart of a quantity that cannot be negative paints one
anyway and nothing in the picture says so. The reference toolkit's spline
series has one method, no choice of it, its control points internal, and no
report at all — so the answer to "did my chart draw a value I never recorded"
is to look at it.

Here the interpolation is a declared choice with two chips (is the line
curved; may that curve invent a value), and `LineChart::overshoot` answers the
question in DATA space without laying anything out.

What each check discriminates:

* **The chips reach the paint.** The series node's command stream must gain
  cubics — a curve drawn as a densely sampled polyline would look identical
  and say something different to a client reading the scene.
* **The report changes with the choice, and matches it.** Straight and
  monotone must report nothing invented; the smooth one must report an
  excursion on this data. A report that always said "nothing" would pass the
  first half alone.
* **The caption and the live region are one derivation.** A sighted reader and
  a screen reader must not be told different things.

Run from the workspace root:
    cargo build -p hello-series-toggle --release
    python3 tools/demos/r1625_the_curve_says_what_it_invented.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    access_node_by_tag,
    assert_eq,
    call,
    find_by_tag,
    run_demo,
    text_of_tag,
)

EXAMPLE = "hello-series-toggle"
WIN = (520, 420)
SERIES = 3


def kinds(tf: RpcSubprocess, tag: str) -> list[str]:
    snap = tf.snapshot(source="paint", viewport=WIN)
    node = find_by_tag(snap, tag)
    assert node is not None, f"{tag} is painted"
    cmds = node.get("commands")
    assert isinstance(cmds, list), f"{tag} publishes its commands: {node}"
    return [c.get("type") for c in cmds]


def caption(tf: RpcSubprocess) -> str:
    return text_of_tag(tf, "caption", viewport=WIN)


def run(tf: RpcSubprocess) -> None:
    # ── 1. straight by default — the join that invents nothing ───────────
    said = caption(tf)
    assert said.startswith("linear"), f"boots straight: {said}"
    assert "no value drawn" in said, said
    first = kinds(tf, "chart.series.0")
    assert_eq(first[0], "MoveTo", "a series starts with a move")
    assert "CurveTo" not in first, f"straight draws segments: {set(first)}"
    assert "LineTo" in first, f"and they are line segments: {set(first)}"
    print(f"[demo] {said}")

    # ── 2. the smooth chip curves the line, in CUBICS ────────────────────
    tf.click(path="smooth")
    tf.tick(0.016)
    smooth = kinds(tf, "chart.series.0")
    assert "CurveTo" in smooth, f"the smooth chip curves the line: {set(smooth)}"
    assert "LineTo" not in smooth, (
        "and does it with cubic commands rather than a sampled polyline, which "
        f"is what a client reading the scene can tell apart: {set(smooth)}"
    )
    assert_eq(
        len([k for k in smooth if k == "CurveTo"]),
        len(first) - 1,
        "one cubic per gap — the same gaps the straight line had",
    )
    print(f"[demo] smooth: {len(smooth) - 1} cubics, no line segments")

    # ── 3. NEGATIVE CONTROL: the smooth curve REPORTS what it invented ───
    #      Not "the caption changed" — the number of segments that left their
    #      samples, on data chosen so there are some.
    said = caption(tf)
    assert said.startswith("catmull-rom"), said
    assert "leave their samples" in said, (
        f"the smooth curve names the excursion rather than hiding it: {said}"
    )
    print(f"[demo] {said}")

    # ── 4. the safe chip keeps the curve and drops the invention ─────────
    tf.click(path="safe")
    tf.tick(0.016)
    safe = kinds(tf, "chart.series.0")
    assert "CurveTo" in safe, f"still a curve: {set(safe)}"
    said = caption(tf)
    assert said.startswith("monotone"), said
    assert "no value drawn" in said, (
        f"the monotone curve is smooth AND invents nothing: {said}"
    )
    print(f"[demo] {said}")

    # ── 5. the caption and the live region are one derivation ────────────
    access = call(tf, "scene/access", {"viewport": list(WIN)})
    node = access_node_by_tag(access, "caption")
    assert node is not None, f"the caption is a live region: {access}"
    assert_eq(
        node.get("name"),
        said,
        "a screen reader and a sighted reader are told the same thing",
    )

    # ── 6. both chips announce themselves ────────────────────────────────
    for tag in ("smooth", "safe"):
        chip = access_node_by_tag(access, tag)
        assert chip is not None, f"{tag} announces itself"
        assert chip.get("role") == "button", f"{tag} is a button: {chip}"
    print("[demo] both interpolation chips are announced")

    # ── 7. every series took the same interpolation ──────────────────────
    for i in range(SERIES):
        assert "CurveTo" in kinds(tf, f"chart.series.{i}"), f"series {i} is curved"

    # ── 8. and turning smoothing off restores the straight line ──────────
    tf.click(path="smooth")
    tf.tick(0.016)
    back = kinds(tf, "chart.series.0")
    assert_eq(back, first, "the straight chart is the chart it was")
    assert "no value drawn" in caption(tf), caption(tf)

    print("[demo] the curve says what it invented")


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        for _ in range(3):
            tf.tick(0.016)
        run(tf)


if __name__ == "__main__":
    run_demo("R1625 §5.28 — a smooth line says what it invented", body)
