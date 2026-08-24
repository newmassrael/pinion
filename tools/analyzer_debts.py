#!/usr/bin/env python3
"""★★★★★ R1809 — **the analysis-tool debts, as a population a command can name.**

## The question this exists to make answerable

The north star this repository is working toward has four closing conditions,
and three of them are already commands: `analyzer_census.py` answers what the
framework still owes, `--check-proofs` answers whether every `have` names a test
that runs, and a walk over the assembled application answers whether the three
canonical screens reproduce their specifications.

The fourth — *the analysis-tool family of open debts is closed* — was **not a
command**, and could not become one, for two reasons measured at R1809:

1. **Nothing said which debts are in the family.** Measured over the memory
   folder: 317 open `debt-*.md`, of which the word `analyzer` appears in 68 —
   far too loose, because the tool is what this whole tree is about and the word
   turns up in prose that has nothing to do with it. There was no field, no
   list, and no derivation. "Analyzer-family" was a judgement somebody made
   again each time they were asked.
2. **Nothing said whether a debt can be worked on now.** The north star already
   excludes two census rows by name — one needs a ratified spec decision, one
   waits on Phase C — but that exclusion lived in the prompt, not in the debts.
   So "are the remaining ones all blocked?" had no denominator and no verdict.

Counting *closed* debts cannot answer it either, and that is not a detail. This
repository's closing discipline is that a debt closing must move whatever it did
not repay into a NEW open file, so the honest rounds are the ones that keep the
number up: R1806 closed one and opened one, R1807 closed one branch and opened
two. A condition read as "the count reached zero" is a condition that punishes
the discipline that makes the count trustworthy.

## What it does instead

* **Derives the population.** A debt is in the family when it names an analyzer
  root (a `[[wiki-link]]` to one of the files the family is rooted in) or names
  an `analyzer-census.json` row id (`lab.t2.16`, `dashboard.t1.3`, ...). Both
  are direct evidence written by the debt itself. Transitive reachability was
  measured and rejected: the memory graph's hubs run to 169 in-edges, so
  everything reaches everything.
* **Requires each one to declare its standing** in its own front matter, from a
  closed vocabulary. An unknown word is refused, not guessed at.
* **Answers the condition**: every family debt that is not blocked is named, so
  the verdict is a list rather than a number.

⚠ **What the derivation cannot see, stated rather than left to bite.** It reads
what a debt WROTE, so a family debt that names neither a root nor a row is
invisible to it. That is not hypothetical: R1809's own closing audit found one —
`debt-one-outline-role-does-two-jobs-and-clears-neither`, opened at R1807 while
proving census row `dashboard.t2.9`, naming neither. It was repaired by making
the debt name its origin, which is the rule this implies and which nothing yet
enforces: **a debt born from a census row must say which row**, or the census
that produced it cannot see it.

## Where the population lives, and why that is stated rather than hidden

⚠ The memory folder is **outside this repository** and is not tracked by it.
Measured at R1809: `git ls-files` matches zero paths under `memory/`, and the
folder sits under the agent's own state directory. So this tool judges a
population the repository cannot version, and a fresh clone has none of it.

Two consequences, both handled here rather than left to bite:

* **It fails OPEN and LOUD when the folder is absent** — on CI, in a fresh
  clone, on anybody else's machine. Absence of the folder is not evidence of no
  debt, and a check that silently passes when it cannot see its subject is worse
  than no check (the rule `ci-status.sh` already follows for a missing `gh`).
* **It writes a SNAPSHOT into the repository** (`docs/analyzer-debts.json`), so
  the judgement is versioned, reviewable in a diff, and visible to a reader who
  has no memory folder at all. `--check` refuses when the snapshot and the live
  folder disagree, which is what keeps the committed copy from rotting into a
  claim about a population that has moved.
"""

from __future__ import annotations

import json
import os
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
SNAPSHOT = ROOT / "docs" / "analyzer-debts.json"


class Finding(Exception):
    """Something the caller must be told rather than worked around."""


#: The files the analysis-tool debt family is rooted in, each with the reason it
#: is a root. A debt that links one of these is claiming kinship with it.
#:
#: ★ A declared list, and therefore checked: `roots_present` refuses a root that
#: no longer exists. A root that quietly stopped resolving would SHRINK the
#: family silently, which is R1798's class — a gate that was right about a
#: population that had become a constant.
ROOTS: dict[str, str] = {
    "analyzer-tool-capability-spec": "the capability specification the census is built from",
    "protocol-analyzer-class-tool-capability-audit": "the audit that enumerated the tool class",
    "analyzer-behaviour-canon-is-the-standalone-prototype": "what the canon IS",
    "analyzer-debt-repayment-loop": "the standing instruction this work runs under",
    "debt-canon-gui-surface-census": "the canon's GUI surfaces, counted",
    "screen-a-vs-canon-operation-census": "screen A against the canon, operation by operation",
    "debt-the-canon-is-one-app-and-we-are-many-binaries": "the structural gap the canon exposes",
}

#: An `analyzer-census.json` row id, which only that census uses.
ROW = re.compile(r"\b(?:lab|dashboard|capture)\.t\d+\.\d+\b")

WIKILINK = re.compile(r"\[\[([^\]|#]+?)\]\]")


#: What stops a debt being worked on now — a CLOSED vocabulary, drawn from what
#: the debts themselves say rather than invented here.
#:
#: The split that matters is `blocks`: `campaign` and `unmeasured` are NOT
#: blocked. A campaign is buildable work that happens to span rounds, and an
#: unmeasured debt's next step is the measurement. Calling either of them
#: blocked would let the condition be met by re-labelling the backlog, which is
#: the one failure mode a self-declared field has.
STANDINGS: dict[str, tuple[bool, str]] = {
    "buildable": (False, "nothing stops it; it has not been done"),
    "campaign": (False, "buildable, but larger than one round"),
    "unmeasured": (False, "its own premise has not been measured yet"),
    "spec-round": (True, "needs a ratified specification decision first"),
    "phase-c": (True, "waits on a phase of the roadmap that has not begun"),
    "gated-axis": (True, "on an axis this machine cannot advance (OS runners)"),
}

BLOCKED = {word for word, (blocks, _) in STANDINGS.items() if blocks}


def memory_dir() -> pathlib.Path | None:
    """Where the debt files live, or `None` when they are not on this machine.

    Derived from the repository's own path the way the agent's state directory
    is named, so a second checkout finds its own memory rather than this one's.
    `PINION_MEMORY_DIR` overrides, which is what makes the tool testable against
    a fixture.
    """
    override = os.environ.get("PINION_MEMORY_DIR")
    if override:
        here = pathlib.Path(override)
        return here if here.is_dir() else None
    slug = str(ROOT).replace("/", "-")
    here = pathlib.Path.home() / ".claude" / "projects" / slug / "memory"
    return here if here.is_dir() else None


def front_matter(text: str) -> dict[str, str]:
    """The `metadata:` block's scalar fields, as written.

    Deliberately not a YAML parse: these files are read by an agent and written
    by hand, and a strict parser would refuse a file over something that has
    nothing to do with the two fields this tool reads.
    """
    out: dict[str, str] = {}
    for line in text.splitlines():
        if line.startswith("---") and out:
            break
        found = re.match(r"^  ([a-z_]+):\s*(.*?)\s*$", line)
        if found:
            out[found.group(1)] = found.group(2)
    return out


def is_family(text: str) -> list[str]:
    """The direct evidence that this debt belongs to the analysis-tool family.

    Empty when there is none. Two kinds, and both are the debt's own words:
    a link to a root, and a census row id.
    """
    why: list[str] = []
    linked = sorted(set(WIKILINK.findall(text)) & set(ROOTS))
    if linked:
        why.append("links " + ", ".join(linked))
    rows = sorted(set(ROW.findall(text)))
    if rows:
        why.append("names " + ", ".join(rows))
    return why


def survey(folder: pathlib.Path) -> list[dict]:
    """Every OPEN debt in the analysis-tool family, with its declared standing."""
    out: list[dict] = []
    for path in sorted(folder.glob("debt-*.md")):
        text = path.read_text(encoding="utf-8", errors="replace")
        meta = front_matter(text)
        if meta.get("status") != "open":
            continue
        why = is_family(text)
        if not why:
            continue
        out.append(
            {
                "name": path.stem,
                # ★★★★★ R1809 — what the SNAPSHOT may call it, which is not
                # always what the file is called. The snapshot is a tracked
                # artifact, and this repository's standing rule is that another
                # project's name must not reach one: restate the sentence as a
                # CAPABILITY and leave the source in memory, where reading it is
                # not only allowed but how completeness is judged.
                #
                # A debt whose own file name carries such a word therefore
                # declares a publishable label, and its author writes it —
                # never this tool, because mechanical substitution is what
                # `reference_names.py` refuses by name: the sentence is carrying
                # the evidence for a decision, and a machine rewriting it drops
                # exactly that.
                #
                # Discovered the way it should be: the ratchet refused this
                # round's first snapshot.
                "public_name": meta.get("public_name", "") or path.stem,
                "why": why,
                "blocked_by": meta.get("blocked_by", ""),
                "priority": meta.get("priority", ""),
            }
        )
    return out


def check_standings(rows: list[dict]) -> list[str]:
    """Every family debt whose declared standing is missing or not a word."""
    out: list[str] = []
    for row in rows:
        word = row["blocked_by"]
        if not word:
            out.append(
                f"{row['name']}: names the analysis tool ({'; '.join(row['why'])}) "
                "and declares no `blocked_by`"
            )
        elif word not in STANDINGS:
            out.append(
                f"{row['name']}: `blocked_by: {word}` is not one of "
                + ", ".join(sorted(STANDINGS))
            )
    return out


def roots_present(folder: pathlib.Path) -> list[str]:
    """Every declared root that is not a file — a silently shrinking population."""
    return [
        f"root `{name}` does not exist ({why}), so every debt that linked it "
        "has quietly left the family"
        for name, why in sorted(ROOTS.items())
        if not (folder / f"{name}.md").is_file()
    ]


def unblocked(rows: list[dict]) -> list[dict]:
    """The family debts that nothing stops — the answer to the condition."""
    return [row for row in rows if row["blocked_by"] not in BLOCKED]


def snapshot_of(rows: list[dict]) -> dict:
    """The committed form: the population and its standings, ordered."""
    return {
        "note": (
            "R1809 — a snapshot of the analysis-tool debt family, which lives "
            "OUTSIDE this repository. Written by tools/analyzer_debts.py "
            "--write; verified by --check wherever the memory folder exists. "
            "A reader with no memory folder has only this."
        ),
        "debts": [
            {"name": r["public_name"], "blocked_by": r["blocked_by"], "why": r["why"]}
            for r in sorted(rows, key=lambda r: r["public_name"])
        ],
    }


def report(rows: list[dict]) -> list[str]:
    """The report, as lines. Pure in `rows`."""
    out = [f"analysis-tool debts — {len(rows)} open in the family", ""]
    for word in sorted(STANDINGS, key=lambda w: (w in BLOCKED, w)):
        here = [r for r in rows if r["blocked_by"] == word]
        if not here:
            continue
        blocks, why = STANDINGS[word]
        out.append(f"  {word:<11} {len(here):>3}  {'blocked' if blocks else 'open '} — {why}")
        for row in here:
            out.append(f"      {row['name']}")
    loose = unblocked(rows)
    out.append("")
    out.append(
        f"  {len(loose)} of {len(rows)} are NOT blocked — the north star's third "
        "condition is that this is zero"
    )
    return out


def selftest() -> int:
    """The tool's own tests: every rule above, exercised."""
    failures = 0

    def check(name: str, ok: bool) -> None:
        nonlocal failures
        if not ok:
            failures += 1
            print(f"FAIL: {name}")

    root = next(iter(ROOTS))
    linked = f"---\nmetadata: \n  status: open\n---\nsee [[{root}]]\n"
    check("a debt linking a root is family", is_family(linked) != [])
    check("and the reason names the root", root in is_family(linked)[0])
    check("a debt naming a census row is family", is_family("about lab.t2.16 here") != [])
    check("prose about the word alone is not", is_family("the analyzer is nice") == [])
    check("a bare version-like number is not a row", is_family("v1.2.3") == [])

    check(
        "front matter reads the fields it needs",
        front_matter(linked).get("status") == "open",
    )
    check(
        "a declared standing is read",
        front_matter("---\nmetadata: \n  status: open\n  blocked_by: phase-c\n---\n").get(
            "blocked_by"
        )
        == "phase-c",
    )

    missing = [{"name": "d", "why": ["links x"], "blocked_by": "", "priority": ""}]
    check("a family debt with no standing is refused", check_standings(missing) != [])
    bogus = [{"name": "d", "why": ["links x"], "blocked_by": "later", "priority": ""}]
    check("a standing outside the vocabulary is refused", check_standings(bogus) != [])
    good = [{"name": "d", "why": ["links x"], "blocked_by": "phase-c", "priority": ""}]
    check("a declared standing passes", check_standings(good) == [])

    check("phase-c blocks", unblocked(good) == [])
    check(
        "a campaign does NOT block, so a backlog cannot be relabelled away",
        unblocked([{**good[0], "blocked_by": "campaign"}]) != [],
    )
    check(
        "nor does unmeasured",
        unblocked([{**good[0], "blocked_by": "unmeasured"}]) != [],
    )
    check("every blocked word is in the vocabulary", BLOCKED <= set(STANDINGS))
    check("and not every word blocks", BLOCKED != set(STANDINGS))

    # ★ R1809 — the snapshot publishes the declared label when there is one, so
    # a debt whose own file name carries another project's name can still be in
    # a tracked artifact. Ordering follows the published label too, or the file
    # would sort by a name it does not show.
    named = {"name": "debt-zzz", "public_name": "debt-aaa", "why": ["x"], "blocked_by": "buildable"}
    plain = {"name": "debt-mmm", "public_name": "debt-mmm", "why": ["x"], "blocked_by": "buildable"}
    shot = snapshot_of([plain, named])
    check(
        "the snapshot publishes the declared label",
        [d["name"] for d in shot["debts"]] == ["debt-aaa", "debt-mmm"],
    )
    check(
        "and the raw name is nowhere in it",
        "debt-zzz" not in json.dumps(shot),
    )

    print(f"selftest: {'PASS' if not failures else 'FAIL'} ({failures} failure(s))")
    return 1 if failures else 0


def main() -> int:
    if "--selftest" in sys.argv:
        return selftest()

    folder = memory_dir()
    if folder is None:
        # ★ Fails OPEN, and says so every time. A fresh clone and CI have no
        # memory folder, and a silent pass there would be a check that stopped
        # happening — the failure mode this repository has measured before.
        print(
            "analysis-tool debts: no memory folder on this machine, so the debt "
            "family cannot be surveyed here. "
            f"The committed snapshot is {SNAPSHOT.relative_to(ROOT)}.",
            file=sys.stderr,
        )
        return 0

    stale = roots_present(folder)
    for line in stale:
        print(f"analysis-tool debts: {line}", file=sys.stderr)
    if stale:
        return 1

    rows = survey(folder)
    broken = check_standings(rows)

    if "--write" in sys.argv:
        SNAPSHOT.write_text(
            json.dumps(snapshot_of(rows), indent=2, ensure_ascii=False) + "\n",
            encoding="utf-8",
        )
        print(f"analysis-tool debts: wrote {SNAPSHOT.relative_to(ROOT)} ({len(rows)} debt(s))")
        return 0

    if "--check" in sys.argv:
        for line in broken:
            print(f"analysis-tool debts: {line}", file=sys.stderr)
        if broken:
            return 1
        if not SNAPSHOT.is_file():
            print(
                f"analysis-tool debts: {SNAPSHOT.relative_to(ROOT)} is missing; "
                "run --write",
                file=sys.stderr,
            )
            return 1
        held = json.loads(SNAPSHOT.read_text(encoding="utf-8"))
        if held.get("debts") != snapshot_of(rows)["debts"]:
            print(
                "analysis-tool debts: the committed snapshot and the memory "
                f"folder disagree; run --write. ({SNAPSHOT.relative_to(ROOT)})",
                file=sys.stderr,
            )
            return 1
        loose = unblocked(rows)
        print(
            f"analysis-tool debts: {len(rows)} in the family, all declared, "
            f"{len(loose)} not blocked"
        )
        return 0

    for line in broken:
        print(f"analysis-tool debts: {line}", file=sys.stderr)
    print("\n".join(report(rows)))
    return 1 if broken else 0


if __name__ == "__main__":
    sys.exit(main())
