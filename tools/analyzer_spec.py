#!/usr/bin/env python3
"""R1730 — **the analysis tool's written specifications, read once.**

## What forced this module

R1728 made `docs/analyzer-rail-spec.json` the reviewed statement of what the
tool's navigation is, and two demos began reading it instead of carrying
literals. R1730 paid a divergence off — it built `keys` — and **five** demos
broke at once, every one of them on a seat list somebody had written out by
hand.

The lift is what stopped that repair from being the disease. Fixing those five
by teaching each of them to read the pin would have left **six** copies of the
loader and four of "which seats does the specification say are shut", which is
this project's mechanical-duplication case arrived at while repairing the
consequences of not having done it.

That is the mechanical-duplication case this project lifts on sight, and the
sharper reason is what the copies were for: two demos disagreeing about which
seats are shut would disagree about the same build, and the one that was never
run would be the one that was wrong. R1695's `elsewhere` expectation had in fact
been stale for two rounds and nothing said so.

## What lives here

The loaders and the three derived rosters every consumer wants. Nothing about a
running application — these read files in this repository, so a demo comparing
a running screen with one of them is comparing two things written by different
hands, which is the whole point of the specifications being separate artifacts.
"""

from __future__ import annotations

import json
import re
from collections.abc import Callable
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DOCS = ROOT / "docs"
RAIL_SPEC_PATH = DOCS / "analyzer-rail-spec.json"
KEYS_SPEC_PATH = DOCS / "analyzer-keys-spec.json"
PACKETS_SPEC_PATH = DOCS / "analyzer-packets-spec.json"
BOARD_SPEC_PATH = DOCS / "analyzer-board-spec.json"
SECTIONS_SPEC_PATH = DOCS / "analyzer-sections-spec.json"
DASHBOARD_SPEC_PATH = DOCS / "analyzer-dashboard-spec.json"
RESERVED_SPEC_PATH = DOCS / "analyzer-reserved-spec.json"


def rail_spec() -> dict:
    """The tool's navigation, as `docs/analyzer-rail-spec.json` states it."""
    return json.loads(RAIL_SPEC_PATH.read_text(encoding="utf-8"))


def board_spec() -> dict:
    """The board's palette row and its carry, as their pin states them (R1733)."""
    return json.loads(BOARD_SPEC_PATH.read_text(encoding="utf-8"))


def keys_spec() -> dict:
    """The key-pattern section's three surfaces, as their pin states them."""
    return json.loads(KEYS_SPEC_PATH.read_text(encoding="utf-8"))


def dashboard_spec() -> dict:
    """The dashboard's own surfaces, the opening board included."""
    return json.loads(DASHBOARD_SPEC_PATH.read_text(encoding="utf-8"))


def reserved_spec() -> dict:
    """The deferred register: what each locked seat is booked under, and — in
    `built` — which of those bookings a release has since DELIVERED."""
    return json.loads(RESERVED_SPEC_PATH.read_text(encoding="utf-8"))


def packets_spec() -> dict:
    """The capture viewer's six surfaces, as their pin states them (R1747).

    Read through here rather than opened by the one demo that wants it, for the
    reason in this module's own header: the first consumer of a pin is never the
    last, and five copies of `json.loads(path)` is how two demos came to
    disagree about one build.
    """
    return json.loads(PACKETS_SPEC_PATH.read_text(encoding="utf-8"))


def sections_spec() -> dict:
    """Which sections of the assembled application are judged, and the reason
    accepted for each that is not (R1738).

    Unlike its siblings this is a claim about THIS BUILD rather than about the
    behaviour reference, which is why it does not repeat the seat list: the
    population is `rail_spec`'s canon and this holds only the remainder.
    """
    return json.loads(SECTIONS_SPEC_PATH.read_text(encoding="utf-8"))


def unjudged_sections() -> dict[str, str]:
    """The sections the pin accepts as unjudged, keyed by seat, valued by the
    sentence the running application is expected to publish for each.

    A dict rather than a list because the gate asserts an EQUALITY against the
    application's own unjudged rows: a section that starts answering must have
    its entry deleted here, and an entry left behind has to fail as loudly as a
    section that went silent.
    """
    return {
        entry["key"]: entry["sentence"] for entry in sections_spec()["unjudged"]["owed"]
    }


def mechanism_remainders() -> list[dict]:
    """★★★★★ (R2043) The remainders about the APPLICATION's own machinery.

    The sibling list `unjudged.owed` is read by six files and asserted equal to
    what the running application publishes, so it shrinks when the work is done
    and fails loudly when an entry is left behind. This one had NO reader at
    all — measured at R1888 and again at R2043, zero files mentioned it — and
    what that cost is on the record: an entry was deleted from it while the
    thing it describes was still true.

    A remainder here is not about a section, so there is nothing in the running
    application to compare it against. What it carries instead is `refuted_by`:
    a needle that will APPEAR in the tree once the work is done. `paid_off`
    below runs it, which is what turns this list from prose into a ratchet.
    """
    return list(sections_spec()["it_is_the_application"]["owed"])


def paid_off(entry: dict, read: Callable[[Path], str]) -> list[str]:
    """Where `entry`'s refutation needle is FOUND — the places that say it is
    paid and the entry must be deleted.

    Pure in `entry` and `read`: the oracle is the caller's, so the rule is
    testable against a fixture rather than only against this tree.
    """
    needle = entry["refuted_by"]["pattern"]
    found: list[str] = []
    for where in entry["refuted_by"]["in"]:
        here = ROOT / where
        for path in sorted(here.rglob("*.rs")) if here.is_dir() else [here]:
            if re.search(needle, read(path)):
                found.append(str(path.relative_to(ROOT)))
    return found


def rail_keys() -> list[str]:
    """Every seat the reference draws, in the reference's order."""
    return [seat["key"] for seat in rail_spec()["canon"]]


def owed_keys() -> list[str]:
    """The seats the reference has working and this build has not written.

    Sorted, because every consumer compares it against a sorted census.
    """
    return sorted(entry["key"] for entry in rail_spec()["owed"])


def ahead_keys() -> list[str]:
    """The seats the reference draws LOCKED and this build opens anyway.

    ★★★★★ R1953 — the other direction, and a separate list because it is a
    separate claim. R1947 and R1948 put entries pointing this way into `owed`,
    and the entry itself said so: *THE FIRST ENTRY HERE THAT POINTS THE OTHER
    WAY*. The prose was right and it was not enforceable — `closed_keys` below
    went on classifying both seats as shut while the live rail opened them, and
    fourteen demos went red for four pushes.
    """
    return sorted(entry["key"] for entry in rail_spec()["ahead"])


def divergences() -> list[dict]:
    """EVERY declared difference between this build's rail and the reference's,
    in the order the entries were written.

    ★★★★★ R1953 — the roster for *"every way the application differs from the
    reference is a way somebody wrote down"*. That assertion wants both
    directions and nothing else; [`closed_keys`] wants only the direction that
    makes a seat shut. Reading one list for both questions is what R1947 did,
    and the two answers had already come apart.
    """
    rail = rail_spec()
    return list(rail["owed"]) + list(rail["ahead"])


def reserved_keys() -> list[str]:
    """The seats the reference itself draws locked, booked under a requirement
    of a release that has not shipped."""
    return sorted(
        seat["key"] for seat in rail_spec()["canon"] if seat.get("kind") == "reserved"
    )


def closed_keys() -> list[str]:
    """What THIS BUILD is expected to declare shut.

    Not "everything the specification says is shut", which is what this said
    until R1953 and what its consumers never meant: every consumer compares the
    answer with a running rail. The specification describes the REFERENCE, and
    the two directions this build differs from it in are declared —
    [`owed_keys`] (behind) adds a seat, [`ahead_keys`] (ahead) removes one.

    A key in both lists would be a contradiction and a key in neither is not
    possible here, because the answer is derived; `selftest` refuses the first
    and checks the partition covers the rail.
    """
    shut = (set(owed_keys()) | set(reserved_keys())) - set(ahead_keys())
    return sorted(shut)


def closed_kinds() -> list[str]:
    """The `kind` words the shut seats carry, derived from WHICH seats are shut.

    ★ R1953 — a shut seat says which kind of shut it is (`reserved` for a
    requirement the reference itself books, `unbuilt` for a section this build
    owes), and the set of words on screen is a consequence of
    [`closed_keys`] rather than a constant. Two demos wrote `{"reserved", …}`
    with `reserved` unconditional, which was true while a reserved seat was
    always shut and became false the round this build opened both of them.
    """
    shut = set(closed_keys())
    kinds = set()
    if shut & set(reserved_keys()):
        kinds.add("reserved")
    if shut & set(owed_keys()):
        kinds.add("unbuilt")
    return sorted(kinds)


def open_keys() -> list[str]:
    """What is left: the seats this build is expected to open."""
    shut = set(closed_keys())
    return sorted(key for key in rail_keys() if key not in shut)


def opening_board() -> list[str]:
    """The cards the dashboard opens with, in board order, as the SPECIFICATION
    states them — `packet#0`-style seat ids, position included.

    ★★★★★ R1846 — this exists because three demos wrote the board out by hand
    and R1843 promoted a sixth card onto it. All three broke, none was run for
    46 rounds, and the promotion itself was correct: the board is what moved,
    and a demo that pins it as a literal fails for the release plan WORKING.
    R1797 had already written that sentence into `r1694` — and then left two
    literals standing on the next line.

    ⚠ Still not the running application's answer, which is the point of this
    module: a demo comparing a screen with a document written by another hand
    is the comparison. A demo comparing a screen with itself agrees with
    whatever the screen happens to say.
    """
    return [seat["key"] for seat in dashboard_spec()["board"]["canon"]]


def opening_kinds() -> list[str]:
    """The same board with each seat's POSITION dropped — `packet#0` -> `packet`.

    The catalogue is keyed by kind and the board by seat, and every consumer
    that wants to ask "is this kind placed this release?" was splitting the id
    itself.
    """
    return [key.split("#", 1)[0] for key in opening_board()]


def reserved_palette_kinds() -> list[str]:
    """The palette seats a later release still books, in the register's order.

    DERIVED as *deferred minus built*, because a seat leaves this list by being
    BUILT and the register records that separately — R1843's `health` entry
    sits in `built` while requirement 18 stays in `deferred`, since the rail's
    `sessions` seat cites the same requirement and is still locked. Reading
    `deferred` alone would report a seat this release places as reserved.
    """
    built = {entry["seat"] for entry in reserved_spec()["built"]}
    return [
        entry["seat"]
        for entry in reserved_spec()["deferred"]
        if entry.get("where") == "palette" and entry["seat"] not in built
    ]


def palette_bookings() -> dict[str, str]:
    """Each still-reserved palette seat and the booking it refuses under, in the
    wording a screen puts in front of a person."""
    # ⚠ Not every deferred entry names a seat — requirement 15 books decoding a
    # payload in a user-supplied format, which is `where: in-place` and has no
    # palette tile to sit on. Keying the register by seat without saying so
    # raises on the first entry that is a capability rather than a widget.
    deferred = {
        entry["seat"]: entry for entry in reserved_spec()["deferred"] if "seat" in entry
    }
    return {
        seat: f"requirement {deferred[seat]['requirement']}"
        for seat in reserved_palette_kinds()
    }


#: ★★★★★ R1964 — the keys of a specification that list a way this build
#: DIFFERS from the reference the specification is of.
#:
#: `owed` is *behind* and `ahead` is *in front*; both are divergences and both
#: are subtracted from a reproduction count. Naming them together is the whole
#: point: R1947 and R1948 added `ahead` to the rail and every reader that had
#: written `len(canon) - len(owed)` by hand went on omitting it.
DIVERGENCE_LISTS = ("owed", "ahead")

#: Keys of a specification that are lists and are NOT divergences from it.
#:
#: `canon` is the thing being measured against. `second_phase_owed` is a
#: remainder against a DIFFERENT reference (R1946's behaviour prototype), so
#: folding it in would subtract one reference's gap from another's count.
#:
#: ⚠ Declared rather than inferred, because the alternative is a fall-through
#: that treats an unrecognised list as *not a divergence* — which is the
#: direction that hides work, and exactly how `ahead` went unread for
#: seventeen rounds.
NOT_A_DIVERGENCE = ("canon", "second_phase_owed")


def declared_divergences(spec: dict) -> list[dict]:
    """Every divergence a specification declares, of every kind it has.

    # Why this exists

    ★★★★★ R1964 — the arithmetic *reproduced = specified − diverging* was
    spelled at FOUR sites, two in the shell and two in `r1730`, and the two in
    the demo held their own copy reading only `owed`. R1947 gave the rail a
    second kind of divergence and the copies could not see it: the demo asserted
    `6 == 8 - 0` and CI stayed red for five pushes, reported as *expected 8, got
    6* — which reads like two missing sections and is nothing of the kind. Both
    seats are built, open and painting; the build is AHEAD of the scope mockup.

    So the count is derived here, once, from every declared kind.

    # Raises

    `KeyError` when the specification carries a list this does not classify. An
    unclassified list is RED rather than ignored: a third kind of divergence
    added later must stop a reader that has not been taught about it, not be
    silently left out of its count.
    """
    unknown = sorted(
        key
        for key, value in spec.items()
        if not key.startswith("$")
        and isinstance(value, list)
        and key not in DIVERGENCE_LISTS
        and key not in NOT_A_DIVERGENCE
    )
    if unknown:
        raise KeyError(
            f"{unknown} is a declared list this does not classify — say whether "
            f"it is a divergence (add it to DIVERGENCE_LISTS) or is not (add it "
            f"to NOT_A_DIVERGENCE). Leaving it out would drop it from every "
            f"reproduction count that reads this."
        )
    found: list[dict] = []
    for key in DIVERGENCE_LISTS:
        found.extend(spec.get(key, []))
    return found


def reproduced(spec: dict) -> int:
    """How many of a specification's seats this build reproduces exactly.

    The one statement of the arithmetic on this side of the wire. The shell
    publishes its own (`specified - divergences.len()`), and a demo comparing
    the two is comparing two derivations of one rule rather than a rule with a
    copy of itself — which is what makes the comparison a gate.
    """
    return len(spec["canon"]) - len(declared_divergences(spec))


def surfaces(spec: dict) -> list[str]:
    """The surfaces a section's specification fixes, by the name it gives them.

    Takes the document rather than reading one, because there is more than one:
    R1730 wrote this bound to the key-pattern pin and R1731's section grew a
    copy of it within the round — the same duplication, one level down, and
    caught by the close audit rather than by the next round.

    Keys beginning with `$` are the document's own commentary and are not
    surfaces, which is the rule `pinion_core::conformance::SpecDocument` takes.
    """
    return sorted(key for key in spec if not key.startswith("$"))


def selftest() -> int:
    """★★★★★ R1953 — **the rail's two divergence lists are classified, and the
    classification is refused when it cannot be.**

    This module derived four rosters from one specification and nothing ever
    asked whether the specification was self-consistent. It was not: R1947 and
    R1948 wrote entries meaning *this build is AHEAD* into the list meaning
    *this build is BEHIND*, [`closed_keys`] concatenated the two, and fourteen
    demos asserted a live rail against a specification that classified two open
    seats as shut. The red survived four pushes because the demo sweep does not
    gate one.

    ⚠ Every check below has a path to failing — each is exercised in the
    round's mutation log by editing the pin — and none of them is a count that
    an empty list satisfies: the partition check compares SETS against the
    rail, so a specification that emptied both lists still has to account for
    every seat.
    """
    rail = rail_spec()
    problems: list[str] = []

    seats = set(rail_keys())
    owed, ahead, reserved = set(owed_keys()), set(ahead_keys()), set(reserved_keys())

    both = sorted(owed & ahead)
    if both:
        problems.append(
            f"{both} are declared BOTH owed (the reference has it, this build "
            f"does not) and ahead (the reference locks it, this build opens "
            f"it). One seat cannot diverge in two directions at once."
        )

    stray = sorted(ahead - reserved)
    if stray:
        problems.append(
            f"{stray} are declared ahead of the reference, but the reference "
            f"does not draw them locked — so there is nothing to be ahead OF. "
            f"Either the seat's `kind` in `canon` is wrong or the entry is."
        )

    for name, keys in (("owed", owed), ("ahead", ahead), ("reserved", reserved)):
        unknown = sorted(keys - seats)
        if unknown:
            problems.append(f"`{name}` names {unknown}, which the rail does not draw")

    partition = set(closed_keys()) | set(open_keys())
    if partition != seats:
        missing, extra = sorted(seats - partition), sorted(partition - seats)
        problems.append(
            f"shut and open do not cover the rail: unaccounted {missing}, "
            f"invented {extra}. A seat in neither is not a pass, it is a seat "
            f"nobody classified."
        )

    for entry in rail["owed"] + rail["ahead"]:
        for field in ("key", "sentence", "since", "why"):
            if not entry.get(field):
                problems.append(f"divergence {entry.get('key')!r} has no `{field}`")

    # ★★★★★ R1964 — [`declared_divergences`] must gather EVERY declared kind,
    # and must refuse a kind it has not been taught.
    #
    # The defect it exists for: two readers spelled `len(canon) - len(owed)` by
    # hand, R1947 gave the rail a second kind, and both copies went on omitting
    # it — `r1730` asserted `6 == 8 - 0` and CI was red for five pushes under a
    # sentence that reads like two missing sections. So the derivation is
    # checked here against the pin's own arrays, and the refusal is checked
    # too: an unclassified list must stop a reader rather than be left out of
    # its count, which is the direction that hides work.
    gathered = sorted(e["key"] for e in declared_divergences(rail))
    both_ways = sorted(owed | ahead)
    if gathered != both_ways:
        problems.append(
            f"the derived divergence set {gathered} is not the pin's own "
            f"{both_ways} — a kind is being dropped from every reproduction "
            f"count that reads it"
        )
    if reproduced(rail) != len(rail["canon"]) - len(both_ways):
        problems.append(
            f"`reproduced` is {reproduced(rail)}, which is not "
            f"{len(rail['canon'])} seats less its {len(both_ways)} divergence(s)"
        )
    try:
        declared_divergences({"canon": [], "owed": [], "ahead": [], "invented": []})
    except KeyError:
        pass
    else:
        problems.append(
            "a specification carrying a list this does not classify was "
            "accepted — an unrecognised divergence kind must be RED, because "
            "falling through treats it as *not a divergence* and drops it"
        )

    # ★★★★★ (R2043) The machinery remainders, and the rule that makes the list
    # a ratchet: an entry declares a needle that will appear when the work is
    # done, and finding it is a REFUSAL — the entry has to go.
    def source_of(path: Path) -> str:
        return path.read_text(encoding="utf-8", errors="replace")

    mechanisms = mechanism_remainders()
    for entry in mechanisms:
        for field in ("key", "sentence", "since", "why", "refuted_by"):
            if field not in entry:
                problems.append(
                    f"the machinery remainder {entry.get('key', '?')!r} carries no "
                    f"{field!r} — an entry nothing can refute is prose again"
                )
        if "refuted_by" not in entry:
            continue
        where = paid_off(entry, source_of)
        if where:
            problems.append(
                f"the machinery remainder {entry['key']!r} is PAID: its refutation "
                f"appears in {', '.join(where)} — delete the entry rather than "
                "leaving a paid remainder that reads like an unpaid one"
            )
    # ★ Both directions of `paid_off`, against fixtures, because this tree's
    # own answer is one-sided: today every entry is unpaid, so a rule that only
    # ran here would never have seen the refusal arm at all.
    # ⚠ A FILE, not a directory: the directory arm globs `*.rs`, so a fixture
    # pointed at a folder with none reads nothing and the needle can never be
    # found. The first draft did exactly that and the refusal arm reported
    # itself dead — which is the assertion working.
    fixture = {
        "key": "fixture",
        "refuted_by": {
            "pattern": "MEMOISED",
            "in": ["crates/pinion-core/src/conformance.rs"],
        },
    }
    if paid_off(fixture, lambda _p: "nothing here"):
        problems.append("a needle absent everywhere must answer nothing")
    if not paid_off(fixture, lambda _p: "a MEMOISED thing"):
        problems.append("a needle present must be FOUND — the refusal arm is dead")

    for problem in problems:
        print(f"analyzer_spec: {problem}")
    if problems:
        print(f"analyzer_spec: {len(problems)} problem(s)")
        return 1
    print(
        f"analyzer_spec selftest: {len(seats)} seat(s), "
        f"{len(owed)} owed, {len(ahead)} ahead, {len(reserved)} reserved, "
        f"{len(closed_keys())} shut, {len(open_keys())} open -- OK"
    )
    return 0


if __name__ == "__main__":
    import sys

    if "--selftest" in sys.argv:
        raise SystemExit(selftest())
    raise SystemExit("usage: analyzer_spec.py --selftest")
