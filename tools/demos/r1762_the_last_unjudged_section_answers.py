#!/usr/bin/env python3
"""R1762 §5.27 §5.38 §5.40 §2 #7 — **the last unjudged section answers, and the
page it is about is the one the reference draws.**

# What this demo exists for

R1738 made the analysis tool count the sections it had been judged on and found
four of six unjudged. R1742, R1747 and R1761 closed three of them. The fourth —
the preferences page — could not be closed by the same edit, and the reason is
the round: a pin that specified only what this build already drew would be a
document that cannot fail.

Measured at R1761 against the behaviour reference, before any of this round's
code existed, this build's preferences page was missing **three of the
reference's ten rows** (a capture-source row and a retention row in the first
group, a payload-format row in the second), had neither the page heading the
reference opens with nor the build strip it closes with, and its four group
headings were loose ink no specification could address. So the page was built
first and the pin written afterwards.

What this drives:

* **A** — the application's own count: **zero** unjudged sections. Every section
  a reader can arrive at is compared with a written specification, and every one
  of those verdicts is about a painted frame.
* **B** — the ten rows the reference draws, on the frame, in the reference's
  order, read back here from `docs/analyzer-settings-spec.json` by a second
  hand.
* **C** — the two rows this build did not have. Their control is a chooser:
  opening it is **not** a write, choosing a word is, and the value it writes is
  the one the application bar shows.
* **D** — the page **scrolls**, which is what the reference's does and what
  makes the last group reachable. Measured: the appearance group is painted
  below the region's foot at rest, and a scroll brings it in.
* **E** — away is not a pass: read from another section the whole verdict is
  away with one reason and reproduces nothing.
* **F** — the reviewed remainder in `docs/analyzer-sections-spec.json` is
  **empty**, and that is asserted as an equality rather than skipped.

# Floor

The floor for *can a page publish what it is supposed to contain, and can a
container add that up* was measured against the reference toolkit 6.11.1 at
R1738 and R1758 (312 and 768 members scanned, 0 naming a specification, a
verdict, evidence or a divergence). This round adds one row to that table,
measured against the same build: the toolkit's own collapsed chooser reports its
value and its item count and carries **no expanded state** unless a platform
layer adds one — so a reader is told what is chosen and never told whether the
list is open. Here the chooser publishes both, and section C reads them.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:97 python3 tools/demos/r1762_the_last_unjudged_section_answers.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from analyzer_spec import DOCS, unjudged_sections  # noqa: E402
from rpc_verify import RpcSubprocess, assert_eq, run_demo  # noqa: E402

SHELL = "hello-analyzer-shell"
EXT = "/external"
SEAT = "settings"
PIN = "analyzer-settings-spec.json"

CHECKS: list[str] = []
PARTS_COMPARED = 0


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def ok(what: str, condition: bool) -> None:
    CHECKS.append(what)
    assert condition, what


def report(app: RpcSubprocess) -> dict:
    return app.query(f"{EXT}/sections")


def row_of(said: dict, key: str) -> dict:
    return next(row for row in said["rows"] if row["key"] == key)


def rects(app: RpcSubprocess) -> dict:
    from rpc_verify import abs_rects_of

    return abs_rects_of(app.snapshot(source="paint"))


def press(app: RpcSubprocess, tag: str) -> None:
    box = rects(app).get(tag)
    assert box is not None, f"the frame drew nothing at {tag}"
    x, y, w, h = box
    before = app.frame_count()
    app.request("scene/click", {"button": "left", "at": {"x": x + w // 2, "y": y + h // 2}})
    app.tick(16)
    app.await_paint(before)


def section_a(app: RpcSubprocess) -> None:
    banner("A — the application has NO unjudged section left")
    said = report(app)
    assert_eq(
        said["unjudged"],
        0,
        "A: ★★★★★ every section a reader can arrive at is compared with a "
        "written specification. R1738 opened this count at four unjudged of "
        "six, and this is the round it reaches zero",
    )
    judged = [row for row in said["rows"] if row["standing"] == "judged"]
    assert_eq(
        {row["key"]: row["conformance"]["evidence"] for row in judged},
        {row["key"]: "paint" for row in judged},
        "A: ★★ and every one of those verdicts is about a PAINTED FRAME, not "
        "about a screen's own tables",
    )
    ok(
        "A: ★ the count is the roster's, so a section cannot be missing from it "
        "by being forgotten",
        said["sections"] == len(said["rows"]) and said["judged"] + said["closed"] == said["sections"],
    )
    print(
        f"  [population] {said['sections']} section(s): {said['judged']} judged "
        f"({said['declared']} from tables), {said['unjudged']} unjudged, "
        f"{said['closed']} closed — {said['reproduced']} of {said['specified']} "
        f"part(s) reproduced"
    )


def section_b(app: RpcSubprocess) -> None:
    global PARTS_COMPARED
    banner("B — the ten rows the reference draws, on the frame and in its order")
    app.intervene_painted(f"{EXT}/nav", SEAT)
    said = report(app)
    verdict = row_of(said, SEAT)["conformance"]
    pin = json.loads((DOCS / PIN).read_text(encoding="utf-8"))
    for name, surface in sorted(verdict["surfaces"].items()):
        declared = [(p["key"], p["title"]) for p in pin[name]["canon"]]
        published = [(p["key"], p["title"]) for p in surface["canon"]]
        assert_eq(
            published,
            declared,
            f"B: the application's canon for `{name}` is the pin's, in the "
            f"pin's own order",
        )
        PARTS_COMPARED += len(declared)
    rows = [key for key, _ in [(p["key"], p["title"]) for p in pin["rows"]["canon"]]]
    assert_eq(
        len(rows),
        10,
        "B: ★★★★★ the reference's page is ten rows in four groups, and the pin "
        "is that list. Three of them did not exist in this build before this "
        "round",
    )
    ok(
        "B: ★★ and every surface is on the frame -- nothing is away while the "
        "reader is standing here",
        verdict["away"] == 0 and verdict["standing"] == len(verdict["surfaces"]),
    )
    ok(
        "B: ★★★ the differences this build has are exactly the ones somebody "
        "wrote down",
        all(not s["unreconciled"] for s in verdict["surfaces"].values()),
    )
    for name, surface in sorted(verdict["surfaces"].items()):
        for d in surface["divergences"]:
            print(f"  [declared] {name}: {d['says']}")
    print(f"  [compared] {PARTS_COMPARED} part(s) against docs/{PIN}")


def section_c(app: RpcSubprocess) -> None:
    banner("C — the chooser: opening is not a write, and choosing is")
    assert_eq(app.query(f"{EXT}/picking"), "", "C: nothing is open to start")
    was = app.query(f"{EXT}/retention")
    ok("C: the retention row holds a word out of its own roster", was in app.query(f"{EXT}/retentions").split(","))

    press(app, "shell.settings.choose.retention")
    assert_eq(app.query(f"{EXT}/picking"), "retention", "C: the roster is open")
    assert_eq(
        app.query(f"{EXT}/retention"),
        was,
        "C: ★★★★★ and OPENING IT WROTE NOTHING. The floor's own collapsed "
        "control commits on every arrow press, so a keyboard reader walking a "
        "roster of six leaves six values behind",
    )
    ok(
        "C: ★★ the roster is on the frame, over the rows it covers",
        "shell.settings.roster.retention" in rects(app),
    )

    words = app.query(f"{EXT}/retentions").split(",")
    other = next(w for w in words if w != was)
    press(app, f"shell.settings.option.retention.{other}")
    assert_eq(app.query(f"{EXT}/retention"), other, "C: ★★ choosing a word writes it")
    assert_eq(app.query(f"{EXT}/picking"), "", "C: and closes the roster")
    ok(
        "C: ★ and the roster left the frame with it",
        "shell.settings.roster.retention" not in rects(app),
    )

    # The other value row is the capture source, which the application bar
    # shows: the reference puts it on this page for exactly that reason.
    source = app.query(f"{EXT}/source")
    press(app, "shell.settings.choose.interface")
    assert_eq(app.query(f"{EXT}/picking"), "interface", "C: the second roster opens")
    others = [s for s in app.query(f"{EXT}/sources").split(",") if s != source]
    press(app, f"shell.settings.option.interface.{others[0]}")
    assert_eq(
        app.query(f"{EXT}/source"),
        others[0],
        "C: ★★★★★ and the row writes THE APPLICATION BAR'S source -- one fact, "
        "two places a reader can see it, which is why the reference puts this "
        "row on this page",
    )
    print(f"  [chose] retention {was} -> {other}, source {source} -> {others[0]}")


def section_d(app: RpcSubprocess) -> None:
    banner("D — the page scrolls, which is what puts the last group in reach")
    # The pair that says it: what the group was PAINTED at, and how much of that
    # a reader can reach. A viewport cuts the second and leaves the first, so
    # `bbox != window` is the page being taller than the region it is in.
    said = app.request(
        "scene/bbox", {"tag": "shell.settings.group.appearance", "from": "paint"}
    ).result
    ok(
        "D: ★★★★★ the last group is cut by the page's viewport, so the page is "
        f"taller than its region (painted {said['bbox']['h']}px, reachable "
        f"{said['window']['h']}px)",
        said["window"]["h"] < said["bbox"]["h"],
    )
    group = rects(app)["shell.settings.group.appearance"]
    before = app.frame_count()
    app.scroll("shell.settings.body", by=(0, 400))
    app.tick(16)
    app.await_paint(before)
    moved = rects(app)["shell.settings.group.appearance"]
    ok(
        "D: ★★ scrolling moves it up, so a reader can reach it -- the "
        f"reference's own page scrolls too ({group[1]} -> {moved[1]})",
        moved[1] < group[1],
    )
    # And the control down there answers a press where it is now painted.
    press(app, "shell.settings.theme.1")
    assert_eq(app.query(f"{EXT}/theme"), "light", "D: ★★★ the appearance choice took")
    press(app, "shell.settings.theme.0")
    before = app.frame_count()
    app.scroll("shell.settings.body", to=(0, 0))
    app.tick(16)
    app.await_paint(before)


def section_e(app: RpcSubprocess) -> None:
    banner("E — away is not a pass")
    app.intervene_painted(f"{EXT}/nav", "dashboard")
    row = row_of(report(app), SEAT)
    verdict = row["conformance"]
    assert_eq(
        (row["showing"], verdict["reproduced"], verdict["away"]),
        (False, 0, len(verdict["surfaces"])),
        "E: ★★★★★ read from another section every surface is away and nothing "
        "is credited -- the host's paint store is full of the page that IS "
        "showing, so this is the answer a judge cannot derive from its marks",
    )
    ok(
        "E: ★★ and declining to be judged is not passing",
        verdict["reconciles"] is False,
    )
    app.intervene_painted(f"{EXT}/nav", SEAT)
    back = row_of(report(app), SEAT)["conformance"]
    ok(
        "E: ★★★ walking back puts every surface on the frame again",
        back["away"] == 0 and back["reproduced"] > 0,
    )


def section_f(app: RpcSubprocess) -> None:
    banner("F — the reviewed remainder is EMPTY, and that is an equality")
    said = report(app)
    unjudged = {
        row["key"] for row in said["rows"] if row["standing"] in ("inline", "unspecified")
    }
    assert_eq(
        sorted(unjudged),
        sorted(unjudged_sections()),
        "F: ★★★★★ the sections this application cannot judge are exactly the "
        "ones docs/analyzer-sections-spec.json still accepts -- and both are "
        "empty, which is a claim rather than the absence of one",
    )
    ok("F: ★ the pin's remainder is empty", not unjudged_sections())
    ok(
        "F: ★★ and the application still refuses to call itself conforming, "
        "because the sections nobody is looking at are away -- a verdict is "
        "about a frame",
        said["conforms"] is False,
    )


def body() -> None:
    with RpcSubprocess(SHELL, boot_grace=1.5) as app:
        section_a(app)
        section_b(app)
        section_c(app)
        section_d(app)
        section_e(app)
        section_f(app)

    print(f"\n=== {len(CHECKS)} named check(s) ===")
    for line in CHECKS:
        print(f"  - {line}")


if __name__ == "__main__":
    run_demo("r1762 the last unjudged section answers", body)
