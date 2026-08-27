#!/usr/bin/env python3
"""R1770 §5.27 §5.38 §5.40 §2 #7 — **a verdict says what size it was read at,
and the analysis tool conforms at a size it can name.**

# What this demo exists for

R1767 made a walk able to hold a verdict no single frame can, and the first
thing that became askable was a question nobody had been able to put: *at what
window?* Measured that round, one binary, one walk, one variable moved:

```text
1440x900   118 of 133   fails at lab/controls and lab/enum_row
2494x1531  129 of 133   fails at settings/rows
```

Both `conforms=false`, the two failing sets **disjoint**, and nothing in either
report said which window it came from. So `debt-the-analysis-tool-conforms-at-no-
window-size` was opened with three candidate repairs, and the round that took it
re-measured all three first. Two of them were wrong:

* the debt said the node lab was judged at **1096x802**. It is judged at
  **1388x848** — 1096x802 is the shell's page region, and the section's own
  surface is bigger than the page it sits on.
* the debt prescribed *make the inspector scroll or abbreviate*, a fold repair.
  Driven across eight sizes: the height moves **nothing** (900 and 1531 give
  identical verdicts) and the width moves everything. The parts that go are the
  right-hand ones — the type word, the applies badge, the remove seat, the pick
  arrow — and they go because this screen **declares** it stops laying out below
  1601 wide and clips instead, a number three rounds measured. Repairing it
  would have undone that decision.

What was true was the third and smallest sentence in the file: *no pin says what
size it was written at*. Counted this round: **zero of twelve**. Meanwhile the
node lab's own gate held `2494x1531` as a private constant inside one test
module, while the assembled tool judged the same pin over a fifth of that area.
One document, two gates, and neither artifact carried the number that told them
apart.

What this drives:

* **A** — every analyzer pin now declares `$at`, the surface extent its canon
  was written against, read here out of the files themselves.
* **B** — every verdict read from a frame names the extent it was read at, and
  every one names what it was read *against*.
* **C** — ★★★★★ the assembled tool **conforms**, at a size it can name. No
  window produced that before this round.
* **D** — and at 1440x900 it does not, for a reason a reader can act on: the
  node lab is given less width than it declares it lays out at, and says so in
  a sentence carrying both numbers.
* **E** — the vocabulary cannot flatter. Away credits nothing, so the smaller
  window's headline goes **down**, not up.
* **F** — one accepted difference is in force at one size and silent at the
  other. Same binary, same walk, one variable — which is the repair: a fold is a
  function of how tall the surface is, and a ledger that could not say so
  demanded the entry be deleted at the size where it does not apply, which would
  have broken the tool at the size where it does.
* **G** — the two sizes stay two claims: *judged at one extent against a canon
  written at another* is a sentence this report can now say, and a reader can
  tell it from a verdict read where its specification was written. ★ R1864 —
  the extent this section is judged at used to be written down here and in the
  check below; it moved the first time any chrome did (a host status band took
  28 pixels of the window), so it is READ from the report now and what is
  asserted is the property: one extent across every credited verdict, and not
  the canon's.

# Floor

Measured against the reference toolkit 6.11.1 at R1738 and R1758: across its
page-stack container, its tabbed container and a plain page, 312 members were
scanned and 0 name a specification, an expectation or a divergence. A size is
readable there — every widget reports its own — but there is nothing for a size
to qualify, so the question this demo asks cannot be put to it at all.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:97 python3 tools/demos/r1770_a_verdict_says_what_size_it_was_read_at.py
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    resize_and_settle,
    run_demo,
)

SHELL = "hello-analyzer-shell"
EXT = "/external"
LAB = "/node_lab/external"

#: Every section a reader can arrive at, in the order this demo walks them.
WALK = ["packets", "keys", "logs", "lab", "settings", "dashboard"]

#: The window the tool opens in, and the one a person maximises it to.
SMALL = (1440, 900)
LARGE = (2494, 1531)

#: The repository's specification pins for this tool. Read from disk rather
#: than listed, so a pin added tomorrow is in the census by existing.
PINS = sorted(
    (Path(__file__).resolve().parents[2] / "docs").glob("analyzer-*-spec.json")
)

CHECKS: list[str] = []


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def ok(what: str, condition: bool) -> None:
    CHECKS.append(what)
    assert condition, what


def eq(actual, expected, what: str) -> None:
    CHECKS.append(what)
    assert_eq(actual, expected, what)


def walk_report(app: RpcSubprocess) -> dict:
    return app.query(f"{EXT}/journey")


def frame_report(app: RpcSubprocess) -> dict:
    return app.query(f"{EXT}/sections")


def row(said: dict, key: str) -> dict:
    return next(r for r in said["rows"] if r["key"] == key)


def act(app: RpcSubprocess, path: str, args) -> object:
    return app.invoke(path, args)


def drive_the_lab(app: RpcSubprocess) -> None:
    """Stand in the lab and put its enumeration row on the card.

    Idempotent, because this walk is taken more than once: the screen refuses a
    second `add_field` for a key the card already carries.
    """
    app.intervene_painted(f"{EXT}/nav", "lab")
    enum_key = json.loads(app.query(f"{LAB}/spec"))["enum_key"]
    act(app, f"{LAB}/select", "P-01")
    if enum_key not in {r["key"] for r in json.loads(app.query(f"{LAB}/form"))}:
        act(app, f"{LAB}/add_field", enum_key)
    if json.loads(app.query(f"{LAB}/picking")) is not None:
        act(app, f"{LAB}/pick", "")
    act(app, f"{LAB}/pick", enum_key)


def walk(app: RpcSubprocess) -> dict:
    for key in WALK:
        if key == "lab":
            drive_the_lab(app)
            continue
        app.intervene_painted(f"{EXT}/nav", key)
    return walk_report(app)


def visits(said: dict) -> "dict[tuple[str, str], dict]":
    """Every (section, surface) the walk has a verdict for."""
    return {
        (r["key"], name): visit
        for r in said["rows"]
        for name, visit in r.get("surfaces", {}).items()
    }


def failing(said: dict) -> "list[tuple[str, str]]":
    return sorted(
        key for key, visit in visits(said).items()
        if visit["stood"] and not visit["reconciles"]
    )


# ── A — every pin says what size its canon was written against ───────────────


def section_a(app: RpcSubprocess) -> None:
    banner("A — the pins a frame is judged against declare the size they were written at")
    ok("A: the tool has specification pins to census at all", len(PINS) >= 6)
    sized = {
        pin.name: json.loads(pin.read_text()).get("$at")
        for pin in PINS
    }
    declaring = {name: at for name, at in sized.items() if at is not None}
    ok(
        "A: pins declare `$at`. Counted at the head of this round: ZERO did, "
        "while one screen's own gate held its size as a private constant inside "
        "a test module and the assembled tool judged the same pin over a fifth "
        "of that area",
        len(declaring) >= 6,
    )
    for name, at in sorted(declaring.items()):
        ok(
            f"A: `{name}`'s `$at` is a size rather than a word",
            isinstance(at, dict) and isinstance(at.get("width"), int)
            and isinstance(at.get("height"), int),
        )
    print(f"  [pins] {len(declaring)} of {len(sized)} declare `$at`")
    for name, at in sorted(sized.items()):
        note = f"{at['width']}x{at['height']}" if at else "— not read from a frame"
        print(f"    {name}: {note}")

    # ★★★★★ The population is derived from the RUNNING tool rather than from
    # the file list, because the invariant is not "every file has a size" — the
    # arrival, press, rail, board and section pins are claims about pointers,
    # seats and this build's own coverage, and a size would mean nothing on
    # them. It is: a verdict read from a FRAME says both numbers.
    from_paint = {
        r["key"]: r["conformance"]
        for r in frame_report(app)["rows"]
        if isinstance(r.get("conformance"), dict)
        and r["conformance"].get("evidence") == "paint"
    }
    ok("A: the tool has sections judged from a frame", len(from_paint) >= 5)
    eq(
        sorted(k for k, c in from_paint.items() if c.get("written_at") is None),
        [],
        "A: ★★★★★ and every one of them says what its canon was written "
        "against. That is the invariant, and it is read off the application "
        "rather than off a glob, so a pin that is not about paint is not asked "
        "for a size it could not have",
    )


# ── B — a verdict names where it was read, and what it was read against ──────


def section_b(app: RpcSubprocess) -> dict:
    banner("B — every verdict from a frame names the extent it was read at")
    said = walk(app)
    stood = {k: v for k, v in visits(said).items() if v["stood"]}
    ok("B: the walk stood in surfaces to have verdicts about", len(stood) >= 20)
    unnamed = sorted(k for k, v in stood.items() if not v.get("at"))
    eq(
        unnamed,
        [],
        "B: ★★★★★ not one standing verdict is silent about its size. Before this "
        "round every one of them was, which is why two walks of one binary "
        "failing at DISJOINT surfaces could not be told apart by anything except "
        "a person remembering which run was which",
    )
    for key in ("packets", "keys", "logs", "settings", "dashboard"):
        conformance = row(frame_report(app), key)["conformance"]
        ok(
            f"B: `{key}`'s own report says what it was read against",
            conformance.get("written_at") is not None,
        )
    sizes = sorted({v["at"] for v in stood.values()})
    print(f"  [sizes] verdicts at {sizes}")
    ok(
        "B: ★★ and the sizes are NOT all one number -- a section mounted as a "
        "page is given a fraction of the window, so the extent a verdict is "
        "about is the SURFACE's rather than the window's",
        len(sizes) > 1,
    )
    return said


# ── C — the tool conforms, at a size it can name ─────────────────────────────


def section_c(app: RpcSubprocess) -> dict:
    banner("C — ★★★★★ the assembled tool conforms, at a size it names")
    resize_and_settle(app, LARGE)
    said = walk(app)
    eq(failing(said), [], "C: no surface of the walk is unreconciled")
    eq(said["stood"], said["surfaces"], "C: every specified surface was stood in")
    ok(
        "C: ★★★★★ `conforms` is TRUE. No window produced that before this round: "
        "at the smaller one the node lab failed and at this one the preferences "
        "ledger did, in opposite directions, and the tool had no size at which "
        "it reproduced its own specification",
        said["conforms"] is True,
    )
    # ★ Read off the WALK rather than off the frame: the reader is standing on
    # the last section of the walk, so the lab's per-frame verdict is away and
    # its store was emptied when it left (R1763). The extent that matters is the
    # one the credited verdict was read at.
    sizes = {name: v["at"] for (key, name), v in visits(said).items() if key == "lab"}
    print(
        f"  [large] {said['reproduced']} of {said['specified']} reproduced, "
        f"conforms={said['conforms']}, lab credited at {sorted(set(sizes.values()))}"
    )
    return said


# ── D — and where it does not, a named reason a reader can act on ────────────


def section_d(app: RpcSubprocess) -> dict:
    banner("D — ★★★★★ there is no window this tool can be cut in")
    # ★★★★★ R1791 — this section used to run at SMALL and assert that the tool
    # DOES NOT claim to conform at the window it opens in, because the node lab
    # declared 1625 against a page of 1388 and its three surfaces declined. A
    # reader opened that window, saw the inspector cut, and asked whether it
    # should not be impossible to cut it in any situation. It is now: the
    # toolbar gives a group up instead of demanding its whole width, the lab
    # declares 1188, and it fits.
    #
    # So the subject moved, and it moved because the defect was repaired. Two
    # repairs were tried here before this one and BOTH measured something better
    # than the clause they were replacing:
    #
    #  * a narrower window — 1100 — to keep producing an away. `scene/resize` to
    #    1100 is granted **1440**, the shell's declared floor. From outside there
    #    is no width at which the assembled tool can be driven below what its
    #    mounted screens declare.
    #  * the opening window, on the assumption the capture viewer still declines
    #    there. It does not: walked at 1440x900 the report is **27 surfaces
    #    stood, none away, conforms=true**. (The capture viewer is short by 37px
    #    against its own declaration — the shell's own ratchet says so — but that
    #    shortfall does not reach the away condition.)
    #
    # ★★★★★ So the fact this section states is the strongest one R1791 produced:
    # **the away condition is unreachable from outside.** A reader cannot make
    # this tool cut a screen by resizing it, because the shell will not take a
    # window that would.
    #
    # The SHAPE claims that used to live here — that an away names both numbers,
    # in the relation that makes declining honest — did not die with the state
    # that produced them. They moved to `hello-node-lab`'s own crate tests
    # (`judge::tests::r1791_*`), where the judge can be handed a narrow surface
    # directly. That is R1786's rule: an assertion that cannot stand moves rather
    # than dies. It also repays something this round created — until it was
    # written, the away condition had **no test at all**, and R1791 had just made
    # it unreachable by every demo that used to exercise it.
    asked = (1100, 900)
    resp = app.request("scene/resize", {"width": asked[0], "height": asked[1]})
    granted = (resp.result["width"], resp.result["height"])
    ok(
        f"D: ★★★★★ the window REFUSES to go below its floor — asked for "
        f"{asked[0]} and granted {granted[0]}, because {resp.result['width_bound']}",
        granted[0] > asked[0] and resp.result["width_bound"]["kind"] == "floor",
    )
    resize_and_settle(app, SMALL)
    said = walk(app)
    away = sorted(
        f"{key}/{name}"
        for (key, name), visit in visits(said).items()
        if visit["stood"] is False
    )
    ok(
        f"D: ★★★★★ and at the window it opens in — the narrowest it has — every "
        f"one of its {said['stood']} surfaces STANDS to be judged. Three of the "
        f"node lab's declined here before this round (away now: {away or 'none'})",
        away == [],
    )
    ok(
        "D: ★★ so the tool claims to conform at the window a reader first sees, "
        "which is the claim that was false when a reader opened it and found the "
        "inspector cut",
        said["conforms"] is True,
    )
    print(f"  [small] {said['reproduced']}/{said['specified']} stood {said['stood']}, away none")
    return said


# ── E — the vocabulary cannot flatter ────────────────────────────────────────


def section_e(small: dict, large: dict) -> None:
    banner("E — the smaller window still reads worse, and now for an honest reason")
    # ★★★★★ R1791 — this section's INEQUALITY survived and its stated REASON did
    # not, which is the more dangerous of the two outcomes: an assertion that
    # goes on passing while the sentence explaining it has become false.
    #
    # It read: *an away surface reproduces nothing, so a section that declines to
    # be judged costs its whole specification*. That was the mechanism when three
    # of the node lab's surfaces declined at the opening window. Measured now:
    # **nothing is away at either size** and the two walks stand the same 27
    # surfaces. The gap is 128 against 129 — ONE part, a real difference in what
    # a smaller window reproduces, not a whole specification withdrawn.
    #
    # The claim about away crediting nothing is not gone; it is R1742's and lives
    # where R1742 put it. What this section can still say is that the smaller
    # window is not flattered.
    ok(
        f"E: ★★★★★ the headline at the opening window is still LOWER "
        f"({small['reproduced']} against {large['reproduced']} of "
        f"{large['specified']}) — a screen given less room reproduces less of "
        "its specification, and no vocabulary here lets it read as more",
        small["reproduced"] < large["reproduced"],
    )
    eq(
        small["stood"],
        large["stood"],
        "E: ★★★★★ and it stands the SAME number of surfaces, which is what R1791 "
        "changed. The gap above is a part this window does not reproduce, not a "
        "section that withdrew from being judged -- those are different failures "
        "and the walk no longer confuses them",
    )
    eq(
        small["specified"],
        large["specified"],
        "E: ★★ while the DENOMINATOR is unmoved -- what the tool is supposed to "
        "be made of does not depend on how big its window is",
    )
    print(
        f"  [small] {small['reproduced']}/{small['specified']} stood {small['stood']}"
        f"   [large] {large['reproduced']}/{large['specified']} stood {large['stood']}"
    )


# ── F — one entry, in force at one size and silent at the other ──────────────


def section_f(app: RpcSubprocess) -> None:
    banner("F — ★★★★★ an accepted difference that holds only where it was measured")
    pin = next(p for p in PINS if p.name == "analyzer-settings-spec.json")
    entry = next(
        e for e in json.loads(pin.read_text())["rows"]["owed"] if e["key"] == "theme"
    )
    ok(
        "F: the preferences ledger's fold entry names the extent it was "
        "measured at",
        entry.get("at") == [{"width": SMALL[0], "height": SMALL[1]}],
    )

    resize_and_settle(app, SMALL)
    app.intervene_painted(f"{EXT}/nav", "settings")
    small_rows = row(frame_report(app), "settings")["conformance"]["surfaces"]["rows"]
    eq(
        small_rows["reproduced"],
        len(small_rows["canon"]) - 1,
        "F: at the size it was measured at, the row IS short -- the entry is "
        "about a real difference rather than about nothing",
    )
    eq(
        [u["says"] for u in small_rows["unreconciled"]],
        [],
        "F: ★★ and the ledger reconciles it there, exactly as it did before",
    )

    resize_and_settle(app, LARGE)
    app.intervene_painted(f"{EXT}/nav", "settings")
    large_rows = row(frame_report(app), "settings")["conformance"]["surfaces"]["rows"]
    eq(
        large_rows["reproduced"],
        len(large_rows["canon"]),
        "F: at a taller window the whole row fits and the difference is gone",
    )
    eq(
        [u["says"] for u in large_rows["unreconciled"]],
        [],
        "F: ★★★★★ and the ledger is SILENT rather than demanding the entry be "
        "deleted. That demand is what R1767 measured: a taller window made the "
        "tool report the entry paid, and deleting it would have broken the same "
        "tool at the window it opens in",
    )
    print(
        f"  [theme] small {small_rows['reproduced']}/{len(small_rows['canon'])}"
        f"   large {large_rows['reproduced']}/{len(large_rows['canon'])}"
        f"   entry at {entry['at']}"
    )


# ── G — read where written, and read somewhere else, stay two claims ─────────


def section_g(app: RpcSubprocess) -> None:
    banner("G — a verdict read where its canon was written, and one that is not")
    resize_and_settle(app, LARGE)
    drive_the_lab(app)
    lab = row(frame_report(app), "lab")["conformance"]
    eq(
        lab["written_at"],
        "2494x1531",
        "G: the lab's canon was written at the size its own gate paints at",
    )
    ok(
        "G: ★★★★★ and it is being judged somewhere else -- the assembled tool "
        "gives this section a surface smaller than the window, so a verdict here "
        "is not the same claim its own gate makes",
        lab["at"] != lab["written_at"],
    )
    eq(
        lab["read_where_written"],
        False,
        "G: which the report says in one word rather than leaving a reader to "
        "compare two strings",
    )
    # ★ Judged over the WALK rather than the frame, because this section's own
    # specification names two surfaces that exclude each other — the roster
    # standing over the row takes the row off the frame — so no single frame of
    # it can reconcile. That is R1767's finding, unchanged by this round.
    walked = walk(app)
    lab_surfaces = {
        name: visit for (key, name), visit in visits(walked).items() if key == "lab"
    }
    eq(
        sorted(n for n, v in lab_surfaces.items() if not v["reconciles"]),
        [],
        "G: ★★ and every one of its surfaces reconciles anyway, so 'read "
        "somewhere else' is a qualifier and never an excuse -- they are judged "
        "exactly as hard here as in their own window",
    )
    # ★★★★★ R1864 — the EXTENT is read, not pinned, and that is a repair.
    #
    # This asserted `["2442x1479"]`, a number measured once by hand, and R1864
    # moved it to 1451 by reserving 28 pixels of the window for a host status
    # band. The number was never the claim: the claim is that every credited
    # verdict of this section names ONE extent (so the report is not a mixture
    # of frames read at different sizes) and that it is not the one its canon
    # was written at (which is what `read_where_written` says above, from the
    # other direction). A pin here rots the first time any chrome moves, and it
    # rotted.
    at = {v["at"] for v in lab_surfaces.values()}
    eq(
        len(at),
        1,
        f"G: ★ each credited verdict names the SAME extent, or the report is a "
        f"mixture of sizes: {sorted(at)}",
    )
    ok(
        f"G: ★ and it is not the extent its canon was written at "
        f"({sorted(at)} vs {lab['written_at']})",
        at != {lab["written_at"]},
    )
    print(f"  [lab] read at {lab['at']} against a canon written at {lab['written_at']}")


def body() -> None:
    with RpcSubprocess(SHELL, boot_grace=1.5) as app:
        section_a(app)
        section_b(app)
        large = section_c(app)
        small = section_d(app)
        section_e(small, large)
        section_f(app)
        section_g(app)

    print(f"\n=== {len(CHECKS)} named check(s) ===")
    for line in CHECKS:
        print(f"  - {line}")


if __name__ == "__main__":
    run_demo("r1770 a verdict says what size it was read at", body)
