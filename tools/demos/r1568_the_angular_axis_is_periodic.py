#!/usr/bin/env python3
"""R1568 §5.38 §2 #7 — the crate's first NON-CARTESIAN coordinate system.

Every chart in `pinion-chart` until this round shared one unexamined
assumption: a value maps to a pixel on ONE axis, x horizontally and y
vertically, and the two compose by sitting at right angles. `ValueScale` is
that assumption's type, and four axis KINDS fit inside it because each is
still a map onto one line.

A polar plot does not fit, and the part that does not is not the trigonometry
— it is that **the angular axis is periodic**. On a line 0 and 360 are two
places; on a compass they are the same place, and everything this round adds
follows from that one fact.

`hello-polar` plots a wind mast's mean gust by bearing, including one reading
that ran past north (372 degrees), and a five-facet radar.

What this script checks, and why each check discriminates:

* **PAST Qt 6.11 (1): the trace closes BY DERIVATION.** The series path's own
  command list is read: five samples and a `Close` on a full turn. A Qt radar
  gets that final segment by appending the first point a second time, which
  puts a duplicate in the data the model does not contain. The counterfactual
  is the sector chip, where the same samples do not close.
* **PAST Qt 6.11 (2): an out-of-period bearing is PLACED, and reported.** 372
  degrees is a bearing of 12, so the mark is drawn — and it lands on the same
  pixel a 12-degree reading would. Qt's angular axis is an ordinary
  `QValueAxis`, so there it is simply out of range and nothing is drawn. The
  caption carries the report, which is a thing only an axis that placed the
  value can say.
* **PAST Qt 6.11 (3): a SECTOR, which `QPolarChart` cannot draw at all.** Half
  a turn opens the loop, drops the out-of-period sample to genuinely
  off-scale, and removes the rim — a full circle there would claim angles the
  axis does not carry. Wrapping is therefore a property of the SWEEP and not
  of the value.
* **PAST Qt 6.11 (4): the winding is a declaration.** `QPolarChart` hard-codes
  clockwise. The chip mirrors the plot, read as pixel positions rather than as
  a flag.
* **The seam is labelled once.** The tick at the end of a closed period is the
  tick at its start, so no two angular labels ever land on the same spoke.
* **The radar's spokes are NAMED** and its polygons close — the nominal
  angular axis, one label per facet.
* **Keyboard and RPC move ONE selection**, so a binding that grew a second
  copy of the choice would be caught.
* **The report reaches assistive technology.** The forms are a `radiogroup`,
  the declarations `button[aria-pressed]`, and the caption a live region
  naming the sweep, the winding and what became of the out-of-period reading.
  QtCharts implements no accessibility interface at all.

Run from the workspace root:
    cargo build -p hello-polar --release
    python3 tools/demos/r1568_the_angular_axis_is_periodic.py
"""

from __future__ import annotations

import math
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    access_node_by_tag,
    assert_eq,
    find_by_tag,
    run_demo,
    walk_nodes,
)

VIEWPORT = (860, 560)

FORM_TAG = "form"
SECTOR_TAG = "sector"
WINDING_TAG = "winding"
CAPTION_TAG = "caption"

FORM_COMPASS, FORM_RADAR = 0, 1

#: `hello-polar`'s mast readings, mirrored rather than imported — a demo that
#: read the fixture out of the code under test could not catch it changing.
GUSTS = [(0.0, 4.2), (90.0, 9.1), (180.0, 2.4), (270.0, 6.8), (372.0, 5.5)]

#: The reading that ran past north, and its index in the series.
WRAPPED_BEARING = 372.0
WRAPPED_INDEX = 4

#: The radar's spokes.
FACETS = ["speed", "range", "armour", "cost", "crew"]
SERIES_COUNT = 2


def snapshot(tf: RpcSubprocess) -> dict:
    res = tf.snapshot(source="paint", viewport=VIEWPORT)
    assert res, "paint snapshot returned no result"
    return res


def node(snap: dict, tag: str) -> dict:
    n = find_by_tag(snap, tag)
    assert n is not None, f"{tag} is in the paint tree"
    return n


def rect(snap: dict, tag: str) -> dict:
    return node(snap, tag)["rect"]


def centre(r: dict) -> tuple[float, float]:
    return float(r["x"]) + float(r["w"]) / 2.0, float(r["y"]) + float(r["h"]) / 2.0


def count_prefix(snap: dict, prefix: str) -> int:
    return sum(
        1 for _, n in walk_nodes(snap) if str(n.get("tag") or "").startswith(prefix)
    )


def tags_with_prefix(snap: dict, prefix: str) -> list[str]:
    """Every tag under `prefix`, in emit order.

    Indexed lookup would be wrong here: a mark's tag carries the index of the
    TICK it came from, so a tick the scale places at radius zero — the
    innermost one, which is the centre — leaves a hole rather than shifting
    every later ring's name. That is the same rule `chart.label.x.{k}` keeps,
    and a consumer reading `chart.ring.2` wants the third tick's ring, not the
    third drawn one.
    """
    return [
        str(n.get("tag"))
        for _, n in walk_nodes(snap)
        if str(n.get("tag") or "").startswith(prefix)
    ]


def caption(snap: dict) -> str:
    return node(snap, CAPTION_TAG)["content"]


def commands(snap: dict, tag: str) -> list[dict]:
    return node(snap, tag)["commands"]


def closes(snap: dict, tag: str) -> bool:
    """Whether the path tagged `tag` carries a `Close` — the derived segment."""
    return any(c.get("type") == "Close" for c in commands(snap, tag))


def pick_form(tf: RpcSubprocess, i: int) -> None:
    tf.click(path=f"{FORM_TAG}#{i}")
    tf.tick(0.016)


def toggle(tf: RpcSubprocess, tag: str) -> None:
    tf.click(path=tag)
    tf.tick(0.016)


def body() -> None:
    with RpcSubprocess("hello-polar", boot_grace=1.5) as tf:
        for _ in range(3):
            tf.tick(0.016)

        # ── (A) the grid is a coordinate system, not two rulers ──────
        snap = snapshot(tf)
        rings = tags_with_prefix(snap, "chart.ring.")
        assert rings, "radial gridlines are rings"
        assert "chart.ring.0" not in rings, (
            "the innermost tick is the centre, so it draws no ring — and the "
            "tags keep the TICK index rather than renumbering around the hole"
        )
        spokes = count_prefix(snap, "chart.spoke.")
        assert spokes > 0, "angular gridlines are spokes"
        assert find_by_tag(snap, "chart.rim") is not None, (
            "a closed axis has a rim — the plot's edge is a circle"
        )
        assert_eq(
            count_prefix(snap, "chart.label.a."),
            spokes,
            "one angular label per spoke",
        )
        # Every ring is concentric on the same centre, which is what makes
        # this ONE coordinate system rather than a stack of shapes.
        centres = [centre(rect(snap, t)) for t in rings]
        for k, c in enumerate(centres):
            assert abs(c[0] - centres[0][0]) <= 1.5 and abs(c[1] - centres[0][1]) <= 1.5, (
                f"ring {k} is concentric with ring 0: {c} vs {centres[0]}"
            )
        # ...and every sample sits at its own radius from that centre, in the
        # order the data does. Bearing 90 has the largest gust, so its mark is
        # furthest out.
        cx, cy = centres[0]
        radii = [
            math.dist(centre(rect(snap, f"chart.point.0.{j}")), (cx, cy))
            for j in range(len(GUSTS))
        ]
        biggest = max(range(len(GUSTS)), key=lambda j: GUSTS[j][1])
        assert_eq(
            max(range(len(radii)), key=lambda j: radii[j]),
            biggest,
            "the largest gust is the furthest from the centre — the radial "
            "axis is a real value axis, not a decoration",
        )

        # ── (B) PAST QT: the trace closes by derivation ──────────────
        assert closes(snap, "chart.series.0"), (
            "PAST QT: the segment from the last sample back to the first is "
            "the AXIS's doing. A Qt radar gets it by appending the first "
            "point again, putting a duplicate in the data the model lacks"
        )
        assert_eq(
            len(commands(snap, "chart.series.0")),
            len(GUSTS) + 1,
            "five samples and a Close, with nothing repeated",
        )

        # ── (C) PAST QT: an out-of-period bearing is PLACED ──────────
        wrapped = rect(snap, f"chart.point.0.{WRAPPED_INDEX}")
        assert wrapped is not None, (
            f"PAST QT: {WRAPPED_BEARING:.0f} degrees is a bearing of 12, so "
            "the reading is drawn. Qt's angular axis is an ordinary "
            "QValueAxis, so there it is out of range and draws nothing"
        )
        # And it lands where a 12-degree bearing lands: just clockwise of the
        # 0-degree sample, on the same side of the vertical.
        north = centre(rect(snap, "chart.point.0.0"))
        near = centre(wrapped)
        assert near[0] > cx, f"12 degrees is clockwise of north: {near}"
        assert abs(near[0] - north[0]) < abs(near[0] - cx) + 40, (
            f"...and close to it rather than a quarter turn away: {near} vs {north}"
        )
        text = caption(snap)
        assert "bearing of 12" in text, text
        assert "1 wrapped, 0 off-scale" in text, (
            f"the wrap is REPORTED — only an axis that placed the value can "
            f"say this: {text}"
        )

        # ── (D) the seam is labelled ONCE ────────────────────────────
        seats = []
        for k in range(count_prefix(snap, "chart.label.a.")):
            seats.append(tuple(rect(snap, f"chart.label.a.{k}").values()))
        assert_eq(
            len(set(seats)),
            len(seats),
            "the tick at the end of a closed period IS the tick at its start, "
            "so no two angular labels stack",
        )

        # ── (E) PAST QT: a sector, which QPolarChart cannot draw ─────
        toggle(tf, SECTOR_TAG)
        snap = snapshot(tf)
        assert not closes(snap, "chart.series.0"), (
            "a sector is not periodic, so the trace stays open"
        )
        assert_eq(
            find_by_tag(snap, f"chart.point.0.{WRAPPED_INDEX}"),
            None,
            "and the same reading is now genuinely off-scale — wrapping is a "
            "property of the SWEEP, not of the value",
        )
        assert_eq(
            find_by_tag(snap, "chart.rim"),
            None,
            "a sector draws no full-circle rim: it would claim angles the "
            "axis does not carry",
        )
        assert count_prefix(snap, "chart.spoke.") > 0, "but it keeps its spokes"
        text = caption(snap)
        assert "Half turn" in text, text
        assert "0 wrapped, 1 off-scale" in text, text
        toggle(tf, SECTOR_TAG)

        # ── (F) PAST QT: the winding is a declaration ────────────────
        snap = snapshot(tf)
        east_cw = centre(rect(snap, "chart.point.0.1"))
        assert "increase clockwise" in caption(snap), caption(snap)
        toggle(tf, WINDING_TAG)
        snap = snapshot(tf)
        east_ccw = centre(rect(snap, "chart.point.0.1"))
        assert east_cw[0] > east_ccw[0] + 20, (
            "PAST QT: the 90-degree bearing sits at 3 o'clock clockwise and "
            f"at 9 o'clock the other way. QPolarChart hard-codes the first: "
            f"{east_cw} vs {east_ccw}"
        )
        assert "increase counter-clockwise" in caption(snap), caption(snap)
        # The RADIUS is untouched — the winding turns the plot, it does not
        # rescale it.
        assert abs(math.dist(east_cw, (cx, cy)) - math.dist(east_ccw, (cx, cy))) < 2.0, (
            "the same sample keeps its radius through a winding change"
        )
        toggle(tf, WINDING_TAG)

        # ── (G) the radar: named spokes, closed polygons ─────────────
        pick_form(tf, FORM_RADAR)
        snap = snapshot(tf)
        assert_eq(
            count_prefix(snap, "chart.spoke."),
            len(FACETS),
            "one spoke per facet — the angular axis is nominal here",
        )
        assert_eq(
            [node(snap, f"chart.label.a.{k}")["content"] for k in range(len(FACETS))],
            FACETS,
            "and each is NAMED, from the category list the chart was built with",
        )
        assert_eq(
            count_prefix(snap, "chart.area."),
            SERIES_COUNT,
            "a radar is filled by default",
        )
        for i in range(SERIES_COUNT):
            assert closes(snap, f"chart.series.{i}"), f"series {i} closes"
            assert_eq(
                len(commands(snap, f"chart.series.{i}")),
                len(FACETS) + 1,
                f"five vertices and a Close on series {i}",
            )

        # ── (H) keyboard and RPC drive ONE selection ─────────────────
        tf.key(path=FORM_TAG, name="Home")
        tf.tick(0.016)
        by_key_home = caption(snapshot(tf))
        assert "bearing of 12" in by_key_home, (
            f"Home selects the compass, as a click on chip 0 does: {by_key_home}"
        )
        tf.key(path=FORM_TAG, name="ArrowRight")
        tf.tick(0.016)
        by_key = caption(snapshot(tf))
        pick_form(tf, FORM_RADAR)
        by_click = caption(snapshot(tf))
        assert_eq(
            by_key,
            by_click,
            "keyboard and pointer reach ONE selection, not two copies of it",
        )
        pick_form(tf, FORM_COMPASS)

        # ── (I) the report reaches assistive technology ──────────────
        toggle(tf, SECTOR_TAG)
        toggle(tf, WINDING_TAG)
        snap = snapshot(tf)
        acc = tf.request("scene/access", {}).result or {}
        radios = [access_node_by_tag(acc, f"{FORM_TAG}#{i}") for i in range(2)]
        assert all(r is not None for r in radios), f"one AT node per form: {radios}"
        assert_eq(
            [r["role"] for r in radios],
            ["radio", "radio"],
            "the forms are a 1-of-N choice, not two switches",
        )
        group = access_node_by_tag(acc, FORM_TAG)
        assert group is not None and group["role"] == "radiogroup", f"{group}"
        for tag in (SECTOR_TAG, WINDING_TAG):
            n_ = access_node_by_tag(acc, tag)
            assert n_ is not None and n_["role"] == "button", f"{tag}: {n_}"
        status = access_node_by_tag(acc, CAPTION_TAG)
        assert status is not None, "the caption is an AT node"
        assert_eq(status["role"], "status", "and it is a live region")
        assert_eq(
            status.get("name"),
            caption(snap),
            "PAST QT: a screen reader is told the SAME sweep, winding and "
            "out-of-period report a sighted reader sees. QtCharts draws into "
            "a QGraphicsScene and implements no accessibility interface",
        )
        name = status.get("name") or ""
        assert "Half turn" in name, name
        assert "off-scale" in name, name
        assert "counter-clockwise" in name, name


if __name__ == "__main__":
    run_demo("r1568_the_angular_axis_is_periodic", body)
