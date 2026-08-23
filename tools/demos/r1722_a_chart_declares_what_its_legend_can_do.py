#!/usr/bin/env python3
"""R1722 §5.38 §5.39 §5.40 §2 #2 §2 #7 — **a chart's legend is a declared chip
row, and a board ASKS rather than knowing.**

# The defect this exists for, measured before it was built

Seven chart kinds in `pinion-chart` name their parts. Measured against the public
surface on 2026-08-19: **two** offered "hide this part by pressing its legend
entry" (line and scatter), **five** did not (polar, donut, bar, box plot,
candlestick), and **nothing anywhere said which was which**. A board assembling
several charts therefore could not ask. It either knew by having read the crate,
or offered an affordance that did nothing — the R1721 shape one layer down, where
two of three screens announced a rule they did not obey.

The two that did offer it each *chose*, per paint: a chart held an
`Option<Vec<String>>` of caller-supplied tags and branched on it. So the tags
were zipped against the series and a caller passing too few silently truncated
its own legend, and the interactive row and the paint row were two painters with
nothing relating the pick to anything a reader could see.

# The floor this is built to beat, measured rather than read

The mature toolkit at 6.11.1 carries two chart modules; its charting module is
not among the ones built on this machine, so this is a source measurement and
says so. Its older module is **uniform where this crate was not** — six
legend-marker kinds, one per series family — and that is the one axis on which it
was ahead. On every other axis it is a floor:

  * a marker emits a *clicked* notification and nothing more: hiding the series
    is left to whoever wired the signal, so "toggle a series from the legend" is
    application work there and no two applications need agree;
  * a marker is not focusable — the item drawing it handles a hover event and no
    key event at all, so the row is reachable only by pointer;
  * **neither chart module contains a single accessibility call.** Measured
    across both trees, the count is zero;
  * nothing declares whether a part *may* be hidden, so a caller cannot ask;
  * and its newer module went further back: a series publishes a list of
    `{colour, border colour, label}` with no identity, no state and no signal.

# What this asserts

  (A) ★★★★★ **the board asks, and is told different things.** Four cards, four
      captions, and the file painting them chooses none of them: three say how
      many parts they have, and the bar card says it has none.
  (B) ★★★★★ **breadth.** The polar and donut charts now offer the gesture, which
      they could not before. Pressing an entry on either hides that part.
  (C) ★★★★★ **the declaration is what paints.** The card that names no parts has
      no entry to press and no focus stop, and this is derived from the same word
      that gives the other three theirs.
  (D) ★★★★★ **two hiding rules, side by side.** Hiding a line leaves its
      neighbours' geometry alone; hiding a slice **re-normalises the ring**,
      because a part-of-whole picture whose parts no longer sum to the whole is a
      lie. Asserted by comparing the survivor's own geometry across the press.
  (E) **an index survives a hide.** The sector a slice draws is tagged by the
      slice's index, not by its position among the drawn ones, so an agent that
      hid one and re-read the others gets the same parts back.
  (F) ★★★★★ **the accessibility tree says what the declaration says**: three
      `group`s of `button` carrying `aria-pressed`, and the fourth card
      contributes nothing at all.
  (G) **the keyboard is the row's.** Each entry is its own Tab stop and `Space`
      moves only the focused part — across three different chart kinds.
  (H) the derived tag: every entry's tag is `{chart prefix}.legend.{i}`, so the
      three charts' namespaces are disjoint without the application naming any
      of them.
  (I) ★★★★★ **the integrated screens.** Three older screens are re-driven on the
      derived tags, and the analysis tool's own dashboard is asserted to be
      **unchanged**: its three chart seats are still booked for a later release,
      and they say so on the wire. A round that builds a capability those seats
      will use must not quietly open one, and must not leave anybody believing
      they are shut because the framework could not.

Run from the workspace root:
    cargo build -p hello-chart-legends -p hello-legend-toggle \\
        -p hello-linked-legend -p hello-rescale-toggle \\
        -p hello-analyzer-shell --release
    python3 tools/demos/r1722_a_chart_declares_what_its_legend_can_do.py
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
    node_center,
    run_demo,
)

EXAMPLE = "hello-chart-legends"
VIEWPORT = (920, 648)

# The four cards, and how many parts each names. The bar card is the one whose
# answer is zero, which is what a board could not previously get.
THROUGHPUT, SHARE, PROFILE, SIZES = "throughput", "share", "profile", "sizes"
PARTS = {THROUGHPUT: 3, SHARE: 3, PROFILE: 2, SIZES: 0}
TOGGLING = (THROUGHPUT, SHARE, PROFILE)


def entry_tag(card: str, i: int) -> str:
    """The tag the chart DERIVES for entry `i` — `{prefix}.legend.{i}`."""
    return f"{card}.legend.{i}"


def paint(tf):
    return tf.snapshot(source="paint", viewport=VIEWPORT)


def caption(snap, card: str) -> str:
    node = find_by_tag(snap, f"{card}.caption")
    assert node is not None, f"{card} has a caption"
    return node.get("content") or ""


def press_entry(tf, snap, card: str, i: int) -> None:
    """Coordinate-press the legend entry at its rect centre — a real press
    through the chart legend's own hit geometry, not a tag shortcut."""
    entry = find_by_tag(snap, entry_tag(card, i))
    assert entry is not None, f"{card} entry {i} is present with a rect"
    tf.click(at=node_center(entry))
    tf.pointer_leave()


def value(tf, card: str, i: int):
    return tf.query(f"/{entry_tag(card, i)}/external/value")


def geometry(snap, tag: str):
    """The path commands of `tag`, or `None` when it is not drawn."""
    node = find_by_tag(snap, tag)
    return None if node is None else node.get("commands")


def the_older_screens_moved_to_the_derived_tags() -> None:
    """(I) The three screens that already had a toggle legend, re-driven on the
    tags the CHART derives rather than the ones they used to choose.

    Each one is a different arrangement — one chart, a selector driving a second
    chart, and a chart that rescales as parts leave — and the tag is the same
    shape in all three because none of them picks it any more.
    """
    for example, prefix, viewport in (
        ("hello-legend-toggle", "chart", (560, 360)),
        ("hello-rescale-toggle", "chart", (560, 360)),
        ("hello-linked-legend", "scatter", (560, 560)),
    ):
        with RpcSubprocess(example, boot_grace=1.5) as tf:
            snap = tf.snapshot(source="paint", viewport=viewport)
            tag = f"{prefix}.legend.0"
            entry = find_by_tag(snap, tag)
            assert entry is not None, f"(I) {example} paints {tag}"
            assert_eq(
                tf.query(f"/{tag}/external/value"),
                True,
                f"(I) {example} binds its first entry to a live toggle",
            )
            tf.click(at=node_center(entry))
            tf.pointer_leave()
            assert_eq(
                tf.query(f"/{tag}/external/value"),
                False,
                f"(I) ★ and a press on the derived tag still hides the part in {example}",
            )


def the_dashboards_chart_seats_are_still_booked() -> None:
    """(I) ★★★★★ The analysis tool's own dashboard, unchanged.

    Its chart seats — a time series and a part-of-whole — are booked for a
    later release. This round built the legend gesture those seats will want,
    and the seats must therefore be exactly where they were: a capability
    landing is not a release decision, and the wire must still say each one is
    waiting on its own requirement rather than on the framework.

    ★★★★★ R1797 — and `latency`, the third seat, MOVED. That is not this rule
    breaking; it is the rule holding and the other thing happening. R1722's
    claim is that building a capability does not promote a seat, and R1797 did
    not promote one by building anything: the reader was asked and chose to
    place the card. A release decision moves a seat, and nothing else does —
    which is why the seat is checked against the placeable list below rather
    than quietly dropped from this table.
    """
    booked = {
        "throughput": "requirement 16",
        "share": "requirement 17",
    }
    with RpcSubprocess("hello-analyzer-shell", boot_grace=1.5) as tf:
        inert = {row["tag"]: row for row in tf.request("scene/disabled", {}).result["disabled"]}
        for kind, booking in booked.items():
            row = inert.get(f"shell.palette.{kind}")
            assert row is not None, f"(I) the {kind} seat is still reported inert"
            assert_eq(row["reason"], "reserved", f"(I) {kind} is inert as a RESERVATION")
            assert_eq(
                row["detail"],
                booking,
                f"(I) ★ {kind} still waits on its own booking, not on a missing capability",
            )
            assert_eq(row["recourse"], "await_release", f"(I) {kind}'s recourse is unchanged")
        for kind in ("packet", "decode", "keymap", "filter"):
            assert f"shell.palette.{kind}" not in inert, (
                f"(I) {kind} is placeable and this round did not touch it"
            )
        # ★ R1797 — and the seat that moved is checked on the OTHER side rather
        # than dropped from the table. A seat that vanished from both lists
        # would leave this demo passing while saying nothing about it.
        assert "shell.palette.latency" not in inert, (
            "(I) ★ latency is placeable since R1797 — a release decision the "
            "reader made, not a capability landing"
        )


def body() -> None:
    the_older_screens_moved_to_the_derived_tags()
    the_dashboards_chart_seats_are_still_booked()

    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) the board asks, and is told four different things ─────────
        snap = paint(tf)
        assert_eq(
            caption(snap, THROUGHPUT),
            "3 parts — press one to hide it",
            "(A) the line card was told it has three parts",
        )
        assert_eq(
            caption(snap, SHARE),
            "3 parts — press one to hide it",
            "(A) the donut card was told it has three parts",
        )
        assert_eq(
            caption(snap, PROFILE),
            "2 parts — press one to hide it",
            "(A) the radar card was told it has two",
        )
        assert_eq(
            caption(snap, SIZES),
            "no parts to name",
            "(A) ★ and the bar card was told it has none — the answer that did not exist",
        )

        # ── (C) the declaration is what paints ────────────────────────────
        for card in TOGGLING:
            for i in range(PARTS[card]):
                entry = find_by_tag(snap, entry_tag(card, i))
                assert entry is not None, f"(C) {card} entry {i} is painted"
                assert entry.get("rect"), f"(C) {card} entry {i} has hit geometry"
                assert_eq(value(tf, card, i), True, f"(C) boot: {card} part {i} shown")
        assert find_by_tag(snap, entry_tag(SIZES, 0)) is None, (
            "(C) ★ the card that names no parts has no entry to press"
        )

        # ── (H) the tags are disjoint because each chart derives its own ──
        assert find_by_tag(snap, "chart.legend.0") is None, (
            "(H) no chart kept the default prefix, so nothing collides"
        )

        # ── (B) breadth: the DONUT offers the gesture, which it could not ─
        press_entry(tf, snap, SHARE, 0)
        assert_eq(value(tf, SHARE, 0), False, "(B) ★ pressing a donut legend entry hides its slice")
        assert_eq(value(tf, SHARE, 1), True, "(B) its neighbour is untouched")
        before_share = snap
        snap = paint(tf)
        assert geometry(snap, "share.slice.0") is None, "(B) the hidden slice draws no sector"
        assert find_by_tag(snap, entry_tag(SHARE, 0)) is not None, (
            "(B) and keeps its entry, which is the toggle back on"
        )

        # ── (D) the donut RE-NORMALISES, which the line must not ──────────
        assert geometry(before_share, "share.slice.1") != geometry(snap, "share.slice.1"), (
            "(D) ★ the surviving slices take the hidden one's share"
        )
        # ── (E) and the survivors keep their own indices ──────────────────
        assert geometry(snap, "share.slice.1") is not None, "(E) slice 1 is still slice 1"
        assert geometry(snap, "share.slice.2") is not None, "(E) slice 2 is still slice 2"

        press_entry(tf, snap, SHARE, 0)
        assert_eq(value(tf, SHARE, 0), True, "(B) pressing again shows the slice")
        snap = paint(tf)

        # ── (D) the LINE leaves its neighbours where they were ────────────
        before_line = snap
        press_entry(tf, snap, THROUGHPUT, 0)
        assert_eq(value(tf, THROUGHPUT, 0), False, "(D) pressing a line legend entry hides it")
        snap = paint(tf)
        assert geometry(snap, "throughput.series.0") is None, "(D) the hidden line draws nothing"
        assert_eq(
            geometry(snap, "throughput.series.1"),
            geometry(before_line, "throughput.series.1"),
            "(D) ★ and its neighbour did not move — the other hiding rule",
        )
        press_entry(tf, snap, THROUGHPUT, 0)
        assert_eq(value(tf, THROUGHPUT, 0), True, "(D) restored")
        snap = paint(tf)

        # ── (B) breadth: the RADAR too ────────────────────────────────────
        press_entry(tf, snap, PROFILE, 1)
        assert_eq(value(tf, PROFILE, 1), False, "(B) ★ pressing a radar legend entry hides it")
        snap = paint(tf)
        assert find_by_tag(snap, "profile.series.1") is None, "(B) the hidden ring is gone"
        assert find_by_tag(snap, "profile.series.0") is not None, "(B) the other ring stays"
        press_entry(tf, snap, PROFILE, 1)
        assert_eq(value(tf, PROFILE, 1), True, "(B) restored")

        # ── (F) the accessibility tree says what the declaration says ─────
        access = tf.request("scene/access").result
        for card in TOGGLING:
            group = access_node_by_tag(access, f"{card}.legend")
            assert group is not None, f"(F) {card} publishes a group for its legend"
            assert_eq(group.get("role"), "group", f"(F) {card}'s legend is a group")
            for i in range(PARTS[card]):
                node = access_node_by_tag(access, entry_tag(card, i))
                assert node is not None, f"(F) {card} entry {i} is announced"
                assert_eq(node.get("role"), "button", f"(F) {card} entry {i} is a button")
                assert_eq(
                    (node.get("state") or {}).get("checked"),
                    True,
                    f"(F) {card} entry {i} carries aria-pressed",
                )
        assert access_node_by_tag(access, f"{SIZES}.legend") is None, (
            "(F) ★ the card that names no parts announces no control at all"
        )

        # ── (G) the keyboard is the row's, across three chart kinds ───────
        for card in TOGGLING:
            tag = entry_tag(card, 0)
            focused = tf.request("focus/set", {"tag": tag}).result.get("focused")
            assert_eq(focused, tag, f"(G) {card} entry 0 is its own Tab stop")
            tf.key(path=tag, name="Space")
            assert_eq(value(tf, card, 0), False, f"(G) Space hides {card} part 0")
            assert_eq(value(tf, card, 1), True, f"(G) and moves no sibling in {card}")
            tf.key(path=tag, name="Space")
            assert_eq(value(tf, card, 0), True, f"(G) Space again restores {card} part 0")

        # ── (F) an off part is announced off ──────────────────────────────
        press_entry(tf, paint(tf), SHARE, 2)
        access = tf.request("scene/access").result
        node = access_node_by_tag(access, entry_tag(SHARE, 2))
        assert node is not None, "(F) the hidden slice keeps its announced entry"
        assert_eq(
            (node.get("state") or {}).get("checked"),
            False,
            "(F) ★ and is announced as not pressed",
        )


if __name__ == "__main__":
    sys.exit(run_demo("R1722 a chart declares what its legend can do", body))
