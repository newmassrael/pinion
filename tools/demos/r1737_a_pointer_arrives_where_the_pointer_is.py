#!/usr/bin/env python3
"""R1737 §5.35 §5.15 §2 #7 — **a pointer arrives where the pointer is**, on
every screen of the analysis tool that resolves a press itself, judged by the
framework's own record and driven by the machine's own pointer.

# What this exists for

R1736 repaired a one-pixel defect. The router hands a self-hit-testing screen a
FRACTION of its painted rectangle; the screen multiplies it back; the fraction
was made by an `f32` division, so the product lands a hair either side of the
pixel it came from and truncating turned "a hair below" into the pixel before. A
press carries no position of its own — it acts on the cursor the last move
recorded — so the whole screen was aimed one pixel away, at some coordinates and
not others. A person reported it as *"sometimes the node is selected and
sometimes the background behind it is."*

**That repair was measured on a screen that happened to publish a cursor
field.** Measured at the start of this round, across the five screens in this
tree that implement a hit test of their own: three publish one, in **two
incompatible spellings** (a `"x,y"` string and an `{x, y}` object), and **two
publish nothing at all**. The check that found the defect was therefore not
runnable on two of the five, and on the other three it went through each
screen's own vocabulary.

The fact was never the screen's to volunteer. At the moment the framework
resolves a reading it holds **both** accounts of where the pointer is: the
cursor the window system reported, and the rectangle the fraction is taken over.
`scene/pointer_arrival` publishes the comparison, for every surface, in one
spelling, whether or not the surface says anything about cursors.

# What it asserts

* **A** — every surface: before anything is touched, every painted surface has
  an arrival row, every row reads `never`, and the row shape is the same on all
  five screens.
* **B** — two accounts: a real pointer inside a surface lands `exact`, with the
  two accounts naming one pixel; a captured pointer marched outside its own
  rectangle lands `strayed` and is **not** a defect.
* **C** — every screen: a contiguous run of columns and of rows, walked one
  pixel at a time with a real pointer, arrives exactly — on all five screens.
* **D** — it is the arrival: the record is the last **delivery** and does not
  follow the cursor out of the window, and a read leaves it where it was.
* **E** — the specification: every clause of `docs/analyzer-arrival-spec.json` is
  named by a check above, and the titles are read out of the file rather than
  written here.

# Floor, measured by building a probe at 6.11.1 and running it

The floor is **above** this tree on one axis and this round is the debt being
paid: an outside observer there can ask, for *any* widget, where the pointer is
in that widget's own frame without the widget having stored anything — exact
over 400 columns and 300 rows of a child, no misses.

What it cannot do is this question. Its answer is where the cursor **is**, not
where the event **arrived**: a press delivered to a child at (37, 21), followed
by moving the cursor, leaves it answering (300, 250). Across the five types such
a record could live on there are 245 declared properties and 195 declared
methods; 3 are point-typed — all of them the widget's own position in its parent
— and 0 methods return a point. And it never compares its own two accounts, so
there is no verdict to read.

Run from the workspace root (needs an X display and a mapped window):
    cargo build --release -p hello-node-lab -p hello-analyzer-shell \\
        -p hello-log-view -p hello-packet-view -p hello-key-patterns -p hello-data-grid
    python3 tools/demos/r1737_a_pointer_arrives_where_the_pointer_is.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RealPointer,
    RpcSubprocess,
    assert_eq,
    assert_no_pointer_drift,
    pointer_arrivals,
    run_demo,
)

CHECKS: list[str] = []

SPEC = Path(__file__).resolve().parent.parent.parent / "docs" / "analyzer-arrival-spec.json"

#: The five screens that implement `External::target_at` — counted, not assumed
#: (`grep -rln "fn target_at" examples/*/src/*.rs`). Every one of them resolves a
#: press against the pixel this record holds, which is what makes the population
#: this list and not "the analyser's screens".
SELF_HIT_TESTING = [
    "hello-node-lab",
    "hello-analyzer-shell",
    "hello-log-view",
    "hello-packet-view",
    "hello-key-patterns",
]

#: How many columns and rows section C walks per screen. A contiguous RANGE and
#: not a chosen list: which pixels a truncating multiplication loses depends on
#: the extent, so a list somebody picks can miss every one of them — which is
#: precisely how R1736's defect survived every gate here.
#:
#: 600 to match R1736's own measurement exactly (35 of 600 columns and 20 of 600
#: rows arrived wrong before the repair), so the number here and the number in the
#: record are about the same population. Affordable at this size only because the
#: framework counts: the query is once per screen, not once per pixel.
SWEEP = 600


def ok(what: str, condition: bool, detail: str = "") -> None:
    # Printed as it passes rather than only in the summary: this demo boots six
    # binaries and walks thousands of pointer positions, so a failure in the last
    # section must not take the record of the first five with it.
    CHECKS.append(what)
    assert condition, f"{what}{(' — ' + detail) if detail else ''}"
    print(f"[demo] ok {what}")


def spec() -> dict:
    return json.loads(SPEC.read_text(encoding="utf-8"))


def titles(surface: dict) -> dict[str, str]:
    return {c["key"]: c["title"] for c in surface["canon"]}


def rows_of(report: dict) -> dict[str, dict]:
    return {row["surface"]: row for row in report["surfaces"]}


# ── A: every surface, before anything is touched ────────────────────────────


def section_a(spec_doc: dict) -> None:
    """A — the arrival is askable for every surface, whatever it publishes."""
    said = titles(spec_doc["every_surface"])
    shapes: dict[str, frozenset] = {}
    for example in SELF_HIT_TESTING:
        with RpcSubprocess(example) as tf:
            report = pointer_arrivals(tf)
            assert report is not None, f"{example} answers scene/pointer_arrival"
            rows = rows_of(report)
            ok(
                f"A/{example}: ★★★★★ {said['askable']} — {len(rows)} surface(s), "
                f"and this screen publishes no cursor field of its own for two of "
                f"the five",
                bool(rows),
                f"report={report}",
            )
            ok(
                f"A/{example}: {said['never-is-named']} — "
                f"{len(report['never'])} named, {report['arrived']} arrived, "
                f"{report['delivered']} delivered",
                sorted(report["never"]) == sorted(rows)
                and report["arrived"] == 0
                and report["delivered"] == 0
                and all(row["state"] == "never" for row in rows.values()),
                f"never={report['never']} arrived={report['arrived']}",
            )
            for row in rows.values():
                ok(
                    f"A/{example}: a surface nobody pointed at carries neither a "
                    f"position nor a count, so it cannot read as one that was "
                    f"exercised and found clean",
                    "last" not in row and "delivered" not in row,
                    f"row={row}",
                )
                break
            shapes[example] = frozenset(report.keys())
    ok(
        f"A: ★★★★★ {said['one-spelling']} — one key set across "
        f"{len(SELF_HIT_TESTING)} screens, where the screens' own cursor fields "
        f"come in two spellings and are absent on two of them",
        len(set(shapes.values())) == 1,
        f"shapes={ {k: sorted(v) for k, v in shapes.items()} }",
    )


# ── B: the two accounts, and the case that is not a defect ──────────────────


def section_b(spec_doc: dict) -> None:
    """B — exact inside the rectangle, strayed outside it."""
    said = titles(spec_doc["two_accounts"])
    with RpcSubprocess("hello-node-lab", visible_window=True) as tf:
        with RealPointer(tf, settle=0.02) as rp:
            rp.move((500, 300))
            report = pointer_arrivals(tf)
            assert report is not None
            arrived = [r for r in report["surfaces"] if r["state"] == "arrived"]
            ok(
                f"B: ★★★★★ {said['exact']} — {len(arrived)} surface(s) arrived and "
                f"the two accounts name one pixel",
                bool(arrived)
                and all(r["last"]["landing"] == "exact" for r in arrived)
                and all(
                    tuple(r["last"]["inside"]) == tuple(r["last"]["resolved"])
                    and tuple(r["last"]["drift"]) == (0, 0)
                    for r in arrived
                ),
                f"arrived={arrived}",
            )
            ok(
                f"B: {said['drifted-is-a-defect']} — the report's own totals "
                f"follow its rows, and a surface convicted by ANY of its arrivals "
                f"stays convicted",
                report["defects"] == sum(1 for r in report["surfaces"] if r.get("drifted"))
                and report["drifts"] == sum(r.get("drifted", 0) for r in report["surfaces"])
                and all(("drifted_at" in r) == bool(r.get("drifted")) for r in arrived),
                f"defects={report['defects']} drifts={report['drifts']}",
            )
    # ★ The other arm, measured rather than argued: a CAPTURED pointer marched
    # outside the rectangle its fraction is taken over. A grid keeps receiving
    # moves after the cursor leaves it — that is what a capture lock is for — so
    # the resolved pixel there is a clamp, and calling it a disagreement would
    # manufacture a defect out of correct behaviour.
    #
    # This screen also proves the ORIGIN is being subtracted: its grid is painted
    # at (20, 110), not at the window's corner, so an arrival that ignored the
    # rectangle's origin would land 20 and 110 pixels out and read as drifted.
    with RpcSubprocess("hello-data-grid", visible_window=True) as tf:
        with RealPointer(tf, settle=0.03) as rp:
            rp.move((300, 200))
            rp.press()
            try:
                rp.move((300, 200))
                held = [
                    r
                    for r in (pointer_arrivals(tf) or {"surfaces": []})["surfaces"]
                    if r["state"] == "arrived"
                ]
                ok(
                    f"B: {said['exact']} — over a rectangle painted at "
                    f"{held[0]['last']['over'] if held else '?'}, so the origin is "
                    f"subtracted rather than assumed to be the window's corner",
                    bool(held)
                    and held[0]["last"]["landing"] == "exact"
                    and tuple(held[0]["last"]["inside"]) == (280, 90),
                    f"held={held}",
                )
                rp.move((1350, 700))
                strayed = [
                    r
                    for r in (pointer_arrivals(tf) or {"surfaces": []})["surfaces"]
                    if r.get("strayed")
                ]
            finally:
                rp.release()
            report = pointer_arrivals(tf)
            assert report is not None
            ok(
                f"B: ★★★★★ {said['strayed-is-not']} — {len(strayed)} surface(s) "
                f"reported a cursor outside the rectangle their fraction is taken "
                f"over, and the report counts {report['defects']} defect(s)",
                bool(strayed) and report["defects"] == 0,
                f"strayed={strayed} report={report}",
            )


# ── C: every screen, one pixel at a time ────────────────────────────────────


def sweep_one(example: str, said: dict[str, str]) -> tuple[int, int]:
    """Walk a contiguous run of columns and rows, then ask ONCE.

    ★ Asking once is not a shortcut — it is what the framework's tally is for.
    The first draft of this section queried after every move, which is a round
    trip per pixel: it loaded this machine enough that the boot of the NEXT
    screen the sweep was about to measure timed out. A probe expensive enough to
    disturb the thing it measures is R1736's lesson in a new costume.
    """
    with RpcSubprocess(example, visible_window=True) as tf:
        with RealPointer(tf, settle=0.01) as rp:
            assert pointer_arrivals(tf) is not None, f"{example} answers the census"
            # Aim inside the screen's OWN painted rectangle, read from the census
            # rather than assumed: the five screens open at four different sizes.
            rp.move((40, 40))
            first = [
                r
                for r in (pointer_arrivals(tf) or {"surfaces": []})["surfaces"]
                if r["state"] == "arrived"
            ]
            assert first, f"{example}: a real pointer reached no surface"
            over = first[0]["last"]["over"]
            x0 = over["x"] + 20
            y0 = over["y"] + 20
            columns = range(x0, min(x0 + SWEEP, over["x"] + over["w"] - 1))
            rows = range(y0, min(y0 + SWEEP, over["y"] + over["h"] - 1))
            for x in columns:
                rp.move((x, y0))
            for y in rows:
                rp.move((x0, y))
            report = assert_no_pointer_drift(tf, label=f"C/{example}")
            assert report is not None
            walked = len(columns) + len(rows)
            ok(
                f"C/{example}: ★★★★★ {said['columns']} and {said['rows']} — "
                f"{len(columns)} columns and {len(rows)} rows walked one pixel at a "
                f"time with the machine's own pointer, and the framework counted "
                f"{report['delivered']} arrival(s) with {report['drifts']} gone "
                f"wrong (with the pre-R1736 cast, 35 of 600 columns and 20 of 600 "
                f"rows arrived one pixel out)",
                report["drifts"] == 0 and report["delivered"] >= walked,
                f"report={report}",
            )
            # ★ And the count is what makes the check cover the sweep rather than
            # its last step: a report that had seen fewer arrivals than the
            # pointer made would mean the sweep went somewhere this surface never
            # heard about, which is a different defect and must not read as this
            # one passing.
            return (len(columns), len(rows))


def section_c(spec_doc: dict) -> None:
    """C — the round trip, on all five screens that resolve a press themselves."""
    said = titles(spec_doc["every_screen"])
    walked = 0
    for example in SELF_HIT_TESTING:
        cols, rows = sweep_one(example, said)
        walked += cols + rows
    ok(
        f"C: ★★★★★ {said['all-five']} — {walked} real pointer positions across "
        f"{len(SELF_HIT_TESTING)} screens, two of which could not be asked this "
        f"question at all before this round",
        len(SELF_HIT_TESTING) == 5 and walked == 5 * 2 * SWEEP,
        f"walked={walked}",
    )


# ── D: the arrival, not the cursor ──────────────────────────────────────────


def live_cursor(tf: RpcSubprocess) -> tuple[float, float] | None:
    """Where the framework says the pointer IS, which is the other fact."""
    state = tf.request("scene/input_state", {})
    assert state is not None
    cursor = state.result.get("cursor")
    return None if cursor is None else (float(cursor["x"]), float(cursor["y"]))


def section_d(spec_doc: dict) -> None:
    """D — the record is the delivery, and a read does not move it.

    ★ Driven on the grid rather than on a full-window screen, and the reason is
    the measurement itself: the two facts only come apart where the pointer can
    be somewhere that is NOT delivered to the surface. A grid takes moves under
    a capture lock and not otherwise, so releasing the button and walking away
    leaves the live cursor and the last delivery in genuinely different places —
    which is the case a live-cursor answer gets wrong.

    The first draft of this section used the node lab and moved the pointer to
    the display's corner. That screen is 1625 pixels wide on a 1440-pixel
    display, so there is no point on the display outside its window and the move
    WAS delivered: the fixture could not tell the two apart. Written down
    because it is the class this whole round is about.
    """
    said = titles(spec_doc["it_is_the_arrival"])
    with RpcSubprocess("hello-data-grid", visible_window=True) as tf:
        with RealPointer(tf, settle=0.03) as rp:
            rp.move((300, 200))
            rp.press()
            rp.release()
            delivered = [
                r
                for r in (pointer_arrivals(tf) or {"surfaces": []})["surfaces"]
                if r["state"] == "arrived"
            ]
            assert delivered, "a captured press reached the surface"
            landed = tuple(delivered[0]["last"]["resolved"])
            counted = delivered[0]["delivered"]
            # Somewhere else in the window, with nothing held — so nothing is
            # delivered from there. Inside the window on purpose: outside it the
            # framework reports no live cursor at all, which makes the two facts
            # trivially different and proves less. The point is that they differ
            # while BOTH are available.
            elsewhere = (200, 40)
            rp.move(elsewhere)
            moved_to = live_cursor(tf)
            after = [
                r
                for r in (pointer_arrivals(tf) or {"surfaces": []})["surfaces"]
                if r["state"] == "arrived"
            ]
            ok(
                f"D: ★★★★★ {said['delivered']} — the record still reads {landed} "
                f"while the framework's live cursor is at {moved_to}. Measured at "
                f"6.11.1, the floor answers the CURSOR here: a press delivered to "
                f"a child at (37,21) followed by moving the cursor leaves it "
                f"answering (300,250), and the delivered position is reported "
                f"nowhere",
                bool(after)
                and tuple(after[0]["last"]["resolved"]) == landed
                and after[0]["delivered"] == counted
                and moved_to is not None
                and (round(moved_to[0]), round(moved_to[1])) == elsewhere,
                f"after={after} live={moved_to}",
            )
            # A read of a DIFFERENT question over a DIFFERENT point. If asking
            # moved the record, this is what would move it — and the COUNT is
            # what would show it, because a record that only kept the last
            # position could be moved and put back unnoticed.
            tf.request("scene/wheel_intent", {"x": 100, "y": 100})
            tf.request("scene/pointer_target", {})
            twice = pointer_arrivals(tf)
            thrice = pointer_arrivals(tf)
            row = next(r for r in twice["surfaces"] if r["state"] == "arrived")
            ok(
                f"D: {said['a-read-moves-nothing']} — two censuses over other "
                f"points and three reads leave it at {landed} with the count still "
                f"{counted}",
                twice == thrice
                and tuple(row["last"]["resolved"]) == landed
                and row["delivered"] == counted,
                f"twice={twice}",
            )


# ── E: the specification ────────────────────────────────────────────────────


def section_e(spec_doc: dict) -> None:
    """E — every clause of the specification is named by a check above."""
    named = {
        "every_surface": {"askable", "never-is-named", "one-spelling"},
        "two_accounts": {"exact", "drifted-is-a-defect", "strayed-is-not"},
        "every_screen": {"columns", "rows", "all-five"},
        "it_is_the_arrival": {"delivered", "a-read-moves-nothing"},
    }
    for surface, keys in named.items():
        declared = {c["key"] for c in spec_doc[surface]["canon"]}
        assert_eq(
            declared,
            keys,
            f"E: ★★ every clause of `{surface}` is answered above, and no check "
            f"here names a clause the specification does not have",
        )
    owed = {
        surface: [o["key"] for o in spec_doc[surface]["owed"]]
        for surface in named
        if spec_doc[surface]["owed"]
    }
    ok(
        f"E: and what is still owed is written down rather than absent: {owed}",
        owed
        == {
            "it_is_the_arrival": [
                "drag-channel",
                "wheel-channel",
                "a-surface-that-stops-being-painted-takes-its-drifts-with-it",
            ]
        },
        f"owed = {owed}",
    )


def body() -> None:
    spec_doc = spec()
    section_a(spec_doc)
    section_b(spec_doc)
    section_c(spec_doc)
    section_d(spec_doc)
    section_e(spec_doc)
    print(f"\n[demo] {len(CHECKS)} named check(s)")
    for line in CHECKS:
        print(f"  - {line}")


if __name__ == "__main__":
    run_demo("R1737 a pointer arrives where the pointer is", body)
