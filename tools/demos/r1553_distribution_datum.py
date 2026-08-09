#!/usr/bin/env python3
"""R1553 §5.38 §2 #7 — a datum can be a DISTRIBUTION, and it says how it was derived.

Every datum `pinion-chart` could plot until this round resolved to one
position: a `DataPoint` is `(x, y)`, a `Bar` is a label and a magnitude, a
`Slice` is a share. A `Distribution` occupies a span of the value axis and
carries interior landmarks, so ONE datum emits a box, a median, two whiskers,
two caps and a mark per outlier — Tukey's schema (1977), optionally notched
after McGill, Tukey & Larsen (1978).

`hello-boxplot` hands the crate five endpoints' RAW per-request latencies.
Nothing in the binding computes a quartile; `Distribution::from_samples` does,
under a definition the reader picks at runtime.

What this script checks, and why each check discriminates:

* **One datum, the whole schema.** The mark cardinalities are counted off the
  paint tree (5 boxes, 5 medians, 10 whiskers, 10 caps, 6 outliers), and the
  parts are checked against EACH OTHER — the median inside its own box, the
  whiskers hanging off the box edges, the caps centred on the box, the
  outliers on the box's own centre line. Six copies of the slot arithmetic
  that happened to agree would pass a per-mark check and fail this one.
* **PAST THE 6.11 FLOOR (1): the quantile definition is a runtime choice, and it
  DECIDES an outlier.** `/search` was hit six times; at `n = 6` Tukey's
  hinges, Hyndman & Fan type 7 and type 6 give three different upper
  quartiles, so the `41 ms` sample is inside one fence and outside another.
  Switching the chip makes an outlier mark appear and disappear. The toolkit's charting module
  computes none of the three — its own box-plot example ships a
  `findMedian()` helper IN THE EXAMPLE — so a box set cannot record which
  definition built it. The counterfactual is in the same section: the OTHER
  four endpoints' outlier counts do not move, so this is not "the method
  reshuffles everything".
* **PAST THE 6.11 FLOOR (2): an outlier is drawn at all.** box set has exactly five
  slots (`LowerExtreme`..`UpperExtreme`) and no per-outlier geometry, so a toolkit
  box plot cannot show one. Here each is its own addressable node, and the
  whisker is required to STOP short of it — which is what makes it Tukey's
  fence rather than the plain extremes the toolkit draws.
* **PAST THE 6.11 FLOOR (3): the notch, because `n` survived the summary.** The waist
  is `median +- 1.58 * IQR / sqrt(n)`; box set carries no sample count, so
  the toolkit could not offer it even as a paint option. Read two ways over the wire:
  the box's own command list grows from 5 to 11, and the median line narrows
  to the waist.
* **PAST THE 6.11 FLOOR (4): a landmark the axis cannot place is REPORTED.**
  `/health` answered two requests from cache and a millisecond-resolution
  timer recorded them as `0.0`. On a log axis they have no pixel: their marks
  vanish, the box still draws, and the caption is the report. On a linear axis
  they are ordinary — so being off-scale is a property of the AXIS.
* **Keyboard and RPC move ONE selection.** The method group is driven both
  ways and the resulting scene compared, so a binding that grew a second copy
  of the choice would be caught.
* **The summary and its provenance reach assistive technology.** The methods
  are a `radiogroup`, the options `button[aria-pressed]`, and the caption a
  live region carrying the derived quartile and the off-scale report. The toolkit's
  charts implement no accessibility interface at all.

Run from the workspace root:
    cargo build -p hello-boxplot --release
    python3 tools/demos/r1553_distribution_datum.py
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
    walk_nodes,
)

VIEWPORT = (820, 500)

METHOD_TAG = "method"
NOTCH_TAG = "notch"
LOG_TAG = "logscale"
CAPTION_TAG = "caption"

#: `hello-boxplot`'s five endpoints, mirrored rather than imported — a demo
#: that read the list out of the code under test could not catch it changing.
ENDPOINTS = ["/health", "/login", "/search", "/report", "/export"]

#: The endpoint whose six samples make the three quantile definitions
#: disagree, and the one whose cached responses were timed as `0.0`.
SMALL_N = 2
ZEROED = 0

#: Method chip order, and the upper quartile each definition gives `/search`.
#: Predicted from the published definitions, not read back from the binding.
METHOD_NAMES = ["tukey", "linear", "exclusive"]
SEARCH_Q3 = ["26.00", "24.25", "29.75"]
#: Whether the 41 ms sample falls outside that method's 1.5*IQR fence.
SEARCH_OUTLIERS = [0, 1, 0]


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


def count_prefix(snap: dict, prefix: str) -> int:
    return sum(
        1 for _, n in walk_nodes(snap) if str(n.get("tag") or "").startswith(prefix)
    )


def centre_x(r: dict) -> float:
    return float(r["x"]) + float(r["w"]) / 2.0


def centre_y(r: dict) -> float:
    return float(r["y"]) + float(r["h"]) / 2.0


def caption(snap: dict) -> str:
    return node(snap, CAPTION_TAG)["content"]


def pick_method(tf: RpcSubprocess, i: int) -> None:
    tf.click(path=f"{METHOD_TAG}#{i}")
    tf.tick(0.016)


def toggle(tf: RpcSubprocess, tag: str) -> None:
    tf.click(path=tag)
    tf.tick(0.016)


def body() -> None:
    with RpcSubprocess("hello-boxplot", boot_grace=1.5) as tf:
        for _ in range(3):
            tf.tick(0.016)

        # ── (A) one datum, the whole schema ──────────────────────────
        snap = snapshot(tf)
        n = len(ENDPOINTS)
        assert_eq(count_prefix(snap, "chart.box."), n, "one box per distribution")
        assert_eq(count_prefix(snap, "chart.median."), n, "one median per box")
        assert_eq(
            count_prefix(snap, "chart.whisker."),
            n * 2,
            "two whiskers per box — a datum with EXTENT, which every other "
            "chart in this crate cannot express",
        )
        assert_eq(count_prefix(snap, "chart.cap."), n * 2, "and a cap on each")
        assert_eq(
            count_prefix(snap, "chart.outlier."),
            6,
            "PAST THE FLOOR: six marks beyond the fence — /health's two cache hits, "
            "/login's 48 ms tail, /report's three far samples. box set has "
            "five slots and no per-outlier geometry at all",
        )
        assert_eq(
            [node(snap, f"chart.xlabel.{i}")["content"] for i in range(n)],
            ENDPOINTS,
            "each slot is labelled by the name the category axis carries",
        )

        # ── (B) the parts derive from ONE geometry ───────────────────
        for i in range(n):
            box = rect(snap, f"chart.box.{i}")
            med = rect(snap, f"chart.median.{i}")
            assert abs(centre_x(box) - centre_x(med)) <= 1.5, (
                f"median {i} is centred on its own box"
            )
            assert float(box["y"]) <= centre_y(med) <= float(box["y"]) + float(
                box["h"]
            ), f"median {i} lies INSIDE the box, not beside it"
            # The whiskers hang off the box edges and reach beyond them.
            hi = rect(snap, f"chart.whisker.{i}.hi")
            lo = rect(snap, f"chart.whisker.{i}.lo")
            assert abs(centre_x(box) - centre_x(hi)) <= 1.5, f"whisker {i}.hi centred"
            assert abs(centre_x(box) - centre_x(lo)) <= 1.5, f"whisker {i}.lo centred"
            assert float(hi["y"]) < float(box["y"]) + 2, (
                f"whisker {i}.hi rises above the box top"
            )
            assert float(lo["y"]) + float(lo["h"]) > float(box["y"]) + float(
                box["h"]
            ) - 2, f"whisker {i}.lo drops below the box bottom"
            # The caps terminate the whiskers and are narrower than the box.
            for end in ("hi", "lo"):
                cap = rect(snap, f"chart.cap.{i}.{end}")
                whisk = rect(snap, f"chart.whisker.{i}.{end}")
                assert abs(centre_x(box) - centre_x(cap)) <= 1.5, f"cap {i}.{end} centred"
                assert float(cap["w"]) < float(box["w"]), (
                    f"cap {i}.{end} is subordinate to its box"
                )
                far = float(whisk["y"]) if end == "hi" else float(whisk["y"]) + float(
                    whisk["h"]
                )
                assert abs(centre_y(cap) - far) <= 2.5, (
                    f"cap {i}.{end} sits at the whisker's far end"
                )
        # The boxes tile the category axis in order, left to right.
        centres = [centre_x(rect(snap, f"chart.box.{i}")) for i in range(n)]
        assert centres == sorted(centres), f"boxes ascend with their slots: {centres}"

        # ── (A2) PAST THE FLOOR: the whisker STOPS at the fence
        # ───────────── Read on /report, whose three far samples clear its
        # whisker by hundreds of milliseconds, so the separation is pixels
        # rather than rounding. The toolkit draws its whiskers to whatever `LowerExtreme`
        # / `UpperExtreme` were set to, with no fence rule and nowhere to put what falls
        # outside one.
        report = 3
        whisk_top = float(rect(snap, f"chart.whisker.{report}.hi")["y"])
        marks = sorted(
            centre_y(rect(snap, f"chart.outlier.{report}.{j}")) for j in range(3)
        )
        assert_eq(
            count_prefix(snap, f"chart.outlier.{report}."),
            3,
            "/report's three far samples are three marks",
        )
        assert marks[-1] < whisk_top - 20, (
            "every one of them sits clear ABOVE the whisker's end "
            f"(highest mark {marks[-1]}, whisker top {whisk_top})"
        )
        assert all(m < whisk_top for m in marks), f"none is inside the whisker: {marks}"
        box_bottom = float(rect(snap, f"chart.box.{report}")["y"]) + float(
            rect(snap, f"chart.box.{report}")["h"]
        )
        assert whisk_top < box_bottom, (
            "and the whisker itself still reaches past the box it hangs from"
        )

        # ── (C) PAST THE FLOOR: the definition decides an outlier
        # ───────────
        far_tag = f"chart.outlier.{SMALL_N}.0"
        assert_eq(
            find_by_tag(snap, far_tag),
            None,
            "under Tukey's hinges /search's 41 ms sample is a whisker end",
        )
        assert SEARCH_Q3[0] in caption(snap), (
            f"the caption states the derived quartile: {caption(snap)}"
        )

        others_before = {
            i: count_prefix(snap, f"chart.outlier.{i}.") for i in range(n) if i != SMALL_N
        }

        pick_method(tf, 1)
        snap = snapshot(tf)
        assert find_by_tag(snap, far_tag) is not None, (
            "PAST THE FLOOR: under Hyndman & Fan type 7 the SAME sample is outside "
            "the fence and becomes its own mark. Same data, same fence rule, "
            "different definition"
        )
        assert SEARCH_Q3[1] in caption(snap), (
            f"and the caption states type 7's quartile: {caption(snap)}"
        )
        assert METHOD_NAMES[1] in caption(snap), "named, not implied"
        # Counterfactual: nothing else moved.
        others_after = {
            i: count_prefix(snap, f"chart.outlier.{i}.") for i in range(n) if i != SMALL_N
        }
        assert_eq(
            others_after,
            others_before,
            "the other four endpoints' outliers are unchanged — the method "
            "did not reshuffle everything, it decided ONE borderline sample",
        )

        pick_method(tf, 2)
        snap = snapshot(tf)
        assert_eq(
            find_by_tag(snap, far_tag),
            None,
            "and type 6's wider quartiles swallow it again",
        )
        assert SEARCH_Q3[2] in caption(snap), f"third quartile: {caption(snap)}"
        assert_eq(
            [count_prefix(snap, f"chart.outlier.{SMALL_N}.") for _ in (0,)],
            [SEARCH_OUTLIERS[2]],
            "so /search's outlier count follows the definition",
        )

        # ── (D) keyboard and RPC drive ONE selection ─────────────────
        tf.key(path=METHOD_TAG, name="Home")
        tf.tick(0.016)
        snap = snapshot(tf)
        assert SEARCH_Q3[0] in caption(snap), (
            "Home selects the first definition — the same state a click on "
            f"chip 0 produces: {caption(snap)}"
        )
        tf.key(path=METHOD_TAG, name="ArrowRight")
        tf.tick(0.016)
        by_key = caption(snapshot(tf))
        pick_method(tf, 1)
        by_click = caption(snapshot(tf))
        assert_eq(
            by_key,
            by_click,
            "keyboard and pointer reach ONE selection, not two copies of it",
        )

        # ── (E) PAST THE FLOOR: the notch, because n survived
        # ───────────────
        pick_method(tf, 0)
        snap = snapshot(tf)
        plain_box = node(snap, "chart.box.0")
        plain_med_w = float(rect(snap, "chart.median.0")["w"])
        assert_eq(
            len(plain_box["commands"]),
            5,
            "an un-notched box is four corners and a close",
        )

        toggle(tf, NOTCH_TAG)
        snap = snapshot(tf)
        notched_box = node(snap, "chart.box.0")
        assert_eq(
            len(notched_box["commands"]),
            11,
            "PAST THE FLOOR: the waist makes it ten outline points and a close — "
            "median +- 1.58*IQR/sqrt(n), which box set cannot express because "
            "it does not carry n",
        )
        notched_med_w = float(rect(snap, "chart.median.0")["w"])
        assert notched_med_w < plain_med_w * 0.7, (
            "and the median narrows to the waist "
            f"({notched_med_w} vs {plain_med_w})"
        )
        assert notched_med_w > plain_med_w * 0.35, (
            f"but not to nothing ({notched_med_w} vs {plain_med_w})"
        )
        # The box's own rect is unchanged: a waist is drawn INSIDE the
        # extent, so this is not "the notch resized the box".
        assert_eq(
            (notched_box["rect"]["y"], notched_box["rect"]["h"]),
            (plain_box["rect"]["y"], plain_box["rect"]["h"]),
            "the box still spans exactly q1..q3",
        )
        toggle(tf, NOTCH_TAG)

        # ── (F) PAST THE FLOOR: what the axis cannot place is reported
        # ──────
        snap = snapshot(tf)
        assert find_by_tag(snap, f"chart.outlier.{ZEROED}.0") is not None, (
            "on a linear axis the two zero-timed cache hits are ordinary marks"
        )
        assert "Linear axis" in caption(snap), caption(snap)
        assert "not plotted" not in caption(snap), (
            f"nothing is off-scale on a linear axis: {caption(snap)}"
        )

        toggle(tf, LOG_TAG)
        snap = snapshot(tf)
        for j in (0, 1):
            assert_eq(
                find_by_tag(snap, f"chart.outlier.{ZEROED}.{j}"),
                None,
                f"a zero has no pixel on a log axis, so mark {j} draws nothing",
            )
        assert find_by_tag(snap, f"chart.box.{ZEROED}") is not None, (
            "and the box itself still draws — one landmark dropping does not "
            "drop the datum"
        )
        text = caption(snap)
        assert "2 landmark(s) not plotted" in text, text
        assert "2 of them outliers" in text, text
        assert ENDPOINTS[ZEROED] in text, f"the report names the endpoint: {text}"
        # The log axis separates what the linear one flattened.
        gap = abs(
            centre_y(rect(snap, "chart.median.0")) - centre_y(rect(snap, "chart.median.1"))
        )
        assert gap > 60, (
            f"/health (~1 ms) and /login (~15 ms) are {gap}px apart on a log axis"
        )
        assert count_prefix(snap, "chart.grid.minor.y.") > 0, (
            "and the log axis carries its per-decade subdivisions, without "
            "which evenly-spaced decade lines read as a linear axis"
        )

        toggle(tf, LOG_TAG)
        snap = snapshot(tf)
        linear_gap = abs(
            centre_y(rect(snap, "chart.median.0")) - centre_y(rect(snap, "chart.median.1"))
        )
        assert linear_gap < 10, (
            "back on linear they collapse into each other again "
            f"({linear_gap}px) — off-scale and flattening are both properties "
            "of the AXIS"
        )
        assert_eq(
            count_prefix(snap, "chart.grid.minor."),
            0,
            "and the minor gridlines go with it",
        )

        # ── (G) the derivation reaches assistive technology ──────────
        acc = tf.request("scene/access", {}).result or {}
        radios = [
            access_node_by_tag(acc, f"{METHOD_TAG}#{i}") for i in range(len(METHOD_NAMES))
        ]
        assert all(r is not None for r in radios), (
            f"one AT node per quantile definition: {radios}"
        )
        assert_eq(
            [r["role"] for r in radios],
            ["radio"] * len(METHOD_NAMES),
            "the definitions are a 1-of-N choice, not three switches",
        )
        group = access_node_by_tag(acc, METHOD_TAG)
        assert group is not None and group["role"] == "radiogroup", (
            f"and they sit under one radiogroup: {group}"
        )
        for tag in (NOTCH_TAG, LOG_TAG):
            n_ = access_node_by_tag(acc, tag)
            assert n_ is not None and n_["role"] == "button", f"{tag}: {n_}"
        status = access_node_by_tag(acc, CAPTION_TAG)
        assert status is not None, "the caption is an AT node"
        assert_eq(status["role"], "status", "and it is a live region")
        assert_eq(
            status.get("name"),
            caption(snap),
            "PAST THE FLOOR: a screen reader is told the SAME derived quartile and "
            "off-scale report a sighted reader sees. The toolkit's charting module draws into a "
            "canvas scene and implements no accessibility interface at all",
        )


if __name__ == "__main__":
    run_demo("r1553_distribution_datum", body)
