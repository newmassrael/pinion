#!/usr/bin/env python3
"""R1567 §5.38 §2 #7 — the datum whose two middle landmarks are UNORDERED.

R1553 gave `pinion-chart` a `Distribution` — five landmarks, totally ordered
by construction — and recorded that a candlestick would be its second
consumer, "the same interval geometry over open / high / low / close".
Building one showed that wrong, and the correction is this round.

A candle has four landmarks and only three order relations among them: the
low is below both middle values and the high above both, while `open` and
`close` are **not ordered against each other at all**. That absence is not a
weaker invariant, it IS the datum — two sessions with the same four numbers,
the same extent and the same body mean opposite things, and a type whose
invariant is "non-decreasing" collapses them into one value.

`hello-candlestick` hands the crate six daily sessions: Monday to Friday, and
then the NEXT Monday.

What this script checks, and why each check discriminates:

* **The schema, and its parts against EACH OTHER.** Six bodies, twelve wicks,
  and the wicks are required to hang off the body edges on the body's own
  centre line — six copies of the slot arithmetic that happened to agree would
  pass a per-mark check and fail this one.
* **PAST the toolkit 6.11 (1): a doji has a NAME.** Wednesday closed exactly where it
  opened. The toolkit's documented rule paints `increasingColor` only when the close is
  *higher* than the open, so a toolkit doji silently takes the losing colour and
  candlestick set has no accessor to say otherwise. Here it is its own
  direction, its body has zero height on the wire, and the caption says so.
* **PAST the toolkit 6.11 (2): the direction is encoded TWICE.** A rising body is
  hollow (`fill.a == 0`) and a falling one solid (`fill.a == 255`) — the
  traditional Japanese form, which predates colour. The "no hue" chip collapses
  all three strokes to one ink and the fill alphas do not move, which is the
  claim: a colour-blind reader keeps the direction. The toolkit encodes it in hue alone,
  and green-and-red is the worst pair for the commonest deficiency.
* **PAST the toolkit 6.11 (3): one datum, two readings of the x-axis.** On the session
  axis the six bodies abut and the weekend takes no width; on the elapsed axis
  the same six sit over real UTC time and the weekend is three days wide. The toolkit
  reaches those two pictures by attaching two axis objects and handing the
  category one a string list unrelated to the sets' timestamps — two
  descriptions of when the sessions were, with nothing checking they agree.
  Here the slot names are DERIVED from the instants, and the script reads them
  back off the axis to prove it.
* **PAST the toolkit 6.11 (4): the label CARDINALITY follows the reading.** One node
  per session on the ordinal axis, one per time tick on the elapsed one, under
  two different tags — two questions, not one tag that lies about what it
  counts.
* **Keyboard and RPC move ONE selection.** The reading group is driven both
  ways and the resulting scene compared, so a binding that grew a second copy
  of the choice would be caught.
* **The direction reaches assistive technology.** The readings are a
  `radiogroup`, the options `button[aria-pressed]`, and the caption a live
  region naming the doji and the measured contrast. The toolkit's charts implement no
  accessibility interface at all.

Run from the workspace root:
    cargo build -p hello-candlestick --release
    python3 tools/demos/r1567_candle_direction_is_the_datum.py
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

VIEWPORT = (860, 520)

READING_TAG = "reading"
CAPS_TAG = "caps"
MONO_TAG = "mono"
CAPTION_TAG = "caption"

#: `hello-candlestick`'s six sessions, mirrored rather than imported — a demo
#: that read the fixture out of the code under test could not catch it
#: changing. `(day offset, open, high, low, close)`.
SESSIONS = [
    (0, 100.0, 104.0, 99.0, 103.0),
    (1, 103.0, 105.0, 101.0, 102.0),
    (2, 102.0, 106.0, 102.0, 106.0),
    (3, 106.0, 107.0, 103.0, 106.0),
    (4, 106.0, 108.0, 104.0, 105.0),
    (7, 105.0, 111.0, 105.0, 110.0),
]

#: The direction each session went, predicted from the numbers above rather
#: than read back: `close > open` rises, `close < open` falls, and `close ==
#: open` is a DOJI — the arm the toolkit has no name for.
DIRECTIONS = ["rising", "falling", "rising", "doji", "falling", "rising"]

#: The fill alpha each direction paints its body's interior at. Hollow is
#: exactly zero: a "hollow" body at a low but non-zero alpha would be a shade,
#: and a shade is a hue distinction wearing a different name.
FILL_ALPHA = {"rising": 0, "falling": 255, "doji": 255}

#: The session that closed where it opened, and the one after the weekend.
DOJI = 3
AFTER_GAP = 5

#: The slot names the ordinal axis DERIVES from the instants. Predicted from
#: the day offsets against 2026-03-02 (a Monday), not read out of the binding.
SLOT_NAMES = ["Mar 02", "Mar 03", "Mar 04", "Mar 05", "Mar 06", "Mar 09"]

ORDINAL, ELAPSED = 0, 1


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


def body_span(snap: dict, i: int) -> float:
    """Session `i`'s body height read off its PATH, not its bounding box.

    A node's rect is padded by the stroke width, so a zero-height body still
    reports `h = 5` there — the bbox cannot tell a doji from a one-pixel
    session. The path commands are the geometry the chart derived, in the
    node's own frame (R1358), so this reads the derivation itself.
    """
    ys = [
        float(c["point"]["y"])
        for c in node(snap, f"chart.candle.{i}")["commands"]
        if "point" in c
    ]
    assert len(ys) == 4, f"a body is four corners and a close: {ys}"
    return max(ys) - min(ys)


def body_ink(snap: dict, i: int) -> tuple[int, tuple[int, int, int]]:
    """`(fill alpha, stroke rgb)` of session `i` — the two encoding channels."""
    n = node(snap, f"chart.candle.{i}")
    fill = n["style"]["fill"]
    stroke = n["style"]["stroke"]["color"]
    return int(fill["a"]), (int(stroke["r"]), int(stroke["g"]), int(stroke["b"]))


def pick_reading(tf: RpcSubprocess, i: int) -> None:
    tf.click(path=f"{READING_TAG}#{i}")
    tf.tick(0.016)


def toggle(tf: RpcSubprocess, tag: str) -> None:
    tf.click(path=tag)
    tf.tick(0.016)


def body() -> None:
    with RpcSubprocess("hello-candlestick", boot_grace=1.5) as tf:
        for _ in range(3):
            tf.tick(0.016)

        # ── (A) the schema, and the parts against each other ─────────
        snap = snapshot(tf)
        n = len(SESSIONS)
        assert_eq(count_prefix(snap, "chart.candle."), n, "one body per session")
        assert_eq(
            count_prefix(snap, "chart.wick."),
            n * 2,
            "two wicks per session — a datum with EXTENT whose interior is "
            "split by two landmarks that are NOT ordered against each other",
        )
        assert_eq(
            count_prefix(snap, "chart.cap."),
            0,
            "the caps are opt-in, as Qt's capsVisible is",
        )
        for i in range(n):
            b = rect(snap, f"chart.candle.{i}")
            hi = rect(snap, f"chart.wick.{i}.hi")
            lo = rect(snap, f"chart.wick.{i}.lo")
            assert abs(centre_x(b) - centre_x(hi)) <= 1.5, f"wick {i}.hi is centred"
            assert abs(centre_x(b) - centre_x(lo)) <= 1.5, f"wick {i}.lo is centred"
            assert float(hi["y"]) <= float(b["y"]) + 2, (
                f"wick {i}.hi rises to the session high, above the body top"
            )
            assert float(lo["y"]) + float(lo["h"]) >= float(b["y"]) + float(b["h"]) - 2, (
                f"wick {i}.lo drops to the session low, below the body bottom"
            )
            assert float(hi["w"]) < float(b["w"]), f"wick {i}.hi is thinner than its body"
        centres = [centre_x(rect(snap, f"chart.candle.{i}")) for i in range(n)]
        assert centres == sorted(centres), f"sessions ascend left to right: {centres}"

        # ── (B) PAST the toolkit: the doji has a name
        # ─────────────────────────
        assert_eq(
            body_span(snap, DOJI),
            0.0,
            "PAST QT: Wednesday opened and closed at 106, so its body has NO "
            "height — that is the glyph, not a rounding error. Qt paints it "
            "with decreasingColor and no accessor can say otherwise",
        )
        for i in (0, 1, 2, 4, 5):
            assert body_span(snap, i) > 1.0, (
                f"session {i} has a real body ({body_span(snap, i)}px) — the "
                "doji is not 'every body is thin', it is the one session that "
                "closed where it opened"
            )
        assert find_by_tag(snap, f"chart.wick.{DOJI}.hi") is not None, (
            "...and it still draws its wicks, so a doji is a visible session "
            "rather than a hole in the series"
        )
        assert find_by_tag(snap, f"chart.wick.{DOJI}.lo") is not None
        assert "a doji" in caption(snap), f"and it is NAMED: {caption(snap)}"
        assert "solid body" in caption(snap), caption(snap)

        # ── (C) PAST the toolkit: the direction is encoded twice
        # ──────────────
        hued = [body_ink(snap, i) for i in range(n)]
        assert_eq(
            [a for a, _ in hued],
            [FILL_ALPHA[d] for d in DIRECTIONS],
            "PAST QT (fill): a rising body is HOLLOW and a falling one SOLID "
            "— the traditional Japanese form, which predates colour. "
            "QCandlestickSeries has bodyOutlineVisible and no such thing",
        )
        hues = {h for _, h in hued}
        assert len(hues) >= 2, f"and the hues differ too: {hues}"
        assert_eq(
            hued[0][1],
            hued[2][1],
            "the two rising sessions share one hue, so this is a DIRECTION "
            "channel and not a per-session palette",
        )
        assert hued[DOJI][1] not in (hued[0][1], hued[1][1]), (
            "PAST QT: the doji gets its own ink. Qt has exactly two colours "
            f"and the equal case falls into one of them: {hued[DOJI][1]}"
        )

        toggle(tf, MONO_TAG)
        snap = snapshot(tf)
        mono = [body_ink(snap, i) for i in range(n)]
        assert_eq(
            len({h for _, h in mono}),
            1,
            "with the hue stripped every stroke is ONE ink — the state a "
            "deuteranope or a grayscale print is in",
        )
        assert_eq(
            [a for a, _ in mono],
            [a for a, _ in hued],
            "PAST QT: and the fill alphas do not move, so the direction is "
            "still readable off the picture. This is the claim: Qt encodes it "
            "in hue alone, so the same reader is left with nothing",
        )
        text = caption(snap)
        assert "Hue removed" in text, text
        assert "1.00:1" in text, f"one ink has no contrast, and it says so: {text}"
        toggle(tf, MONO_TAG)
        snap = snapshot(tf)
        assert "1.11:1" in caption(snap), (
            "PAST QT: the measured contrast of the CONVENTIONAL green/red "
            "pair is published — the two are all but isoluminant, which is "
            f"why the shipped pair is separated in luminance: {caption(snap)}"
        )

        # ── (D) PAST the toolkit: one datum, two readings of the x-axis
        # ───────
        assert_eq(
            [node(snap, f"chart.xlabel.{i}")["content"] for i in range(n)],
            SLOT_NAMES,
            "PAST QT: the slot names are DERIVED from the sessions' own "
            "instants. Qt's QBarCategoryAxis takes a QStringList supplied "
            "separately from the sets' timestamps, so a Qt chart holds two "
            "descriptions of when its sessions were and checks neither",
        )
        ordinal_gaps = [centres[k + 1] - centres[k] for k in range(n - 1)]
        assert abs(ordinal_gaps[AFTER_GAP - 1] - ordinal_gaps[0]) < 2.0, (
            "on the SESSION axis the weekend takes no width — the six bodies "
            f"abut whatever real time separates them: {ordinal_gaps}"
        )
        assert_eq(
            count_prefix(snap, "chart.grid.x."),
            0,
            "and an ordinal axis has no numeric x-gridlines: a slot boundary "
            "is not a value",
        )

        pick_reading(tf, ELAPSED)
        snap = snapshot(tf)
        e_centres = [centre_x(rect(snap, f"chart.candle.{i}")) for i in range(n)]
        elapsed_gaps = [e_centres[k + 1] - e_centres[k] for k in range(n - 1)]
        assert elapsed_gaps[AFTER_GAP - 1] > elapsed_gaps[0] * 2.5, (
            "PAST QT: on the ELAPSED axis the SAME six sessions sit over real "
            "UTC time, so the weekend is three days wide. One datum, two "
            f"readings, one declaration: {elapsed_gaps}"
        )
        for k in range(AFTER_GAP - 1):
            assert abs(elapsed_gaps[k] - elapsed_gaps[0]) < 2.0, (
                f"...and the weekdays are still one day apart: {elapsed_gaps}"
            )
        assert "Elapsed axis" in caption(snap), caption(snap)
        # The bodies still do not overlap: the pitch is the NARROWEST gap, not
        # a mean. A mean over this fixture would be 1.4 days and the weekday
        # bodies would run into each other.
        for i in range(n - 1):
            a = rect(snap, f"chart.candle.{i}")
            b = rect(snap, f"chart.candle.{i + 1}")
            assert float(a["x"]) + float(a["w"]) <= float(b["x"]) + 1, (
                f"bodies {i} and {i + 1} do not overlap on the elapsed axis"
            )

        # ── (E) PAST the toolkit: the label cardinality follows the reading
        # ───
        assert_eq(
            count_prefix(snap, "chart.xlabel."),
            0,
            "an elapsed axis has no per-slot labels",
        )
        assert count_prefix(snap, "chart.label.x.") > 0, (
            "it has per-TICK ones instead — two tags because they answer two "
            "questions, rather than one tag that lies about what it counts"
        )
        assert count_prefix(snap, "chart.grid.x.") > 0, (
            "and the ticks carry gridlines, which the ordinal reading had none of"
        )

        # ── (F) keyboard and RPC drive ONE selection ─────────────────
        tf.key(path=READING_TAG, name="Home")
        tf.tick(0.016)
        snap = snapshot(tf)
        assert "Session axis" in caption(snap), (
            f"Home selects the first reading, as a click on chip 0 does: {caption(snap)}"
        )
        tf.key(path=READING_TAG, name="ArrowRight")
        tf.tick(0.016)
        by_key = caption(snapshot(tf))
        pick_reading(tf, ELAPSED)
        by_click = caption(snapshot(tf))
        assert_eq(
            by_key,
            by_click,
            "keyboard and pointer reach ONE selection, not two copies of it",
        )
        pick_reading(tf, ORDINAL)

        # ── (G) the caps are the toolkit's capsVisible, and they are marks
        # ────
        toggle(tf, CAPS_TAG)
        snap = snapshot(tf)
        assert_eq(
            count_prefix(snap, "chart.cap."),
            n * 2,
            "each session gains a cap at its high and its low",
        )
        for i in range(n):
            b = rect(snap, f"chart.candle.{i}")
            for end in ("hi", "lo"):
                cap = rect(snap, f"chart.cap.{i}.{end}")
                wick = rect(snap, f"chart.wick.{i}.{end}")
                assert abs(centre_x(b) - centre_x(cap)) <= 1.5, f"cap {i}.{end} centred"
                assert float(cap["w"]) < float(b["w"]), (
                    f"cap {i}.{end} is subordinate to its body"
                )
                far = (
                    float(wick["y"])
                    if end == "hi"
                    else float(wick["y"]) + float(wick["h"])
                )
                assert abs(centre_y(cap) - far) <= 2.5, (
                    f"cap {i}.{end} sits at the wick's far end"
                )
        toggle(tf, CAPS_TAG)
        snap = snapshot(tf)
        assert_eq(count_prefix(snap, "chart.cap."), 0, "and they go away again")

        # ── (H) the derivation reaches assistive technology ──────────
        acc = tf.request("scene/access", {}).result or {}
        radios = [access_node_by_tag(acc, f"{READING_TAG}#{i}") for i in range(2)]
        assert all(r is not None for r in radios), f"one AT node per reading: {radios}"
        assert_eq(
            [r["role"] for r in radios],
            ["radio", "radio"],
            "the readings are a 1-of-N choice, not two switches",
        )
        group = access_node_by_tag(acc, READING_TAG)
        assert group is not None and group["role"] == "radiogroup", (
            f"and they sit under one radiogroup: {group}"
        )
        for tag in (CAPS_TAG, MONO_TAG):
            n_ = access_node_by_tag(acc, tag)
            assert n_ is not None and n_["role"] == "button", f"{tag}: {n_}"
        status = access_node_by_tag(acc, CAPTION_TAG)
        assert status is not None, "the caption is an AT node"
        assert_eq(status["role"], "status", "and it is a live region")
        assert_eq(
            status.get("name"),
            caption(snap),
            "PAST QT: a screen reader is told the SAME doji, reading and "
            "measured contrast a sighted reader sees. QtCharts draws into a "
            "QGraphicsScene and implements no accessibility interface at all",
        )
        assert "a doji" in (status.get("name") or ""), status.get("name")


if __name__ == "__main__":
    run_demo("r1567_candle_direction_is_the_datum", body)
