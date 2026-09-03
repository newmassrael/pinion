#!/usr/bin/env python3
"""R1791 §5.38 §5.15 §2 #2 §2 #7 — **the toolbar gives a group up instead of
painting past its own edge**, and says which one it gave.

# What this demo exists for

A reader opened the assembled analysis tool and reported that the node lab's
inspector was cut off. Measured: the shipped window is 1440 wide, the lab's page
gets 1388, and the lab declared it needed 1625 — short by 237. And 1029 of that
1625 was the **toolbar**, in a rigid row, so what cut the inspector was not the
inspector.

★★★★★ Re-measuring for this round found it worse than the report. The right
cluster's groups need **607** with their gaps, and get **410** at this screen's
own design width and **358** in the page the shell gives it — and **595 at the
1625 it declared as its minimum**. It did not fit at its own floor either. The
constant it was checked against, 609, was a *reach* — how far the rightmost seat
came in — and a reach is not a sum: it holds only if the two clusters are flush.

The screen's own source had written the answer down and could not take it:
*"what would take it back is an overflow affordance on the toolbar, which this
tree does not have and which is a round of its own; until then a screen whose
chrome outgrows its window clips"*.

# The floor, built and run at 6.11

The reference **has** an overflow affordance, so by this project's rule a
consumer for one exists. Measured, ten actions squeezed from 1200px to 220px:

| asked | what it does |
|---|---|
| how many stay | **1 of 10**, the rest behind an extension button |
| which ones did it hide | **no member says** |
| is a hidden action still "visible" | **`isVisible()` answers true** |

The third is the one this inverts: a reader asking *what can this toolbar do
right now* is told there about controls a person cannot see.

  (A) nothing is painted past the window's edge, at the size that was reported.
  (B) the toolbar says what it HOLDS, what is on the row, what it moved, and
      that it fits.
  (B2) and the ORDER it gives groups up in, before it has had to.
  (C) the control names what it holds — the answer the floor has no member for.
  (D) a moved control is REACHABLE: open the overflow and it is there, keeping
      its own tag, and pressing it does the thing it always did.
  (E) closing puts it back, and the row is unchanged.

# ★★★★★ R1990 — what this walk got wrong, and the second defect it uncovered

(B) used to compare the union of the two lists against **four words written in
this file**. R1988 added a fifth toolbar group and CI went red saying *"nothing
fell between them"* — when nothing had fallen; a group had **arrived**, and this
gate was holding a stale copy of an answer the screen can derive. A gate that
re-spells the rule is a second copy of it, free to disagree. The screen
publishes the population now (`groups`), and this compares against that.

Repairing it exposed the second defect. R1988's doc said the focus chip is
*"Leftmost, so it is the first thing a narrow toolbar gives up"*. Measured at
the shipped 1440: it **stays**, while `export` and `file` go — ordinary groups
are given up from the **end** of the row, so leftmost is the last. The sentence
was false on the day it was written, and nothing could have said so, because the
row published *what moved* and never *the order it moves things in*. That order
is a property of the row rather than of this width, so (B2) reads it at a width
where nothing has moved as well as at one where two have.

Underneath both: `right_cluster` chose each group's concession policy with
`if group == Run { kept() } else { item }`, so a group joining later got one
**by default**. It is an exhaustive match on the type now, and the compiler asks.

Run from the workspace root:
    cargo build -p hello-analyzer-shell --release
    python3 tools/demos/r1791_a_row_says_what_it_moved.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    run_demo,
)

SHELL = "hello-analyzer-shell"
LAB = "hello-node-lab"
EXT = "/external"
SHIPPED = (1440, 900)

CHECKS: list[str] = []


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def ok(what: str, condition: bool) -> None:
    CHECKS.append(what)
    assert condition, what


def rects(app: RpcSubprocess) -> dict:
    return abs_rects_of(app.snapshot(source="paint", viewport=SHIPPED))


def body() -> None:
    # ── (A) the reported defect, at the size it was reported ────────
    banner("A — in the assembled tool, nothing is painted past the window")
    with RpcSubprocess(SHELL, boot_grace=1.5) as app:
        app.intervene(f"{EXT}/nav", "lab")
        app.tick_ms(16)
        painted = rects(app)
        past = sorted(
            tag
            for tag, r in painted.items()
            if tag.startswith("lab.") and r[0] + r[2] > SHIPPED[0]
        )
        assert_eq(past, [], "★★ nothing reaches past the right edge")
        inspector = painted["lab.inspector"]
        assert_eq(
            inspector[0] + inspector[2],
            SHIPPED[0],
            "★★★★★ the inspector ends exactly at the window's edge — whole, "
            "where a reader found it cut",
        )
        assert_eq(inspector[2], 312, "at its full declared width")
        ok(
            "and the overflow control is on the row, because something moved",
            "lab.toolbar.more" in painted,
        )

        # ★★★★★ R1990 — the toolbar's account read from the ASSEMBLED TOOL, at
        # the size a reader opened, and not only from the standalone binary in
        # (B). The mounted screen's external lives under its destination
        # (`/node_lab/external`, R1989); the shell's own `/external` refuses
        # this path, which is the point — this is the lab's answer, given while
        # it is a section of an application rather than a program of its own.
        # The page the shell grants is narrower than the window, so the row here
        # is decided for a different width than (B)'s: the ORDER holding across
        # both is what says it is a property of the row and not of a width.
        mounted: Any = app.query("/node_lab/external/toolbar_overflow")
        assert_eq(
            sorted(mounted["on_the_row"] + mounted["moved"]),
            sorted(mounted["groups"]),
            "★★★★★ in the assembled tool too, the two lists partition every "
            "group the toolbar holds",
        )
        assert_eq(
            mounted["gives_up"][-1],
            "focus",
            "★★ and the give-up order is the same one the standalone row "
            "states — a property of the row, not of the width it was read at",
        )
        assert_eq(
            mounted["short_by"], 0, "★★ and it fits in the page the shell grants"
        )

    # ── (B)-(E) the toolbar's own account, on the lab itself ────────
    with RpcSubprocess(LAB, boot_grace=1.5) as lab:
        banner("B — the toolbar says what it holds and what it moved")
        state: Any = lab.query(f"{EXT}/toolbar_overflow")
        assert_eq(
            sorted(state),
            [
                "control",
                "gives_up",
                "gives_up_next",
                "groups",
                "moved",
                "moved_seats",
                "on_the_row",
                "open",
                "short_by",
            ],
            "nine facts: what the toolbar HOLDS, what stayed, what moved, the "
            "seats those groups hold, the order it gives groups up in and what "
            "goes next, whether the control is drawn, whether it is open -- and "
            "`short_by`, which is what makes 'never cut' checkable",
        )
        assert_eq(state["short_by"], 0, "★★★★★ it fits — that is R1791")
        ok("something had to move", len(state["moved"]) > 0)
        ok("and something stayed", len(state["on_the_row"]) > 0)
        ok("the control is drawn, because something moved", state["control"] is True)

        # ★★★★★ R1990 — against the POPULATION THE SCREEN PUBLISHES, not a list
        # written here. This assertion used to compare the union with four words
        # spelled in this file; R1988 added a fifth group and it went red saying
        # "nothing fell between them" while nothing had fallen — a group had
        # merely arrived and the gate held a stale copy of the answer. A gate
        # that re-spells the rule is a second copy of it.
        assert_eq(
            sorted(state["on_the_row"] + state["moved"]),
            sorted(state["groups"]),
            "★★★★★ the two lists partition every group the toolbar holds — "
            "nothing fell between them, and nothing joined without being placed",
        )
        assert_eq(
            sorted(set(state["on_the_row"]) & set(state["moved"])),
            [],
            "★★ and they are disjoint — the floor's hidden action answers "
            "'visible' true, so being in one list has to mean not being in the "
            "other",
        )
        ok(
            "★★ the launch seat never moves, whatever the width",
            "run" in state["on_the_row"],
        )

        banner("B2 — and the order it would give them up in, before it has to")
        gives_up: list[str] = state["gives_up"]
        ok(
            "the launch seat may never move, so it is not in the order at all",
            "run" not in gives_up,
        )
        assert_eq(
            sorted(gives_up),
            sorted(g for g in state["groups"] if g != "run"),
            "★★ and every other group is — 'may move' is readable, not only "
            "'did move'",
        )
        assert_eq(
            sorted(gives_up[: len(state["moved"])]),
            sorted(state["moved"]),
            "★★★★★ what moved is the FIRST N of that order — a row cannot "
            "report giving up a later group while keeping an earlier one",
        )
        still_here = gives_up[len(state["moved"]) :]
        assert_eq(
            state["gives_up_next"],
            still_here[0] if still_here else None,
            "★★ and what goes next is the first it has not taken yet",
        )
        assert_eq(
            gives_up[-1],
            "focus",
            "★★★★★ R1990 — the focus chip is the LAST group this row gives up, "
            "not the first. R1988's doc said 'Leftmost, so it is the first "
            "thing a narrow toolbar gives up'; measured, it stays while `export` "
            "and `file` go, because ordinary groups are given up from the END. "
            "The sentence stood for two rounds because nothing performed it — "
            "this is that sentence, performed",
        )

        banner("C — the control names what it is holding")
        painted = abs_rects_of(lab.snapshot(source="paint", viewport=SHIPPED))
        ok("the control is painted", "lab.toolbar.more" in painted)
        nodes = lab.request("scene/access").result["nodes"]
        by_tag = {n.get("tag"): n for n in nodes if n.get("tag")}
        name = by_tag.get("lab.toolbar.more", {}).get("name", "")
        ok(f"and its name lists them: {name!r}", all(g in name for g in state["moved"]))

        banner("D — a moved control is reachable, and does what it always did")
        # ★ R1990 — an assertion, where this was `if "export" in moved` and the
        # whole of D was skipped when it was not. A conditional that stops
        # running reports nothing, and the seat this presses is the one the
        # screen says it moved: `moved_seats` is on the wire for exactly that.
        moved_seat = "lab.toolbar.config"
        ok(
            f"the screen says it moved {moved_seat}, so D has something to press",
            moved_seat in state["moved_seats"],
        )
        if moved_seat:
            ok("the moved seat is not on the row", moved_seat not in painted)
            before = json.loads(lab.query(f"{EXT}/produced"))
            assert_eq(before["config"], None, "nothing exported yet")
            more = painted["lab.toolbar.more"]
            lab.click(at=(more[0] + more[2] // 2, more[1] + more[3] // 2))
            lab.tick_ms(16)
            opened = abs_rects_of(lab.snapshot(source="paint", viewport=SHIPPED))
            ok(
                "★★★★★ opening the overflow puts it on screen, keeping its own tag",
                moved_seat in opened,
            )
            seat = opened[moved_seat]
            lab.click(at=(seat[0] + seat[2] // 2, seat[1] + seat[3] // 2))
            lab.tick_ms(16)
            after = json.loads(lab.query(f"{EXT}/produced"))
            ok(
                "★★ and pressing it exports the configuration — it moved, it did "
                "not become something else",
                after["config"] is not None,
            )

            banner("E — closing puts it back")
            painted = abs_rects_of(lab.snapshot(source="paint", viewport=SHIPPED))
            more = painted["lab.toolbar.more"]
            lab.click(at=(more[0] + more[2] // 2, more[1] + more[3] // 2))
            lab.tick_ms(16)
            closed = abs_rects_of(lab.snapshot(source="paint", viewport=SHIPPED))
            ok("the moved seat is off screen again", moved_seat not in closed)
            assert_eq(
                lab.query(f"{EXT}/toolbar_overflow")["moved"],
                state["moved"],
                "and the row is unchanged — opening a menu is not a resize",
            )

    print(f"\n=== {len(CHECKS)} named check(s) ===")
    for line in CHECKS:
        print(f"  - {line}")


if __name__ == "__main__":
    sys.exit(run_demo("R1791 a row says what it moved", body))
