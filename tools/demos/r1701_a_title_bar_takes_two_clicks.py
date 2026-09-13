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
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    painted_address,
    painted_const,
    painted_part,
    run_demo,
)

EXT = "/external"
#: The screen whose grammar every address below is composed from.
#:
#: ★★★★★ R2223 §5.2 — this walk used to spell a card's parts: `f"{card}.grip"`,
#: `f"{card}.maximize"`, `f"{card}.body"`. The screen publishes every one of
#: them (`src/painted_grammar.tsv`, emitted from the composers themselves), so
#: a spelling here was a second copy of the screen's own composition, in another
#: language, with nothing holding the two together.
#:
#: ⚠ And the copy had already gone wrong. `card.{id}.body` is an address this
#: screen does not paint — see section F, where the negative control it named
#: had been skipping itself since R1701.
SCREEN = "hello-analyzer-shell"
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


def card_ids(app: RpcSubprocess) -> list[str]:
    """Every card the board is showing, by the id the screen addresses it with.

    The family prefix is the screen's own (`const CARD`), and a card's address
    is that prefix and nothing else after it — which is what `count` states.
    """
    prefix = painted_const(SCREEN, "CARD")
    rects = abs_rects_of(app.snapshot(source="paint"))
    return sorted(
        tag[len(prefix) :]
        for tag in rects
        if tag.startswith(prefix) and tag.count(".") == 1
    )


def body() -> None:
    with RpcSubprocess(SCREEN) as app:
        ids = card_ids(app)
        ok("the board opens with cards to press", len(ids) >= 2)
        cid = ids[0]
        card = painted_address(SCREEN, "card", id=cid)
        grip = painted_address(SCREEN, "card_grip", id=cid)
        # ★ The affordance by the word the screen publishes it under, formatted
        # into the screen's own family — not `f"{card}.maximize"`, which is this
        # walk composing the screen's address for it.
        control = painted_part(SCREEN, "maximize", id=cid)
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
        #
        # 🟥🟥🟥 ★★★★★ R2223 — **and for 522 rounds only ONE of the two ran.**
        # The card's arm named `card.{id}.body`, an address this screen paints
        # nowhere: the loop's own `if elsewhere not in painted: continue` then
        # skipped it every single run, silently, so the negative control the
        # comment above calls load-bearing was half absent. That is this
        # campaign's failure mode exactly — a spelled address names nothing and
        # the walk reads the absence as a fact about the screen — and it is why
        # the addresses here are composed from the screen's published grammar
        # now: `painted_address(SCREEN, "card_body", …)` would have raised on
        # the first run, naming the parts the screen does paint.
        #
        # What a card's body IS, said in terms the screen publishes: the card's
        # own region, aimed at its centre. The assertion below is what makes
        # that a body press rather than a hopeful one — the aim has to fall
        # BELOW the header, and the header's address is published too.
        body_aim = aim(app, card)
        grip_rect = abs_rects_of(app.snapshot(source="paint"))[grip]
        ok(
            "F: the card's centre is below its header, so aiming there is a "
            "press on the body",
            body_aim[1] > grip_rect[1] + grip_rect[3],
        )
        for elsewhere, what in (
            (card, "a card's body"),
            ("shell.rail", "the navigation rail"),
        ):
            painted = abs_rects_of(app.snapshot(source="paint"))
            assert elsewhere in painted, (
                f"F: {what} is not painted under {elsewhere!r} — a negative "
                f"control that skips itself measures nothing"
            )
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
        for other in ids[1:]:
            other_grip = painted_address(SCREEN, "card_grip", id=other)
            app.double_click(aim(app, other_grip))
            app.tick_ms(16)
            assert_eq(
                app.query(f"{EXT}/maximized") != "",
                True,
                f"E: {other} maximises from its header too",
            )
            app.double_click(aim(app, other_grip))
            app.tick_ms(16)
            assert_eq(app.query(f"{EXT}/maximized"), "", f"E: and {other} restores")
            assert_eq(
                app.query(f"{EXT}/layout"),
                opened["layout"],
                f"E: and {other}'s round trip leaves the board where it was",
            )
        ok("every card on the board was driven", len(ids) >= 2)
        print(f"\n[demo] {len(ids)} card header(s) driven, {len(CHECKS)} named check(s)")


run_demo("R1701 a title bar takes two clicks", body)
