#!/usr/bin/env python3
"""R1732 §5.21 §5.38 §5.40 §2 #2 §2 #7 — **an enumeration row collapses into a
roster you open, the way the behaviour reference draws it.**

# What this demo exists for

The node inspector is the analysis tool's settings editor, and the reference
gives a field whose value is one of a fixed set a **closed** control: the word it
holds, an arrow, and a roster that appears when you open it — styled exactly
like the text field beside it, so the two read as the same kind of thing.

This build drew every option side by side instead, always. Measured before the
change with the inspector's own measurements: a three-word roster spent 229px of
a 284px pane saying one word, a six-word roster ran **50px past its own
control** and a seven-word one **113px**, with no wrap, no clip and no scroll —
the rectangles were simply laid outside the box.

So the claim this demo states is one about pixels and about presses:
`docs/analyzer-inspector-spec.json` says what a row of this kind is made of, and
the running application is that, in the paint, under a real pointer.

What this drives:

* **A** — the three surfaces the specification fixes, compared with the file
  read here from the repository. Both sides of every comparison come from
  different places, or the application is agreeing with itself.
* **B** — the roster on the wire: what the row will take, where the reader is in
  it, and the fact that **moving is not writing**.
* **C** — the machine's own pointer: press the control, press an option, and the
  document holds the word that was under the finger.
* **D** — the keyboard, which the reference does not have at all (measured: zero
  key handlers in the whole prototype). This is the second pass over what the
  first left pointer-only, and it is an ADDITION — section C is unchanged.
* **E** — what a reader is told: a combo box that says whether it is open and
  names the roster it controls, and options that say which one the document
  holds.

# Floor, measured by building a probe against 6.11.1 and running it

Its collapsed chooser, driven with real key events under the offscreen platform:

* **nothing answers what committing right now would choose.** Of 123 members,
  exactly two name the highlight and BOTH ARE SIGNALS — an event you had to be
  listening for, never a value you can ask for.
* a check written the natural way **passes while the reader is looking at
  another row**: asserted the committed index while the open roster showed the
  fifth, and it held.
* a roster with nothing in it is **accepted** — count 0, index −1, empty text,
  no complaint.
* a word the roster does not offer is **silently ignored**: the call returns
  void, emits no signal, and the control goes on showing something else. An
  index past the end is accepted and clears the control to nothing.
* a letter on the closed control **commits** — the value moved from the first
  word to the fifth without the roster ever being shown — and typing it again
  over three words starting with it never leaves the second.
* arrows on the closed control commit **one document write per press** and do
  not wrap at the end.

Every one of those is a row in the table this round's picker was written
against, and sections B and D are where the difference is driven.

Run from the workspace root (a real pointer needs a display):
    cargo build --release -p hello-node-lab
    DISPLAY=:97 python3 tools/demos/r1732_an_enum_row_is_a_roster_that_collapses.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from analyzer_spec import DOCS, surfaces  # noqa: E402
from rpc_verify import (  # noqa: E402
    RealPointer,
    RealPointerUnavailable,
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    run_demo,
)

LAB = "hello-node-lab"
EXT = "/external"
INSPECTOR_SPEC_PATH = DOCS / "analyzer-inspector-spec.json"

CHECKS: list[str] = []
REAL_POINTER_RUNS = 0


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def ok(what: str, condition: bool) -> None:
    CHECKS.append(what)
    assert condition, what


def q(app: RpcSubprocess, path: str):
    return app.query(f"{EXT}/{path}")


def qj(app: RpcSubprocess, path: str):
    """A read this screen answers as JSON TEXT — its whole table surface is."""
    return json.loads(q(app, path))


def control_of(app: RpcSubprocess, key: str) -> str:
    """The address that row's control is painted under — the SCREEN's.

    ★ R2050 — asked rather than spelled: the framework composes these from the
    form's prefix and the row's key, a walk cannot call that, and a wrong letter
    here would aim at a mark that is not there.
    """
    return next(row["control"] for row in qj(app, "form") if row["key"] == key)


def inspector_spec() -> dict:
    """The reviewed artifact, read from the repository rather than from the app."""
    return json.loads(INSPECTOR_SPEC_PATH.read_text(encoding="utf-8"))


def pointer(app: RpcSubprocess):
    global REAL_POINTER_RUNS
    try:
        driver = RealPointer(app)
    except RealPointerUnavailable as exc:
        print(f"[real-pointer] UNAVAILABLE — section C is not driven: {exc}")
        return None
    REAL_POINTER_RUNS += 1
    return driver


def enum_key(app: RpcSubprocess) -> str:
    """The path whose roster is driven — the SCREEN's, not this file's."""
    return qj(app, "spec")["enum_key"]


def open_the_row(app: RpcSubprocess) -> str:
    """Select a card and give it the enumeration row, the way a session does."""
    app.invoke(f"{EXT}/select", "P-01")
    key = enum_key(app)
    app.invoke(f"{EXT}/add_field", key)
    app.tick(8)
    rows = {row["key"]: row for row in qj(app, "form")}
    ok(f"the palette's chip put {key} on the card", key in rows)
    return key


def section_a(app: RpcSubprocess, spec: dict, key: str) -> None:
    banner("A — the inspector's specification, published and reproduced")
    published = qj(app, "spec")["inspector"]
    ok(
        "A: the application publishes every surface the file fixes",
        sorted(published) == surfaces(spec),
    )
    for surface in surfaces(spec):
        assert_eq(
            [p["key"] for p in published[surface]["canon"]],
            [p["key"] for p in spec[surface]["canon"]],
            f"A: the {surface} surface's parts, in the specified order",
        )
        assert_eq(
            [p["title"] for p in published[surface]["canon"]],
            [p["title"] for p in spec[surface]["canon"]],
            f"A: and what each part is called",
        )
        assert_eq(
            [e["says"] for e in published[surface]["owed"]],
            [e["sentence"] for e in spec[surface]["owed"]],
            f"A: ★★ the {surface} surface's declared remainder is EXACTLY the "
            "file's -- so an entry quietly paid off or quietly added fails here",
        )

    # ★ The entries that run the OTHER way: parts this build draws differently
    # from the reference, or does not draw at all. The order rule keeps them;
    # the ledger records them.
    #
    # ★★★★★ R1850 — FOUR, where this said two, and the two R1842 added run the
    # opposite way from the first two. `int` and `list` are parts this build
    # has and the reference draws plainly; `perm` and `derived` are parts the
    # REFERENCE has and this build does not, because R1842 found the single
    # set-valued row was keyed at a path the target has no leaf for and
    # exported an array it refuses. Both directions belong in one ledger — a
    # remainder is a difference from the canon, whichever side is short.
    #
    # ⚠ And the list is read off the published ledger rather than counted:
    # this said "the two", which is a number in prose, and it became wrong the
    # moment a round recorded a third and a fourth.
    kept = [e["key"] for e in published["controls"]["owed"]]
    ok(
        "A: ★★★ every second-pass control is RECORDED rather than deleted -- "
        "the reference draws a whole number and an array in the plain text box "
        "where this build gives them a clamping stepper and per-element rows, "
        "and it draws controls for a set of named booleans and for a read-out "
        "that this build does not draw at all",
        sorted(kept) == ["derived", "int", "list", "perm"],
    )
    for entry in published["controls"]["owed"]:
        ok(
            f"A: and the {entry['key']} entry says which round accepted it and why",
            # ⚠ A ROUND, not R1732: this demanded the round that wrote the first
            # two, so the ledger could not take an entry from any later one
            # without failing here for the wrong reason.
            entry["since"].startswith("R") and len(entry["why"]) > 80,
        )

    rects = abs_rects_of(app.snapshot(source="paint"))
    for part in spec["enum_row"]["canon"]:
        tag = f"lab.form.{part['key']}.{key}"
        ok(f"A: the row's {part['key']} is painted at {tag}", tag in rects)
    # ★ And the collapsed control holds them: the arrow and the word are INSIDE
    # the box, which is the property a chip row could not have at any length.
    box = rects[control_of(app, key)]
    for inner in ("shown", "pick"):
        r = rects[f"lab.form.{inner}.{key}"]
        ok(
            f"A: ★★ the {inner} lies inside the control's own box",
            r[0] >= box[0] and r[0] + r[2] <= box[0] + box[2],
        )


def section_b(app: RpcSubprocess, spec: dict, key: str) -> None:
    banner("B — the roster on the wire, and moving is not writing")
    row = next(r for r in qj(app, "form") if r["key"] == key)
    words = [p["key"] for p in spec["enum_roster"]["canon"]]
    assert_eq(
        row["options"],
        words,
        "B: ★★★★★ the row publishes THE WORDS IT WILL TAKE -- an agent could "
        "read the value and had no way at all to learn what else was allowed",
    )
    ok(
        "B: and a row with no roster says `null` rather than omitting the key",
        next(r for r in qj(app, "form") if r["key"] == "id")["options"] is None,
    )

    assert_eq(qj(app, "picking"), None, "B: nothing is open to begin with")
    highlighted = app.invoke(f"{EXT}/pick", key)
    picking = qj(app, "picking")
    assert_eq(picking["key"], key, "B: the roster opens on the row it was asked for")
    assert_eq(picking["options"], words, "B: carrying the words in the specified order")
    assert_eq(
        picking["highlighted"],
        row["value"],
        "B: ★★ and the highlight starts on the word the document HOLDS",
    )
    assert_eq(highlighted, picking["highlighted"], "B: which the verb answered too")
    assert_eq(
        picking["holding"],
        None,
        "B: with nothing owed -- `holding` names a word the roster does not offer",
    )

    before = row["value"]
    # ★★★★★ R1850 — ONE step, where this took two. The roster this drives has
    # two words since R1842 read the option surface off the target's own
    # declaration (`routing.peer.mode`, where the paraphrase `routing.mode` had
    # three), so two steps WRAP BACK to where they started and the highlight
    # "did not move" — a true reading of a step count that had stopped suiting
    # the roster. One step moves it for any roster of two or more, which is the
    # claim, and the roster's size is asserted rather than assumed.
    assert len(words) >= 2, f"B: a roster of one has no step to take: {words}"
    app.invoke(f"{EXT}/key", "ArrowDown")
    app.tick(8)
    moved = qj(app, "picking")
    ok("B: a step moved the highlight", moved["highlighted"] != before)
    assert_eq(
        next(r for r in qj(app, "form") if r["key"] == key)["value"],
        before,
        "B: ★★★★★ and the DOCUMENT did not move -- the floor's collapsed "
        "control commits on every arrow press, so a keyboard reader walking a "
        "roster of six leaves six values behind it",
    )
    assert_eq(
        moved["at"],
        words.index(moved["highlighted"]),
        "B: the index and the word are one fact",
    )

    # It wraps, in both directions, where the floor stops at the ends.
    app.invoke(f"{EXT}/key", "End")
    app.tick(4)
    assert_eq(qj(app, "picking")["highlighted"], words[-1], "B: End reaches the last")
    app.invoke(f"{EXT}/key", "ArrowDown")
    app.tick(4)
    assert_eq(
        qj(app, "picking")["highlighted"],
        words[0],
        "B: ★★ and one more step WRAPS, where the floor simply stops",
    )

    app.invoke(f"{EXT}/key", "Escape")
    app.tick(4)
    assert_eq(qj(app, "picking"), None, "B: Escape shuts it")
    assert_eq(
        next(r for r in qj(app, "form") if r["key"] == key)["value"],
        before,
        "B: ★ dismissing is not choosing",
    )

    # A row that cannot be picked from REFUSES, rather than answering silence.
    try:
        app.invoke(f"{EXT}/pick", "id")
        raise AssertionError("B: a free-text row must refuse the roster verb")
    except Exception as exc:  # noqa: BLE001 — the refusal is the assertion
        ok(
            "B: ★★ a row that takes any text refuses to be picked from, and "
            "says why -- silence would read as an open roster with nothing in it",
            "fixed set of words" in str(exc),
        )
    assert_eq(qj(app, "picking"), None, "B: and nothing was opened by the refusal")


def section_c(app: RpcSubprocess, key: str) -> None:
    banner("C — the machine's own pointer opens it and chooses from it")
    driver = pointer(app)
    if driver is None:
        return
    with driver as hand:
        held = next(r for r in qj(app, "form") if r["key"] == key)["value"]
        rects = abs_rects_of(app.snapshot(source="paint"))
        box = rects[control_of(app, key)]
        hand.move((box[0] + box[2] / 2, box[1] + box[3] / 2))
        hand.press()
        hand.release()
        app.tick(16)
        picking = qj(app, "picking")
        ok(
            "C: ★★★ a real press anywhere on the collapsed control opens its "
            "roster -- the whole box, not only the arrow",
            picking is not None and picking["key"] == key,
        )

        rects = abs_rects_of(app.snapshot(source="paint"))
        for word in picking["options"]:
            ok(f"C: the roster paints {word}", f"lab.form.option.{key}.{word}" in rects)
        wanted = next(w for w in picking["options"] if w != held)
        rect = rects[f"lab.form.option.{key}.{wanted}"]
        hand.move((rect[0] + rect[2] / 2, rect[1] + rect[3] / 2))
        hand.press()
        hand.release()
        app.tick(16)
        assert_eq(
            next(r for r in qj(app, "form") if r["key"] == key)["value"],
            wanted,
            "C: ★★★★ a real press on an option writes the word it was aimed at",
        )
        assert_eq(qj(app, "picking"), None, "C: and shuts the roster")
        rects = abs_rects_of(app.snapshot(source="paint"))
        ok(
            "C: ★★ the roster is gone from the PAINT too, not only from the "
            "state -- an option nobody can see must not still take a press",
            f"lab.form.option.{key}.{wanted}" not in rects,
        )
        ok(
            "C: ★ and the row still shows exactly one control, collapsed",
            f"lab.form.pick.{key}" in rects and f"lab.form.shown.{key}" in rects,
        )


def section_d(app: RpcSubprocess, key: str) -> None:
    banner("D — the keyboard, which the reference does not have at all")
    held = next(r for r in qj(app, "form") if r["key"] == key)["value"]
    app.invoke(f"{EXT}/pick", key)
    app.tick(4)
    # A letter walks every word that starts with it and comes back round; the
    # floor's typeahead stops on the first match and stays there.
    words = qj(app, "picking")["options"]
    letter = words[0][0]
    same_letter = [w for w in words if w.startswith(letter)]
    seen = []
    for _ in range(len(same_letter) + 1):
        app.invoke(f"{EXT}/key", letter)
        app.tick(4)
        seen.append(qj(app, "picking")["highlighted"])
    ok(
        f"D: ★★★ typing {letter!r} walks every word that starts with it and "
        "wraps -- four presses on the floor never left the second match",
        set(seen) == set(same_letter) or len(same_letter) == 1,
    )
    assert_eq(
        next(r for r in qj(app, "form") if r["key"] == key)["value"],
        held,
        "D: ★★ and none of that typing wrote anything -- on the floor a letter "
        "on the closed control commits the value outright",
    )
    chosen = qj(app, "picking")["highlighted"]
    app.invoke(f"{EXT}/key", "Enter")
    app.tick(8)
    assert_eq(
        next(r for r in qj(app, "form") if r["key"] == key)["value"],
        chosen,
        "D: ★ Enter writes what was under the highlight",
    )
    assert_eq(qj(app, "picking"), None, "D: and shuts the roster")


def section_e(app: RpcSubprocess, key: str) -> None:
    banner("E — what a reader is told")
    app.invoke(f"{EXT}/pick", "")
    app.tick(4)
    tree = {n["tag"]: n for n in app.request("scene/access").result["nodes"]}
    control = tree[control_of(app, key)]
    assert_eq(control["role"], "combobox", "E: ★★★ a collapsed roster is a COMBO BOX")
    assert_eq(
        control.get("expanded"),
        False,
        "E: ★★ and it says it is shut, rather than saying nothing",
    )
    ok(
        "E: ★ the arrow is NOT announced separately -- it is the control's own, "
        "and a node for it would read the same act out twice on every focus move",
        f"lab.form.pick.{key}" not in tree,
    )

    app.invoke(f"{EXT}/pick", key)
    app.tick(8)
    held = next(r for r in qj(app, "form") if r["key"] == key)["value"]
    tree = {n["tag"]: n for n in app.request("scene/access").result["nodes"]}
    control = tree[control_of(app, key)]
    assert_eq(control.get("expanded"), True, "E: opening says so")
    assert_eq(
        control.get("controls"),
        f"lab.form.roster.{key}",
        "E: ★★ and NAMES the roster it opened, so a reader can go to it",
    )
    roster = tree[f"lab.form.roster.{key}"]
    assert_eq(roster["role"], "listbox", "E: the roster is a listbox")
    for word in qj(app, "picking")["options"]:
        option = tree[f"lab.form.option.{key}.{word}"]
        assert_eq(option["role"], "option", f"E: {word} announces as an option")
        assert_eq(
            option.get("selected"),
            word == held,
            f"E: ★★ and {word} says whether the DOCUMENT holds it -- which is a "
            "different fact from where the highlight is",
        )
    app.invoke(f"{EXT}/pick", "")
    app.tick(4)


def body() -> None:
    spec = inspector_spec()
    named = surfaces(spec)
    ok("the specification fixes three surfaces", len(named) == 3)
    ok(
        "and every one of them declares an ordered roster of named parts",
        all(
            [p["ordinal"] for p in spec[s]["canon"]] == list(range(1, len(spec[s]["canon"]) + 1))
            for s in named
        ),
    )

    with RpcSubprocess(LAB, boot_grace=1.5, visible_window=True) as app:
        key = open_the_row(app)
        section_a(app, spec, key)
        section_b(app, spec, key)
        section_c(app, key)
        section_d(app, key)
        section_e(app, key)

    banner("what was checked")
    for line in CHECKS:
        print(f"  · {line}")
    print(
        f"\n[coverage] {REAL_POINTER_RUNS} real-pointer session(s) contributed to "
        f"this run; {len(CHECKS)} named check(s) plus the assert_eq comparisons above."
    )
    if REAL_POINTER_RUNS == 0:
        print(
            "[coverage] ⚠ section C's presses did NOT run on this host. The run "
            "is shorter than it looks and this line is the only evidence."
        )


if __name__ == "__main__":
    run_demo("R1732 an enum row is a roster that collapses", body)
