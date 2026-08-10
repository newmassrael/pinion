#!/usr/bin/env python3
"""R1629 §5.12 §5.28 §2 #7 — a chart says what it derived, on the wire.

A picture is made from two sources — the data it was given and the request
that said how to draw it — and neither survives into the pixels. Every chart
in this tree already knew what it had done to them: `LineChart::overshoot`
names where a spline left the data, `Density` knows the kernel and bandwidth
that decided a violin's whole outline, `CandlestickChart` knew that
`with_caps(true)` means nothing under a bar mark. All of it was **in-process
Rust**, so the §2 #2 primary client — which holds a `Scene` and no chart — was
the one reader who could not ask.

`scene/derivations` is that channel. Every entry says which of four
disagreements it is, and the four are a closed 2x2 over
{data, request} x {the picture has more, the source has more}:

    invented   the picture shows a value the data does not contain
    omitted    the data contains a value the picture does not show
    chosen     the picture rests on a decision the request left open
    discarded  the picture ignores a decision the request made

The reference toolkit's chart module will read `capsVisible` back to you
through its meta-object protocol, so "what did I ask for" is answerable there.
What is not — not by the meta-object, not by any signal — is what the drawing
DID with it. Its spline has one algorithm and no report; its candlestick
accepts caps on a series drawn without them and says nothing.

What each check discriminates:

* **The report tracks the choice, both ways.** A channel that always said
  "nothing invented" would pass the straight-line half alone; one that always
  said "invented" would pass the spline half alone. Both are asserted, on the
  same data, one click apart.
* **A choice is published even when nothing went wrong.** `interpolation` and
  `kernel` are on the wire for a chart with no excursion at all — a control
  that only appears once it has misbehaved is not a control.
* **An `invented` entry exists exactly when something was invented.** The
  bounded/unbounded pair is the counterfactual: same samples, same violin, and
  `spill` present in one and absent in the other.
* **A discarded setting is named rather than dropped.** Caps under a bar mark
  used to add no node and say nothing.
* **The composition answers, and its own children do not.** A client that
  walked into the tree is told it is at the wrong KIND of node
  (`channel: "painted"`), not that the chart forgot.
* **A typo is refused.** `kind: "inventing"` must not read as "this picture
  invented nothing".

Run from the workspace root:
    cargo build -p hello-series-toggle -p hello-boxplot -p hello-candlestick --release
    python3 tools/demos/r1629_a_chart_says_what_it_derived.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    assert_rpc_error,
    call,
    run_demo,
)

WIN = (520, 420)
BOX_WIN = (820, 500)
KINDS = ("invented", "omitted", "chosen", "discarded")


def derivations(tf: RpcSubprocess, tag: str, win: tuple, kind: str | None = None) -> dict:
    """`scene/derivations` for `tag`, read off the painted frame."""
    params: dict = {"tag": tag, "viewport": list(win)}
    if kind is not None:
        params["kind"] = kind
    return call(tf, "scene/derivations", params)


def entries(answer: dict, kind: str, name: str) -> list[dict]:
    return [
        d
        for d in answer.get("derivations", [])
        if d.get("kind") == kind and d.get("name") == name
    ]


def only(answer: dict, kind: str, name: str) -> dict:
    found = entries(answer, kind, name)
    assert len(found) == 1, f"exactly one {kind}/{name}: {found}"
    return found[0]


def assert_shape(answer: dict, who: str) -> None:
    """Every answer is self-describing: the channel, the domain, and both axes
    of the 2x2 on every entry. A client must never have to consult our source
    to read one."""
    assert_eq(answer.get("published"), True, f"{who} publishes")
    assert_eq(answer.get("channel"), "composes", f"{who} is a composition")
    assert answer.get("domain"), f"{who} says what a span would index"
    for d in answer.get("derivations", []):
        assert d.get("kind") in KINDS, f"{who}: unknown kind {d}"
        assert d.get("source") in ("data", "request"), f"{who}: no source axis {d}"
        assert isinstance(d.get("picture_has_more"), bool), f"{who}: no direction {d}"
        # `source` + `picture_has_more` reconstruct `kind`: two spellings of
        # one fact, so a client keying off either reads the same thing.
        table = {
            ("data", True): "invented",
            ("data", False): "omitted",
            ("request", True): "chosen",
            ("request", False): "discarded",
        }
        assert_eq(
            table[(d["source"], d["picture_has_more"])],
            d["kind"],
            f"{who}: the axes disagree with the kind: {d}",
        )
        evidence = d.get("evidence", {})
        assert evidence.get("type") in ("name", "real", "count", "flag"), (
            f"{who}: untyped evidence {d}"
        )


def a_spline_says_what_it_invented(tf: RpcSubprocess) -> None:
    # ── 1. straight lines: the CHOICE is published, and nothing is invented ──
    straight = derivations(tf, "chart", WIN)
    assert_shape(straight, "the straight chart")
    assert_eq(
        only(straight, "chosen", "interpolation")["evidence"]["name"],
        "linear",
        "the join is on the wire before anything goes wrong",
    )
    assert_eq(
        entries(straight, "invented", "overshoot"),
        [],
        "a straight line draws no value the data lacks",
    )
    assert_eq(
        derivations(tf, "chart", WIN, kind="invented")["derivations"],
        [],
        "and the whole picture-vs-data row is empty",
    )
    print("[demo] linear: the choice is published, nothing is invented")

    # ── 2. the smooth chip: the same data now reports an excursion ───────────
    tf.click(path="smooth")
    tf.tick(0.016)
    smooth = derivations(tf, "chart", WIN)
    assert_shape(smooth, "the smooth chart")
    assert_eq(
        only(smooth, "chosen", "interpolation")["evidence"]["name"],
        "catmull-rom",
        "the choice tracked the chip",
    )
    shoots = entries(smooth, "invented", "overshoot")
    assert shoots, "the spline left the data somewhere"
    for shoot in shoots:
        assert shoot.get("subject", "").startswith("series."), (
            f"an excursion belongs to a series: {shoot}"
        )
        assert_eq(shoot["evidence"]["type"], "real", "an excursion has a size")
        assert shoot["evidence"]["value"] > 0.0, f"and the size is positive: {shoot}"
        assert_eq(shoot.get("unit"), "value", "measured in the value axis' units")
        span = shoot.get("span")
        assert span is not None, f"and localized to the gap that made it: {shoot}"
        assert_eq(span["end"] - span["start"], 2, "a gap covers both its samples")
    counted = entries(smooth, "invented", "overshoot_segments")
    assert counted, "and how many gaps did it, which the localized entry cannot say"
    for entry in counted:
        assert_eq(entry["evidence"]["type"], "count", f"a tally is a count: {entry}")
    print(f"[demo] catmull-rom: {len(shoots)} series invented a value")

    # ── 3. the safe chip: smooth AND inventing nothing ──────────────────────
    #      The discriminating case. A report keyed off "is the line curved"
    #      would fire here; a monotone cubic is curved and provably inside its
    #      own endpoints.
    tf.click(path="safe")
    tf.tick(0.016)
    safe = derivations(tf, "chart", WIN)
    assert_eq(
        only(safe, "chosen", "interpolation")["evidence"]["name"],
        "monotone",
        "still a curve",
    )
    assert_eq(
        entries(safe, "invented", "overshoot"),
        [],
        "and it invents nothing — curvature is not the question",
    )
    print("[demo] monotone: curved, and nothing invented")

    # ── 4. the composition answers; its children say they are the wrong node ─
    ink = derivations(tf, "chart.series.0", WIN)
    assert_eq(ink.get("published"), False, "a painted node states nothing")
    assert_eq(
        ink.get("channel"),
        "painted",
        "and the wire says WHY, so a client need not read our source",
    )
    assert_rpc_error(
        lambda: derivations(tf, "chart.nobody", WIN),
        data="UnknownTag: chart.nobody",
    )
    print("[demo] the composition answers and its ink does not")

    # ── 5. a typo is refused, and the refusal names the accepted set ────────
    # The refusal names the whole accepted set, so a client learns the filter
    # vocabulary from the refusal instead of from our source.
    assert_rpc_error(
        lambda: derivations(tf, "chart", WIN, kind="inventing"),
        data=(
            'UnknownDerivationKind: "inventing" is not one of '
            "invented, omitted, chosen, discarded"
        ),
    )
    print("[demo] an unknown kind is refused rather than silently widened")


def an_outline_says_what_chose_it(tf: RpcSubprocess) -> None:
    # ── 6. boxes only: no estimate, so no estimate parameters ───────────────
    boxes = derivations(tf, "chart", BOX_WIN)
    assert_shape(boxes, "the box chart")
    assert_eq(
        only(boxes, "chosen", "mark")["evidence"]["name"], "box", "drawn as boxes"
    )
    assert_eq(entries(boxes, "chosen", "kernel"), [], "nothing was estimated")
    assert_eq(
        entries(boxes, "discarded", "density"),
        [],
        "and nothing was asked for, so nothing was discarded",
    )
    print("[demo] boxes: no estimate, and nothing discarded")

    # ── 7. the violin chip: four choices decide the outline ─────────────────
    tf.click(path="violin")
    tf.tick(0.016)
    violin = derivations(tf, "chart", BOX_WIN)
    assert_shape(violin, "the violin chart")
    # `violin+box` — this example draws the outline WITH the box inside it
    # (R1626), and the published choice says which of the three marks it is
    # rather than a boolean "is there a violin".
    assert_eq(
        only(violin, "chosen", "mark")["evidence"]["name"],
        "violin+box",
        "drawn as violins over their boxes",
    )
    kernels = entries(violin, "chosen", "kernel")
    assert kernels, "the kernel is on the wire"
    for kernel in kernels:
        assert_eq(kernel["evidence"]["type"], "name", f"a kernel is a name: {kernel}")
        assert kernel.get("subject", "").startswith("slot."), (
            f"a kernel belongs to one distribution: {kernel}"
        )
    bandwidths = entries(violin, "chosen", "bandwidth")
    assert len(bandwidths) == len(kernels), "one bandwidth per estimated outline"
    for bw in bandwidths:
        assert bw["evidence"]["value"] > 0.0, f"a bandwidth is positive: {bw}"
        assert_eq(bw.get("unit"), "value", "a bandwidth without units is unreadable")
    assert entries(violin, "chosen", "bandwidth_rule"), "and the rule that resolved it"
    assert entries(violin, "chosen", "violin_scale"), "and how the widths compare"
    for bounded in entries(violin, "chosen", "bounded"):
        assert_eq(bounded["evidence"]["type"], "flag", f"bounded is yes/no: {bounded}")
    print(f"[demo] {len(kernels)} outlines, each naming its kernel and bandwidth")

    # ── 8. what the outline replaced, and what it invented ──────────────────
    samples = entries(violin, "omitted", "samples")
    assert samples, "a density curve shows the shape of its measurements, not one of them"
    for entry in samples:
        assert_eq(entry["evidence"]["type"], "count", f"a sample tally is a count: {entry}")
        assert entry["evidence"]["count"] > 0, f"and non-empty: {entry}"
    spills = entries(violin, "invented", "spill")
    assert spills, "an unbounded Gaussian always reaches past the data"
    for spill in spills:
        assert 0.0 < spill["evidence"]["value"] < 1.0, f"a share of the mass: {spill}"
        assert_eq(spill.get("unit"), "fraction", "and it says so")
    print(f"[demo] {len(spills)} outlines report mass outside the observed range")

    # ── 9. the filter is the query a client actually has ────────────────────
    invented = derivations(tf, "chart", BOX_WIN, kind="invented")
    assert_eq(invented.get("filter"), "invented", "the answer echoes the narrowing")
    assert_eq(
        len(invented["derivations"]),
        len(spills),
        "'did this picture invent anything' is one call",
    )
    for entry in invented["derivations"]:
        assert_eq(entry["kind"], "invented", f"and only that kind: {entry}")
    assert_eq(
        invented.get("published"),
        True,
        "a narrowed answer is still an answer",
    )
    print("[demo] one call answers 'did this picture invent anything'")

    # ── 10. NEGATIVE CONTROL: a log axis reports what it could not place ────
    tf.click(path="logscale")
    tf.tick(0.016)
    logged = derivations(tf, "chart", BOX_WIN)
    assert_shape(logged, "the log chart")
    assert entries(logged, "chosen", "kernel"), "a log axis still estimates"
    print("[demo] the log axis publishes too")


def caps_under_a_bar_are_discarded(tf: RpcSubprocess) -> None:
    # ── 11. candles with caps: a real setting, nothing discarded ────────────
    tf.click(path="caps")
    tf.tick(0.016)
    candles = derivations(tf, "chart", WIN)
    assert_shape(candles, "the candle chart")
    assert_eq(
        only(candles, "chosen", "mark")["evidence"]["name"], "candle", "drawn as candles"
    )
    assert_eq(
        entries(candles, "discarded", "caps"),
        [],
        "a candle HAS caps, so asking for them discards nothing",
    )
    print("[demo] caps under a candle: honoured, nothing discarded")

    # ── 12. the same setting under a bar mark: reported, not dropped ────────
    #       This is the fact the round exists for. Before it, the builder
    #       accepted `with_caps(true)`, drew nothing extra, and said nothing —
    #       so the option reproduced as "it does not work".
    tf.click(path="bar")
    tf.tick(0.016)
    bars = derivations(tf, "chart", WIN)
    assert_shape(bars, "the bar chart")
    assert_eq(only(bars, "chosen", "mark")["evidence"]["name"], "ohlc", "drawn as bars")
    discarded = only(bars, "discarded", "caps")
    assert_eq(
        discarded["evidence"]["name"],
        "ohlc",
        "and the evidence names the mark that made the setting meaningless",
    )
    assert_eq(discarded.get("subject"), "mark", "the setting's subject is the mark")
    assert_eq(discarded["source"], "request", "the picture ignored what was asked")
    print("[demo] caps under a bar: named as discarded")

    # ── 13. and turning the setting off retracts the report ─────────────────
    tf.click(path="caps")
    tf.tick(0.016)
    quiet = derivations(tf, "chart", WIN)
    assert_eq(
        entries(quiet, "discarded", "caps"),
        [],
        "nothing was asked for, so nothing is discarded",
    )
    assert_eq(
        only(quiet, "chosen", "mark")["evidence"]["name"],
        "ohlc",
        "while the mark is still a published choice",
    )
    print("[demo] the report retracts with the setting")


def body() -> None:
    with RpcSubprocess("hello-series-toggle", boot_grace=1.5) as tf:
        for _ in range(3):
            tf.tick(0.016)
        a_spline_says_what_it_invented(tf)
    with RpcSubprocess("hello-boxplot", boot_grace=1.5) as tf:
        for _ in range(3):
            tf.tick(0.016)
        an_outline_says_what_chose_it(tf)
    with RpcSubprocess("hello-candlestick", boot_grace=1.5) as tf:
        for _ in range(3):
            tf.tick(0.016)
        caps_under_a_bar_are_discarded(tf)


if __name__ == "__main__":
    run_demo("R1629 §5.12 — a chart says what it derived", body)
