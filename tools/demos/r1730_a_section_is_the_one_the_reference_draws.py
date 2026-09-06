#!/usr/bin/env python3
"""R1730 §5.27 §5.38 §5.40 §2 #2 §2 #7 — **a section's surfaces are the ones
that were specified for it, and where they are not, it says so.**

# What this demo exists for

R1728 wrote the analysis tool's *navigation* down as a reviewed artifact and
made something fail when the application stopped matching it. The mechanism
found three real defects in its first three runs, and it could reach exactly one
thing: the rail. A list's columns, a pane's sections, a header's parts — every
surface a screen is actually made of — had no way to be checked at all, while
the reason the rail had been wrong for several hundred rounds (nothing compared
it with anything) applied to all of them unchanged.

`pinion_core::conformance` is the part of that mechanism that was never about
navigating, and this section is its first consumer, three times over. What it
drives:

* **A** — the specification is a FILE (`docs/analyzer-keys-spec.json`), read
  here from the repository, and the running section publishes how much of each
  of its three surfaces it reproduces. Both sides of every comparison come from
  different places, or the comparison is the application agreeing with itself.
* **B** — the list on the screen: seven columns in the specified order, eight
  declarations, each pressed with the **machine's own pointer**, and the record
  pane follows the press.
* **C** — the filter is live and the summary is DERIVED: narrowing the list
  changes the sentence above it, and a row that went away names the clause that
  dropped it.
* **D** — the reference's own action out of the section is drawn and REFUSES,
  with the reason `docs/analyzer-rail-spec.json` gives the seat it leads to —
  so the affordance is reproduced without becoming a promise the specification
  does not keep.
* **E** — and mounted in the shell it is a page of the one application: the
  third rail seat opens, the section paints inside the host's chrome, and the
  rail's own declared remainder drops from two to one.

# Floor, measured by building a probe against 6.11.1 and running it

The probe builds a seven-column table view and asks its header and its model
what they can say about a column roster.

* A column has **no key**. Six members across the header and the model take a
  name at all and all six name the *object* — its object name, its window
  title, its style sheet. A column is an ordinal and a title.
* **Zero** members across the table view, the header and the model name a
  specification, an expectation or a divergence. There is nothing to write the
  statement in.
* And the row that decides it: move section 0 to position 3, and a check
  written against the model **still passes** — the model answers the specified
  order while what the reader is looking at has changed. A conformance check is
  only worth writing if it fails when the product stops matching, and there the
  most natural place to write one cannot see the difference a person can.

Run from the workspace root (a real pointer needs a display):
    cargo build --release -p hello-key-patterns -p hello-analyzer-shell
    DISPLAY=:97 python3 tools/demos/r1730_a_section_is_the_one_the_reference_draws.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from analyzer_spec import (  # noqa: E402
    declared_divergences,
    keys_spec,
    rail_spec,
    reproduced,
    surfaces,
)
from rpc_verify import (  # noqa: E402
    RealPointer,
    RealPointerUnavailable,
    RpcSubprocess,
    abs_rects_of,
    address_prefix,
    assert_eq,
    run_demo,
)

SECTION = "hello-key-patterns"
SHELL = "hello-analyzer-shell"
EXT = "/external"

CHECKS: list[str] = []

#: How many real-pointer sessions actually opened. Zero means this host could
#: not drive one, and the coverage line at the end says so rather than letting a
#: shorter run read as a pass (R1727.2).
REAL_POINTER_RUNS = 0


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def ok(what: str, condition: bool) -> None:
    CHECKS.append(what)
    assert condition, what


def q(app: RpcSubprocess, path: str):
    return app.query(f"{EXT}/{path}")


#: The reviewed artifacts, read from the repository rather than from the
#: application: both sides of every comparison below must come from different
#: places, or the comparison is the application agreeing with itself. The
#: loaders live in `tools/analyzer_spec.py` because six demos had grown a copy
#: of them by this round.


def pointer(app: RpcSubprocess):
    """A real pointer, or `None` with a loud line. Never a silent fallback to
    the wire: section B's claim is that a *person's* press reaches a row, and a
    run that could not make one must say so."""
    global REAL_POINTER_RUNS
    try:
        driver = RealPointer(app)
    except RealPointerUnavailable as exc:
        print(f"[real-pointer] UNAVAILABLE — section B is not driven: {exc}")
        return None
    REAL_POINTER_RUNS += 1
    return driver


def section_a(app: RpcSubprocess, spec: dict) -> None:
    banner("A — the section says how much of its specification it is")
    verdict = q(app, "conformance")
    # ★ R1758 — the slot publishes the WHOLE verdict now: what it was read from,
    # the totals, and the surfaces under a key of their own.
    ok(
        "A: ★★★★★ and it says what it was read from -- this section judges the "
        "frame it painted, not the tables it holds",
        verdict["evidence"] == "paint",
    )
    conformance = verdict["surfaces"]
    ok(
        "A: it reports every surface the specification fixes",
        sorted(conformance) == surfaces(spec),
    )
    for surface in surfaces(spec):
        canon = spec[surface]["canon"]
        owed = spec[surface]["owed"]
        row = conformance[surface]
        assert_eq(
            row["specified"],
            len(canon),
            f"A: the {surface} surface counts what it is measured against",
        )
        assert_eq(
            [d["says"] for d in row["divergences"]],
            [entry["sentence"] for entry in owed],
            f"A: ★★ every way the {surface} surface differs from the reference "
            "is a way somebody wrote down -- and EXACTLY those, so a divergence "
            "quietly paid off fails here too",
        )
        assert_eq(
            row["unreconciled"],
            [],
            f"A: the {surface} surface's difference reconciles with its ledger",
        )
        # R1964 — `reproduced(...)`, not `len(canon) - len(owed)` written out
        # again. The rail grew a SECOND kind of divergence at R1947 (`ahead`)
        # and both hand copies of this arithmetic went on omitting it; the
        # derivation subtracts every declared kind and refuses a kind it has
        # not been taught.
        assert_eq(
            row["reproduced"],
            reproduced(spec[surface]),
            f"A: and the {surface} reproduction is a number, not an impression",
        )

    published = q(app, "spec")
    assert_eq(
        [c["key"] for c in published["columns"]],
        [p["key"] for p in spec["columns"]["canon"]],
        "A: the columns the SCREEN publishes are the specified columns, in the "
        "specified order -- the row the floor cannot answer, where a column has "
        "no key at all and reordering the header changes nothing the model says",
    )
    assert_eq(
        [p["key"] for p in published["detail"]],
        [p["key"] for p in spec["detail"]["canon"]],
        "A: and so are the record pane's eleven parts",
    )


def section_b(app: RpcSubprocess, spec: dict) -> None:
    banner("B — the list on the screen, pressed by the machine's own pointer")
    rects = abs_rects_of(app.snapshot(source="paint"))
    columns = spec["columns"]["canon"]
    lefts = []
    for column in columns:
        tag = f"kp.column.{column['key']}"
        ok(f"B: the {column['key']} column is painted", tag in rects)
        lefts.append(rects[tag][0])
    ok(
        "B: ★ and they run left to right in the specified order -- a roster is a "
        "list and a header is a row, and the two agreeing is a separate fact",
        lefts == sorted(lefts) and len(set(lefts)) == len(lefts),
    )

    rows = int(q(app, "row_count"))
    assert_eq(rows, 8, "B: the section holds the reference's eight declarations")

    driver = pointer(app)
    if driver is None:
        return
    with driver as hand:
        pressed = 0
        for n in range(rows):
            tag = f"kp.list.row.{n}"
            if tag not in rects:
                continue
            rect = rects[tag]
            hand.move((rect[0] + rect[2] / 2, rect[1] + rect[3] / 2))
            hand.press()
            hand.release()
            app.tick(16)
            assert_eq(
                int(q(app, "selected_row")),
                n,
                f"B: a real press on declaration {n} opens its record",
            )
            record = q(app, "record")
            ok(
                f"B: and the record pane is showing declaration {n}",
                record["id"] == str(n + 1),
            )
            pressed += 1
        ok(f"B: all {pressed} painted declarations took a real press", pressed >= 8)


def section_c(app: RpcSubprocess) -> None:
    banner("C — the filter is live and the sentence above it is derived")
    app.invoke(f"{EXT}/filter", "")
    app.tick(8)
    whole = q(app, "summary")
    ok("C: unfiltered, the summary counts the whole section", whole.startswith("8 declared"))
    ok("C: and says how many resolved to a number only", whole.endswith("1 numeric-only"))

    app.invoke(f"{EXT}/filter", "direction in (declare publish)")
    app.tick(8)
    kept = q(app, "kept_rows")
    assert_eq(len(kept), 3, "C: the query keeps the declarations that publish")
    narrowed = q(app, "summary")
    ok("C: ★ and the summary FOLLOWED it", narrowed != whole)
    ok("C: reading the narrowed count", narrowed.startswith("3 of 8 declared"))

    hidden = q(app, "why_hidden")
    assert_eq(len(hidden), 5, "C: five declarations went away")
    ok(
        "C: ★★ and each says which clause dropped it -- the question the floor "
        "answers with an invalid index and nothing else",
        all(row["clause"] == "direction in (declare publish)" for row in hidden),
    )

    app.invoke(f"{EXT}/filter", "direction in (")
    app.tick(8)
    assert_eq(
        len(q(app, "kept_rows")),
        8,
        "C: a half-typed query keeps everything rather than flashing the "
        "section away and back",
    )
    ok("C: and the reason is not swallowed", bool(q(app, "query_fault")))
    app.invoke(f"{EXT}/filter", "")
    app.tick(8)


def section_d(app: RpcSubprocess) -> None:
    banner("D — the action out of the section refuses, and says what would open it")
    rail = rail_spec()
    target = q(app, "declarer")
    seat = next(s for s in rail["canon"] if s["key"] == target["section"])
    assert_eq(
        target["kind"],
        seat["kind"],
        "D: ★★ the action's refusal is the standing the RAIL specification "
        "gives the seat it leads to -- read from that file, not spelled twice",
    )
    assert_eq(seat["standing"], "closed", "D: and the reference locks that section itself")
    assert_eq(
        target["recourse"],
        "await_release",
        "D: so the reader is told to wait rather than that nothing can be done",
    )
    ok("D: the refusal names what the seat is booked under", "requirement" in target["detail"])

    app.invoke(f"{EXT}/show_declarer", "")
    app.tick(8)
    said = q(app, "said")
    ok("D: pressing it reaches the person", target["section"] in said)
    ok("D: with the reason", target["detail"] in said)
    ok("D: ★ and with the recourse, not a disabled bit", "release" in said)

    voice = app.voice()
    rows = {row["tag"]: row for row in voice.get("regions", voice.get("nodes", []))}
    ok(
        "D: and the action is announced at all",
        any(tag.startswith("kp.detail.declarer") for tag in rows),
    )


def section_e(spec: dict) -> None:
    banner("E — mounted, it is a page of the one application")
    rail = rail_spec()
    owed = rail["owed"]
    ok(
        "E: ★★★ the rail's declared remainder no longer holds `keys` -- an entry "
        "is DELETED when it is paid, because the gate asserts equality and a "
        "paid divergence left behind fails just as loudly as a new one",
        not any(entry["key"] == "keys" for entry in owed),
    )
    with RpcSubprocess(SHELL, boot_grace=1.5) as shell:
        rail_keys = shell.query(f"{EXT}/rail").split(",")
        assert_eq(
            rail_keys,
            [seat["key"] for seat in rail["canon"]],
            "E: the shell's rail is still the specified rail",
        )
        conformance = shell.query(f"{EXT}/conformance")
        # ★★★★★ R1964 — BOTH kinds of divergence, and the derivation rather
        # than a second copy of the shell's arithmetic.
        #
        # This read `len(canon) - len(owed)` and `owed` alone, and R1947 gave
        # the rail a kind it had never heard of: `ahead`, a seat the scope
        # mockup locks and this build OPENS. So it asserted `6 == 8 - 0` and CI
        # was red for five pushes under the sentence *expected 8, got 6* —
        # which reads like two sections nobody built, and is the opposite:
        # `hello-topology-view` (R1947) and `hello-sessions-view` (R1948) are
        # both mounted, open and painting, and `behaviour.reproduced` is 8 of 8.
        declared = declared_divergences(rail)
        assert_eq(
            sorted(d["key"] for d in conformance["divergences"]),
            sorted(entry["key"] for entry in declared),
            "E: ★★ every way the rail differs from the mockup is one somebody "
            "wrote down -- BEHIND and AHEAD both, so a build that quietly "
            "opened a locked seat fails here just as loudly as one that lost a "
            "section",
        )
        assert_eq(
            [d["says"] for d in conformance["divergences"]],
            [entry["sentence"] for entry in declared],
            "E: with the remainder exactly as declared, sentence for sentence",
        )
        assert_eq(
            conformance["reproduced"],
            reproduced(rail),
            "E: ★★ and the count the shell publishes is the count the "
            "specification derives -- two derivations of one rule, not a rule "
            "and a copy of it",
        )
        # ★ The second reference, whose number is the one a person asked about:
        # every section the behaviour prototype builds is one this build opens.
        assert_eq(
            conformance["behaviour"]["reproduced"],
            conformance["behaviour"]["builds"],
            "E: ★★★★★ and against the BEHAVIOUR reference nothing is owed -- "
            "every section it builds, this build opens",
        )
        ok(
            f"E: ★ the shell reconciles its live difference with its ledger -- "
            f"{conformance['reconciles']}",
            conformance["reconciles"] is True,
        )
        shell.intervene(f"{EXT}/nav", "keys")
        shell.tick(16)
        assert_eq(shell.query(f"{EXT}/nav"), "keys", "E: the third seat opens")
        rects = abs_rects_of(shell.snapshot(source="paint"))
        ok(
            "E: arriving paints the section inside the host",
            any(tag.startswith("kp.") for tag in rects),
        )
        # ★ R2051 — the address, recovered from one the application publishes.
        for chrome in (
            "shell.appbar",
            "shell.rail",
            f"{address_prefix(q(shell, 'spec')['rail'])}keys",
        ):
            ok(f"E: and the host's {chrome} survives -- a page, not a takeover", chrome in rects)
        ok(
            "E: ★ every column of the specified list is painted in the host too",
            all(f"kp.column.{c['key']}" in rects for c in spec["columns"]["canon"]),
        )
        shell.intervene(f"{EXT}/nav", "dashboard")
        shell.tick(16)
        rects = abs_rects_of(shell.snapshot(source="paint"))
        ok(
            "E: ★★ and leaving takes it away -- the page is not painted everywhere "
            "at once",
            not any(tag.startswith("kp.") for tag in rects),
        )


def body() -> None:
    spec = keys_spec()
    named = surfaces(spec)
    ok("the specification fixes three surfaces", len(named) == 3)
    ok(
        "and every one of them declares an ordered roster of named parts",
        all(
            [p["ordinal"] for p in spec[s]["canon"]] == list(range(1, len(spec[s]["canon"]) + 1))
            for s in named
        ),
    )

    with RpcSubprocess(SECTION, boot_grace=1.5, visible_window=True) as app:
        section_a(app, spec)
        section_b(app, spec)
        section_c(app)
        section_d(app)

    section_e(spec)

    banner("what was checked")
    for line in CHECKS:
        print(f"  · {line}")
    print(
        f"\n[coverage] {REAL_POINTER_RUNS} real-pointer session(s) contributed to "
        f"this run; {len(CHECKS)} named check(s) plus the assert_eq comparisons above."
    )
    if REAL_POINTER_RUNS == 0:
        print(
            "[coverage] ⚠ section B's presses did NOT run on this host. The run "
            "is shorter than it looks and this line is the only evidence."
        )


if __name__ == "__main__":
    run_demo("R1730 a section is the one the reference draws", body)
