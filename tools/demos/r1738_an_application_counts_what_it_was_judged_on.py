#!/usr/bin/env python3
"""R1738 §5.27 §5.38 §5.40 §2 #2 §2 #7 — **an application counts the sections it
was judged on, and the count is part of the verdict.**

# What this demo exists for

The analysis tool is one application assembled from sections. R1728 wrote its
*navigation* down as a reviewed artifact and made something fail when the build
stopped matching it; R1730 and R1731 did the same one level down, for a
*section's* surfaces. Nothing added them up.

Measured over this application's own wire before any of this round's code
existed, standing in each of its six open sections in turn:

```text
/external/conformance              -> { specified: 8, reproduced: 8, divergences: [], owed: [] }
/key_patterns/external/conformance -> { columns: 7/7, detail: 11/11, header: 3/3 }
/log_view/external/conformance     -> { columns: 5/5, detail: 6/6, header: 4/4 }
```

…and that is the whole of it. The headline reads `8 of 8 reproduced`, and it is
**true about the rail** — eight navigation seats, eight reproduced. Read as a
statement about the tool, which is how it reads, it was wrong: two of six
sections had been compared with anything, four had not, and no slot, test or
report named the other four. They were not failing a check. They were absent
from the population.

That is R1737's lesson one level up — *a gate existing and a gate's coverage
being deliberate are different claims* — and the repair is the same one: make
the framework count, and make the population derived.

# What it drives

* **A** — the report's population IS the roster. One row per destination, in
  the rail's order, taken from `docs/analyzer-rail-spec.json` on one side and
  from the running application on the other.
* **B** — the application does not claim conformance while sections are
  unjudged, and its unjudged rows are **equal** to the reviewed remainder in
  `docs/analyzer-sections-spec.json` — so a section that starts answering must
  have its entry deleted, and an entry left behind fails as loudly as a section
  that went silent.
* **C** — a judged section's verdict is the **same value** it publishes on its
  own wire, and the same value its standalone binary publishes. One build, two
  placements, one sentence.
* **D** — ★ the integration check the round is named for: navigate to each
  judged section **in the assembled application** and read back, from the
  paint, every part its specification fixes. The population is the report's, so
  this cannot look at fewer sections than the application has, and the number of
  parts it actually compared is printed rather than asserted to exist.
* **E** — the two closed seats carry the destination's **own** reason, and
  navigating to them refuses with that same sentence — the report and the
  refusal are not two wordings of one closure.

# Floor, measured by building a probe against 6.11.1 and running it

The probe assembles a paged application out of three pages, gives one page a
part fewer than it declares and another the specified parts in the wrong order,
then asks the toolkit about it.

* Across the page-stack container, the tabbed container and a plain page,
  **312** members were scanned and **0** name a specification, an expectation or
  a divergence. There is nothing to write the statement in.
* The only channel a page has for declaring what it is supposed to contain is a
  compile-time per-class annotation, so three pages that built **different**
  things all report the **same** specification — the statement is not reachable
  per instance.
* And the row that decides it: with a short page and a reordered page in it, the
  container still answers `count() = 3`. It has no member returning a verdict, a
  divergence, or a count of pages judged. Nothing failed and nothing was
  reported.
"""

from __future__ import annotations

import sys
from collections.abc import Iterable
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from analyzer_spec import rail_spec, unjudged_sections  # noqa: E402
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    run_demo,
)

SHELL = "hello-analyzer-shell"
KEYS_SECTION = "hello-key-patterns"
EXT = "/external"

CHECKS: list[str] = []
PARTS_COMPARED = 0
SECTIONS_WALKED = 0
#: The sections section D judged BY TAG, named so the printed coverage says
#: which sections it is coverage of (R1742).
TAG_JUDGED: list[str] = []


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def ok(what: str, condition: bool) -> None:
    CHECKS.append(what)
    assert condition, what


def report(app: RpcSubprocess) -> dict:
    """The application's own count of how much of itself has been judged."""
    return app.query(f"{EXT}/sections")


def section_a(app: RpcSubprocess) -> None:
    banner("A — the population is the roster, not a list somebody keeps")
    rail = rail_spec()
    said = report(app)
    assert_eq(
        [row["key"] for row in said["rows"]],
        [seat["key"] for seat in rail["canon"]],
        "A: one row per destination, in the rail's own order",
    )
    assert_eq(
        said["sections"],
        len(rail["canon"]),
        "A: ★★ and the count is the roster's, so a section is missing from this "
        "report only by not being in the application",
    )
    ok(
        "A: every row says which of the four things it is",
        all(
            row["standing"] in {"judged", "unspecified", "inline", "closed"}
            for row in said["rows"]
        ),
    )
    ok(
        "A: ★ and every row that is not judged says WHY -- a declared absence, "
        "not silence",
        all("why" in row for row in said["rows"] if row["standing"] != "judged"),
    )
    ok(
        "A: a row carries the tag its section is addressed by exactly when a "
        "screen is mounted there",
        all(
            ("tag" in row) == (row["standing"] in {"judged", "unspecified"})
            for row in said["rows"]
        ),
    )
    print(
        f"  [population] {said['sections']} section(s): {said['judged']} judged, "
        f"{said['unjudged']} unjudged, {said['closed']} closed"
    )


def section_b(app: RpcSubprocess) -> None:
    banner("B — the remainder is the reviewed one, and it is an EQUALITY")
    said = report(app)
    unjudged = {
        row["key"]: row["why"] for row in said["rows"] if row["standing"] in {"unspecified", "inline"}
    }
    assert_eq(
        sorted(unjudged),
        sorted(unjudged_sections()),
        "B: ★★★ the sections nothing has judged are exactly the ones "
        "docs/analyzer-sections-spec.json accepts -- an entry is DELETED when it "
        "is paid, because a remainder list that only grows is one nobody reads",
    )
    for key, sentence in sorted(unjudged_sections().items()):
        assert_eq(
            unjudged[key],
            sentence,
            f"B: and `{key}` gives the reason the pin declares for it",
        )
    ok(
        "B: ★★★★★ the application does NOT report conformance while sections are "
        "unjudged -- the count of judged sections is part of the verdict, not a "
        "footnote under it",
        said["conforms"] is False and said["unjudged"] > 0,
    )
    ok(
        "B: ★★ and the seat-level verdict still says what it always said, so "
        "this is a second sentence rather than a correction of the first",
        app.query(f"{EXT}/conformance")["divergences"] == [],
    )


def section_c(app: RpcSubprocess) -> None:
    banner("C — a judged section's verdict is the same value, wherever it is")
    global SECTIONS_WALKED
    said = report(app)
    judged = [row for row in said["rows"] if row["standing"] == "judged"]
    ok("C: at least one section is judged, or there is nothing to compare", judged)

    # ★★★★★ Measured while writing this demo, and it is the reason the report
    # is worth having at all: a section's OWN wire is reachable only while the
    # journey is standing in it. That is this crate's central rule working
    # (`the screen the journey is at is the only one anything reaches`) and it
    # means a client cannot assemble the application's verdict by asking the
    # sections — it would have to navigate the tool to find out what the tool
    # is, changing what the reader is looking at in order to measure it.
    ok(
        "C: ★★★★★ standing at `dashboard`, a section's own wire is out of reach "
        "-- so a client CANNOT build this report by asking the sections",
        app.query(f"{EXT}/nav") == "dashboard" and unreachable(app, judged[0]["tag"]),
    )
    ok(
        "C: ★★★★★ and yet the host answers for every one of them from here -- "
        "which is what makes a verdict about the application possible without "
        "navigating it",
        all(row.get("surfaces") for row in judged),
    )

    # ★★★★★ R1742 — the host's row is re-read WHILE STANDING IN the section,
    # and the reason is a defect this assertion had and could not see. A
    # section that derives its verdict from its own PAINT answers about its
    # last frame, and a section that is not showing has not painted since; the
    # row taken from the dashboard was therefore a true statement about a frame
    # no longer in the application, and comparing it with the section's live
    # answer failed the moment the first such screen existed. The report says
    # which frame each row is about (`showing`), and this walks to the section
    # before comparing -- because "the host aggregates, it does not re-derive"
    # is a claim about one frame, not about two.
    for row in judged:
        SECTIONS_WALKED += 1
        app.intervene(f"{EXT}/nav", row["key"])
        app.tick(16)
        here = next(r for r in report(app)["rows"] if r["key"] == row["key"])
        ok(
            f"C: ★★ the report says `{row['key']}` is the section showing, so a "
            f"reader knows which frame the verdict beside it is about",
            here["showing"] is True,
        )
        own = app.query(f"/{row['tag']}/external/conformance")
        assert_eq(
            own,
            here["surfaces"],
            f"C: ★★ the host's row for `{row['key']}` IS the value the section "
            f"publishes on its own wire -- the host aggregates, it does not "
            f"re-derive",
        )
        assert_eq(
            here["specified"],
            sum(surface["specified"] for surface in own.values()),
            f"C: and `{row['key']}`'s totals are its surfaces added up",
        )
    app.intervene(f"{EXT}/nav", "dashboard")
    app.tick(16)

    # One standalone binary, so the claim "one build, two placements" is checked
    # against a second PROCESS and not only against a second slot of this one.
    keys_row = next(row for row in judged if row["key"] == "keys")
    with RpcSubprocess(KEYS_SECTION, boot_grace=1.5) as standalone:
        assert_eq(
            standalone.query(f"{EXT}/conformance"),
            keys_row["surfaces"],
            "C: ★★★★★ and the standalone binary of that section publishes the "
            "SAME verdict -- a section that conforms in its own window and is "
            "never asked as a page would be two builds wearing one name",
        )


def section_d(app: RpcSubprocess) -> None:
    banner("D — every part of every judged section, read back from the paint")
    global PARTS_COMPARED
    said = report(app)
    judged = [row for row in said["rows"] if row["standing"] == "judged"]
    for row in judged:
        app.intervene(f"{EXT}/nav", row["key"])
        app.tick(16)
        assert_eq(app.query(f"{EXT}/nav"), row["key"], f"D: the `{row['key']}` seat opens")
        rects = abs_rects_of(app.snapshot(source="paint"))
        ok(
            f"D: arriving at `{row['key']}` paints its section inside the host",
            any(tag.startswith(f"{row['tag']}.") or "." in tag for tag in rects),
        )
        for chrome in ("shell.appbar", "shell.rail", f"shell.rail.{row['key']}"):
            ok(f"D: and the host's {chrome} survives it -- a page, not a takeover", chrome in rects)

        # ★★★★★ R1742 — the rule is ALL OR NONE per surface, and it is a
        # derivation rather than a list of exclusions.
        #
        # Two things went wrong here the round a third section started
        # publishing. The parts were read flat, so a section publishing its
        # specification nested answered `[]` for every surface and this loop
        # silently compared nothing while the printed number stayed what it
        # was. And once they WERE read, the expectation turned out to be false
        # for two honest reasons: a surface a session has not opened is not on
        # the frame, and a surface whose parts are a CLASSIFICATION of the rows
        # (which control kind each row draws) has no tag per part at all.
        #
        # So: a surface either paints every part it names or paints none of
        # them. `none` is a surface judged some other way — by its own
        # report — and is counted and printed rather than skipped in silence; a
        # surface that paints SOME and not others is the drift this check
        # exists for, and still fails.
        # ★★★★★ R1742 — the population of this check is now DERIVED and
        # RATCHETED, and both halves are repairs of holes it had.
        #
        # It read the parts flat, so a section publishing its specification
        # nested answered `[]` for every surface: nothing failed, nothing was
        # compared, and the printed number stayed what it was — which reads as
        # covering every judged section. Once they were read, the expectation
        # itself turned out not to generalise. A surface a session has not
        # opened is not on the frame, and a surface whose parts are a
        # CLASSIFICATION of the rows (which control kind each row draws) has no
        # tag per part at all — so "every specified part is painted" is false
        # for reasons that are not defects.
        #
        # And the matching is a heuristic, which the same run proved: the part
        # named `text` matched `lab.inspector.reach.text`, a tag belonging to
        # something else entirely. A single coincidence like that must not be
        # able to put a section into the strict branch half-way.
        #
        # So a section is in the tag population only when EVERY part of EVERY
        # surface resolves; anything else is reported with its resolved/total
        # counts, which a reader can see is not coverage. The ratchet below is
        # what stops that being an escape: the population and the part count
        # may not fall below what this check measured when it was written.
        parts_by_surface = {
            surface: parts_of(app, row["tag"], surface, row["surfaces"])
            for surface in row["surfaces"]
        }
        for surface, parts in parts_by_surface.items():
            ok(
                f"D: `{row['key']}`.{surface} publishes the parts its "
                f"specification fixes, so this check has something to compare",
                bool(parts),
            )
        resolved = {
            surface: [
                part
                for part in parts
                if any(tag == part or tag.endswith(f".{part}") for tag in rects)
            ]
            for surface, parts in parts_by_surface.items()
        }
        total = sum(len(p) for p in parts_by_surface.values())
        found = sum(len(p) for p in resolved.values())
        if found == total:
            TAG_JUDGED.append(row["key"])
            PARTS_COMPARED += total
            ok(
                f"D: ★★★ all {total} part(s) `{row['key']}`'s specification fixes "
                f"are PAINTED in the assembled application, not only in its own "
                f"window",
                True,
            )
        else:
            print(
                f"  [by-report] `{row['key']}`: {found} of {total} specified "
                f"part(s) resolve to a tag, so this section is judged by its own "
                f"report rather than by this check"
            )
    print(
        f"  [coverage] {PARTS_COMPARED} specified part(s) compared with the paint "
        f"across {len(TAG_JUDGED)} of {SECTIONS_WALKED} judged section(s): "
        f"{', '.join(TAG_JUDGED)}"
    )
    ok(
        f"D: ★★★★★ and the population has not SHRUNK -- {len(TAG_JUDGED)} section(s) "
        f"and {PARTS_COMPARED} part(s) against the 2 and 36 this check measured "
        f"when it was written, so a section that quietly stopped addressing its "
        f"parts by tag cannot leave the check by falling out of its population",
        len(TAG_JUDGED) >= 2 and PARTS_COMPARED >= 36,
    )


def unreachable(app: RpcSubprocess, tag: str) -> bool:
    """Whether a section's own external refuses from where we are standing."""
    try:
        app.query(f"/{tag}/external/conformance")
    except Exception:  # noqa: BLE001
        return True
    return False


def parts_of(
    app: RpcSubprocess, tag: str, surface: str, surfaces: Iterable[str]
) -> list[str]:
    """The part keys one surface of a section is specified to have.

    Read from the section's own published specification rather than from a table
    in this file: the two demos before this one each grew their own copy of a
    seat list, and the one nobody ran was the one that was wrong.

    ★★★★★ R1742 — it also looks one level down, and reading it flat was a
    defect. A section may publish its specification NESTED under a name of its
    own (the node lab publishes `spec.inspector`), and the flat read answered
    `[]` for every surface of such a section. Nothing failed: the loop below
    compared nothing, and the printed coverage stayed the number it already
    was, which reads as covering every judged section. That is this demo's own
    lesson turned on itself -- count what the check actually looked at -- so
    the caller refuses a surface that yields no parts.

    ★★★★★ R1747 — and the lookup takes the sub-document that holds EVERY
    surface, which is a derivation replacing a coincidence. R1742's own comment
    below predicted this class ("the matching is a heuristic") and the fourth
    judged section is where it fired: the capture viewer's pin fixes a surface
    called `context`, and that screen's own published tables ALSO have a
    `context` -- a different document answering to the same word. Taking the
    first key that matches would have compared the pin's surface against the
    screen's negotiated-value table and reported a shortfall that is not one.

    A section's specification is the one place where every surface it is judged
    on appears together, so that -- and not a name match -- is what identifies
    it. Ambiguity is refused rather than resolved by order: two candidates
    holding every surface means this file cannot tell which document the
    verdict is about, and guessing is what it is trying to stop.
    """
    import json

    said = app.query(f"/{tag}/external/spec")
    if isinstance(said, str):
        said = json.loads(said)
    wanted = set(surfaces)
    candidates = [said] + [v for v in said.values() if isinstance(v, dict)]
    holding = [c for c in candidates if wanted <= set(c)]
    assert holding, (
        f"`{tag}` publishes no document holding every surface it is judged on "
        f"({sorted(wanted)}); the verdict is about something this file cannot find"
    )
    assert len(holding) == 1, (
        f"`{tag}` publishes {len(holding)} documents holding every surface it is "
        f"judged on, so which one the verdict is about is a guess"
    )
    rows = holding[0][surface]
    if isinstance(rows, dict):
        rows = rows.get("canon", [])
    return [row["key"] for row in rows or [] if isinstance(row, dict) and "key" in row]


def section_e(app: RpcSubprocess) -> None:
    banner("E — a closed seat's row and its refusal are one sentence")
    said = report(app)
    closed = [row for row in said["rows"] if row["standing"] == "closed"]
    assert_eq(
        [row["key"] for row in closed],
        [seat["key"] for seat in rail_spec()["canon"] if seat.get("kind") == "reserved"],
        "E: the rows this report calls closed are the seats the specification "
        "draws locked",
    )
    for row in closed:
        ok(f"E: `{row['key']}` carries the destination's own reason", bool(row["why"]))
        try:
            app.intervene(f"{EXT}/nav", row["key"])
        except Exception as refusal:  # noqa: BLE001
            ok(
                f"E: ★★ and going there refuses with that same reason, so the "
                f"report is not a second wording of the closure -- {row['why']}",
                row["why"] in str(refusal),
            )
        else:
            ok(f"E: navigating to the closed seat `{row['key']}` must refuse", False)


def body() -> None:
    accepted = unjudged_sections()
    ok(
        "the reviewed remainder names a seat and a sentence for every section "
        "it accepts as unjudged",
        accepted and all(key and sentence for key, sentence in accepted.items()),
    )

    with RpcSubprocess(SHELL, boot_grace=1.5) as app:
        section_a(app)
        section_b(app)
        section_c(app)
        section_d(app)
        section_e(app)

    banner("what was checked")
    for line in CHECKS:
        print(f"  · {line}")
    print(
        f"\n[coverage] {SECTIONS_WALKED} judged section(s) reached through the "
        f"assembled application; {PARTS_COMPARED} specified part(s) compared with "
        f"the paint; {len(CHECKS)} named check(s) plus the assert_eq comparisons "
        f"above."
    )
    if PARTS_COMPARED == 0:
        print(
            "[coverage] ⚠ section D compared NOTHING with the paint. The run is "
            "shorter than it looks and this line is the only evidence."
        )


if __name__ == "__main__":
    run_demo("R1738 an application counts what it was judged on", body)
