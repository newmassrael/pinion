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
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from analyzer_spec import (  # noqa: E402
    ahead_keys,
    closed_keys,
    divergences,
    rail_keys,
    rail_spec,
    unjudged_sections,
)
from analyzer_spec import reserved_keys as reserved_rail_keys  # noqa: E402
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
KEYS_SECTION = "hello-key-patterns"
EXT = "/external"

#: ★ R1770 — a window at which every section of this tool is given the width it
#: declares it lays out at. See the comment in `body` for the measurement.
LAB_WINDOW = (1800, 900)

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
    # ★★★★★ R1761 — this read `("tag" in row) == (row["standing"] in {"judged",
    # "unspecified"})`, which was a true statement about a world where the only
    # way to be judged was to be a mounted binding. It stopped being true the
    # round a page the HOST paints started answering for itself: that row is
    # `judged` and has no tag, because there is no separate surface to address.
    #
    # The claim it was reaching for is about mounting, so it now asks the fact
    # about mounting — published on the roster's own wire since R1724 — instead
    # of inferring it from a standing that no longer implies it.
    mounted = {
        row["key"]
        for row in app.query(f"{EXT}/destinations")["destinations"]
        if row["mounted"]
    }
    ok(
        "A: a row carries the tag its section is addressed by exactly when a "
        "screen is mounted there -- a page the host paints itself is judged "
        "with no tag, because there is nothing else to ask",
        all(("tag" in row) == (row["key"] in mounted) for row in said["rows"]),
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
    # ★★★★★ R1762 — this read `conforms is False and unjudged > 0`, and the
    # second half was the rule being exercised: while a section was unjudged,
    # conformance was refused. R1762 judged the last one, so `unjudged` is 0 and
    # this demo can no longer be the place that rule is measured — it is a unit
    # test of `pinion_screen::ApplicationConformance` instead, where a fixture
    # can hold an unjudged section on purpose.
    #
    # What is still measurable here, and is the sharper fact: conformance is
    # refused ANYWAY, because a verdict is about a frame and the sections a
    # reader is not looking at are away. So the count reaching zero did not buy
    # a `conforms: true`, and the reason it did not is worth reading.
    ok(
        "B: ★★★★★ the application does NOT report conformance -- with every "
        "section judged, what refuses it is that a verdict is about a FRAME and "
        "the sections nobody is looking at are away",
        said["conforms"] is False,
    )
    print(
        f"  [refusal] unjudged {said['unjudged']}, declared {said['declared']}, "
        f"reproduced {said['reproduced']} of {said['specified']}"
    )
    # ★★★★★ R1953 — *what it always said* is the DECLARED divergence list, not
    # the empty list.
    #
    # This spelled it `== []`, which was the same thing while the rail owed the
    # reference nothing in either direction. R1947 and R1948 then declared this
    # build AHEAD of the scope mockup at two seats, so the seat-level verdict
    # reports two differences — correctly, and this read the correctness as a
    # correction of the sentence above it.
    assert_eq(
        [d["says"] for d in app.query(f"{EXT}/conformance")["divergences"]],
        [entry["sentence"] for entry in divergences()],
        "B: ★★ and the seat-level verdict still says what it always said, so "
        "this is a second sentence rather than a correction of the first",
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
    # ★ R1761 — the first judged row is the DASHBOARD now, and it has no wire of
    # its own to be out of reach: a page the host paints is answered for by the
    # host. The claim below is about a section with a surface to address, so the
    # population is the judged rows that have one.
    addressable = [row for row in judged if "tag" in row]
    ok("C: some judged section is a mounted screen with a wire of its own", addressable)
    ok(
        "C: ★★★★★ standing at `dashboard`, a section's own wire is out of reach "
        "-- so a client CANNOT build this report by asking the sections",
        app.query(f"{EXT}/nav") == "dashboard"
        and unreachable(app, addressable[0]["tag"]),
    )
    ok(
        "C: ★★★★★ and yet the host answers for every one of them from here -- "
        "which is what makes a verdict about the application possible without "
        "navigating it",
        all(row.get("conformance") for row in judged),
    )
    # ★★★★★ R1758 — and every one of them says WHAT IT WAS READ FROM. Measured
    # here at R1747 and again at R1758 before the repair: two of these four
    # reported every part of their specification reproduced from a page they had
    # not painted a frame on, because their verdict came from their own tables.
    # A count with no qualifier could not tell those two from the two answering
    # honestly, and this row is the qualifier.
    ok(
        "C: ★★★★★ and every judged section's verdict is about a PAINTED FRAME "
        "-- a verdict read from a screen's own tables cannot fail for the "
        "reason judging exists",
        [row["conformance"]["evidence"] for row in judged]
        == ["paint"] * len(judged),
    )
    ok(
        "C: ★★★★★ so a section that is not showing reports its surfaces AWAY "
        "rather than reproduced -- this is the exact row that was wrong",
        all(
            row["conformance"]["reproduced"] == 0
            and row["conformance"]["away"] == len(row["conformance"]["surfaces"])
            for row in judged
            if not row["showing"]
        ),
    )
    ok(
        "C: ★ and the application refuses to call that conformance",
        said["conforms"] is False and said["declared"] == 0,
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
        # ★ R1761 — the frame, not the tick: the verdict read next is a fact
        # about the last PAINTED frame.
        app.intervene_painted(f"{EXT}/nav", row["key"])
        here = next(r for r in report(app)["rows"] if r["key"] == row["key"])
        ok(
            f"C: ★★ the report says `{row['key']}` is the section showing, so a "
            f"reader knows which frame the verdict beside it is about",
            here["showing"] is True,
        )
        # ★ R1761 — a page the host paints has no wire of its own, and the
        # absent `tag` is the row saying so rather than an omission. The
        # comparison below is the one that only exists for a mounted screen;
        # for the host's own page the checkable claim is the other one — that
        # there is no second place this verdict is published from.
        if "tag" in row:
            own = app.query(f"/{row['tag']}/external/conformance")
            assert_eq(
                own,
                here["conformance"],
                f"C: ★★ the host's row for `{row['key']}` IS the value the "
                f"section publishes on its own wire -- the host aggregates, it "
                f"does not re-derive",
            )
        else:
            own = here["conformance"]
            ok(
                f"C: ★★ `{row['key']}` is a page the host paints, so its "
                f"verdict has exactly one publisher: no surface of its own "
                f"answers, even standing in it",
                app.query(f"{EXT}/nav") == row["key"]
                and unreachable(app, row["key"]),
            )
        assert_eq(
            here["conformance"]["specified"],
            sum(surface["specified"] for surface in own["surfaces"].values()),
            f"C: and `{row['key']}`'s totals are its surfaces added up",
        )
        ok(
            f"C: ★★★★★ and standing IN `{row['key']}` its surfaces report "
            f"STANDING -- the verdict moved when the frame did, which is what "
            f"says it is about the frame",
            here["conformance"]["standing"] > 0,
        )

    # One standalone binary, so the claim "one build, two placements" is checked
    # against a second PROCESS and not only against a second slot of this one.
    #
    # ★★★★★ R1758 — the host's row is taken WHILE STANDING IN the section. It
    # used to be taken from the dashboard, which worked only because the section
    # answered from tables that do not change with the frame; now that the
    # verdict is about the paint, a row read from another page is about a frame
    # that is not in the application any more. Same correction R1742 had to make
    # one assertion earlier, found by running this old gate after the change.
    app.intervene_painted(f"{EXT}/nav", "keys")
    keys_row = next(r for r in report(app)["rows"] if r["key"] == "keys")
    with RpcSubprocess(KEYS_SECTION, boot_grace=1.5) as standalone:
        alone = standalone.query(f"{EXT}/conformance")
        page = keys_row["conformance"]
        # ★★★★★ R1770 — the verdicts are compared APART FROM the size each was
        # read at, and the sizes are then asserted to DIFFER. Until that round
        # this was a plain equality and it passed, because neither verdict said
        # what extent it came from: one process was reading its whole window and
        # the other a page inside a bigger one, and nothing in either answer
        # could tell you so. Dropping the qualifier and asserting it separately
        # keeps the claim this check exists for -- one build, two placements --
        # and adds the one it could not make.
        assert_eq(
            without_extent(alone),
            without_extent(page),
            "C: ★★★★★ and the standalone binary of that section publishes the "
            "SAME verdict -- a section that conforms in its own window and is "
            "never asked as a page would be two builds wearing one name",
        )
        ok(
            f"C: ★★★★★ and they were read at DIFFERENT sizes -- standalone "
            f"{alone['at']}, as a page {page['at']} -- which is exactly why the "
            "sameness above is a claim about the build and not about the window",
            alone["at"] != page["at"] and alone["at"] and page["at"],
        )
        ok(
            "C: ★★ while both name the same canon extent, so 'the same verdict' "
            "is about the same specification read the same way",
            alone["written_at"] == page["written_at"] is not None,
        )
    app.intervene_painted(f"{EXT}/nav", "dashboard")


def section_d(app: RpcSubprocess) -> None:
    banner("D — every part of every judged section, read back from the paint")
    global PARTS_COMPARED
    said = report(app)
    judged = [row for row in said["rows"] if row["standing"] == "judged"]
    # ★ R2051 — the address a rail seat is painted under, recovered from one the
    # application publishes rather than spelled here.
    seat_tag = address_prefix(app.query(f"{EXT}/spec")["rail"])
    for row in judged:
        # ★ R1761 — the frame, not the tick: the verdict read next is a fact
        # about the last PAINTED frame.
        app.intervene_painted(f"{EXT}/nav", row["key"])
        assert_eq(app.query(f"{EXT}/nav"), row["key"], f"D: the `{row['key']}` seat opens")
        rects = abs_rects_of(app.snapshot(source="paint"))
        # ★ R1761 — a mounted section paints under its own root tag; a page the
        # host paints has no such tag, and what says its section is there is
        # the host's page region carrying marks. Two cases because there are
        # two kinds of section, not because one is a special case.
        #
        # 🟥 And the old form was `startswith(f"{row['tag']}.") or "." in tag`,
        # which any dotted tag on the screen satisfied — so it passed for every
        # section whatever was painted. Measured while narrowing it: a mounted
        # screen's marks carry ITS OWN tag prefixes (`pv.filter.query`), not its
        # root's, so the first disjunct was never the one doing the work. What
        # is true and checkable is that the guest's ROOT is on the frame.
        ok(
            f"D: arriving at `{row['key']}` paints its section inside the host",
            row["tag"] in rects
            if "tag" in row
            else any(tag.startswith("shell.canvas") for tag in rects),
        )
        for chrome in ("shell.appbar", "shell.rail", f"{seat_tag}{row['key']}"):
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
        #
        # ★★★★★ R1758 — and the canon comes out of the VERDICT now, which
        # deleted a rule this file used to need. A verdict said how many parts
        # were specified and not which, so the canon had to be fetched from
        # whatever the section published beside it and identified by "the
        # sub-document holding every surface" — itself a repair, made at R1747
        # after a screen's own `context` table collided with its pin's `context`
        # surface. A verdict that names its own parts has nothing to search and
        # nothing to disambiguate.
        surfaces_said = row["conformance"]["surfaces"]
        parts_by_surface = {
            surface: [part["key"] for part in surfaces_said[surface]["canon"]]
            for surface in surfaces_said
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


# ★★★★★ R1758 — `parts_of` used to be HERE, and deleting it is the round's
# smallest visible result and one of its clearest.
#
# It answered *which parts one surface of a section is specified to have* by
# fetching `/{tag}/external/spec` and hunting for the right sub-document, because
# a verdict said how MANY parts were specified and never which. That hunt needed
# two repairs of its own: R1742 taught it to look one level down (a section may
# publish its specification nested, and the flat read answered `[]` for every
# surface while the printed coverage stayed put), and R1747 replaced name
# matching with "the sub-document holding every surface" after a screen's own
# `context` table collided with its pin's `context` surface — one word, two
# documents.
#
# A verdict that carries its own canon has nothing to search and nothing to
# disambiguate, so all of it goes.
#
# ⚠ WHAT WENT WITH IT, named rather than left for the next reader to notice:
# the two assertions the lookup made on the way past — that a judged section
# publishes AT LEAST ONE document holding every surface it is judged on, and
# AT MOST one. Both were preconditions of the search, not claims anybody
# needed: they existed so this file could find the canon, and the canon is
# in the verdict now. The claim they were standing in for — *a client can ask
# a running section what its verdict is about without reading this
# repository* — is answered more directly than before, by the section itself.
# What is no longer checked anywhere is the SHAPE a section publishes its
# specification in on the `spec` slot, and nothing depends on that shape now.


def section_e(app: RpcSubprocess) -> None:
    banner("E — a closed seat's row and its refusal are one sentence")
    said = report(app)
    closed = [row for row in said["rows"] if row["standing"] == "closed"]
    # ★★★★★ R1953 — the seats the reference draws locked LESS the ones this
    # build declares itself ahead on.
    #
    # This read the canon's `reserved` seats straight, which was the same set
    # while every seat the reference locks was one this build locked too.
    # R1947 and R1948 opened both and declared the difference, so the report
    # calls nothing closed — correctly — and this asked for two rows.
    assert_eq(
        [row["key"] for row in closed],
        [key for key in rail_keys() if key in set(closed_keys())],
        "E: the rows this report calls closed are the seats the specification "
        "draws locked and this build has not opened",
    )
    if not closed:
        # The empty case says what makes it empty rather than passing over
        # (R1651.1): every seat the reference locks is one this build declares
        # itself ahead on, which is a fact rather than an absence of one.
        assert_eq(
            sorted(reserved_rail_keys()),
            sorted(ahead_keys()),
            "E: ★ nothing is closed because this build opens every seat the "
            "reference locks, and says so",
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
    # ★★★★★ R1762 — this read `accepted and all(...)`, which required the
    # remainder to be NON-EMPTY. That was a guard against a vacuous check and it
    # was a true statement about a world where some section was unjudged; the
    # round that judged the last one made it fail. The claim it was reaching for
    # is that every entry is well formed, which an empty remainder satisfies —
    # and what keeps the check from being vacuous is section B, which asserts
    # the remainder EQUALS the application's own unjudged rows either way.
    ok(
        "the reviewed remainder names a seat and a sentence for every section "
        "it accepts as unjudged",
        all(key and sentence for key, sentence in accepted.items()),
    )
    if not accepted:
        print(
            "  · the remainder is EMPTY — every section of this application is "
            "judged, and section B asserts that as an equality"
        )

    with RpcSubprocess(SHELL, boot_grace=1.5) as app:
        # ★★★★★ R1770 — taken at a window where every section is given the width
        # it declares it lays out at. The node lab declares 1625 and clips below
        # it; the shell keeps 52 of the window for its own chrome; so at the
        # 1440x900 this tool opens in that section is handed 1388 and DECLINES
        # to be judged, in a sentence naming both numbers. This demo stands in
        # it and asks what it reproduces, which is a question about a whole
        # frame — so it asks at a window where the frame is whole. Before R1770
        # nothing could tell the two situations apart and this walk was reading
        # a clipped screen without knowing it.
        resize_and_settle(app, LAB_WINDOW)
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
