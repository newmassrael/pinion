#!/usr/bin/env python3
"""R1701 §5.16 §5.49 §5.35 — **a title bar takes two clicks**, driven through a
real window.

# What this exists for

A person asked whether double-clicking a window should not toggle it between its
size and its maximum. It should, and nothing here did. Measured before anything
was changed, with a working positive control:

  * the dashboard's card header — pressing its maximize CONTROL took `maximized`
    from `''` to `'decode#1'` and back, so the witness moves; double-clicking the
    header's grip left it at `''`;
  * the client-side window chrome — its tag SSOT maps the grip to a move and
    nothing else, and the shell consumes a chrome press and returns before the
    widget router runs, so the router's double-click detector never sees a title
    bar at all.

The floor does it, built and run offscreen at 6.11 rather than read about: an
in-application sub-window's title-bar double-click takes it from 300x200 to its
parent's full 900x600, and a docking panel's takes it from docked to floating. A
frameless top-level is left entirely to its application there, with no member
that maps the gesture — and a client-side chrome IS that case, so this framework
is where the application lives.

The behaviour reference settles neither way: it is a browser prototype with no
window chrome, and its 194,828 bytes of application script contain zero
double-click handlers. So this is floor parity, stated as floor parity.

# What it asserts

* **A** — the positive control: the header's maximize button moves the witness.
  Without it the rest measures nothing, and the FIRST draft of this measurement
  measured nothing — it watched the chrome's painted glyphs, which are paths
  rather than text runs, and read "nothing happened" off an empty list.
* **B** — ★ a double-click on the header toggles maximize.
* **C** — ★★ and it carries NOTHING ELSE. A grip press opens a board drag, so
  before the repair the trailing release committed a move aimed at the board
  that existed before the card grew: "Decode Inspector moved, displacing Message
  Stream, Identifier Map, Search & Filter", and a second double-click never came
  back to the arrangement the screen opened with. The assertion is equality of
  the published layout.
* **D** — a single click on a header changes nothing and SAYS nothing. R1697
  wrote that rule and built it for a detached panel; the arm beside it, for a
  card on the board, told the same lie.
* **E** — every card's header, not just the one the round was written against.

Run from the workspace root:
    cargo build -p hello-analyzer-shell --release
    python3 tools/demos/r1701_a_title_bar_takes_two_clicks.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import RpcSubprocess, abs_rects_of, assert_eq, run_demo  # noqa: E402

EXT = "/external"
CHECKS: list[str] = []


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def ok(what: str, condition: bool) -> None:
    CHECKS.append(what)
    assert condition, what


def state(app: RpcSubprocess) -> dict:
    return {
        "maximized": app.query(f"{EXT}/maximized"),
        "layout": app.query(f"{EXT}/layout"),
        "toast": app.query(f"{EXT}/toast"),
        "drag": app.query(f"{EXT}/drag"),
    }


def aim(app: RpcSubprocess, tag: str) -> tuple[float, float]:
    """The middle of `tag`, read out of the PAINT each time it is needed.

    Each time, because a card that has just maximised is not where it was: an
    aim reused across a state change is an aim at the previous screen.
    """
    x, y, w, h = abs_rects_of(app.snapshot(source="paint"))[tag]
    return (x + w / 2, y + h / 2)


def body() -> None:
    with RpcSubprocess("hello-analyzer-shell") as app:
        rects = abs_rects_of(app.snapshot(source="paint"))
        cards = sorted(t for t in rects if t.startswith("card.") and t.count(".") == 1)
        ok("the board opens with cards to press", len(cards) >= 2)
        card = cards[0]
        grip, control = f"{card}.grip", f"{card}.maximize"
        opened = state(app)

        banner("A — the positive control: the header BUTTON moves the witness")
        app.click(aim(app, control))
        app.tick_ms(16)
        assert_eq(
            app.query(f"{EXT}/maximized") != "",
            True,
            "A: pressing the maximize control maximises the card",
        )
        app.click(aim(app, control))
        app.tick_ms(16)
        assert_eq(app.query(f"{EXT}/maximized"), "", "A: and pressing it again restores")
        assert_eq(
            app.query(f"{EXT}/layout"),
            opened["layout"],
            "A: the control's round trip leaves the board where it was",
        )

        banner("B — ★ two clicks on the TITLE BAR do the same thing")
        app.double_click(aim(app, grip))
        app.tick_ms(16)
        grown = state(app)
        assert_eq(
            grown["maximized"] != "",
            True,
            "B: a double-click on the card's header maximises it",
        )
        assert_eq(
            grown["layout"] != opened["layout"],
            True,
            "B: and the board is a different arrangement, or nothing was proven",
        )
        # ★ The board holds ONE tile while a card is maximised, which is what
        # "the whole board" means and is a different claim from "the layout
        # changed".
        assert_eq(
            grown["layout"].count('"id"'),
            1,
            "B: the maximised card is the only tile on the board",
        )
        # ★★ And the SENTENCE. This is the lie the round removed: before it, a
        # double-click reported a MOVE, because the trailing release committed
        # one. Asserting the wording is what makes that irreversible.
        assert_eq(
            grown["toast"].endswith("maximised"),
            True,
            f"B: ★★ and it says it maximised, not that it moved — {grown['toast']!r}",
        )

        banner("C — ★★ and it carries nothing else")
        app.double_click(aim(app, grip))
        app.tick_ms(16)
        back = state(app)
        assert_eq(back["maximized"], "", "C: a second double-click restores it")
        assert_eq(
            back["layout"],
            opened["layout"],
            "C: ★★ and the board is EXACTLY the arrangement it opened with — the "
            "trailing release of a double-click commits no move",
        )
        assert_eq(back["drag"], "", "C: and leaves no gesture in flight")
        assert_eq(
            back["toast"].endswith("restored"),
            True,
            f"C: and says it restored — {back['toast']!r}",
        )
        assert_eq(
            back["layout"].count('"id"'),
            opened["layout"].count('"id"'),
            "C: with every card back on the board",
        )

        banner("D — a click that carried nothing says nothing")
        said = app.query(f"{EXT}/toast")
        app.click(aim(app, grip))
        app.tick_ms(16)
        assert_eq(
            app.query(f"{EXT}/layout"),
            opened["layout"],
            "D: a single click on a header leaves the board alone",
        )
        assert_eq(
            app.query(f"{EXT}/toast"),
            said,
            "D: ★ and says nothing, because there is nothing to say",
        )

        banner("F — the negative controls: not everything is a title bar")
        # ★ The floor's answer for a sub-window's BODY is that the content owns
        # the gesture, and this screen's answer is the same: nothing. Without
        # these two the round would read as "a double-click anywhere maximises",
        # which is a different and worse screen.
        for elsewhere, what in (
            (f"{card}.body", "a card's body"),
            ("shell.rail", "the navigation rail"),
        ):
            painted = abs_rects_of(app.snapshot(source="paint"))
            if elsewhere not in painted:
                continue
            before = state(app)
            app.double_click(aim(app, elsewhere))
            app.tick_ms(16)
            assert_eq(
                app.query(f"{EXT}/maximized"),
                before["maximized"],
                f"F: double-clicking {what} maximises nothing",
            )
            assert_eq(
                app.query(f"{EXT}/layout"),
                before["layout"],
                "F: and leaves the board alone",
            )

        banner("E — every card's header answers, not just the one")
        for other in cards[1:]:
            app.double_click(aim(app, f"{other}.grip"))
            app.tick_ms(16)
            assert_eq(
                app.query(f"{EXT}/maximized") != "",
                True,
                f"E: {other} maximises from its header too",
            )
            app.double_click(aim(app, f"{other}.grip"))
            app.tick_ms(16)
            assert_eq(app.query(f"{EXT}/maximized"), "", f"E: and {other} restores")
            assert_eq(
                app.query(f"{EXT}/layout"),
                opened["layout"],
                f"E: and {other}'s round trip leaves the board where it was",
            )
        ok("every card on the board was driven", len(cards) >= 2)
        print(f"\n[demo] {len(cards)} card header(s) driven, {len(CHECKS)} named check(s)")


run_demo("R1701 a title bar takes two clicks", body)
