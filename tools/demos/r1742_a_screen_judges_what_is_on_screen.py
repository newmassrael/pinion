#!/usr/bin/env python3
"""R1742 §5.27 §5.38 §5.40 §2 #2 §2 #7 — **the assembled analysis tool judges
its node lab's inspector against the written specification, in the app, through
the UI — and a surface a session has not opened says so instead of failing.**

# What this demo exists for

R1732 wrote `docs/analyzer-inspector-spec.json` — what the node inspector is
made of, in neutral vocabulary, as the behaviour reference draws it — and
compared the painted inspector against it **inside a unit test of one binary**.
R1738 then made the assembled application count the sections it was judged on
and measured that four of its six open sections published no verdict at all.
The node lab was the interesting one: the only section that *had* a written
specification and could not publish what it already knew.

Publishing it needed one decision, and it is the decision this round is named
for. The inspector's surfaces are **session-dependent**: rows exist once a card
is selected, and the roster one row collapses exists once that row is opened. A
lab nobody has touched draws none of them. Reporting `0 of 15 reproduced` there
would say a working screen is broken — the loudest kind of false, the kind that
teaches a reader to ignore the report. So a screen now answers, per surface,
either *here are its parts* or *it is not on screen, and here is why*; an away
surface counts as **0 reproduced and 0 judged**, and a report holding one does
not reconcile. Declining to be judged is not passing.

# What it drives

* **A** — the assembled application's own report: `lab` is `judged`, and the
  sections nothing has judged are **exactly** the reviewed remainder in
  `docs/analyzer-sections-spec.json`. An entry paid off must be deleted there,
  so this fails as loudly for a stale ledger as for a silent section.
* **B** — ★ arriving at the lab, the two surfaces a session has to build report
  `standing: false` with *different* sentences of their own, `reproduced` is 0
  while `specified` is unchanged, and their ledgers survive being away. The
  third one is standing and diverging, and the demo says so rather than looking
  away: it is judged against the form on screen, and the card the tool opens on
  does not hold a row of every kind.
* **C** — ★★★★★ the UI test the round is named for: drive the session **through
  the painted rectangles**, the way a hand does — press the card, wheel the
  inspector to reach the palette chip that adds the row, press it, press the
  collapsed control that opens the roster — and watch each surface become
  standing and each part be compared. Nothing here calls a model method; every
  step is a press or a wheel at a rectangle the frame drew. It is also where the
  round's sharpest measurement is: at the window the tool opens at the page it
  gives the lab is **74 pixels wide** and the row reproduces one of its seven
  parts, and after a resize the same build reproduces all seven — the verdict
  moves with the WINDOW, which is the *two builds wearing one name* risk R1738
  named and nothing could measure before.
* **C, second finding** — two of the three surfaces are **alternatives**: the
  pin specifies the row with its roster *shut*, so opening the roster takes the
  row's surface away and puts the roster's there. Asserted as a swap, and the
  section is checked to have judged every specified surface **across** the
  session — which is the honest form of "this build reproduces its
  specification" for a document whose surfaces exclude each other.
* **D** — every addressable part the specification fixes is **painted in the
  assembled application**, at the tag the specification names, with the host's
  own chrome still standing beside it — a page, not a takeover. The parts that
  are a *classification* rather than a tag are counted separately rather than
  silently skipped, because one number covering both would read as covering all
  of them.
* **E** — one build, two placements: the host's row for `lab` IS the value the
  section publishes on its own wire, and a **second process** running the same
  section standalone answers differently when freshly started and identically
  once driven into the same session.

# Floor, measured by building a probe against the 6.11.1 release and running it

The probe is a property-editor-shaped pane whose rows are created when a subject
is selected and whose enumeration roster exists only while its drop-down is
open — the same shape as the screen above.

* Across the pane, its form layout, the enumeration control, the plain text
  control, the page-stack container and the tabbed container, **564** members
  were scanned and **0** name a specification, an expectation, a divergence or a
  verdict. There is nowhere to write the statement and nothing to read.
* The count that reports what the pane **contains** answers `3` while every one
  of those three rows is off screen, and the count that reports what is
  **showing** answers `0` in that state *and* in the state where nothing was
  ever built. So a caller comparing a surface with a declared list must choose
  between a count that credits parts nobody can see and a count that cannot tell
  *not built* from *not shown*. Neither is the number this file asserts.
* The enumeration answers `count() = 3` whether its roster is shut or open — the
  roster's parts cannot be asked whether they are on screen at all; only a
  different object's visibility can, and it is not the parts.
* And the words: of three **named** regions, **2** answer one uniform call for
  what they read and **3** answer only a call that names the class first — the
  one that fails the uniform route is the enumeration, which is the surface this
  round's roster is about. Of the three row labels the layout drew — the words a
  reader sees most — **0** carry a name a caller could ask by; they are reachable
  only by index into the layout.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from analyzer_spec import unjudged_sections  # noqa: E402
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    address_prefix,
    assert_eq,
    resize_and_settle,
    run_demo,
    without_extent,
)

SHELL = "hello-analyzer-shell"
LAB = "hello-node-lab"
EXT = "/external"
SEAT = "lab"

#: ★★★★★ R1770 — the window this demo stands in, and it is no longer the one the
#: tool opens in.
#:
#: The node lab declares it lays out at 1625 wide and CLIPS below that — a policy
#: R1712-R1714 measured three times — and the shell keeps 52 of the window for
#: its own chrome. So at 1440x900 that section is handed 1388 and, since R1770,
#: declines to be judged in a sentence naming both numbers. Everything below
#: reads what the lab draws, so it reads at a window where the lab is whole.
#:
#: ⚠ This was never a choice before: the section was clipped at the opening
#: window the whole time, reporting `controls` 1 of 5 and `enum_row` 1 of 7 as
#: though the BUILD were short, and nothing in the verdict said the window was
#: what took those parts away.
LAB_WINDOW = (1800, 900)

#: The window this tool actually opens in, kept because section C measures what
#: happens there on purpose.
OPENING_WINDOW = (1440, 900)

CHECKS: list[str] = []
PARTS_COMPARED = 0
PRESSES = 0


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def ok(what: str, condition: bool) -> None:
    CHECKS.append(what)
    assert condition, what


def report(app: RpcSubprocess) -> dict:
    """The application's own count of how much of itself has been judged."""
    return app.query(f"{EXT}/sections")


def lab_row(app: RpcSubprocess) -> dict:
    row = next(r for r in report(app)["rows"] if r["key"] == SEAT)
    return row


def lab_surfaces(app: RpcSubprocess) -> dict:
    """The lab's per-surface verdict.

    ★ R1758 — one accessor rather than `["conformance"]["surfaces"]` at fifteen
    call sites. The row publishes the section's WHOLE verdict under one key now
    (what it was read from, the totals, and the surfaces), because a row that
    spread three of a verdict's facts flat is how a partial verdict comes to
    read as a complete one.
    """
    return lab_row(app)["conformance"]["surfaces"]


def reveal(app: RpcSubprocess, tag: str, over: str = "lab.inspector.body") -> None:
    """Wheel `over` until `tag` is painted, the way a hand reaches a chip below
    the fold.

    ★ Measured rather than assumed: the inspector's palette offers five chips
    on this card and the page the assembled tool gives the lab paints ONE of
    them. A demo that pressed without scrolling would be asserting about a
    window size rather than about the screen.
    """
    for _ in range(20):
        if tag in abs_rects_of(app.snapshot(source="paint")):
            return
        app.wheel(path=over, lines=(0.0, 3.0))
        app.tick(16)
    raise AssertionError(f"{tag} never came into view after 20 wheel notches")


def press_tag(app: RpcSubprocess, tag: str) -> None:
    """Press the rectangle the last frame drew for `tag`.

    ★ The rect is re-read immediately before aiming and the press has no
    movement in it: R1736 measured that a probe which moves and then presses is
    driving a DRAG, so what it measures is the drift rather than the press.
    """
    global PRESSES
    rects = abs_rects_of(app.snapshot(source="paint"))
    box = rects.get(tag)
    assert box is not None, f"the frame drew nothing at {tag}"
    x, y, w, h = box
    app.click((x + w / 2, y + h / 2))
    app.tick(16)
    PRESSES += 1


def enum_key(app: RpcSubprocess) -> str:
    """The configuration path whose roster is driven — the SCREEN's, not this
    file's."""
    import json

    return json.loads(app.query(f"/{tag_of(app)}/external/spec"))["enum_key"]


def tag_of(app: RpcSubprocess) -> str:
    return lab_row(app)["tag"]


def control_of(app: RpcSubprocess, key: str, at: str) -> str:
    """The address that row's control is painted under — the SCREEN's.

    ★ R2050 — asked rather than spelled. A walk cannot call the derivation the
    framework composes these with, so it is handed the answer; a wrong letter
    written here would aim at a mark that is not there and read as the screen
    not painting it.
    """
    import json

    # ⚠ The LIVE form, not the specification's static field list: a row a chip
    # added exists on the screen and is not in that list, and this walk drives
    # exactly such a row.
    #
    # ⚠ And `at` is the read path, because this walk drives the section BOTH
    # mounted in the host and alone in its own process — the two reach the same
    # screen by different addresses, which is the whole of section E.
    rows = json.loads(app.query(f"{at}/form"))
    return next(row["control"] for row in rows if row["key"] == key)


def section_a(app: RpcSubprocess) -> None:
    banner("A — the application judges the lab, and the remainder is an equality")
    said = report(app)
    row = lab_row(app)
    assert_eq(
        row["standing"],
        "judged",
        "A: ★★★★★ the node lab publishes a verdict about its own specification "
        "where the application it is a page of can reach it",
    )
    unjudged = {
        r["key"]: r["why"]
        for r in said["rows"]
        if r["standing"] in {"unspecified", "inline"}
    }
    assert_eq(
        sorted(unjudged),
        sorted(unjudged_sections()),
        "A: ★★★ the sections nothing has judged are exactly the ones "
        "docs/analyzer-sections-spec.json accepts -- so the entry this round "
        "paid off had to be DELETED there, and one left behind fails here",
    )
    ok(
        "A: `lab` is no longer in the reviewed remainder",
        SEAT not in unjudged_sections(),
    )
    ok(
        "A: every row says which of the four things it is",
        all(
            r["standing"] in {"judged", "unspecified", "inline", "closed"}
            for r in said["rows"]
        ),
    )
    ok(
        "A: and the judged row carries the tag its section is addressed by, so "
        "a reader of this report can go and ask the section itself",
        row["tag"] and row["conformance"]["surfaces"],
    )
    # ★★★★★ Found by re-running an OLDER gate against this round's build, which
    # is why it is asserted here: a section that derives its verdict from its
    # own paint answers about its LAST frame, and a section that is not showing
    # has not painted since. Standing at the dashboard, the lab's row is
    # therefore a true statement about a frame no longer in the application --
    # and the report now says which frame each row is about.
    assert_eq(
        app.query(f"{EXT}/nav"),
        "dashboard",
        "A: this report is being read from the dashboard",
    )
    ok(
        "A: ★★★★★ every row says whether its section was the one SHOWING, and "
        "exactly one is -- without it, a verdict about a frame that has left "
        "the application reads as a verdict about the application",
        [r["key"] for r in said["rows"] if r["showing"]] == ["dashboard"],
    )
    ok(
        "A: ★★ and the lab's own row is NOT showing here, so its numbers are "
        "about its last frame -- section B walks there before reading them",
        row["showing"] is False,
    )
    ok(
        "A: and the application still does not claim conformance, because "
        "three sections remain unjudged",
        said["conforms"] is False and said["unjudged"] == len(unjudged_sections()),
    )
    print(
        f"  [population] {said['sections']} section(s): {said['judged']} judged, "
        f"{said['unjudged']} unjudged, {said['closed']} closed"
    )


def section_b(app: RpcSubprocess) -> None:
    banner("B — an untouched lab says its surfaces are AWAY, not missing")
    # ★ R1761 — and not `intervene` + `tick`: every surface read below is a fact
    # about the last PAINTED frame, and this demo failed once in a 34-demo sweep
    # and never in 20 isolated re-runs — a read racing the render, which is
    # exactly what that shape is. `intervene_painted` returns once the window
    # has drawn the page this asks for.
    app.intervene_painted(f"{EXT}/nav", SEAT)
    assert_eq(app.query(f"{EXT}/nav"), SEAT, "B: the lab seat opens")

    row = lab_row(app)
    surfaces = row["conformance"]["surfaces"]
    ok("B: the section names every surface its specification fixes", len(surfaces) == 3)

    # ★ Measured rather than assumed: the tool opens the lab with a card
    # already selected, so the two surfaces that need a session are away and
    # the one that only needs an inspector is standing.
    for name in ("enum_row", "enum_roster"):
        said = surfaces[name]
        ok(
            f"B: ★★ `{name}` says it is not on screen rather than failing -- "
            f"{said.get('why', '<no reason>')}",
            said["standing"] is False and said["why"],
        )
        ok(
            f"B: ★★★★★ and it accuses the build of NOTHING -- an unopened "
            f"surface has no divergences and reproduces 0 of its "
            f"{said['specified']} specified part(s)",
            said["divergences"] == [] and said["reproduced"] == 0 and said["specified"] > 0,
        )
    ok(
        "B: ★★★ and what each is SPECIFIED to be does not depend on a session -- "
        "the shortfall is a fact about the session, not about the build",
        row["conformance"]["specified"]
        == sum(s["specified"] for s in surfaces.values())
        > 0,
    )

    # ⚠ And the one the round wants a reader to see with its eyes open. The
    # `controls` surface IS standing, and it diverges -- because the card the
    # tool opens on does not hold a row of every kind. The pin says in its own
    # words that this surface is read from `a form holding one row of each
    # kind`, which is a session; the away-condition that would hide it was
    # refused as an escape hatch and the caveat written down instead.
    controls = surfaces["controls"]
    ok(
        "B: ⚠ `controls` is standing and diverges, and the divergence is a fact "
        f"about the card the tool opens on: {[d['key'] for d in controls['divergences']]}",
        controls["standing"] is True and controls["divergences"],
    )
    ok(
        "B: ★★ the two away sentences are DIFFERENT -- each is the screen's own "
        "reason and not one wording reused, which is the whole value of a "
        "declared absence over silence",
        surfaces["enum_row"]["why"] != surfaces["enum_roster"]["why"],
    )
    ok(
        "B: ★ and a declared remainder survives being away -- an entry cannot "
        "be retired by never drawing the surface it is about",
        surfaces["controls"]["owed"] and surfaces["enum_row"]["owed"] == [],
    )


def section_c(app: RpcSubprocess) -> str:
    banner("C — the session builds them, driven through the painted rectangles")
    global PARTS_COMPARED
    key = enum_key(app)

    # A card, selected the way a reader selects one: press the card the canvas
    # drew. The palette chip that offers the row is only there once a card is.
    press_tag(app, "lab.node.P-01")
    assert_eq(
        app.query(f"/{tag_of(app)}{EXT}/selected"),
        "P-01",
        "C: a press on the card the canvas drew selects THAT card",
    )

    surfaces = lab_surfaces(app)
    assert_eq(
        surfaces["controls"]["standing"],
        True,
        "C: ★★ selecting a card puts the value controls on screen, and the "
        "verdict changes with the session because it is ABOUT the session",
    )
    ok(
        "C: the enumeration row is still away -- nobody has added it",
        surfaces["enum_row"]["standing"] is False,
    )

    # The row: reach the palette chip that offers this configuration path — it
    # is below the fold on the page the tool gives the lab — and press it.
    reveal(app, f"lab.form.add.{key}")
    press_tag(app, f"lab.form.add.{key}")
    surfaces = lab_surfaces(app)
    ok(
        f"C: ★★ pressing the chip that offers `{key}` puts the specified row "
        f"on screen",
        surfaces["enum_row"]["standing"] is True,
    )
    ok(
        "C: and the roster it collapses is still shut, which is a THIRD state "
        "-- away, standing-and-shut, standing-and-open are not two things",
        surfaces["enum_roster"]["standing"] is False,
    )

    # 🟥★★★★★ MEASURED HERE, AND IT WAS THE ROUND'S SHARPEST FINDING — now
    # stated the way R1770 made sayable. At the window the tool OPENS at, the
    # page it gives the lab leaves the inspector a fraction of its width, and
    # this demo used to report that as the row reproducing 3 of its 7 parts:
    # "two builds wearing one name", inferred from a SHORTFALL.
    #
    # R1770 measured what the shortfall was — the section is handed 1388 of the
    # 1625 it declares it lays out at, and below that width it clips by
    # declaration — so the section now DECLINES to be judged there and says so
    # in a sentence carrying both numbers. That is the same finding with the
    # inference taken out of it, and it is strictly better: a build that had
    # genuinely stopped drawing those parts would have been indistinguishable
    # from a clipped one under the old reading, and is not under this one.
    # ★★★★★ R1791 — **the clause that stood here has been made false, and making
    # it false is what the reader asked for.** It read: *at the opening window
    # the inspector is 310px wide and the row DECLINES rather than reporting a
    # shortfall the window caused*, with 1388 and 1625 in the sentence. That was
    # the honest report of a screen that did not fit; the reader who saw the
    # window asked whether it should not be impossible to cut it in any
    # situation. The lab's toolbar now gives a group up instead of demanding its
    # whole width, so it declares 1188, the page's 1388 satisfies it, and the
    # inspector is drawn at its full 312.
    #
    # What is asserted instead is the fact that replaced it: at the window the
    # tool OPENS at, the row is judged rather than declining.
    resize_and_settle(app, OPENING_WINDOW)
    app.tick(16)
    cramped = lab_surfaces(app)["enum_row"]
    body = abs_rects_of(app.snapshot(source="paint")).get("lab.inspector.body")
    ok(
        f"C: ★★★★★ at the opening window the inspector is {body[2]}px wide and "
        f"the row is JUDGED rather than declining — the shortfall a reader "
        f"reported is gone: {cramped.get('why') or 'no reason to give'}",
        cramped["standing"] is True and not cramped.get("why"),
    )

    # ★ And at a window where the page is big enough to draw the section, the
    # same build reproduces it. The verdict moved with the WINDOW, which is
    # what makes the sentence above a fact about the page and not about the
    # screen.
    resize_and_settle(app, (2560, 1600))
    app.tick(16)
    shut = lab_surfaces(app)
    for name in ("enum_row", "controls"):
        said = shut[name]
        ok(
            f"C: ★★★ `{name}` reproduces {said['reproduced']} of "
            f"{said['specified']} specified part(s), read from the paint "
            f"-- unreconciled: {said['unreconciled'] or 'none'}",
            said["standing"] is True and said["unreconciled"] == [],
        )
        PARTS_COMPARED += said["specified"]

    # The roster: press the collapsed control.
    control = control_of(app, key, f"/{tag_of(app)}{EXT}")
    reveal(app, control)
    press_tag(app, control)
    opened = lab_surfaces(app)
    said = opened["enum_roster"]
    ok(
        f"C: ★★★ pressing the control opens the roster, and it reproduces "
        f"{said['reproduced']} of {said['specified']} specified word(s) in the "
        f"order the configuration offers them "
        f"-- unreconciled: {said['unreconciled'] or 'none'}",
        said["standing"] is True and said["unreconciled"] == [],
    )
    PARTS_COMPARED += said["specified"]

    # ★★★★★ The finding this section is really for, measured rather than
    # designed: the pin specifies the row WITH ITS ROSTER SHUT, so opening the
    # roster takes the row's surface away and puts the roster's there.
    ok(
        "C: ★★★★★ and the row is now AWAY -- two of the three surfaces are "
        "ALTERNATIVES, so this document cannot be fully judged at any one "
        f"instant: {opened['enum_row'].get('why')}",
        opened["enum_row"]["standing"] is False
        and "shut" in opened["enum_row"].get("why", ""),
    )
    ok(
        "C: ★★ which is a swap and not a loss -- the same number of surfaces "
        "stands in both states",
        sum(1 for s in shut.values() if s["standing"])
        == sum(1 for s in opened.values() if s["standing"])
        == 2,
    )
    judged = {n for n, s in shut.items() if s["standing"]} | {
        n for n, s in opened.items() if s["standing"]
    }
    assert_eq(
        sorted(judged),
        sorted(shut),
        "C: ★★★★★ and across the session EVERY surface the specification "
        "declares was judged -- which is the honest form of 'this build "
        "reproduces its specification' for a document whose surfaces exclude "
        "each other",
    )
    print(f"  [coverage] {PARTS_COMPARED} specified part(s) judged, {PRESSES} press(es)")
    return key


def section_d(app: RpcSubprocess, key: str) -> None:
    banner("D — every specified part is PAINTED inside the assembled application")
    row = lab_row(app)
    rects = abs_rects_of(app.snapshot(source="paint"))
    tag = row["tag"]

    # The host is still there. A page, not a takeover.
    # ★ R2051 — the address, recovered from one the application publishes.
    seat_tag = address_prefix(app.query(f"{EXT}/spec")["rail"])
    for chrome in ("shell.appbar", "shell.rail", f"{seat_tag}{SEAT}"):
        ok(f"D: the host's {chrome} survives the lab being on it", chrome in rects)
    ok("D: and the section is addressed by the tag the report names", tag == "node_lab")

    import json

    published = json.loads(app.query(f"/{tag}{EXT}/spec"))["inspector"]
    assert_eq(
        sorted(published),
        sorted(row["conformance"]["surfaces"]),
        "D: the surfaces the section publishes a SPECIFICATION for are the ones "
        "it publishes a VERDICT for -- one document, two readings",
    )
    missing: list[str] = []
    compared = 0
    classified = 0
    for surface, said in published.items():
        for part in said["canon"]:
            if surface == "enum_row":
                wanted = f"lab.form.{part['key']}.{key}"
            elif surface == "enum_roster":
                wanted = f"lab.form.option.{key}.{part['key']}"
            else:
                # The control kinds are not one tag each -- they are a
                # classification OF the rows, so the row that carries each kind
                # is what the paint has. Judged by the report above; here only
                # the row family is checked to be on the frame at all.
                wanted = None
            if wanted is None:
                classified += 1
                continue
            compared += 1
            if wanted not in rects:
                missing.append(f"{surface}.{part['key']} ({wanted})")
    ok(
        f"D: ★★★ every one of the {compared} addressable parts the inspector "
        f"specification fixes is painted in the assembled application "
        f"-- missing: {missing or 'none'}",
        not missing and compared > 0,
    )
    ok(
        f"D: ★ and the {classified} part(s) that are a CLASSIFICATION rather "
        f"than a tag are counted here rather than silently skipped -- one "
        f"number covering both would read as covering all of them",
        classified + compared == sum(len(s["canon"]) for s in published.values()),
    )
    print(
        f"  [paint] {compared} specified part(s) read from the assembled frame, "
        f"{classified} judged by classification in the report"
    )


def section_e(app: RpcSubprocess, key: str) -> None:
    banner("E — one build, two placements, one sentence")
    row = lab_row(app)
    own = app.query(f"/{row['tag']}{EXT}/conformance")
    assert_eq(
        own,
        row["conformance"],
        "E: ★★ the host's row for `lab` IS the value the section publishes on "
        "its own wire -- the host aggregates, it does not re-derive",
    )
    assert_eq(
        row["conformance"]["specified"],
        sum(s["specified"] for s in own["surfaces"].values()),
        "E: and the row's totals are its surfaces added up",
    )
    # ★★★★★ R1758 — and the verdict names what it was READ FROM. A count with
    # no qualifier let two sections of this tool report every part reproduced
    # from a page they had not painted.
    assert_eq(
        own["evidence"],
        "paint",
        "E: ★★★★★ and it says the verdict is about a painted frame",
    )

    ok(
        "E: ★ and the section's own wire is reachable because the journey is "
        "standing in it -- which is why the host answering for every section "
        "from anywhere is what makes an application-wide verdict possible",
        app.query(f"{EXT}/nav") == SEAT
        and set(own["surfaces"]) == set(row["conformance"]["surfaces"]),
    )

    with RpcSubprocess(LAB, boot_grace=1.5) as alone:
        fresh = alone.query(f"{EXT}/conformance")
        ok(
            "E: ★★★★★ freshly started, the same section says its two "
            "session-dependent surfaces are AWAY -- the verdict is about a "
            "session, so two processes in different sessions must differ",
            fresh["surfaces"]["enum_row"]["standing"] is False
            and fresh["surfaces"]["enum_roster"]["standing"] is False
            and without_extent(fresh) != without_extent(own),
        )
        # The SAME gestures, over the same painted rectangles, in a process
        # that has never seen the shell.
        press_tag(alone, "lab.node.P-01")
        reveal(alone, f"lab.form.add.{key}")
        press_tag(alone, f"lab.form.add.{key}")
        alone_control = control_of(alone, key, EXT)
        reveal(alone, alone_control)
        press_tag(alone, alone_control)
        # ★ R1770 — apart from the size each was read at; the two are in
        # different windows and the round that introduced the extent asserts
        # that difference separately rather than folding it in here.
        assert_eq(
            without_extent(alone.query(f"{EXT}/conformance")),
            without_extent(own),
            "E: ★★★★★ and driven into the SAME session it publishes the SAME "
            "verdict -- a section that conforms in its own window and is never "
            "asked as a page would be two builds wearing one name",
        )


def body() -> None:
    accepted = unjudged_sections()
    # ★ R1762 — the non-emptiness guard came off for the reason it was there:
    # the remainder is empty now, and what makes this non-vacuous is section A's
    # equality against the application's own unjudged rows.
    ok(
        "the reviewed remainder names a seat and a sentence for every section "
        "it still accepts as unjudged",
        all(k and s for k, s in accepted.items()),
    )

    with RpcSubprocess(SHELL, boot_grace=1.5) as app:
        resize_and_settle(app, LAB_WINDOW)
        section_a(app)
        section_b(app)
        key = section_c(app)
        section_d(app, key)
        section_e(app, key)

    print(f"\n=== {len(CHECKS)} named check(s) ===")
    for line in CHECKS:
        print(f"  - {line}")


if __name__ == "__main__":
    run_demo("r1742 a screen judges what is on screen", body)
