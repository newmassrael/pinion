#!/usr/bin/env python3
"""R1761 §5.27 §5.38 §5.40 §2 #7 — **a page the host paints itself answers for
itself, and the route that was written down for twenty-three rounds would not
have worked.**

# What this demo exists for

R1738 made the analysis tool count the sections it was judged on, found four of
six unjudged, and wrote the closing move into the framework's own type: *the
host paints this page itself, so there is no screen to ask; closing it means
giving that page a `Screen` of its own, which is what the trait being public is
for*. R1742 and R1747 closed the two `unspecified` entries. The two `inline`
ones did not move.

Measured over this application's own wire at R1761, before any of this round's
code existed, standing on the section that entry had been open for since R1738:

```text
shell.canvas    1096x802 at (52, 98)     <- the page region a screen is granted
shell.subbar    1096x46  at (52, 52)     <- that section's layout bar, ABOVE it
shell.palette    292x848 at (1148, 52)   <- that section's palette, BESIDE it
```

A host paints a section's chrome outside the page region because that is what
chrome is, and a screen judges what it paints. The recorded route would have
shipped a verdict blind to about a quarter of its own section — including the
palette, which is where the reference makes its whole argument about what the
first release is. An instruction nobody could act on for twenty-three rounds
turned out to be one that would not have worked.

What this drives:

* **A** — the section a reader is standing in is judged, from the frame in front
  of them, by the host that painted it. Before this round it was the only
  section of the application that could answer nothing.
* **B** — the measurement above, taken live: every surface of this section's
  verdict is asked where it is, and three of the five are outside the rectangle
  a mounted screen would have been given.
* **C** — away is not a pass. Read from another section the whole verdict is
  away with one reason, reproduces nothing and does not reconcile; walking back
  puts it on the frame again, which is what makes A a measurement rather than a
  screen that always says yes.
* **D** — the verdict NAMES the parts it is about, in the pin's own order, read
  off disk here by a second hand. A verdict saying *twenty-eight were specified*
  cannot be checked by anybody.
* **E** — the two places this build differs from the reference are the two
  places somebody wrote down why, and nothing else differs.
* **F** — the application's own population moved with it, and the reviewed
  remainder in `docs/analyzer-sections-spec.json` is exactly the rows that are
  still unjudged.

# Floor

The floor for *can a page publish what it is supposed to contain, and can a
container add that up* was measured against the reference toolkit 6.11.1 at
R1738 (312 members across three page kinds, 0 naming a specification, an
expectation or a divergence) and again at R1758 (768 members, 0 naming a
verdict, evidence, a canon or a divergence; a page never shown answers
structurally identically to the one on screen). This round asks a narrower
question of the same surface — can the HOST say what its own inline page is
compared against — and the answer there is the same 0, for the same reason:
there is nothing to write the statement in. No new probe was built, and this
paragraph says so rather than implying one.

Run from the workspace root:
    cargo build --release -p hello-analyzer-shell
    DISPLAY=:97 python3 tools/demos/r1761_a_page_the_host_paints_answers_for_itself.py
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

#: The section this round closed, and the page a reader lands on.
SEAT = "dashboard"

#: The specification it is judged against, so section D compares the running
#: application's canon with a document this process reads off disk.
PIN = "analyzer-dashboard-spec.json"

#: The rectangle a mounted screen would have been granted at this destination.
PAGE_REGION = "shell.canvas"

#: Where each judged surface is painted, so B can ask whether a screen at this
#: destination could have reached it. Read from the paint, not declared here:
#: the tag is the stem the judge reads that surface's parts under.
SURFACE_TAG = {
    "layout_bar": "shell.subbar",
    "palette_head": "shell.palette",
    "palette_groups": "shell.palette",
    "palette": "shell.palette",
    "board": "shell.canvas",
}

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


def bbox(app: RpcSubprocess, tag: str) -> dict:
    answer = app.request("scene/bbox", {"tag": tag, "from": "paint"})
    assert answer is not None and answer.result, f"scene/bbox {tag}: {answer}"
    return answer.result["bbox"]


def inside(outer: dict, inner: dict) -> bool:
    return (
        inner["x"] >= outer["x"]
        and inner["y"] >= outer["y"]
        and inner["x"] + inner["w"] <= outer["x"] + outer["w"]
        and inner["y"] + inner["h"] <= outer["y"] + outer["h"]
    )


def section_a(app: RpcSubprocess) -> None:
    banner("A — the page the HOST paints is judged, and it is the one showing")
    said = report(app)
    row = row_of(said, SEAT)
    assert_eq(app.query(f"{EXT}/nav"), SEAT, "A: this report is read from the page")
    assert_eq(
        (row["standing"], row["showing"]),
        ("judged", True),
        "A: ★★★★★ the section a reader is standing in publishes a verdict. It "
        "was `inline` -- the host paints this page itself and nothing answers "
        "for it -- for twenty-three rounds",
    )
    verdict = row["conformance"]
    assert_eq(
        verdict["evidence"],
        "paint",
        "A: ★★ and the verdict is about a PAINTED FRAME, not about the host's "
        "own tables, which is what the count one level up refuses to accept",
    )
    ok(
        "A: ★ there is still no screen to address, and the row says so by "
        "carrying no tag -- being judged did not make this page a screen",
        "tag" not in row,
    )
    ok(
        "A: the verdict covers every surface the specification names, and each "
        "of them is on the frame",
        verdict["standing"] == len(verdict["surfaces"]) and verdict["away"] == 0,
    )
    print(
        f"  [verdict] {verdict['reproduced']} of {verdict['specified']} part(s) "
        f"across {verdict['standing']} surface(s), evidence "
        f"{verdict['evidence']}, reconciles {verdict['reconciles']}"
    )


def section_b(app: RpcSubprocess) -> None:
    banner("B — why a `Screen` at this destination could not have judged it")
    region = bbox(app, PAGE_REGION)
    said = report(app)
    surfaces = row_of(said, SEAT)["conformance"]["surfaces"]
    ok(
        "B: every surface of the verdict has a painted home to ask about",
        set(surfaces) == set(SURFACE_TAG),
    )
    outside = []
    for surface in sorted(surfaces):
        where = bbox(app, SURFACE_TAG[surface])
        if not inside(region, where):
            outside.append((surface, where))
        print(
            f"  {surface:<15} painted at {where['w']}x{where['h']} "
            f"({where['x']}, {where['y']}) — "
            f"{'inside' if inside(region, where) else '★ OUTSIDE'} the page region"
        )
    print(
        f"  [page region] {region['w']}x{region['h']} at ({region['x']}, "
        f"{region['y']}) — what a mounted screen is granted here"
    )
    ok(
        "B: ★★★★★ surfaces of this section are painted OUTSIDE the rectangle a "
        "mounted screen would be given, so the route recorded for twenty-three "
        "rounds -- give the page a `Screen` -- would have produced a verdict "
        "that could not reach them",
        len(outside) >= 2,
    )
    # Distinct rectangles, not distinct surfaces: three of this section's
    # surfaces are painted inside one panel, and adding that panel's area up
    # three times would be a number about this loop rather than about the
    # screen.
    homes = {SURFACE_TAG[surface]: where for surface, where in outside}
    lost = sum(where["w"] * where["h"] for where in homes.values())
    print(
        f"  [unreachable] {len(outside)} surface(s) in {len(homes)} rectangle(s), "
        f"{lost} px² of this section painted where a page's own verdict cannot "
        f"look"
    )


def section_c(app: RpcSubprocess) -> None:
    banner("C — away is not a pass, and the verdict moves when the frame does")
    elsewhere = "settings"
    # ★ R1761 — and not `intervene` + `tick`: everything read below is a fact
    # about the last PAINTED frame, so the read has to happen after the frame
    # that drew the new page rather than after the call that asked for it.
    app.intervene_painted(f"{EXT}/nav", elsewhere)
    row = row_of(report(app), SEAT)
    verdict = row["conformance"]
    assert_eq(
        (row["showing"], verdict["reproduced"], verdict["away"]),
        (False, 0, len(verdict["surfaces"])),
        "C: ★★★★★ read from another section every surface is away and nothing "
        "is credited. The host's paint store is NOT empty here -- it is full of "
        "the page that IS showing -- so this is the one answer a judge cannot "
        "derive from its own marks",
    )
    ok(
        "C: ★★ and declining to be judged is not passing: the report does not "
        "reconcile while a surface is away",
        verdict["reconciles"] is False,
    )
    reasons = {surface["why"] for surface in verdict["surfaces"].values()}
    ok(
        "C: ★ one reason, in the host's words, and it names where the reader "
        "is rather than what the judge failed to find",
        len(reasons) == 1 and "another section" in next(iter(reasons)),
    )
    print(f"  [away] {next(iter(reasons))}")

    app.intervene_painted(f"{EXT}/nav", SEAT)
    back = row_of(report(app), SEAT)["conformance"]
    ok(
        "C: ★★★ walking back puts every surface on the frame again -- the "
        "verdict follows the paint, which is what makes A a measurement",
        back["away"] == 0 and back["reproduced"] > 0,
    )
    print(f"  [returned] {back['reproduced']} of {back['specified']} reproduced")


def section_d(app: RpcSubprocess) -> None:
    global PARTS_COMPARED
    banner("D — the verdict names the parts, and a second hand reads the pin")
    pin = json.loads((DOCS / PIN).read_text(encoding="utf-8"))
    surfaces = row_of(report(app), SEAT)["conformance"]["surfaces"]
    for name, said in sorted(surfaces.items()):
        declared = [(part["key"], part["title"]) for part in pin[name]["canon"]]
        published = [(part["key"], part["title"]) for part in said["canon"]]
        assert_eq(
            published,
            declared,
            f"D: the application's canon for `{name}` is the pin's, in the "
            f"pin's own order",
        )
        PARTS_COMPARED += len(declared)
    ok(
        "D: ★★ every part of every surface was compared with a document read "
        "off disk by this process, so no count here is the screen's own word",
        PARTS_COMPARED == sum(len(pin[name]["canon"]) for name in surfaces),
    )
    print(f"  [compared] {PARTS_COMPARED} part(s) against docs/{PIN}")


def section_e(app: RpcSubprocess) -> None:
    banner("E — the differences this build has are the differences declared")
    pin = json.loads((DOCS / PIN).read_text(encoding="utf-8"))
    surfaces = row_of(report(app), SEAT)["conformance"]["surfaces"]
    found = {
        (name, difference["says"])
        for name, said in surfaces.items()
        for difference in said["divergences"]
    }
    declared = {
        (name, entry["sentence"])
        for name, body in pin.items()
        if not name.startswith("$")
        for entry in body["owed"]
    }
    assert_eq(
        sorted(found),
        sorted(declared),
        "E: ★★★★★ every way this build differs from the reference is a way "
        "somebody wrote down, and every way somebody wrote down is one it still "
        "has -- equality, so a remainder cannot outlive the difference it "
        "excuses any more than a difference can go undeclared",
    )
    ok(
        "E: ★★ and nothing is unreconciled, which is what `reconciles` means "
        "on a surface with a remainder",
        all(not said["unreconciled"] for said in surfaces.values()),
    )
    for name, sentence in sorted(found):
        print(f"  [declared] {name}: {sentence}")


def section_f(app: RpcSubprocess) -> None:
    banner("F — the application's population moved, and the pin agrees with it")
    said = report(app)
    unjudged = {row["key"] for row in said["rows"] if row["standing"] in ("inline", "unspecified")}
    assert_eq(
        sorted(unjudged),
        sorted(unjudged_sections()),
        "F: ★★★★★ the sections this application cannot judge are exactly the "
        "ones docs/analyzer-sections-spec.json still accepts -- so the entry "
        "this round paid off had to be DELETED there, and one left behind "
        "would fail as loudly as a section that went silent",
    )
    ok(
        f"F: ★ `{SEAT}` is no longer among them",
        SEAT not in unjudged and SEAT not in unjudged_sections(),
    )
    ok(
        "F: ★★ and the application still refuses to call itself conforming, "
        "because one section is unjudged and the sections nobody is looking at "
        "are away -- a count of judged sections is part of the verdict",
        said["conforms"] is False,
    )
    print(
        f"  [population] {said['sections']} section(s): {said['judged']} judged "
        f"({said['declared']} from tables), {said['unjudged']} unjudged, "
        f"{said['closed']} closed — {said['reproduced']} of "
        f"{said['specified']} part(s) reproduced"
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
    run_demo("r1761 a page the host paints answers for itself", body)
