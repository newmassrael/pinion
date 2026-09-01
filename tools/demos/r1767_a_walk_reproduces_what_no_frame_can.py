#!/usr/bin/env python3
"""R1767 §5.27 §5.40 §2 #7 — **a walk reproduces a specification no single frame
can, and the application publishes the walk.**

# What this demo exists for

By R1763 every clause of `ApplicationConformance::conforms` was finally honest.
Together they had also become **unreachable by construction**, and this demo
measures that before it measures anything else:

```text
one frame paints one section
=> every other section is away
=> an away surface reconciles nothing (R1742)
=> an application with two open sections can never report conformance
```

Measured on this application at the head of this round, walking all six open
sections and coming back: headline `26 of 133`, `conforms=false` — the boot
number, which is R1763's repair working exactly as written.

★★★★★ **And the half the debt did not know about.** Standing *inside* the node
lab, its verdict still does not reconcile, and not because the screen is wrong:
that section's specification names an enumeration row **with its roster shut**
and the roster **standing over it**, and those two states exclude each other.
The lab's own judge wrote it down in prose at R1742 — *this document cannot be
fully judged at any one instant; a reader who wants the whole verdict drives the
session and reads twice* — and nothing anywhere could hold the two readings, so
"reads twice" meant *a person compares two printouts*.

So the missing vocabulary was never merely per-section. It is per **surface**,
and the unit that carries it is the walk.

What this drives:

* **A** — the per-frame verdict is structurally unreachable, and still is. This
  round does not repair it, because it is not broken.
* **B** — `/external/journey` is the same population as `/external/sections`,
  taken from the roster rather than from a list, so a section is missing from it
  only by not being in the application.
* **C** — the walk accumulates: every open section stood in, every verdict from
  a painted frame, every one naming the step it was read at.
* **D** — ★ the headline: the lab's two alternatives are **both** credited, at
  **different steps**, while no frame ever had both. That is the sentence no
  per-frame report can say.
* **E** — nothing is credited without a frame: a section the walk has not
  reached is in the denominator and out of the numerator.
* **F** — the two reports stay two claims. Reading the walk does not change it,
  and the frame's verdict is untouched by it.
* **G** — what is left, named. The application still does not conform, and the
  difference is the round: it fails at **one named surface at a named step**
  instead of failing structurally.

# Floor

Measured against the reference toolkit 6.11.1 at R1738 and R1758: across its
page-stack container, its tabbed container and a plain page, **312** members
were scanned and **0** name a specification, an expectation or a divergence, so
the per-frame question cannot be asked there at all — let alone accumulated over
a walk. The nearest thing that toolkit has is a page history, which records
where you went and nothing about what was there.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:97 python3 tools/demos/r1767_a_walk_reproduces_what_no_frame_can.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from analyzer_spec import open_keys  # noqa: E402
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    resize_and_settle,
    run_demo,
)

SHELL = "hello-analyzer-shell"
EXT = "/external"
LAB = "/node_lab/external"

#: The order this demo walks the sections in — the dashboard last, because the
#: checks after the walk expect to be standing on it.
#:
#: ★★★★★ R1953 — the ORDER is written here and the POPULATION is derived. This
#: was one list of six, and R1947 and R1948 opened two more sections: the walk
#: went on visiting six, `unvisited` answered 2, and the demo failed saying two
#: open sections were not stood in — which was true, and about the demo rather
#: than the application. A roster of what exists belongs to the specification;
#: what belongs here is the sequence.
WALK_ORDER = [
    "packets",
    "keys",
    "logs",
    "lab",
    "topology",
    "sessions",
    "settings",
    "dashboard",
]
WALK = [key for key in WALK_ORDER if key in set(open_keys())]

#: ★★★★★ R1770 — the window sections A-F stand in, and it is not the one the
#: tool opens in. The node lab declares it lays out at 1625 wide and clips below
#: that; the shell keeps 52 of the window; so at 1440x900 that section is handed
#: 1388 and DECLINES to be judged. Sections A-F are about what a walk holds when
#: sections answer, so they ask where the sections can. G goes back to the
#: opening window on purpose and H maximises, which is what makes the pair a
#: measurement of one variable.
LAB_WINDOW = (1800, 900)
OPENING_WINDOW = (1440, 900)

CHECKS: list[str] = []


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def ok(what: str, condition: bool) -> None:
    CHECKS.append(what)
    assert condition, what


def eq(actual, expected, what: str) -> None:
    """An equality is a named check too, and is counted as one."""
    CHECKS.append(what)
    assert_eq(actual, expected, what)


def frame_report(app: RpcSubprocess) -> dict:
    """What the application says about the frame in front of the reader."""
    return app.query(f"{EXT}/sections")


def walk_report(app: RpcSubprocess) -> dict:
    """What the application says about the walk the reader has taken."""
    return app.query(f"{EXT}/journey")


def row(said: dict, key: str) -> dict:
    return next(r for r in said["rows"] if r["key"] == key)


def act(app: RpcSubprocess, path: str, args) -> object:
    """Invoke an action and do not come back until the window has painted it.

    The action-channel peer of `intervene_painted`: everything this demo reads
    is derived from marks, so a read that races the render reads the frame
    before the one it asked for.
    """
    before = app.frame_count()
    out = app.invoke(path, args)
    app.tick(16.0)
    app.await_paint(before)
    return out


def section_a(app: RpcSubprocess) -> dict:
    banner("A — the per-frame verdict is unreachable by construction, still")
    boot = frame_report(app)
    eq(app.query(f"{EXT}/nav"), "dashboard", "A: the tool opens here")
    ok(
        "A: at boot the application does not claim to reproduce its "
        "specification",
        boot["conforms"] is False,
    )
    judged = [r for r in boot["rows"] if r["standing"] == "judged"]
    showing = [r for r in judged if r["showing"]]
    eq(len(showing), 1, "A: exactly one section is on the frame")
    ok(
        "A: ★★ and every OTHER judged section is wholly away, so none of them "
        "reconciles -- that, and not a defect, is why no frame can conform",
        all(
            r["conformance"]["away"] == len(r["conformance"]["surfaces"])
            and r["conformance"]["reconciles"] is False
            for r in judged
            if not r["showing"]
        ),
    )
    print(
        f"  [boot frame] {boot['reproduced']} of {boot['specified']} reproduced, "
        f"conforms={boot['conforms']}"
    )
    return boot


def section_b(app: RpcSubprocess, boot_frame: dict) -> None:
    banner("B — the walk's population is the roster's, not a list")
    said = walk_report(app)
    eq(
        [r["key"] for r in said["rows"]],
        [r["key"] for r in boot_frame["rows"]],
        "B: ★★ every destination is a row here too, in the roster's order -- a "
        "section is missing from this report only by not being in the tool",
    )
    eq(said["sections"], boot_frame["sections"], "B: and the same count")
    eq(said["closed"], boot_frame["closed"], "B: closed seats agree")
    ok(
        "B: a closed destination says why, in the destination's own words",
        all(r.get("why") for r in said["rows"] if r["standing"] == "closed"),
    )
    eq(
        said["unvisited"],
        said["open"] - 1,
        "B: ★★ at boot the walk has stood in exactly one open section -- the "
        "one it opened on",
    )
    ok(
        "B: and it does not claim conformance on the strength of that one",
        said["conforms"] is False,
    )
    for key in [r["key"] for r in said["rows"] if not r["visited"]]:
        ok(
            f"B: `{key}` has no arrival step, because the walk has not been there",
            row(said, key)["arrived"] is None,
        )
        break
    print(
        f"  [boot walk] {said['stood']} of {said['surfaces']} surfaces stood, "
        f"{said['unvisited']} unvisited, conforms={said['conforms']}"
    )


def drive_the_lab(app: RpcSubprocess) -> tuple[dict, dict]:
    """Stand in the lab and drive the session BOTH of its states need.

    Returns the per-frame verdict for the lab with the roster shut and with it
    open — the two frames that cannot coexist.
    """
    app.intervene_painted(f"{EXT}/nav", "lab")
    enum_key = json.loads(app.query(f"{LAB}/spec"))["enum_key"]
    act(app, f"{LAB}/select", "P-01")
    # Idempotent, because this walk is taken more than once: the screen REFUSES
    # a second `add_field` for a key the card already carries, and a demo that
    # relied on the session being fresh would be a demo that only works once.
    if enum_key not in {r["key"] for r in json.loads(app.query(f"{LAB}/form"))}:
        act(app, f"{LAB}/add_field", enum_key)
    if json.loads(app.query(f"{LAB}/picking")) is not None:
        act(app, f"{LAB}/pick", "")
    shut = row(frame_report(app), "lab")["conformance"]
    act(app, f"{LAB}/pick", enum_key)
    opened = row(frame_report(app), "lab")["conformance"]
    return shut, opened


def section_c(app: RpcSubprocess) -> tuple[dict, dict, dict]:
    banner("C — the walk accumulates what the reader saw")
    # ★ R1953 — a section the specification opens and this order does not name
    # is a section the walk would silently skip, which is the defect one level
    # up from the one that brought this here.
    eq(
        sorted(WALK),
        sorted(open_keys()),
        "C: the walk's order names every open section",
    )
    lab_frames: tuple[dict, dict] = ({}, {})
    for key in WALK:
        if key == "lab":
            # The lab is DRIVEN rather than merely arrived at, because its
            # specification names two states and one visit is one of them. That
            # driving is part of the walk, which is the point.
            lab_frames = drive_the_lab(app)
            continue
        app.intervene_painted(f"{EXT}/nav", key)
    said = walk_report(app)
    eq(said["unvisited"], 0, "C: ★★★ every open section was stood in")
    eq(said["unanswered"], 0, "C: something answered for every one of them")
    eq(
        said["declared"],
        0,
        "C: ★★ and every one answered from a PAINTED FRAME -- a verdict read "
        "from a screen's own tables is refused over a walk as over a frame",
    )
    ok(
        "C: every visited section names the step it arrived at",
        all(
            isinstance(r["arrived"], int) and r["arrived"] >= 1
            for r in said["rows"]
            if r["visited"]
        ),
    )
    ok(
        "C: ★★ and steps are readings rather than arrivals -- the reader opened "
        "something at a stop the walk had already made",
        said["steps"] > said["stops"],
    )
    print(
        f"  [walked] {said['stood']} of {said['surfaces']} surfaces stood over "
        f"{said['steps']} steps at {said['stops']} stops"
    )
    return said, lab_frames[0], lab_frames[1]


def section_d(app: RpcSubprocess, shut: dict, opened: dict) -> None:
    banner("D — ★★★★★ the two frames that cannot coexist, both held")
    ok(
        "D: with the roster shut, the enumeration ROW is on the frame",
        shut["surfaces"]["enum_row"]["standing"] is True,
    )
    ok(
        "D: and its ROSTER is not, with the section's own reason",
        shut["surfaces"]["enum_roster"]["standing"] is False
        and bool(shut["surfaces"]["enum_roster"].get("why")),
    )
    ok(
        "D: with it open the roster is on the frame",
        opened["surfaces"]["enum_roster"]["standing"] is True,
    )
    ok(
        "D: ★★ and the ROW is not -- the two are alternatives, so no frame of "
        "this application ever has both",
        opened["surfaces"]["enum_row"]["standing"] is False,
    )
    ok(
        "D: so neither frame reconciles this section",
        shut["reconciles"] is False and opened["reconciles"] is False,
    )

    said = walk_report(app)
    lab = row(said, "lab")
    enum_row = lab["surfaces"]["enum_row"]
    roster = lab["surfaces"]["enum_roster"]
    ok(
        "D: ★★★★★ the WALK holds the row -- it was on a frame this reader saw",
        enum_row["stood"] is True,
    )
    ok(
        "D: ★★★★★ and the roster too, which no instant could say with it",
        roster["stood"] is True,
    )
    ok(
        "D: ★★★★★ and they are credited to DIFFERENT steps, which is how this "
        "keeps `a verdict is about one frame` rather than relaxing it",
        enum_row["step"] != roster["step"],
    )
    ok(
        "D: ★ each step named is a reading the walk actually took",
        1 <= enum_row["step"] <= said["steps"]
        and 1 <= roster["step"] <= said["steps"],
    )
    ok(
        "D: ★★ and the row still says it is not on the frame NOW, so a credited "
        "verdict is never mistaken for a live one",
        bool(enum_row.get("why")),
    )
    print(
        f"  [lab] enum_row stood at step {enum_row['step']}, "
        f"enum_roster at step {roster['step']}, of {said['steps']}"
    )


def section_e(app: RpcSubprocess, walked: dict) -> None:
    banner("E — nothing is credited to a frame nobody saw")
    ok(
        "E: no surface is credited that was never on a frame",
        walked["stood"] <= walked["surfaces"],
    )
    ok(
        "E: ★★ and a surface that never stood reproduces nothing, while its "
        "SPECIFICATION is still counted",
        all(
            visit["reproduced"] == 0
            for r in walked["rows"]
            for visit in r.get("surfaces", {}).values()
            if visit["stood"] is False
        ),
    )
    ok(
        "E: the totals are the sum of the rows, so no row is outside them",
        walked["specified"]
        == sum(
            visit["specified"]
            for r in walked["rows"]
            for visit in r.get("surfaces", {}).values()
        ),
    )
    ok(
        "E: ★ a closed destination carries no surfaces to credit",
        all("surfaces" not in r for r in walked["rows"] if r["standing"] == "closed"),
    )


def section_f(app: RpcSubprocess, boot_frame: dict) -> None:
    banner("F — the two reports are two claims, and stay so")
    once = walk_report(app)
    twice = walk_report(app)
    eq(
        once,
        twice,
        "F: ★★ reading the walk does not change it -- a report that accumulated "
        "by being read would credit a section for being asked about",
    )
    frame = frame_report(app)
    eq(
        frame["reproduced"],
        row(frame, app.query(f"{EXT}/nav"))["conformance"]["reproduced"],
        "F: ★★ the frame's headline is still the SHOWING section's, whole -- "
        "R1763 is untouched by any of this",
    )
    ok(
        "F: and the frame still refuses to call the application conformant",
        frame["conforms"] is False,
    )
    ok(
        "F: ★★★ while the walk has seen strictly more than the frame can -- "
        "which is the whole reason it exists",
        once["reproduced"] > frame["reproduced"],
    )
    print(
        f"  [frame] {frame['reproduced']} of {frame['specified']}   "
        f"[walk] {once['reproduced']} of {once['specified']}"
    )


def section_g(app: RpcSubprocess) -> list[tuple[str, str]]:
    banner("G — what is left, named rather than structural")
    # ★★★★★ R1770 — back to the window the tool OPENS in, and walked again there
    # so what follows is about that window rather than about the wider one the
    # sections above needed. The re-walk is not a formality: since that round a
    # credited verdict is dropped when the surface is next read at a different
    # extent, so a report taken here without walking again would be crediting
    # frames painted at a size that no longer exists.
    resize_and_settle(app, OPENING_WINDOW)
    for key in WALK:
        app.intervene_painted(f"{EXT}/nav", key)
        if key == "lab":
            drive_the_lab(app)
    said = walk_report(app)
    # ★★★★★ R1770 — every surface either stood on a frame of this walk or SAID
    # WHY it could not, and the two together are the whole roster. It was a
    # plain `stood == surfaces` when this section was written, and at this
    # window that is now false: the node lab is handed 1388 of the 1625 it
    # declares it lays out at, so its three surfaces decline. The claim this
    # section exists to make is unweakened — nothing is missing from the walk,
    # and nothing is silent — and it is now the claim rather than a stronger one
    # that happened to hold at the size it was written at.
    accounted = said["stood"] + sum(
        1
        for r in said["rows"]
        for visit in r.get("surfaces", {}).values()
        if not visit["stood"] and visit.get("why")
    )
    eq(
        accounted,
        said["surfaces"],
        "G: ★★★★★ EVERY surface this application's specifications name was on "
        "some frame of this walk or says why it was not. Before this round "
        "nothing could say either, because a frame can hold at most one "
        "section's",
    )
    unreconciled = [
        (r["key"], name)
        for r in said["rows"]
        for name, visit in r.get("surfaces", {}).items()
        if visit["stood"] and not visit["reconciles"]
    ]
    eq(
        len(unreconciled),
        said["unreconciled"],
        "G: the count and the rows agree about what is unreconciled",
    )
    # ★★★★★ R1795 — the clause that stood here has been made FALSE, and making it
    # false is the outcome of R1791.
    #
    # It read: *this walk is taken at the window the tool OPENS in, and at that
    # window the node lab is handed 1388 of the 1625 it declares, so its three
    # surfaces decline to be judged ... the application still does not conform
    # here*. R1791 gave the toolbar the ability to give a group up instead of
    # demanding its whole width, the lab declares 1188, and 1388 satisfies it.
    #
    # So what is asserted is the fact that replaced it, and the SHAPE of the
    # remaining report — that a surface which does decline says why, per surface,
    # rather than the walk reporting "one frame shows one section" — is still
    # asserted below, over however many decline.
    declined = [
        (r["key"], name)
        for r in said["rows"]
        for name, visit in r.get("surfaces", {}).items()
        if not visit["stood"] and visit.get("why")
    ]
    ok(
        f"G: ★★★★★ the walk's verdict is per SURFACE and says so — "
        f"{len(unreconciled)} unreconciled, {len(declined)} declined, "
        f"conforms={said['conforms']}. Before R1791 three of the node lab's "
        "surfaces declined at this window because it was given 1388 of the 1625 "
        "it then declared; it declares 1188 now and they stand",
        isinstance(said["conforms"], bool),
    )
    ok(
        "G: ★★ and the node lab is not among the ones that decline, which is "
        f"what that round bought: {sorted(declined)}",
        not any(key == "lab" for key, _ in declined),
    )
    for key, surface in declined:
        why = row(said, key)["surfaces"][surface].get("why") or ""
        ok(
            f"G: ★ `{key}`/`{surface}` says why it declines rather than "
            "reporting a shortfall the window caused",
            why != "",
        )
    print(f"  [declined] {sorted(declined)}")
    for key, surface in unreconciled:
        visit = row(said, key)["surfaces"][surface]
        ok(
            f"G: `{key}`/`{surface}` was on the frame at step {visit['step']} "
            f"and is not what it is specified to be",
            visit["stood"] is True and visit["step"] is not None,
        )
        ok(
            f"G: ★ and it says how, part by part ({len(visit['unreconciled'])} "
            "difference(s) nobody declared)",
            len(visit["unreconciled"]) >= 1,
        )
    print(f"  [remaining] {unreconciled}")
    return unreconciled


def unreconciled_of(said: dict) -> list[tuple[str, str]]:
    return [
        (r["key"], name)
        for r in said["rows"]
        for name, visit in r.get("surfaces", {}).items()
        if visit["stood"] and not visit["reconciles"]
    ]


def section_h(app: RpcSubprocess, small: list[tuple[str, str]]) -> None:
    banner("H — ★ and the named failure is now ACTIONABLE: one variable moves it")
    # ★★★★★ R1770 — this section's original claim was that the application had
    # NO size at which it conforms, and this round is the one that made it
    # false. It stays here as the measurement it was, with its outcome corrected
    # rather than its question dropped: one variable — the window — still moves
    # what is left, and what it now moves it to is conformance.
    said_small = walk_report(app)
    declined_small = sorted(
        (r["key"], name)
        for r in said_small["rows"]
        for name, visit in r.get("surfaces", {}).items()
        if not visit["stood"] and visit.get("why")
    )
    # ★★★★★ R1795 — and the correction goes one step further than R1770's did.
    # That round corrected the OUTCOME (there is a size at which it conforms);
    # this one corrects the SUBJECT: what was outstanding at the opening window
    # was the node lab's, and R1791 removed it. So the set is asserted to be
    # whatever it is, minus the lab — the section's question ("one variable
    # moves what is left") is unchanged and its answer has moved again.
    outstanding = {key for key, _ in small} | {key for key, _ in declined_small}
    ok(
        f"H: at the window this walk was taken in, what is outstanding is "
        f"{sorted(outstanding) or 'nothing'} — and the node lab is NOT among "
        "them any more, which is what R1791 bought by letting its toolbar give "
        "a group up instead of demanding a width the shipped window cannot give",
        "lab" not in outstanding,
    )
    # One variable: the window. The node lab's OWN gate paints the inspector at
    # 2494x1531; the assembled tool gives that section a page region less than
    # half as wide. Nothing else changes.
    resize_and_settle(app, (2494, 1531))
    for key in WALK:
        app.intervene_painted(f"{EXT}/nav", key)
        if key == "lab":
            drive_the_lab(app)
    said = walk_report(app)
    big = unreconciled_of(said)
    eq(
        said["stood"],
        said["surfaces"],
        "H: every specified surface is still on some frame at this size",
    )
    ok(
        "H: ★★★★★ and the node lab's surfaces all reconcile here -- so what "
        "failed at the smaller window was the WINDOW, isolated by moving one "
        "variable, and not the screen",
        not any(key == "lab" for key, _ in big),
    )
    eq(
        big,
        [],
        "H: ★★★★★ and NOTHING is outstanding at this size. When this section was "
        "written a different section failed here instead -- the preferences "
        "ledger declared a fold that a taller window repaired, and demanded its "
        "own deletion -- so the sentence recorded was `this application has no "
        "size at which it conforms`. R1770 gave that entry the extent it was "
        "measured at, and the sentence stopped being true",
    )
    ok(
        "H: ★★★★★ so the application CONFORMS here. Read at one window it "
        "declines to be asked and at another it reproduces its whole "
        "specification, and both of those are now sentences it can say about "
        "itself",
        said["conforms"] is True,
    )
    print(f"  [small window] {small} declined {declined_small}\n  [large window] {big}")
    print(
        f"  [large] {said['reproduced']} of {said['specified']} reproduced over "
        f"{said['steps']} steps"
    )


def body() -> None:
    with RpcSubprocess(SHELL, boot_grace=1.5) as app:
        resize_and_settle(app, LAB_WINDOW)
        boot_frame = section_a(app)
        section_b(app, boot_frame)
        walked, shut, opened = section_c(app)
        section_d(app, shut, opened)
        after = walk_report(app)
        section_e(app, after)
        section_f(app, boot_frame)
        small = section_g(app)
        section_h(app, small)
        # ★ The per-frame verdict is measured LAST as well as first, because a
        # round that added a second report must be shown not to have moved the
        # first one.
        end = frame_report(app)
        ok(
            "★★★★★ and the frame's own verdict is unmoved by every one of these "
            "readings: still false, still about the section on screen",
            end["conforms"] is False,
        )
        print(f"\n  walked report: {walked['steps']} steps / {walked['stops']} stops")

    print(f"\n=== {len(CHECKS)} named check(s) ===")
    for line in CHECKS:
        print(f"  - {line}")


if __name__ == "__main__":
    run_demo("r1767 a walk reproduces what no frame can", body)
