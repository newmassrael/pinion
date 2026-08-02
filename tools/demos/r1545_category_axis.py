#!/usr/bin/env python3
"""R1545 §5.38 §2 #7 — a category is an axis KIND, and it says what it shows.

`pinion-chart` has drawn a categorical x since R1374, but never as an axis.
The slot metric was `left + i * slot`, written out three times inside `bar.rs`
— for the bar box, its label and its click surface — reachable by no other
chart and by no consumer. So the crate could offer a linear, a logarithmic
(R1528) and a time (R1529) axis interchangeably, and could not offer the
fourth kind it was already drawing.

`hello-category-axis` plots twelve monthly buckets TWICE from one axis: a
`BarChart` of revenue, and a `LineChart::x_category` of attainment over the
same slots. The line chart is the proof the axis is a kind — it is a numeric-x
chart taking the categorical axis exactly as it takes the other three.

What this script checks, and why each check discriminates:

* **The band is published, and everything derives from it.** A bar's rect and
  its label's box are compared against the axis's own band arithmetic, which is
  what says they come from ONE definition rather than three copies that agree
  today. Qt has no such accessor at all: a `QBarSeries`' rect is computed
  inside the private series painter, so a Qt application cannot ask where
  category 3 is drawn.
* **One window narrows BOTH charts.** The bars that survive and the x labels
  the LINE chart paints are read separately and required to name the same
  months. Two charts windowed by two mechanisms would pass a per-chart check
  and fail this one.
* **The window widens what it keeps.** Three of twelve categories in the same
  pixels must be more than twice as wide, which is the reason a window is worth
  having rather than a filter.
* **PAST Qt 6.11 (1): an unresolvable name is REPORTED.** Qt's
  `QBarCategoryAxis::setRange(QString, QString)` returns `void`; a name that is
  not a category leaves the axis silently unwindowed, indistinguishable from a
  range that happened to be full. Here the action channel answers the failure,
  `error` carries it, and the caption states it. The counterfactual is in the
  same section: a name that DOES resolve answers the empty string, so this is
  not a surface that always complains.
* **PAST Qt 6.11 (2): the categories in view are readable.** `visible` answers
  how many slots the live window shows. Qt reports `count()` — every category,
  whatever the range is — and the min/max NAMES it was set to, never what is on
  screen.
* **Keyboard and RPC move ONE window.** The same three actions are driven both
  ways and the resulting state is compared, so a binding that grew a second
  copy of the range would be caught.
* **The window reaches assistive technology.** QtCharts draws into a
  `QGraphicsScene` whose axis labels carry no accessible relationship, so a Qt
  screen-reader user cannot be told which categories are shown.

Run from the workspace root:
    cargo build -p hello-category-axis --release
    python3 tools/demos/r1545_category_axis.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    access_node_by_tag,
    assert_eq,
    find_by_tag,
    run_demo,
)

VIEWPORT = (720, 520)
WINDOW_TAG = "category_window"

#: `hello-category-axis`'s twelve categories, mirrored rather than imported —
#: a demo that read the list out of the code under test could not catch the
#: code changing it.
MONTHS = [
    "Jan",
    "Feb",
    "Mar",
    "Apr",
    "May",
    "Jun",
    "Jul",
    "Aug",
    "Sep",
    "Oct",
    "Nov",
    "Dec",
]

#: The example's `BAR_RECT` and the default `ChartStyle` margins, so the band
#: arithmetic can be predicted from outside the binding.
BAR_RECT_X, BAR_RECT_W = 16, 720 - 32
MARGIN_LEFT, MARGIN_RIGHT = 52, 16
#: `BAR_GAP_FRAC` — the fraction of a band left empty between bars.
BAR_GAP_FRAC = 0.2

FROM_PATH = f"/{WINDOW_TAG}/external/from"
TO_PATH = f"/{WINDOW_TAG}/external/to"
LO_PATH = f"/{WINDOW_TAG}/external/lo"
HI_PATH = f"/{WINDOW_TAG}/external/hi"
VISIBLE_PATH = f"/{WINDOW_TAG}/external/visible"
ERROR_PATH = f"/{WINDOW_TAG}/external/error"
RANGE_ACTION = f"/{WINDOW_TAG}/external/range"
PAN_ACTION = f"/{WINDOW_TAG}/external/pan"
RESET_ACTION = f"/{WINDOW_TAG}/external/reset"


def snapshot(tf: RpcSubprocess) -> dict:
    res = tf.snapshot(source="paint", viewport=VIEWPORT)
    assert res, "paint snapshot returned no result"
    return res


def bar_indices(snap: dict) -> list[int]:
    """Which category indices the bar chart drew, by tag."""
    return [i for i in range(len(MONTHS)) if find_by_tag(snap, f"bars.bar.{i}") is not None]


def bar_rect(snap: dict, i: int) -> dict:
    n = find_by_tag(snap, f"bars.bar.{i}")
    assert n is not None, f"bar {i} is in the paint tree"
    return n["rect"]


def label_rect(snap: dict, i: int) -> dict:
    n = find_by_tag(snap, f"bars.xlabel.{i}")
    assert n is not None, f"x label {i} is in the paint tree"
    return n["rect"]


def label_text(snap: dict, i: int) -> str:
    n = find_by_tag(snap, f"bars.xlabel.{i}")
    assert n is not None, f"x label {i} is in the paint tree"
    return n["content"]


def trend_labels(snap: dict) -> list[str]:
    """The LINE chart's x tick labels — a numeric-x chart on the same axis."""
    out: list[str] = []
    k = 0
    while (n := find_by_tag(snap, f"trend.label.x.{k}")) is not None:
        out.append(n["content"])
        k += 1
    return out


def caption(snap: dict) -> str:
    n = find_by_tag(snap, "caption")
    assert n is not None, "the caption is in the paint tree"
    return n["content"]


def band_width(visible: int) -> float:
    """The band width the axis must produce for `visible` categories in the
    example's plot area — predicted from outside, so a chart that stopped
    deriving from the axis fails here."""
    plot_w = BAR_RECT_W - (MARGIN_LEFT + MARGIN_RIGHT)
    return plot_w / visible


def set_range(tf: RpcSubprocess, lo: str, hi: str) -> str:
    answer = tf.invoke(RANGE_ACTION, {"from": lo, "to": hi})
    tf.tick(0.016)
    return answer


def body() -> None:
    with RpcSubprocess("hello-category-axis", boot_grace=1.5) as tf:
        for _ in range(3):
            tf.tick(0.016)

        # ── (A) premise: the un-windowed axis ─────────────────────────
        snap = snapshot(tf)
        assert_eq(
            bar_indices(snap),
            list(range(12)),
            "every category is drawn when nothing is windowed",
        )
        assert_eq(
            [label_text(snap, i) for i in range(12)],
            MONTHS,
            "and each slot is labelled by the name the AXIS carries for it",
        )
        assert_eq(int(tf.query(LO_PATH)), -1, "no window is not 'windowed onto 0'")
        assert_eq(int(tf.query(HI_PATH)), -1, "on either endpoint")
        assert_eq(int(tf.query(VISIBLE_PATH)), 12, "twelve categories in view")
        assert_eq(tf.query(ERROR_PATH), "", "and nothing failed to resolve")
        assert_eq(tf.query(FROM_PATH), "", "no range was requested")

        # ── (B) the band IS the slot, and everything derives from it ──
        # Predicted from outside the binding: 12 bands across the plot area.
        want_band = band_width(12)
        for i in (0, 5, 11):
            r = bar_rect(snap, i)
            centre = float(r["x"]) + float(r["w"]) / 2.0
            want_centre = BAR_RECT_X + MARGIN_LEFT + (i + 0.5) * want_band
            assert abs(centre - want_centre) <= 1.5, (
                f"bar {i} is centred in its band: got {centre}, want {want_centre}"
            )
            assert abs(float(r["w"]) - want_band * (1 - BAR_GAP_FRAC)) <= 1.5, (
                f"bar {i} fills its band less the gap: got {r['w']}"
            )
        # The LABEL box is the band itself — the same call, not a second copy.
        for i in (0, 5, 11):
            lr = label_rect(snap, i)
            assert abs(float(lr["w"]) - want_band) <= 1.5, (
                f"label {i} spans the whole band: got {lr['w']}"
            )
            br = bar_rect(snap, i)
            bar_centre = float(br["x"]) + float(br["w"]) / 2.0
            lab_centre = float(lr["x"]) + float(lr["w"]) / 2.0
            assert abs(bar_centre - lab_centre) <= 1.5, (
                f"label {i} is centred on its own bar ({lab_centre} vs {bar_centre})"
            )
        # Adjacent bands TILE: band k's right edge is band k+1's left edge.
        for i in range(11):
            gap = float(label_rect(snap, i + 1)["x"]) - (
                float(label_rect(snap, i)["x"]) + float(label_rect(snap, i)["w"])
            )
            assert abs(gap) <= 1.5, f"bands {i}/{i + 1} tile the axis (gap {gap})"

        # ── (C) one window narrows BOTH charts ───────────────────────
        assert_eq(set_range(tf, "Apr", "Jun"), "", "a resolvable range applies")
        snap = snapshot(tf)
        assert_eq(bar_indices(snap), [3, 4, 5], "only the windowed bars are drawn")
        assert_eq(
            [label_text(snap, i) for i in (3, 4, 5)],
            ["Apr", "May", "Jun"],
            "labelled by the axis, at their ORIGINAL category indices",
        )
        assert_eq(
            trend_labels(snap),
            ["Apr", "May", "Jun"],
            "and the LINE chart — a numeric-x chart on the categorical axis — "
            "labels the same months and no others, which is the whole claim: "
            "two charts, one axis",
        )
        assert_eq(int(tf.query(LO_PATH)), 3, "the resolved window's first slot")
        assert_eq(int(tf.query(HI_PATH)), 5, "and its last")
        assert_eq(
            int(tf.query(VISIBLE_PATH)),
            3,
            "PAST QT: how many categories are actually on screen. "
            "QBarCategoryAxis::count() answers 12 whatever the range is",
        )

        # ── (D) a window WIDENS what it keeps ────────────────────────
        want_band3 = band_width(3)
        r = bar_rect(snap, 4)
        assert abs(float(r["w"]) - want_band3 * (1 - BAR_GAP_FRAC)) <= 1.5, (
            f"the kept bars widened to the new band: got {r['w']}, "
            f"want {want_band3 * (1 - BAR_GAP_FRAC)}"
        )
        assert float(r["w"]) > band_width(12) * 2, (
            "3 of 12 in the same pixels is more than twice as wide — the "
            "reason a window beats a filter"
        )
        assert_eq(
            caption(snap),
            "showing Apr-Jun — 3 of 12 categories on one axis, both charts",
            "and the caption reports the chart's OWN visible_categories()",
        )

        # ── (E) PAST QT: an unresolvable name is reported ────────────
        answer = set_range(tf, "Smarch", "Jun")
        assert_eq(
            answer,
            'no category named "Smarch"',
            "the action channel answers WHICH endpoint did not resolve. "
            "Qt's setRange returns void and this request would be a no-op "
            "the caller cannot detect",
        )
        assert_eq(tf.query(ERROR_PATH), answer, "and the field carries the same text")
        snap = snapshot(tf)
        assert_eq(
            bar_indices(snap),
            list(range(12)),
            "the axis stays whole rather than silently keeping the old window",
        )
        assert "Smarch" in caption(snap), f"the caption names it: {caption(snap)}"
        assert "NOT applied" in caption(snap), "and says the range did not take"
        assert_eq(int(tf.query(LO_PATH)), -1, "an unresolved request is no window")
        assert_eq(
            int(tf.query(VISIBLE_PATH)), 12, "so every category is back in view"
        )
        # The counterfactual: a resolvable range answers the empty string, so
        # this surface is not one that always complains.
        assert_eq(set_range(tf, "Feb", "May"), "", "a good name resolves silently")
        assert_eq(tf.query(ERROR_PATH), "", "and clears the report")
        assert_eq(bar_indices(snapshot(tf)), [1, 2, 3, 4], "onto its own slots")

        # ── (F) panning moves whole categories, and writes NAMES back ─
        assert tf.invoke(PAN_ACTION, 2) is True, "the window can move right"
        tf.tick(0.016)
        assert_eq(int(tf.query(LO_PATH)), 3, "shifted by two categories")
        assert_eq(int(tf.query(HI_PATH)), 6, "keeping its span")
        assert_eq(
            tf.query(FROM_PATH),
            "Apr",
            "and the moved endpoints are written back as NAMES — the "
            "round-trip that proves the index and name forms agree",
        )
        assert_eq(tf.query(TO_PATH), "Jul", "on both ends")
        assert_eq(bar_indices(snapshot(tf)), [3, 4, 5, 6], "the bars followed")
        assert tf.invoke(PAN_ACTION, 99) is True, "a pan past the end still moves"
        tf.tick(0.016)
        assert_eq(int(tf.query(HI_PATH)), 11, "clamped to the last category")
        assert (
            tf.invoke(PAN_ACTION, 99) is False
        ), "and a pan that cannot move says so rather than pretending"

        # ── (G) keyboard and RPC move ONE window ─────────────────────
        tf.invoke(RESET_ACTION, None)
        tf.tick(0.016)
        assert_eq(int(tf.query(LO_PATH)), -1, "reset clears the window")
        assert_eq(bar_indices(snapshot(tf)), list(range(12)), "and the bars return")
        # The toolbar is the only focus stop, and the keys are gated on it —
        # an unfocused digit must NOT move the window, or the binding would be
        # swallowing keystrokes the rest of an app wants.
        tf.key(path=WINDOW_TAG, name="2")
        tf.tick(0.016)
        assert_eq(
            int(tf.query(LO_PATH)),
            -1,
            "a digit with nothing focused is not this widget's to consume",
        )
        assert_eq(
            tf.request("focus/get").result.get("tab_order"),
            [WINDOW_TAG],
            "the window toolbar is the binding's focus stop",
        )
        tf.request("focus/set", {"tag": WINDOW_TAG})
        tf.tick(0.016)
        tf.key(path=WINDOW_TAG, name="2")
        tf.tick(0.016)
        assert_eq(
            (int(tf.query(LO_PATH)), int(tf.query(HI_PATH))),
            (3, 5),
            "the keyboard preset lands on the SAME window the RPC range did — "
            "one range, two drivers",
        )
        tf.key(path=WINDOW_TAG, name="ArrowRight")
        tf.tick(0.016)
        assert_eq(int(tf.query(LO_PATH)), 4, "and the arrow pans it")
        assert_eq(tf.query(FROM_PATH), "May", "writing the name back")
        tf.key(path=WINDOW_TAG, name="3")
        tf.tick(0.016)
        assert_eq(
            tf.query(ERROR_PATH),
            'no category named "Smarch"',
            "the stale-view preset reaches the same resolution failure",
        )
        assert_eq(bar_indices(snapshot(tf)), list(range(12)), "and shows everything")
        tf.key(path=WINDOW_TAG, name="0")
        tf.tick(0.016)
        assert_eq(tf.query(ERROR_PATH), "", "0 clears the request")

        # ── (H) the window reaches assistive technology ──────────────
        set_range(tf, "Apr", "Jun")
        access = tf.request("scene/access", {}).result
        node = access_node_by_tag(access, WINDOW_TAG)
        assert node is not None, "the window control is in the access tree"
        name = node.get("name") or ""
        assert "Apr to Jun" in name, f"named by the categories in view: {name}"
        assert "3 of 12" in name, f"and by how many of them there are: {name}"
        tf.invoke(RESET_ACTION, None)
        tf.tick(0.016)
        access = tf.request("scene/access", {}).result
        name = (access_node_by_tag(access, WINDOW_TAG) or {}).get("name") or ""
        assert "all 12" in name, (
            f"and the unwindowed axis says so too, so the name is derived "
            f"from the window rather than fixed: {name}"
        )


if __name__ == "__main__":
    run_demo("r1545 categorical axis", body)
