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
  (B) the toolbar says what is on the row, what it moved, and that it fits.
  (C) the control names what it holds — the answer the floor has no member for.
  (D) a moved control is REACHABLE: open the overflow and it is there, keeping
      its own tag, and pressing it does the thing it always did.
  (E) closing puts it back, and the row is unchanged.

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

    # ── (B)-(E) the toolbar's own account, on the lab itself ────────
    with RpcSubprocess(LAB, boot_grace=1.5) as lab:
        banner("B — the toolbar says what is on the row and what it moved")
        state: Any = lab.query(f"{EXT}/toolbar_overflow")
        assert_eq(
            sorted(state),
            ["control", "moved", "moved_seats", "on_the_row", "open", "short_by"],
            "six facts: what stayed, what moved, the seats those groups hold, "
            "whether the control is drawn, whether it is open -- and `short_by`, "
            "which is what makes 'never cut' checkable",
        )
        assert_eq(state["short_by"], 0, "★★★★★ it fits — that is the round")
        ok("something had to move", len(state["moved"]) > 0)
        ok("and something stayed", len(state["on_the_row"]) > 0)
        ok("the control is drawn, because something moved", state["control"] is True)
        both = sorted(state["on_the_row"] + state["moved"])
        assert_eq(
            both,
            ["export", "file", "run", "zoom"],
            "★ the two lists are disjoint and their union is every group — "
            "nothing fell between them",
        )
        ok(
            "★★ the launch seat never moves, whatever the width",
            "run" in state["on_the_row"],
        )

        banner("C — the control names what it is holding")
        painted = abs_rects_of(lab.snapshot(source="paint", viewport=SHIPPED))
        ok("the control is painted", "lab.toolbar.more" in painted)
        nodes = lab.request("scene/access").result["nodes"]
        by_tag = {n.get("tag"): n for n in nodes if n.get("tag")}
        name = by_tag.get("lab.toolbar.more", {}).get("name", "")
        ok(f"and its name lists them: {name!r}", all(g in name for g in state["moved"]))

        banner("D — a moved control is reachable, and does what it always did")
        moved_seat = "lab.toolbar.config" if "export" in state["moved"] else None
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
