#!/usr/bin/env python3
"""R2055 §5.11 §5.2 — **the status band greets a reader with the help, not with
a report of something they did not do.**

# What this walk exists for

Standing rule (7) asks for the analyzer UI assembled and asserted by one walk.
The host's status band has ONE slot, deliberately: a reader looks in one place,
and what is there is whatever the application most recently had to say. Its idle
occupant is the gesture sentence — the one line telling a reader what the
pointer does here — and a transient sentence takes the slot for as long as it
lives and hands it straight back.

That structure is sound. What this round measured is what was being put INTO it
at start-up.

# ★★★★★ What was wrong, and how it was found

The application said a sentence on arrival, so the slot opened occupied. The
cost was exact: the gesture help was absent from the paint AND had no
accessibility node at all for the first 2.6 seconds of EVERY session — precisely
when somebody who has just arrived would look for it, and before they had done
anything at all.

Three measurements settled it, and each alone would have been enough:

  * the behaviour reference raises a transient sentence from **46 call sites and
    every one of them is a verb a person invoked** — rename, add, delete,
    detach, redock, open the palette. None runs at start-up. A sentence there is
    the answer to something you did.
  * the sentence it said named the layout preset, which this application ALREADY
    shows permanently and announces permanently one strip up. It was a second,
    expiring account of a fact that never expires.
  * so what it bought was nothing, and what it cost was the whole of a reader's
    arrival.

⚠ This walk does NOT re-litigate WHERE a transient sentence sits. That trade-off
is settled where it is decided, and it is about placement. This is the other
axis: WHEN one is raised.

# ★★ And what the measuring got wrong twice, kept here so it is not repeated

Two things looked like defects and were not, and only isolation said so:

  * the transient region STAYS in the accessibility tree after it expires, with
    empty text and no bounds. That is correct practice, not a stale
    announcement: removing and re-adding a live region is how a future update
    stops being announced. Clause (E) pins it so a later round does not "fix" it.
  * its lifetime looked short when timed by polling, and was not. Reading a
    frame between ticks advances the pump, so the instrument was moving the
    thing it measured — three step sizes gave three answers.

    ⚠⚠ AND CLAUSE (D)'S FIRST DRAFT REPEATED THAT MISTAKE ONE LEVEL UP. It
    stopped polling and instead ticked a fixed amount and looked once, which
    fixed the three-answers problem and left a tenth of a second of margin
    against the boundary. That PASSED run alone and FAILED under the sweep,
    where the instrument's own cost crossed it. A timing test with a margin
    smaller than the cost of looking is a flake wearing a measurement's clothes.
    So (D) does not time anything now: the type PUBLISHES its life and what is
    left of it, precisely so a test does not pin a number the type owns.

# What this walk holds

  (A) ★★★★★ a reader ARRIVES to the gesture sentence — painted and announced,
      on the opening frame, with nothing transient over it.
  (B) the slot's contract is intact: a verb makes the application speak, and the
      sentence takes the slot.
  (C) and hands it straight back when its life is spent.
  (D) the life is ASKED FOR, never timed: a sentence just raised has its whole
      declared life left, and one whose life is spent has none.
  (E) ★★ the transient region persists as an empty live region throughout, so a
      later sentence is still announced.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    run_demo,
    screen_spec,
)

EXAMPLE = "hello-analyzer-shell"
EXT = "/external"

#: The slot's idle occupant — the sentence a reader arrives to.
GESTURE = "shell.status.gesture"

#: The slot's transient occupant.
SAID = "shell.toast"

CHECKS: list[str] = []


def ok(what: str, condition: bool) -> None:
    assert condition, f"FAILED: {what}"
    CHECKS.append(what)


def rects(app):
    return abs_rects_of(app.snapshot(source="paint"))


def node(app, tag):
    """The accessibility node carrying `tag`, or `None`."""
    for n in app.request("scene/access").result["nodes"]:
        if n.get("tag") == tag:
            return n
    return None


def saying(app) -> dict:
    """What the transient holder says about itself — its sentence, what is left
    of its life, and the life it was given.

    ★ Asked, never timed. The type publishes these three so a test does not pin
    a number the type owns.
    """
    said = app.query(f"{EXT}/saying")
    return json.loads(said) if isinstance(said, str) else said


def speak(app) -> None:
    """Make the application say something, changing nothing else.

    ★ A REFUSAL, deliberately. A verb that succeeded would move the screen this
    walk is asserting about; a refused one makes the application speak and
    leaves everything else where it was, so the readings either side of it are
    about the same screen.
    """
    try:
        app.invoke(f"{EXT}/title", "no.such.card,x")
    except Exception:  # noqa: BLE001 - the refusal IS the affordance here
        pass
    app.tick_ms(16)


def body() -> None:
    with RpcSubprocess(EXAMPLE) as app:
        # ── (A) what a reader arrives to ────────────────────────────────
        app.tick_ms(16)
        opening, tree = rects(app), node(app, GESTURE)
        ok(
            "A: ★★★★★ the gesture sentence is PAINTED on the opening frame — a "
            "reader who has just arrived can see what the pointer does",
            GESTURE in opening,
        )
        ok(
            "A: ★★★★★ and it is ANNOUNCED on the opening frame, so a reader who "
            "does not see the drawing arrives to it too",
            tree is not None,
        )
        ok(
            "A: ★★★ and NOTHING TRANSIENT is over it — the application does not "
            "open by reporting something the reader did not do",
            SAID not in opening,
        )
        print(f"[demo] a reader arrives to {(tree or {}).get('name')!r}")

        # ── (B) the slot's contract is intact ───────────────────────────
        speak(app)
        busy = rects(app)
        ok(
            "B: ★★ a verb makes the application speak, and the sentence takes "
            "the slot",
            SAID in busy,
        )
        ok(
            "B: ★ which is what ONE slot means — the idle occupant steps aside "
            "rather than the two sharing a line",
            GESTURE not in busy,
        )

        # ── (C) and hands it back ───────────────────────────────────────
        app.tick(3.0)
        back = rects(app)
        ok("C: ★★ the slot is handed back when the sentence's life is spent",
           GESTURE in back)
        ok("C: and the transient sentence is gone from the paint", SAID not in back)

        # ── (D) the life is ASKED FOR, never timed ──────────────────────
        #
        # ★★★★★ This clause was written twice, and the first version is the
        # lesson. It timed the life by ticking and looking: still up at 2.5s,
        # gone at 2.6s. That PASSED alone and FAILED in the sweep, because the
        # margin it left was a tenth of a second and reading a frame advances
        # the pump — so the instrument's own cost crossed the boundary under
        # load. Timed by polling at three step sizes the same lifetime had
        # already answered 2.0s, 1.6s and 1.15s: a number that is a function of
        # how often you look is not a measurement.
        #
        # The type publishes what it owns. `Saying::to_wire` carries `left` and
        # `life` for exactly this reason, in as many words — *a test that
        # guesses the duration is a test that pins a number this type owns*. So
        # this asks, and the two facts that matter are structural rather than
        # temporal: a sentence that has just been raised has its WHOLE life left,
        # and one whose life is spent has none.
        speak(app)
        raised = saying(app)
        ok(
            "D: ★★★ a sentence just raised is saying something and has time "
            "left, and never more than the life its own type declares",
            raised["said"] is not None and 0.0 < raised["left"] <= raised["life"],
        )
        # ★★★★★ And the run-down is driven by what the TYPE says is left, not by
        # a number written here. No tolerance is needed and no boundary is
        # approached: whatever has already been spent by getting here is
        # already off `left`, so ticking that much can only finish the life.
        app.tick(raised["left"])
        spent = saying(app)
        assert_eq(spent["left"], 0.0, "D: ★★ once its life is spent it has none left")
        assert_eq(spent["said"], None, "D: ★★ and it is no longer saying anything")

        # ── (E) the live region persists, empty ─────────────────────────
        said = node(app, SAID)
        ok(
            "E: ★★ the transient region is STILL in the tree after it expired — "
            "removing a live region is how the next sentence stops being "
            "announced, so persisting is correct rather than stale",
            said is not None,
        )
        assert_eq(
            (said or {}).get("value", {}).get("text"),
            "",
            "E: ★★★ and it says NOTHING, so a reader is not told about a "
            "sentence that is no longer on the screen",
        )
        ok(
            "E: ★ and it is still a live region, so the next one is announced",
            (said or {}).get("live") is not None,
        )

        # ── (F) navigating speaks, deliberately rather than by accident ──
        #
        # ★★★★★ This clause exists because of what removing the greeting made
        # INVISIBLE, which is the third question this project's closing audit
        # asks. A gate over the band's two occupancies used to OPEN by asserting
        # that a sentence was already up, and gave as its ground that navigating
        # says one. That assertion was, incidentally, the only thing anywhere
        # checking that navigation speaks at all — and it was a poor check,
        # because it also passed when the sentence in the band was the greeting,
        # which is exactly what it was getting at the destination the
        # application opens on.
        #
        # That gate now MAKES the state instead of assuming it, which is right,
        # and the accidental check went with it. So the property is asserted
        # here, on purpose and on the assembled application: going somewhere
        # tells a reader they went there.
        # The seats are the SCREEN's, not this file's: the application publishes
        # its rail, so a seat that is renamed moves this walk with it.
        #
        # ⚠ Two moves, and the assertion is about the SECOND. This application
        # publishes no path naming where it currently is, and going to the seat
        # already open is a no-op that rightly says nothing — so the first move
        # is what makes the second one certainly a change, whichever seat the
        # application happened to open on.
        # ★ Through the seat a person presses, and at the ADDRESS the screen
        # publishes for it — not a wire verb invented here. R2051 gave the rail
        # its declaration precisely so a reader of it is handed the address.
        seats = [s["tag"] for s in screen_spec(app, EXT)["rail"] if s["open"]]
        app.click(path=seats[0])
        app.tick(3.0)
        before = saying(app)
        app.click(path=seats[1])
        app.tick_ms(16)
        after = saying(app)
        ok(
            "F: ★★★ navigating says a sentence — a reader who cannot see the "
            "rail is told where they arrived",
            before["said"] is None and after["said"] is not None,
        )
        ok(
            "F: ★★ and it takes the slot, which is the same one act as any "
            "other sentence — the band has no second place to put this one",
            SAID in rects(app) and GESTURE not in rects(app),
        )

    print(f"\n[demo] {len(CHECKS)} named check(s)")
    for line in CHECKS:
        print(f"  - {line}")


if __name__ == "__main__":
    run_demo("R2055 a reader arrives to the sentence that helps them", body)
