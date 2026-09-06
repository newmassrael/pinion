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

## ★★★★★ R1820 — the condition was read off a population that grows

Everything above builds the *family*. R1820 measured what happens when a closing
condition is read off it, and the answer is that it cannot close.

**The family grows as it is repaid.** This repository's closing discipline is
that a debt closing moves whatever it did not repay into a NEW open file, so the
honest rounds are the ones that keep the number up. R1809's own docstring says
this — up in "The question this exists to make answerable", where it explains
why counting *closed* debts cannot answer it either — and then the report ends
`N of M are NOT blocked — the north star's third condition is that this is
zero`. **The tool knew, and pointed the condition at the growing number anyway.**

The loop's standing rules caught it from the other side, recorded it as a
structural defect, and prescribed the repair: pin a fixed cohort at a date and
write the date as a *command*. **Nothing executed the prescription**, which is
this repository's own R1791 lesson recurring — *a prescription nobody executes
is not a repair* — so the cohort stayed a number people carried in prose.

**Measured at R1820**, the prose number had drifted the way prose numbers do:
carried forward as *20 members, 17 the loop's to close*, and reconstructed from
the pinned commit it is **33 members, of which 21 are the loop's**. Both terms
wrong, in the direction that flatters — a smaller denominator and a smaller
remainder than the truth.

So the cohort is data now, and the data is checkable against this repository's
own history: `COHORT_PIN` is a commit, `--check` re-reads it out of git, and a
committed cohort that has drifted from the pin is refused. Three standings come
out of it that a bare count could not express — `left` (still open, but no
longer names the family: leaving is not closing), `person` (the loop repays it
and a person closes it), and `gone` (a member no file answers to, which is
refused rather than counted).

## ⚠ Two things this deliberately does NOT do

* **It does not shrink the goal.** A debt opened after the pin is derived,
  declared, checked and reported exactly as before; it is simply not in the
  denominator of a condition that was fixed before it existed. The evidence that
  nothing was quietly dropped is that the family count and the cohort count are
  both printed, side by side, and they disagree.
* **It does not decide that a person-closed debt is finished.** `closed_by`
  removes a debt from what the *loop* can close and from nothing else — not from
  the family, not from the cohort, not from the not-blocked count.
"""

from __future__ import annotations

import json
import os
import pathlib
import re
import subprocess
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
    # ★★★★★ R1892 — the repair lives in ANOTHER REPOSITORY, which this
    # repository's own hard rule forbids editing from a pinion session
    # (`CLAUDE.md`, 2026-06-19: even "continue that repo's work" does not
    # authorise it). Distinct from `gated-axis`, which is about a MACHINE this
    # tree does not have; here the machine is present and the *permission* is
    # not. The handoff is what a pinion round can produce, so a debt wearing
    # this word must still name what it did on this side — the first one does
    # (`tools/round_verdict.py`), and its `standing_because` citation is what
    # makes that checkable rather than promised.
    "cross-repo": (True, "the repair is another repository's; a handoff is what a round here can make"),
}

BLOCKED = {word for word, (blocks, _) in STANDINGS.items() if blocks}


#: Who can CLOSE a debt, when that is not whoever repays it — a second closed
#: vocabulary, and a different question from `blocked_by`.
#:
#: ★★★★★ R1820 — a debt can be entirely buildable, be worked on, be repaid, and
#: still not be closable by the thing that repaid it. The loop's own standing
#: rules say so of three debts in this family: a person reported each of them
#: while looking at a running window, and `never-record-unverified-outcomes`
#: (R1736) is why a sweep cannot retire that reading — **a sweep is complete
#: only about the window size it measured**. So the loop repays them and writes
#: down what is left; the person closes them.
#:
#: ⚠ This field removes a debt from what the loop can FINISH, so it is abusable
#: in the way `blocked_by`'s blocking words are, and it carries the same
#: defence: a `closed_because` citation, checked by the same code. It does NOT
#: remove the debt from the family, from the cohort, or from the not-blocked
#: count — all three still name it. Only the loop-closable tally drops it.
#:
#: ⚠⚠ Before this field the set was reproduced by **grepping the debts' prose**
#: for a sentence one of them happened to write. That is the practice this
#: repository bans by name (`status:` is a field query, never a prose grep) and
#: it fails in both directions: a debt that means it and phrases it differently
#: is missed, and a debt quoting the sentence to discuss it is caught.
CLOSERS: dict[str, str] = {
    "person": "a person reported it from a running window, and a person closes it",
}


#: The commit whose snapshot of this family IS the cohort — the fixed population
#: the north star's third condition is judged against.
#:
#: ★★★★★ R1820 — why a cohort at all, and why it is a commit rather than a list
#: somebody keeps. The condition was written "the analysis-tool open debts are
#: closed", over the family as it stands. That population **grows as it is
#: repaid**: this repository's closing discipline is that a debt closing moves
#: whatever it did not repay into a NEW open file, so an honest round raises the
#: count (R1806 closed one and opened one; R1807 closed one branch and opened
#: two). A condition whose denominator grows with the work is not a predicate,
#: and the loop's rules recorded exactly that and prescribed a set pinned at a
#: date. **Nothing built it**, so the cohort stayed a number written in prose —
#: and it drifted, which is this round's measurement.
#:
#: ⚠ The pin is R1809, not the R1805 the prose names, and the difference is
#: stated rather than smoothed over: the debts live OUTSIDE this repository in
#: an untracked folder, so no record of the family at R1805 exists anywhere.
#: R1809 is the first commit that wrote one down. Pinning where the evidence is
#: may over-count by whatever closed in those four rounds unrecorded; pinning
#: where the evidence is NOT would be a population nobody can reproduce.
#:
#: ★ Being a commit is what makes it a command: `--check` re-reads the pin out
#: of git and refuses a committed cohort that has drifted from it. A hand-kept
#: list would have the defect this round is repairing.
#: ★★★★★ R2009 — the pin MOVED, and which of its two effects was intended.
#:
#: Moving a pin does two things in one act, and the loop's standing rules
#: require the mover to say which one they meant: it **re-opens the closing
#: condition** (a spent cohort reads met, a fresh one does not), and it
#: **exposes every debt opened since the old pin** to the goal.
#:
#: Measured immediately before the move: the R1809 cohort held 33 members, of
#: which 28 were closed, 4 blocked and 1 a person's — `loop` was **0**, so the
#: third condition was MET — while the live family held 61 open debts, 54 of
#: them not blocked and **47 of them the loop's to close**. The condition was
#: true about 33 names fixed 199 rounds earlier and said nothing about the work.
#:
#: **The intended effect is the second: exposure.** The first is accepted, not
#: sought. The R1809 cohort was genuinely reached — its last open member closed
#: at R2008, with the reference census at 76/76 and 210/210 — and that result
#: stays true of the population it was about. What the move says is that the
#: population is no longer the one the goal is about.
#:
#: The new pin is R2008's closing commit. Its `docs/analyzer-debts.json` is
#: byte-identical to the one R2008's code commit wrote (`76a26b33`), so R1809's
#: evidence rule — pin where a snapshot was actually recorded — holds at either
#: address, and the round boundary is the more honest of the two.
#:
#: ⚠ What kept the staleness invisible for 199 rounds is repaired in the same
#: round rather than left to recur: nothing could tell a cohort that was MET
#: from one that was SPENT, because both print `0 ... are the loop's to close`.
#: `check_cohort_live` refuses that state from now on.
COHORT_PIN = "1ead8b4e"
COHORT_ROUND = "R2008"


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
            value = found.group(2)
            # ★ R1835 — a surrounding pair of quotes is YAML's, not the value's.
            # These files already quote `description:` that way, and a citation
            # holding a comma or a colon has to be quoted too; without this the
            # quotes reached the matcher and every quoted citation was refused
            # as malformed. Only a MATCHED pair is stripped, so a value that
            # genuinely starts with a quote is left alone.
            if len(value) >= 2 and value[0] == value[-1] and value[0] in "\"'":
                value = value[1:-1]
            out[found.group(1)] = value
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
                # ★ R1835 — one field for "why this standing", whatever the
                # standing is. It was `blocked_because` and required only of the
                # words that BLOCK, which left every other standing a bare word;
                # the old spelling is still read so a file that has not been
                # migrated is a citation rather than a silence.
                "standing_because": meta.get("standing_because", "")
                or meta.get("blocked_because", ""),
                # The file's own PROSE — front matter deliberately excluded, so
                # a `body:` citation has to quote the reasoning rather than the
                # `description:` sitting beside the word it is justifying.
                # Citing the description would be the declaration vouching for
                # itself, which is the shape this field exists to end.
                "body": text.split("\n---\n", 2)[-1],
                # R1820 — a second axis: who can close it, when that is not
                # whoever repays it. Absent for almost every debt, which is the
                # correct default: repaying is closing unless something says
                # otherwise.
                "closed_by": meta.get("closed_by", ""),
                "closed_because": meta.get("closed_because", ""),
                "priority": meta.get("priority", ""),
            }
        )
    return out


def all_debts(folder: pathlib.Path) -> dict[str, dict]:
    """Every debt file on this machine, keyed by the name the SNAPSHOT uses.

    Open or closed, family or not — because the cohort holds names that have
    since left the family, and a cohort member that left is not a cohort member
    that closed. Keying by the published label is what lets a cohort row and a
    debt file be compared at all; see `public_name` in `survey`.

    ⚠ Same `debt-*.md` glob as `survey`, and therefore the same registered blind
    spot: a debt file named the other way round (`*-debt.md`) is invisible to
    both. Shared deliberately — a cohort member and its family row must be found
    by the same rule, or one of them would go missing without the other.
    """
    out: dict[str, dict] = {}
    for path in sorted(folder.glob("debt-*.md")):
        meta = front_matter(path.read_text(encoding="utf-8", errors="replace"))
        label = meta.get("public_name", "") or path.stem
        out[label] = {
            "stem": path.stem,
            "status": meta.get("status", ""),
            "blocked_by": meta.get("blocked_by", ""),
            "closed_by": meta.get("closed_by", ""),
        }
    return out


def check_standings(rows: list[dict]) -> list[str]:
    """Every family debt whose declared standings are missing or not words.

    Two fields, and they are asymmetric on purpose: `blocked_by` is REQUIRED of
    every family debt, because the family is what the condition ranges over and
    a debt with no standing has no place in it. `closed_by` is optional, because
    repaying IS closing unless something says otherwise — the default is the
    common case, and only the exception declares itself.
    """
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
        closer = row.get("closed_by", "")
        if closer and closer not in CLOSERS:
            out.append(
                f"{row['name']}: `closed_by: {closer}` is not one of "
                + ", ".join(sorted(CLOSERS))
            )
    return out


def check_public_names(folder: pathlib.Path) -> list[str]:
    """Two debts published under one label — a collision nothing else would see.

    The snapshot is keyed by the published label, so two files claiming the same
    one would silently become a single row, and a cohort comparison would answer
    about whichever was read last. `all_debts` cannot report this itself (it
    returns a dict, which is exactly where the collision disappears), so the
    folder is walked again here.
    """
    pairs = []
    for path in sorted(folder.glob("debt-*.md")):
        meta = front_matter(path.read_text(encoding="utf-8", errors="replace"))
        pairs.append((path.stem, meta.get("public_name", "") or path.stem))
    return collisions(pairs)


def collisions(pairs: list[tuple[str, str]]) -> list[str]:
    """Every published label claimed by more than one debt. Pure in `pairs`."""
    seen: dict[str, list[str]] = {}
    for stem, label in pairs:
        seen.setdefault(label, []).append(stem)
    return [
        f"`{label}` is published by {len(stems)} debts ({', '.join(stems)}); "
        "a snapshot row cannot mean two files"
        for label, stems in sorted(seen.items())
        if len(stems) > 1
    ]


#: The three forms a `blocked_because` citation may take, each answerable by
#: something other than the debt that wrote it.
#:
#: ★★★★★ R1810 — a standing without a citation is the census's `have` without a
#: `proven_by`, one field over: R1809 built the vocabulary and checked that a
#: word was IN it, and never that the word was the right one. This is the half
#: that makes a standing answerable.
#:
#: ⚠ Demanded of the BLOCKED standings only, and that scoping is the argument
#: rather than a convenience. `buildable`, `campaign` and `unmeasured` all stay
#: in the denominator, so mislabelling one costs nothing the condition can
#: notice; `spec-round`, `phase-c` and `gated-axis` are the only words that
#: REMOVE a debt from the count, so one wrong is the condition reading true
#: while being false. Cite what leaves.
CITATION = re.compile(r"^(census|axis|rule):([A-Za-z0-9_.\-]+)$")

#: ★★★★★ R1835 — the fourth citation form, and the ONLY one a debt may point at
#: itself with: the sentence in its own body that the standing came from.
#:
#: Deliberately not accepted where a standing SHRINKS a count. A debt leaving
#: the denominator must cite something else — that is R1810's defence and the
#: single abuse path this whole field exists to close. What `body:` is for is
#: the other twenty-three: standings that stay in the count, whose word was one
#: person's reading and which nothing asked to justify.
#:
#: ⚠ What it proves and what it does not. It proves the sentence EXISTS in that
#: file, so a body rewritten without revisiting its standing fails — which is
#: the drift this catches. It does NOT prove the standing follows from the
#: sentence; no checker can. What it buys is that the basis is nameable and
#: reviewable instead of being a word with nothing behind it, which is exactly
#: the step from "is there a declaration" to "does the declaration have a
#: basis".
BODY_CITATION = re.compile(r"^body:(.+)$", re.S)


def check_citations(rows: list[dict], census: dict, gated: set[str], rules) -> list[str]:
    """Every debt whose standing cites nothing, or cites something wrong.

    Two words SHRINK a count and owe an EXTERNAL citation: a BLOCKED
    `blocked_by`, which takes a debt out of the not-blocked count, and any
    `closed_by`, which takes it out of what the loop can finish. Those are
    checked exactly as R1810 built them — `census:` / `axis:` / `rule:`, each
    answered by something that is not this file.

    ★★★★★ R1835 — **and every OTHER standing now owes one too.** This docstring
    used to end *"the unblocking standings owe nothing — a wrong `campaign`
    costs the work order, not the verdict"*, and that is true of the verdict and
    false of the cost: measured at R1835, 23 of 28 open debts carried a bare
    word with nothing behind it, so the order this loop picks work in rested on
    one person's single reading of each file. A wrong `buildable` sends a round
    at something that cannot be built; a wrong `campaign` hides a debt that
    could have been closed in an afternoon.

    So a non-blocking standing owes a `body:` citation — the sentence in its own
    file the word came from. That is a WEAKER claim than the external forms and
    is meant to be: it cannot leave the denominator, and a debt that tries to
    shrink a count with one is refused by name.

    `census` maps a row id to its verdict, `gated` holds the axis keys the Phase
    B tally marks ungainable, and `rules` answers whether a memory file exists.
    All three are injected, so the rule is testable without either tool.
    """
    out: list[str] = []
    for row in rows:
        # ★★★★★ R1835 — the standing that does NOT shrink a count still owes its
        # basis, and may cite its own body for it.
        if row["blocked_by"] and row["blocked_by"] not in BLOCKED:
            cited = row.get("standing_because", "")
            if not cited:
                out.append(
                    f"{row['name']}: `blocked_by: {row['blocked_by']}` is a bare word — "
                    "cite the sentence it came from (`standing_because: body:<phrase>`)"
                )
            elif CITATION.match(cited):
                out.append(
                    f"{row['name']}: `standing_because: {cited}` cites something ELSE, but "
                    f"`{row['blocked_by']}` shrinks no count — cite this file's own sentence "
                    "with `body:<phrase>`"
                )
            else:
                found = BODY_CITATION.match(cited)
                if not found:
                    out.append(
                        f"{row['name']}: `standing_because: {cited}` is not `body:<phrase>`"
                    )
                elif found.group(1).strip() not in row.get("body", ""):
                    out.append(
                        f"{row['name']}: cites a sentence its own body does not contain — "
                        f"{found.group(1).strip()[:60]!r}"
                    )
        # ★ R1820 — two fields, one rule. `blocked_by` takes a debt out of the
        # not-blocked count; `closed_by` takes it out of what the loop can
        # finish. Both are self-declared, both shrink something a condition is
        # read off, so both owe the same citation to something else.
        for field, word, triggers in (
            ("standing_because", row["blocked_by"], BLOCKED),
            ("closed_because", row.get("closed_by", ""), set(CLOSERS)),
        ):
            if word not in triggers:
                continue
            declared = "blocked_by" if field == "standing_because" else "closed_by"
            cited = row.get(field, "")
            if not cited:
                out.append(
                    f"{row['name']}: `{declared}: {word}` takes it out of the "
                    f"count and cites nothing (`{field}`)"
                )
                continue
            found = CITATION.match(cited)
            if not found:
                out.append(
                    f"{row['name']}: `{field}: {cited}` is not "
                    "`census:<row>`, `axis:<key>` or `rule:<memory-file>`"
                )
                continue
            kind, what = found.group(1), found.group(2)
            if kind == "census":
                if what not in census:
                    out.append(
                        f"{row['name']}: cites census row `{what}`, which is not in the pin"
                    )
                elif census[what] != "gap":
                    out.append(
                        f"{row['name']}: cites census row `{what}`, whose verdict is "
                        f"`{census[what]}` — a row that is not a gap blocks nothing"
                    )
            elif kind == "axis":
                if what not in gated:
                    out.append(
                        f"{row['name']}: cites axis `{what}`, which the Phase B tally "
                        "does not mark gated"
                    )
            elif not rules(what):
                out.append(f"{row['name']}: cites rule `{what}`, which is not a memory file")
    return out


def rule_named(folder: pathlib.Path):
    """An answer to *is there a memory file for this rule name*, over `folder`.

    ★★★★★ R2028 — **named, because it is an ORACLE.** This was written inline
    at the call site as `lambda name: (folder / f"{name}.md").is_file()`, and
    `tools/oracle_census.py` counts that as a gap for a reason that is not
    style: a lambda has no name, so nothing can assert it and no mutation can
    be aimed at it. A `rules` oracle that answered `True` for everything would
    let any `rule:` citation stand, and every fixture in this file's selftest
    hands `check_citations` a synthetic one — so the rule was tested and the
    thing deciding what the rule sees was not. R1884's shape, counted at R2028.
    """

    def known(name: str) -> bool:
        return (folder / f"{name}.md").is_file()

    return known


def census_verdicts() -> dict[str, str]:
    """Each analysis-tool census row's verdict, from the pin beside this tool."""
    pin = ROOT / "docs" / "analyzer-census.json"
    if not pin.is_file():
        return {}
    return {r["id"]: r["verdict"] for r in json.loads(pin.read_text(encoding="utf-8"))}


def gated_axes() -> set[str]:
    """The Phase B axis keys the tally marks gated — read from the tally itself.

    A gated axis is one this machine cannot advance, and the tally is where that
    is decided. Reading it here rather than restating it means the two cannot
    disagree about which axes those are.
    """
    tally = ROOT / "tools" / "phase_b_tally.py"
    if not tally.is_file():
        return set()
    source = tally.read_text(encoding="utf-8")
    # ★ Split at each axis's `"key"` first, then read that axis's own flag.
    #
    # Written as one `"key": "X" .*? "gated": True` pattern to begin with, which
    # is WRONG in a way that reads correct: `.*?` with `DOTALL` walks past an
    # axis whose flag is `False` to the next axis's `True`, and the match then
    # consumes the axis that flag belonged to.
    #
    # Measured against this tally by re-running the bad pattern: it answered
    # `{api, dcc}` where the truth is `{api, osnative}` — one false positive and
    # one false negative, with the third right by luck. (The first draft of this
    # comment called that "an exact inversion", which overstates it; the round's
    # closing audit re-ran the pattern rather than trusting the sentence.) A
    # regex spanning records cannot be trusted to stay inside one.
    keys = list(re.finditer(r"\"key\":\s*\"([a-z0-9]+)\"", source))
    out: set[str] = set()
    for nth, found in enumerate(keys):
        end = keys[nth + 1].start() if nth + 1 < len(keys) else len(source)
        if re.search(r"\"gated\":\s*True", source[found.end() : end]):
            out.add(found.group(1))
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


def cohort_from_git() -> list[str] | None:
    """The pinned cohort, read out of this repository's own history.

    `None` when git cannot answer — a shallow clone, a missing object, no git at
    all. That is a **fail-open** case and every caller says so out loud: the
    absence of the history is not evidence that the committed cohort is right.
    """
    try:
        done = subprocess.run(
            ["git", "-C", str(ROOT), "show", f"{COHORT_PIN}:docs/analyzer-debts.json"],
            capture_output=True,
            text=True,
            timeout=30,
            check=False,
        )
    except (OSError, subprocess.SubprocessError):
        return None
    if done.returncode != 0:
        return None
    try:
        held = json.loads(done.stdout)
    except json.JSONDecodeError:
        return None
    return sorted({str(d["name"]) for d in held.get("debts", []) if "name" in d})


def cohort_members() -> tuple[list[str], str]:
    """The cohort and where this reading of it came from.

    Git first, because that is the pin; the committed block second, so a reader
    with no history still gets an answer and is told it is second-hand.
    """
    found = cohort_from_git()
    if found is not None:
        return found, f"git {COHORT_PIN}"
    if SNAPSHOT.is_file():
        held = json.loads(SNAPSHOT.read_text(encoding="utf-8")).get("cohort", {})
        return sorted(str(n) for n in held.get("members", [])), "the committed snapshot"
    return [], "nothing on this machine"


#: What a cohort member's standing is now, in the order a reader needs them.
#: `left` is deliberately its own answer rather than being folded into `closed`:
#: a debt that stopped naming a root or a census row leaves the FAMILY without
#: closing, and calling that "closed" is how a condition reads true by attrition.
COHORT_STANDINGS = ("gone", "closed", "left", "blocked", "person", "loop")


def cohort_standing(label: str, index: dict[str, dict], family: set[str]) -> str:
    """Where one cohort member stands now — one of `COHORT_STANDINGS`."""
    held = index.get(label)
    if held is None:
        return "gone"
    if held["status"] != "open":
        return "closed"
    if label not in family:
        return "left"
    if held["blocked_by"] in BLOCKED:
        return "blocked"
    if held["closed_by"] in CLOSERS:
        return "person"
    return "loop"


def outside_cohort(members: list[str], rows: list[dict]) -> list[str]:
    """The loop-closable family debts the cohort does not name.

    ★★★★★ R2009. The cohort can only shrink and the family grows, so this is
    the gap between the condition and the population it is a sample OF. It is
    reported on every run because without it *met* and *spent* are the same
    sentence — both end `0 of N cohort debt(s) are the loop's to close` — and
    the difference between them is the whole of whether work remains.

    The same three predicates the cohort's own `loop` standing uses, so the two
    numbers add up to one population: open, not blocked, not a person's to close.
    """
    held = set(members)
    return sorted(
        row["public_name"]
        for row in rows
        if row["public_name"] not in held
        and row["blocked_by"] not in BLOCKED
        and row.get("closed_by", "") not in CLOSERS
    )


def cohort_spent(members: list[str], index: dict[str, dict], family: set[str]) -> bool:
    """Whether no member of the cohort is the loop's to close any more.

    Deliberately NOT the same predicate as "the condition is met": an empty
    cohort satisfies this one too, and an empty cohort is the vacuous shape
    `check_cohort` refuses on its own. Callers here have been through it first.
    """
    return not any(cohort_standing(name, index, family) == "loop" for name in members)


def check_cohort_live(
    members: list[str], source: str, index: dict[str, dict], rows: list[dict]
) -> list[str]:
    """Whether the cohort is still a denominator the condition can be read off.

    ★★★★★ R2009. A spent cohort is not a defect by itself — reaching it is the
    point of the loop. It becomes one the moment loop-closable debts stand
    OUTSIDE it, because from then on the condition reports *met* about a
    population that has stopped moving while the work goes on, and nothing says
    so. Measured: that state began at R2008, when the last open member closed,
    over a pin fixed at R1809, with 47 loop-closable family debts standing.

    A REFUSAL rather than a line of prose, because the repair is a person's
    decision — move the pin, or rewrite the condition — and this repository has
    measured what an unexecuted prescription is worth (R1791). The old report
    stated its number correctly for 199 rounds and nobody acted on it.

    Three states this deliberately does NOT refuse, each because it is not the
    defect: an empty cohort (`check_cohort` owns the vacuous case), a live
    cohort, and a spent cohort with nothing loop-closable outside it — the last
    of those is the goal actually reached, and refusing it would make the tool
    stop the line on success.

    ⚠ `source` is REQUIRED and the message names it rather than `COHORT_PIN`,
    for the reason this round's own counterfactual exposed: the constant is what
    a future cohort will be read from, and the argument is what THIS verdict was
    read from, and they are the same only while nobody drives the function with
    anything else. A message that re-spells a fact it was handed reports the
    wrong one the first time the two differ — the defect class `check_cohort`
    already avoids by taking the same argument.
    """
    if not members:
        return []
    if not cohort_spent(members, index, {r["public_name"] for r in rows}):
        return []
    outside = outside_cohort(members, rows)
    if not outside:
        return []
    return [
        f"the cohort read from {source} ({len(members)} member(s)) is SPENT — "
        f"no member is the loop's to close — while {len(outside)} loop-closable "
        f"family debt(s) stand outside it. The third condition now reads met "
        f"about {len(members)} names and not about the work. Move COHORT_PIN to "
        "a commit whose snapshot records today's family, and say which of the "
        "two effects was intended (re-opening the condition, or exposing the "
        "debts opened since the old pin); or rewrite the condition."
    ]


def check_cohort(members: list[str], source: str, index: dict[str, dict]) -> list[str]:
    """Whether the cohort can be trusted as a denominator at all.

    Two ways it silently cannot, both measured rather than imagined:

    * **A member no file answers to.** That is what a published label with no
      recorded link back looks like from this side, and a reader hit it on
      2026-08-25 and read a live debt as closed. It is refused here rather than
      reported as a standing, because a denominator with a phantom in it is
      wrong by one for as long as nobody notices.
    * **An empty cohort.** Git could not answer AND the snapshot has no block —
      so every cohort number would be a vacuous zero, which is the shape a
      condition reads as *met*.

    Not an error: git being unavailable while the committed block stands. That
    fails open, like the missing memory folder, and the caller prints where its
    reading came from either way.
    """
    if not members:
        return [
            f"the cohort is empty (read from {source}); the third condition "
            "would be vacuously true. Run --write where git can read "
            f"{COHORT_PIN}."
        ]
    return [
        f"cohort member `{name}` matches no debt file. If it is a published "
        "label, the debt it stands for must declare `public_name: " + name + "`"
        for name in members
        if name not in index
    ]


def cohort_report(
    members: list[str], source: str, index: dict[str, dict], rows: list[dict]
) -> list[str]:
    """The cohort's standing, as lines. Pure in its arguments."""
    family = {r["public_name"] for r in rows}
    by_standing: dict[str, list[str]] = {word: [] for word in COHORT_STANDINGS}
    for label in members:
        by_standing[cohort_standing(label, index, family)].append(label)

    out = [
        "",
        f"cohort — {len(members)} debt(s), pinned at {COHORT_ROUND} "
        f"(read from {source})",
        "",
    ]
    legend = {
        "closed": "closed since the pin",
        "left":   "still open, but no longer names the family",
        "blocked": "blocked — spec-round / phase-c / gated-axis",
        "person": "a person closes it — the loop repays, and leaves it open",
        "loop":   "the loop can close it",
        "gone":   "NO FILE ANSWERS THIS NAME",
    }
    for word in ("closed", "left", "blocked", "person", "loop", "gone"):
        here = by_standing[word]
        if not here:
            continue
        out.append(f"  {word:<8} {len(here):>3}  — {legend[word]}")
        if word in ("loop", "person", "gone", "left"):
            out.extend(f"      {name}" for name in here)
    # ★★★★★ R2009 — what the cohort COVERS, printed beside what it holds.
    # Without this line a spent cohort and a met one read identically, which is
    # how a pin fixed at R1809 went on answering the condition for 199 rounds
    # while the family it was a sample of grew past it.
    outside = outside_cohort(members, rows)
    out.append("")
    out.append(
        f"  the family holds {len(by_standing['loop']) + len(outside)} "
        f"loop-closable debt(s) — {len(by_standing['loop'])} named by this "
        f"cohort, {len(outside)} outside it"
    )
    out.append(
        f"  {len(by_standing['loop'])} of {len(members)} cohort debt(s) are the "
        "loop's to close — the north star's third condition is that this is zero"
    )
    return out


def snapshot_of(rows: list[dict], members: list[str]) -> dict:
    """The committed form: the population and its standings, ordered.

    `members` is REQUIRED and deliberately has no default. A default would let a
    caller write a snapshot whose cohort is silently empty, and an empty cohort
    is the shape a closing condition reads as *met* — the one error here that
    flatters. `check_cohort` refuses an empty cohort for the same reason.
    """
    return {
        "note": (
            "R1809 — a snapshot of the analysis-tool debt family, which lives "
            "OUTSIDE this repository. Written by tools/analyzer_debts.py "
            "--write; verified by --check wherever the memory folder exists. "
            "A reader with no memory folder has only this."
        ),
        "cohort": {
            "note": (
                "R1820 — the FIXED population the north star's third condition "
                "is judged against, and not the same thing as `debts` below. "
                "`debts` is the family as it stands and GROWS as it is repaid, "
                "because closing a debt here means opening a new file for "
                "whatever it did not repay; a condition read off it can never "
                "be met by honest work. This set is the family as the pinned "
                "commit recorded it, so it can only shrink. A debt opened after "
                "the pin is registered and reported like any other, and is "
                "simply not in this denominator. "
                "★ R2009 — this pin has MOVED once, from R1809 to R2008, after "
                "every member of the R1809 cohort stopped being the loop's to "
                "close while 47 loop-closable debts stood outside it. Moving a "
                "pin re-opens the closing condition AND exposes the debts "
                "opened since; the intended effect was the second. A spent "
                "cohort is refused from now on (`check_cohort_live`), so the "
                "next move is demanded rather than remembered."
            ),
            "pin": COHORT_PIN,
            "round": COHORT_ROUND,
            "members": members if members is not None else [],
        },
        "debts": [
            {
                "name": r["public_name"],
                # ★★★★★ R1820 — whether `name` above is the debt file's own name
                # or a label published in its place.
                #
                # This file is TRACKED and this repository's confidentiality
                # ratchet refuses another project's name in a tracked file, so a
                # debt whose file name carries one declares a neutral label and
                # the snapshot writes that. Correct — and until now the fact was
                # written NOWHERE, so a reader comparing this list against the
                # debt folder found one name that answered to no file and had no
                # way to tell "closed" from "published under another name".
                #
                # Measured 2026-08-25: a reader did exactly that and read a live
                # debt as closed. The cohort comparison was silently one short.
                # Recording the substitution costs one boolean and discloses
                # nothing — the private name stays in the untracked folder,
                # where the `public_name:` field points back at this one.
                "substituted": r["public_name"] != r["name"],
                "blocked_by": r["blocked_by"],
                "blocked_because": r.get("blocked_because", ""),
                "closed_by": r.get("closed_by", ""),
                "closed_because": r.get("closed_because", ""),
                "why": r["why"],
            }
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
        f"  {len(loose)} of {len(rows)} are NOT blocked — the backlog, which "
        "grows as it is repaid"
    )
    # ★★★★★ R1820 — this line used to end "the north star's third condition is
    # that this is zero", and that sentence was wrong in a way nothing could
    # notice: the family grows every time a debt is honestly closed, so the
    # number it names cannot reach zero by working. The condition is read off
    # the COHORT now (`cohort_report`), and this number keeps its real job —
    # saying how much is open, which is a different question and a useful one.
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
    shot = snapshot_of([plain, named], ["debt-aaa", "debt-mmm"])
    check(
        "the snapshot publishes the declared label",
        [d["name"] for d in shot["debts"]] == ["debt-aaa", "debt-mmm"],
    )
    check(
        "and the raw name is nowhere in it",
        "debt-zzz" not in json.dumps(shot),
    )

    # ── R1810: a standing that leaves the count must cite ────────────────
    census = {"lab.t2.16": "gap", "dashboard.t1.3": "have"}
    gated = {"osnative"}
    rules = {"zero-flake-policy"}.__contains__

    def cite(word: str, because: str, body: str = "") -> list[dict]:
        return [
            {
                "name": "d",
                "why": ["x"],
                "blocked_by": word,
                "standing_because": because,
                "body": body,
            }
        ]

    check(
        "a blocked standing with no citation is refused",
        check_citations(cite("phase-c", ""), census, gated, rules) != [],
    )
    # ── R1835: an UNBLOCKED standing owes one too ────────────────────────
    #
    # This block replaces a case that asserted the opposite — "an UNBLOCKED
    # standing needs none — it never leaves the count" — which was true of the
    # VERDICT and false of the work order: 23 of 28 open debts carried a bare
    # word, so which debt the loop reached for next rested on one unreviewed
    # reading of each file.
    check(
        "an unblocked standing with no citation is refused too",
        check_citations(cite("buildable", ""), census, gated, rules) != [],
    )
    check(
        "and it is satisfied by a phrase its own body contains",
        check_citations(
            cite("buildable", "body:because the seam is already there", "because the seam is already there"),
            census,
            gated,
            rules,
        )
        == [],
    )
    check(
        "a body citation to a phrase the file does NOT contain is refused",
        check_citations(
            cite("buildable", "body:a sentence nobody wrote", "some other prose"),
            census,
            gated,
            rules,
        )
        != [],
    )
    # ★★★★★ THE ABUSE PATH, held shut: a `body:` citation is the file vouching
    # for itself, so it must never let a debt LEAVE the denominator. R1810 built
    # that defence for the blocking words and R1835 must not open it by
    # widening the field.
    check(
        "a blocked standing may NOT cite its own body",
        check_citations(
            cite("phase-c", "body:it is blocked, obviously", "it is blocked, obviously"),
            census,
            gated,
            rules,
        )
        != [],
    )
    check(
        "and an unblocked standing may not cite something ELSE instead",
        check_citations(cite("buildable", "census:lab.t2.16"), census, gated, rules) != [],
    )
    check(
        "a census citation resolves when the row is a gap",
        check_citations(cite("spec-round", "census:lab.t2.16"), census, gated, rules) == [],
    )
    check(
        "a census citation to a row that is NOT a gap is refused",
        check_citations(cite("spec-round", "census:dashboard.t1.3"), census, gated, rules) != [],
    )
    check(
        "a census citation to a row nobody has is refused",
        check_citations(cite("spec-round", "census:lab.t9.9"), census, gated, rules) != [],
    )
    check(
        "an axis citation resolves only when the tally gates that axis",
        check_citations(cite("gated-axis", "axis:osnative"), census, gated, rules) == []
        and check_citations(cite("gated-axis", "axis:dcc"), census, gated, rules) != [],
    )
    check(
        "a rule citation resolves only to a memory file",
        check_citations(cite("spec-round", "rule:zero-flake-policy"), census, gated, rules) == []
        and check_citations(cite("spec-round", "rule:no-such-rule"), census, gated, rules) != [],
    )
    check(
        "a citation in no known form is refused",
        check_citations(cite("phase-c", "because I said so"), census, gated, rules) != [],
    )

    # ★ The axis derivation reads one record at a time. Written as a single
    # `"key" .*? "gated": True` pattern first, it walked past an ungated axis to
    # the next axis's flag and consumed the axis that flag belonged to —
    # measured as an exact inversion of the truth. This pins the shape.
    fixture = '{"key": "aaa", "gated": False}, {"key": "bbb", "gated": True}'
    keys = list(re.finditer(r"\"key\":\s*\"([a-z0-9]+)\"", fixture))
    seen = {
        found.group(1)
        for nth, found in enumerate(keys)
        if re.search(
            r"\"gated\":\s*True",
            fixture[found.end() : (keys[nth + 1].start() if nth + 1 < len(keys) else len(fixture))],
        )
    }
    check("an ungated axis does not borrow the next one's flag", seen == {"bbb"})

    # ── R1820: the closer axis ───────────────────────────────────────────────
    def closer(**over) -> list[dict]:
        # ★ R1835 — the standing half is satisfied here so these cases isolate
        # the CLOSER axis. Without it the widened standing rule fires on every
        # fixture and two closer assertions fail for a reason that has nothing
        # to do with closers, which is how a fixture stops testing its variable.
        row = {
            "name": "d",
            "why": ["links x"],
            "blocked_by": "buildable",
            "standing_because": "body:a stated basis",
            "body": "a stated basis",
            "priority": "",
        }
        row.update(over)
        return [row]

    check(
        "a closer outside the vocabulary is refused",
        check_standings(closer(closed_by="somebody")) != [],
    )
    check(
        "`closed_by: person` is accepted as a word",
        check_standings(closer(closed_by="person")) == [],
    )
    check(
        "a person-closed debt is still NOT blocked",
        unblocked(closer(closed_by="person")) != [],
    )
    check(
        "a closer that cites nothing is refused",
        check_citations(closer(closed_by="person"), {}, set(), lambda _: True) != [],
    )
    check(
        "a closer citing a rule that exists passes",
        check_citations(
            closer(closed_by="person", closed_because="rule:a-rule"),
            {},
            set(),
            lambda name: name == "a-rule",
        )
        == [],
    )
    check(
        "a closer citing a rule that does not exist is refused",
        check_citations(
            closer(closed_by="person", closed_because="rule:a-rule"),
            {},
            set(),
            lambda _: False,
        )
        != [],
    )
    # ★ The counterfactual that makes the pair above load-bearing: without a
    # closer declared, the citation rule must stay silent. A check that fired on
    # every row would pass both assertions for the wrong reason.
    check(
        "no closer declared means no citation is demanded",
        check_citations(closer(), {}, set(), lambda _: False) == [],
    )

    # ── R1820: the cohort ────────────────────────────────────────────────────
    index = {
        "shut": {"stem": "shut", "status": "closed", "blocked_by": "", "closed_by": ""},
        "left": {"stem": "left", "status": "open", "blocked_by": "buildable", "closed_by": ""},
        "held": {"stem": "held", "status": "open", "blocked_by": "phase-c", "closed_by": ""},
        "mine": {"stem": "mine", "status": "open", "blocked_by": "buildable", "closed_by": ""},
        "hers": {"stem": "hers", "status": "open", "blocked_by": "buildable", "closed_by": "person"},
    }
    family = {"held", "mine", "hers"}
    for label, want in (
        ("shut", "closed"),
        ("left", "left"),
        ("held", "blocked"),
        ("mine", "loop"),
        ("hers", "person"),
        ("ghost", "gone"),
    ):
        check(f"cohort standing of `{label}` is `{want}`", cohort_standing(label, index, family) == want)

    check(
        "a member no file answers to is refused, not counted",
        check_cohort(["ghost"], "test", index) != [],
    )
    check(
        "an empty cohort is refused rather than read as met",
        check_cohort([], "test", index) != [],
    )
    check(
        "a cohort whose members all resolve passes",
        check_cohort(sorted(index), "test", index) == [],
    )

    # ★★★★★ The measurement this round exists for, pinned as a test: the FAMILY
    # count and the COHORT count answer different questions, and only the second
    # can reach zero. A debt opened after the pin enters the family and not the
    # cohort — so repaying every cohort member drives the cohort to zero while
    # the family is still non-empty, which is what an honest round looks like.
    rows = [
        {"public_name": "mine", "blocked_by": "buildable", "closed_by": "", "why": ["links x"]},
        {"public_name": "newer", "blocked_by": "buildable", "closed_by": "", "why": ["links x"]},
    ]
    after = dict(index)
    after["mine"] = {"stem": "mine", "status": "closed", "blocked_by": "", "closed_by": ""}
    after["newer"] = {"stem": "newer", "status": "open", "blocked_by": "buildable", "closed_by": ""}
    still = [
        name
        for name in ("mine",)
        if cohort_standing(name, after, {r["public_name"] for r in rows}) == "loop"
    ]
    check("a repaid cohort reaches zero", still == [])
    check("while the family it left behind does not", unblocked(rows) != [])

    check(
        "the snapshot records that a name is a published label",
        snapshot_of(
            [
                {
                    "name": "private",
                    "public_name": "public",
                    "blocked_by": "buildable",
                    "why": ["links x"],
                }
            ],
            [],
        )["debts"][0]["substituted"]
        is True,
    )
    check(
        "and that an unsubstituted one is not",
        snapshot_of(
            [
                {
                    "name": "plain",
                    "public_name": "plain",
                    "blocked_by": "buildable",
                    "why": ["links x"],
                }
            ],
            [],
        )["debts"][0]["substituted"]
        is False,
    )
    check(
        "two debts publishing one label are refused",
        collisions([("one", "same"), ("two", "same")]) != [],
    )
    check(
        "and distinct labels are not",
        collisions([("one", "a"), ("two", "b")]) == [],
    )
    check(
        "a debt publishing under its own name collides with nothing",
        collisions([("one", "one"), ("two", "two")]) == [],
    )

    # ★ The pin has to actually resolve HERE, or the cohort silently falls back
    # to the committed block and the "read from git" claim is decoration. This
    # asserts the shape of what it returns rather than its size — a count
    # written here would be a number that rots the next time a debt closes.
    read = cohort_from_git()
    check("the pin resolves in this repository", read is not None and read != [])
    check(
        "and every name it yields is a debt",
        all(name.startswith("debt-") for name in read or ["debt-"]),
    )

    # ★★★★★ R2009 — the cohort's LIVENESS, which is a different question from
    # its drift. `--check` already refused a cohort that disagrees with the
    # folder; nothing asked whether the cohort still ranges over the work.
    # Fixtures rather than the live folder, so these assertions keep their
    # meaning after the next debt closes.
    def standing(status: str, blocked: str = "", closer: str = "") -> dict:
        return {"stem": "d", "status": status, "blocked_by": blocked, "closed_by": closer}

    pair = [
        {"public_name": "in", "blocked_by": "", "closed_by": ""},
        {"public_name": "out", "blocked_by": "", "closed_by": ""},
    ]
    live = {"in": standing("open"), "out": standing("open")}
    spent = {"in": standing("closed"), "out": standing("open")}
    check(
        "a cohort with an open member of its own is not spent",
        cohort_spent(["in"], live, {"in", "out"}) is False,
    )
    check(
        "and a live cohort is refused by nothing",
        check_cohort_live(["in"], "a fixture", live, pair) == [],
    )
    check(
        "a cohort whose every member has closed is spent",
        cohort_spent(["in"], spent, {"out"}) is True,
    )
    check("the loop-closable debts outside it are named", outside_cohort(["in"], pair) == ["out"])
    check(
        "a spent cohort with work standing outside it is refused",
        check_cohort_live(["in"], "a fixture", spent, pair) != [],
    )
    check(
        "and the refusal counts what stands outside",
        "1 loop-closable" in (check_cohort_live(["in"], "a fixture", spent, pair) or [""])[0],
    )
    # ★ The message must name the cohort it was HANDED, not the constant a
    # future one will be read from. Caught by this round's counterfactual, which
    # drove the function with the old pin's cohort and was told the new pin's
    # name.
    check(
        "and it names where this verdict's cohort came from",
        "a fixture" in (check_cohort_live(["in"], "a fixture", spent, pair) or [""])[0],
    )
    check(
        "a spent cohort with nothing outside it is the goal, not a defect",
        check_cohort_live(["in"], "a fixture", {"in": standing("closed")}, pair[:1]) == [],
    )
    check(
        "a blocked debt outside the cohort is not work it is missing",
        check_cohort_live(
            ["in"],
            "a fixture",
            spent,
            [pair[0], {"public_name": "out", "blocked_by": "phase-c", "closed_by": ""}],
        )
        == [],
    )
    check(
        "nor is one only a person can close",
        check_cohort_live(
            ["in"],
            "a fixture",
            spent,
            [pair[0], {"public_name": "out", "blocked_by": "", "closed_by": "person"}],
        )
        == [],
    )
    check(
        "an empty cohort is check_cohort's to refuse, not this one",
        check_cohort_live([], "a fixture", {}, pair) == [],
    )

    # ★★★★★ R2028 — THE FIVE ORACLES, against the real memory folder.
    #
    # Every rule above is pure and every case above hands it a fixture, so the
    # functions that decide WHAT those rules look at were watched by nothing —
    # `tools/oracle_census.py` counts fifteen call sites in this file over five
    # oracles, more than the tool where R1884 first met the shape. An oracle
    # that answered nothing would make every rule here vacuous and every case
    # above would still pass.
    # ⚠ Through `memory_dir`, which is itself an oracle and fails open when the
    # folder is not on this machine (a fresh clone has none). So the assertions
    # below are skipped there rather than failing — and the SKIP is reported,
    # because a check that quietly stops happening is what this whole class is
    # about.
    home = memory_dir()
    if home is not None:
        seen = survey(home)
        check("the survey finds this project's debts", len(seen) > 20)
        check(
            "and every row it returns carries the fields the rules read",
            all("name" in row and "blocked_by" in row for row in seen),
        )
        held = all_debts(home)
        check("the index holds at least what the survey found", len(held) >= len(seen))
        # ★★★★★ A key is a debt's FILE STEM or its PUBLIC NAME, and asserting
        # only the first was this round's own wrong guess: one debt in this
        # folder is filed under a private stem and published under a neutral
        # one, and the index carries BOTH so a cohort citing either resolves.
        # The first draft of this line asserted `key == stem` and the run said
        # so — which is also how the ratchet in `reference_names.py` caught the
        # first draft of this very comment, written with the private name in it.
        substituted = sum(1 for name, row in held.items() if name != row.get("stem"))
        check(
            "every key is a stem, or a public name standing in for one",
            all(
                name == row.get("stem") or row.get("stem", "").startswith("debt-")
                for name, row in held.items()
            ),
        )
        check(
            "and the substitution is REACHABLE — an index that resolved only "
            "stems would refuse every cohort citation written in public words",
            substituted > 0,
        )
        known = rule_named(home)
        check(
            "the rule oracle finds a rule this project actually keeps",
            known("zero-flake-policy"),
        )
        check(
            "and refuses one it does not — an oracle that said yes to every "
            "name would let any `rule:` citation stand",
            not known("no-such-rule-r2028"),
        )
    else:
        print(
            "selftest: SKIPPED the four memory-folder oracles — no debt folder "
            "on this machine, which is a fresh clone rather than a failure"
        )
    verdicts = census_verdicts()
    # ⚠ A floor rather than the count: a number here would be the second copy
    # of a figure the pin already holds, which is the class this repository
    # keeps paying for. What must not happen is an EMPTY answer, because that
    # makes `check_citations`'s `census:` arm accept anything.
    check("the census pin answers verdicts", len(verdicts) > 20)
    check(
        "and each is a word the citation rule knows",
        all(isinstance(v, str) and v for v in verdicts.values()),
    )
    gated = gated_axes()
    check("the tally names its gated axes", len(gated) >= 2)
    check(
        "and they are keys the tally itself carries",
        all(axis in (ROOT / "tools" / "phase_b_tally.py").read_text(encoding="utf-8") for axis in gated),
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
    index = all_debts(folder)
    members, source = cohort_members()
    broken = (
        check_standings(rows)
        + check_citations(
            rows,
            census_verdicts(),
            gated_axes(),
            rule_named(folder),
        )
        + check_public_names(folder)
        + check_cohort(members, source, index)
        + check_cohort_live(members, source, index, rows)
    )

    if "--write" in sys.argv:
        SNAPSHOT.write_text(
            json.dumps(snapshot_of(rows, members), indent=2, ensure_ascii=False) + "\n",
            encoding="utf-8",
        )
        print(
            f"analysis-tool debts: wrote {SNAPSHOT.relative_to(ROOT)} "
            f"({len(rows)} debt(s), cohort {len(members)} from {source})"
        )
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
        fresh = snapshot_of(rows, members)
        for part in ("debts", "cohort"):
            if held.get(part) != fresh[part]:
                print(
                    f"analysis-tool debts: the committed snapshot's `{part}` and "
                    f"the memory folder disagree; run --write. "
                    f"({SNAPSHOT.relative_to(ROOT)})",
                    file=sys.stderr,
                )
                return 1
        loose = unblocked(rows)
        mine = [
            name
            for name in members
            if cohort_standing(name, index, {r["public_name"] for r in rows}) == "loop"
        ]
        print(
            f"analysis-tool debts: {len(rows)} in the family, all declared, "
            f"{len(loose)} not blocked; cohort {len(members)} pinned at "
            f"{COHORT_ROUND}, {len(mine)} the loop's to close, "
            f"{len(outside_cohort(members, rows))} loop-closable outside it"
        )
        return 0

    for line in broken:
        print(f"analysis-tool debts: {line}", file=sys.stderr)
    print("\n".join(report(rows) + cohort_report(members, source, index, rows)))
    return 1 if broken else 0


if __name__ == "__main__":
    sys.exit(main())
