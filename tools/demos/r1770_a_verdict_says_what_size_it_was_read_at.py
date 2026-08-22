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
* **G** — the two sizes stay two claims: judged at 2442x1479 against a canon
  written at 2494x1531 is a sentence this report can now say, and a reader can
  tell it from a verdict read where its specification was written.

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
    banner("D — at the opening window it does not, and says exactly why")
    resize_and_settle(app, SMALL)
    said = walk(app)
    ok(
        "D: the tool does not claim to conform at the window it opens in",
        said["conforms"] is False,
    )
    lab_surfaces = {
        name: visit
        for (key, name), visit in visits(said).items()
        if key == "lab"
    }
    ok("D: the lab's surfaces are all in the report", len(lab_surfaces) == 3)
    for name, visit in sorted(lab_surfaces.items()):
        why = visit.get("why") or ""
        ok(
            f"D: `lab`/`{name}` declines to be judged here rather than failing",
            visit["stood"] is False and why != "",
        )
        laid_out = re.search(r"laid out (\d+) wide", why)
        given = re.search(r"given is (\d+)x(\d+)", why)
        ok(
            f"D: ★★ and the reason for `{name}` names BOTH numbers -- the width "
            "this screen declares it lays out at, and the width it was actually "
            "given -- so a reader is not left to find either of them",
            laid_out is not None and given is not None,
        )
        ok(
            f"D: ★★★★★ and the two are in the relation that makes the away "
            f"honest for `{name}`: it was given LESS than it declares. This is "
            "a state of the host's grant rather than a case in which the judge "
            "would fail, which is the test R1742 set for an away condition",
            int(laid_out.group(1)) > int(given.group(1)),
        )
        eq(
            f"{given.group(1)}x{given.group(2)}",
            visit["at"],
            f"D: and the width in the sentence is the extent the verdict for "
            f"`{name}` was read at, rather than a second account of it",
        )
    first = next(iter(sorted(lab_surfaces.items())))[1]
    print(f"  [small] {sorted(lab_surfaces)} away: {first['why']}")
    return said


# ── E — the vocabulary cannot flatter ────────────────────────────────────────


def section_e(small: dict, large: dict) -> None:
    banner("E — away credits nothing, so the smaller window reads WORSE")
    ok(
        "E: ★★★★★ the headline at the opening window went DOWN, not up. An away "
        "surface reproduces nothing (R1742's rule), so a section that declines "
        "to be judged costs its whole specification -- which is what stops "
        "'this frame is not what my specification is about' being a way to pass",
        small["reproduced"] < large["reproduced"],
    )
    ok(
        "E: and the walk says so as a count of surfaces it never stood in",
        small["stood"] < large["stood"],
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
    eq(
        sorted({v["at"] for v in lab_surfaces.values()}),
        ["2442x1479"],
        "G: ★ and each credited verdict names the same extent, which is not the "
        "one its canon was written at",
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
