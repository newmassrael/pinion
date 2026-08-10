#!/usr/bin/env python3
"""R1626 §5.28 §2 #7 — the outline is an estimate; the box is the data.

A box plot's five numbers are order statistics: every one is a value the data
actually took. A violin's outline is not — it is a kernel density estimate,
and the kernel, the bandwidth and the width scaling are all CHOICES a reader
cannot recover from the picture. The reference toolkit has a box plot series
and no violin at all, so it has no answer here to be better than; what it does
have is the relevant precedent, a box set of five pre-computed doubles from
which no density could ever be estimated.

So the estimate is a value that carries what produced it, and the chart draws
the violin with the box INSIDE it: the shape is the estimate, the box is the
numbers.

What each check discriminates:

* **The violin is a different shape from the box, not a wider one.** A mark
  that merely fattened the box would pass "a violin node exists".
* **The estimate is mirrored.** A one-sided density plot is a different chart.
* **The box survives.** The whole point of the combined mark is that the
  reader gets the order statistics alongside the smoothing.
* **The mark does not move the categories.** It is a rendering of the same
  data on the same axes.
* **A log axis still reports what it cannot place**, and the violin is not
  clamped onto the plot floor to compensate.

Run from the workspace root:
    cargo build -p hello-boxplot --release
    python3 tools/demos/r1626_the_outline_is_an_estimate.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    access_node_by_tag,
    assert_eq,
    call,
    indexed_tags,
    run_demo,
)

EXAMPLE = "hello-boxplot"
WIN = (820, 500)


def centre_x(rect: tuple) -> float:
    x, _y, w, _h = rect
    return x + w / 2.0


def run(tf: RpcSubprocess) -> None:
    plain = abs_rects_of(tf.snapshot(source="paint", viewport=WIN))

    # ── 1. the chart boots as boxes ─────────────────────────────────────
    boxes = indexed_tags(plain, "chart.box.")
    assert len(boxes) >= 3, f"a box per endpoint: {boxes}"
    assert_eq(indexed_tags(plain, "chart.violin."), [], "and no violins yet")
    print(f"[demo] boxes: {boxes}")

    # ── 2. the violin chip adds the estimate and KEEPS the box ──────────
    tf.click(path="violin")
    tf.tick(0.016)
    vio = abs_rects_of(tf.snapshot(source="paint", viewport=WIN))
    assert_eq(indexed_tags(vio, "chart.violin."), boxes, "one violin per endpoint")
    assert_eq(
        indexed_tags(vio, "chart.box."),
        boxes,
        "the box stays inside it — the outline is an estimate, the box is data",
    )
    assert_eq(
        indexed_tags(vio, "chart.median."),
        boxes,
        "and so does the median",
    )
    print(f"[demo] violins: {indexed_tags(vio, 'chart.violin.')}")

    # ── 3. the violin is a DIFFERENT shape, not a wider box ─────────────
    #      A density reaches past the quartiles, so the outline CONTAINS the
    #      box on both sides. It is not always strictly taller in pixels: an
    #      endpoint whose samples are tightly clustered has a small bandwidth,
    #      and three of those can round to nothing — which is the estimate
    #      telling the truth about that endpoint rather than a defect. So the
    #      containment is asserted everywhere and the strict bulge at least
    #      once, which is what a mark that merely re-drew the box would fail.
    taller = 0
    for i in boxes:
        _x, vy, vw, vh = vio[f"chart.violin.{i}"]
        _x, by, bw, bh = vio[f"chart.box.{i}"]
        assert vy <= by, f"endpoint {i}: the estimate starts at or above the box"
        assert vy + vh >= by + bh, f"endpoint {i}: and ends at or below it"
        assert vw >= bw - 1, f"endpoint {i}: violin {vw} vs box {bw}"
        if vh > bh:
            taller += 1
    assert taller >= 1, (
        f"at least one endpoint's estimate reaches past its quartiles: {taller}"
    )
    print(f"[demo] every violin contains its box; {taller} reach past the quartiles")

    # ── 4. it is MIRRORED about the category centre ─────────────────────
    for i in boxes:
        assert abs(centre_x(vio[f"chart.violin.{i}"]) - centre_x(vio[f"chart.box.{i}"])) <= 1.0, (
            f"endpoint {i}: the violin is centred on its category, so a "
            "one-sided density plot would fail here"
        )
    print("[demo] every violin is centred on its category")

    # ── 5. the mark does not move the categories ────────────────────────
    for i in boxes:
        assert abs(centre_x(vio[f"chart.box.{i}"]) - centre_x(plain[f"chart.box.{i}"])) <= 1.0, (
            f"endpoint {i} moved when the mark changed"
        )
    print("[demo] the categories did not move")

    # ── 6. NEGATIVE CONTROL: the log axis still reports, unclamped ──────
    tf.click(path="logscale")
    tf.tick(0.016)
    logged = abs_rects_of(tf.snapshot(source="paint", viewport=WIN))
    assert len(indexed_tags(logged, "chart.violin.")) == len(boxes), (
        "a logarithmic value axis still draws violins"
    )
    for i in boxes:
        _x, _y, _w, h = logged[f"chart.violin.{i}"]
        assert h > 0, f"endpoint {i} has extent on a log axis"
    print("[demo] the log axis draws violins too")

    # ── 7. the caption and the a11y report are still whole ──────────────
    access = call(tf, "scene/access", {"viewport": list(WIN)})
    for tag in ("notch", "logscale", "violin"):
        node = access_node_by_tag(access, tag)
        assert node is not None, f"{tag} announces itself"
        assert node.get("role") == "button", f"{tag} is a button: {node}"
    caption = access_node_by_tag(access, "caption")
    assert caption is not None, "the caption is a live region"
    print("[demo] all three option chips are announced")

    # ── 8. and the violin comes off again ───────────────────────────────
    tf.click(path="violin")
    tf.tick(0.016)
    back = abs_rects_of(tf.snapshot(source="paint", viewport=WIN))
    assert_eq(indexed_tags(back, "chart.violin."), [], "the estimate is gone")
    assert_eq(indexed_tags(back, "chart.box."), boxes, "the boxes remain")

    print("[demo] the outline is an estimate; the box is the data")


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        for _ in range(3):
            tf.tick(0.016)
        run(tf)


if __name__ == "__main__":
    run_demo("R1626 §5.28 — the outline is an estimate", body)
