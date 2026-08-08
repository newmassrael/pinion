#!/usr/bin/env python3
"""R1601 — the reference census, computed rather than remembered.

The node-graph campaign's coverage claim rests on counting what Blender and
Unreal can do and checking it off. That count has been a hand-written `grep`,
re-typed each round, and it has been **wrong three times in recorded ways**
([[debt-blender-census-by-field-name]]):

* R1587.1 — *one name, two spellings*: `no_mute_links` is read under that name
  and set through a builder called `no_muted_links`, so a field census reported
  **zero** users where there are 42.
* R1589 — *one spelling, two meanings*: `NODE_OT_detach` (from a frame) and
  `NODE_OT_links_detach` (a wire) are both "detach", so an operator we had for
  one was counted as covering the other.
* R1598 — *the name is not where the implementation is*: `NODE_OT_swap_node`
  appears in `space_node/*.cc` only as a **string argument**, and is really a
  Python operator.

This tool is the mechanism that discipline never had. It is the same class of
fix as `tools/counterfactual.py` (R1600) and `tools/blast_radius.py` (R1582):
the rule was right and nobody had written the computation.

## What it computes

**Blender registers node operators FOUR ways**, and the old census saw one:

| mechanism            | how it is written                                   |
|----------------------|-----------------------------------------------------|
| `cpp`                | `ot->idname = "NODE_OT_x"`                            |
| `macro`              | `WM_operatortype_append_macro("NODE_OT_x", ...)`      |
| `python-core`        | `bl_idname = "node.x"` under `scripts/startup/`       |
| `python-addon`       | the same, anywhere else                               |

The distinction is not bookkeeping. A **macro** is a composition of other
operators (`NODE_OT_translate_attach` is `TRANSFORM_OT_translate` then
`NODE_OT_attach`), so counting one as a missing capability is a *false gap* —
the error R1577 already recorded for Blender's 429 registered node types. An
**addon** operator is not Blender's node system at all.

**Unreal's peer unit is not a node class.** `UK2Node_*` is content (113 of them,
the analogue of Blender's registered node types); the surface that answers "what
can this editor DO" is `FGraphEditorCommandsImpl` in `GraphEditorActions.h`.

## What it refuses to do

It does not judge. Coverage lives in `docs/reference-census.json`, one verdict
per operator, and an operator the pin does not judge is reported as a
**finding** rather than counted as either covered or missing. That is the whole
point: a census that silently absorbs a new upstream operator into a percentage
is the thing that went wrong.

## R1602 — and a judgement is not a claim, it is a test

Enumerating completely still leaves every verdict a hand judgement, and the two
directions are not equally safe: a wrong `absent` puts a fake item on the gap
list and the next round finds it by trying to close it, while a wrong **`have`
inflates the coverage number and nobody trips over it**. That is the direction
R1601 itself had to correct.

So each `have` row carries `proven_by`, addressed `<crate>::<test>`, and the
named crate holds a `tests/reference_census.rs` whose own test asserts a
**bijection** between the pin's rows addressed to it and the proofs it contains.
Between the two halves — this file checks the address resolves to a census file
that exists, and that file checks the address is answered — a `have` cannot be
added without a test, a proof cannot be deleted without the pin noticing, and a
row cannot name a proof belonging to a different capability, because the proof's
name is *derived* from the operator rather than transcribed.

`covered_by` stays: it names the API, which is what a person reading the pin
wants. `proven_by` names what runs.

## Running it

    python3 tools/reference_census.py              # live vs pinned
    python3 tools/reference_census.py --emit       # a starter pin from the live trees
    python3 tools/reference_census.py --selftest   # the tool's own arithmetic
    python3 tools/reference_census.py --check-pin  # the committed judgement, tree-free

The reference trees are GPL / EULA'd and live outside this repo, so they cannot
be vendored and the census cannot be a `cargo test`. Absent trees **fail open**
with a printed notice — infrastructure absence is not evidence of agreement —
which is the shape `.githooks/lib/ci-status.sh` already uses.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
from dataclasses import dataclass, field
from pathlib import Path

BLENDER = Path(os.environ.get("PINION_BLENDER_REF", Path.home() / "blender-ref"))
UNREAL = Path(os.environ.get("PINION_UNREAL_REF", Path.home() / "UnrealEngine"))
REPO = Path(__file__).resolve().parent.parent
PIN = REPO / "docs" / "reference-census.json"

#: Where a crate's own census proofs live. One file per crate, so the crate that
#: owns the capability is the crate that proves it.
PROOF_FILE = "tests/reference_census.rs"

#: `<crate>::<test>` — a bare test name would not say which crate runs it, and
#: the capabilities behind these verdicts are not all in one.
PROOF_ADDRESS = re.compile(r"^([a-z0-9][a-z0-9-]*)::([a-z][a-z0-9_]*)$")

#: Verdicts a pinned operator may carry. `have` and `absent` are the two that
#: move the coverage number; the others take it out of the denominator, each
#: for a stated reason.
VERDICTS = {
    "have": "the crate or a binding does this",
    "absent": "a node-system capability we do not have",
    "composition": "a macro over other operators — covering the parts covers it",
    "app-content": "the reference application's own subject matter, not editor mechanism",
    "addon": "not the reference's node system at all",
    "host-framework": "the reference's own object / UI framework, not its node system",
}

#: R1603 — **a reference has more than one surface**, and a census that reads
#: only one is blind to whole capability classes rather than merely incomplete.
#:
#: R1593 (a link may convert) and R1594 (a value is authored on a socket) each
#: closed something a node substrate cannot host a material or Blueprint graph
#: without. Both are `UEdGraphSchema` virtuals in Unreal
#: (`CreateAutomaticConversionNodeAndConnections`, `TrySetDefaultValue`) and
#: `bNodeType` / `bNodeTreeType` callbacks in Blender — and **neither is an
#: operator**, so the R1601 census read both as zero and the coverage judged on
#: top of it was overstated by exactly the amount nobody could see.
#:
#: So a surface is a *kind of question* the reference answers, and each has its
#: own denominator: merging them into one percentage would let a fat one hide a
#: starved one.
SURFACES = {
    "operator": "a user-invokable action, registered and bound to a menu or key",
    "command": "the same thing under Unreal's name for it",
    "hook": "a decision the editor asks the node system to make — the extension "
    "surface, which no operator census can see",
}

#: `mechanism` says how the reference writes it; the surface is a property of
#: the mechanism rather than a second judgement.
SURFACE_OF = {
    "cpp": "operator",
    "macro": "operator",
    "python-core": "operator",
    "python-addon": "operator",
    "graph-editor-command": "command",
    "bNodeType": "hook",
    "bNodeTreeType": "hook",
    "bNodeSocketType": "hook",
    "UEdGraphSchema": "hook",
    "UEdGraphNode": "hook",
}


@dataclass
class Operator:
    name: str
    mechanism: str
    #: Where it was found, for the reproduction command.
    where: str = ""


@dataclass
class Census:
    blender: dict[str, Operator] = field(default_factory=dict)
    unreal: dict[str, Operator] = field(default_factory=dict)

    def all(self) -> dict[str, dict[str, Operator]]:
        return {"blender": self.blender, "unreal": self.unreal}


def read(path: Path) -> str:
    try:
        return path.read_text(errors="replace")
    except OSError:
        return ""


def walk(root: Path, suffixes: tuple[str, ...]) -> list[Path]:
    found: list[Path] = []
    for path in root.rglob("*"):
        if path.suffix in suffixes and path.is_file():
            found.append(path)
    return found


# ---------------------------------------------------------------- Blender

CPP_IDNAME = re.compile(r'ot->idname\s*=\s*"(NODE_OT_[a-z_0-9]+)"')
CPP_MACRO = re.compile(r'WM_operatortype_append_macro\(\s*"(NODE_OT_[a-z_0-9]+)"')
PY_IDNAME = re.compile(r'bl_idname\s*=\s*"node\.([a-z_0-9]+)"')


def census_blender(root: Path) -> dict[str, Operator]:
    """The four mechanisms, in precedence order.

    A name found by more than one mechanism keeps the FIRST — an operator with a
    C++ registration is a C++ operator even if a Python file mentions it, which
    is the R1598 attribution error stated as a rule instead of a hazard.
    """
    found: dict[str, Operator] = {}

    def add(name: str, mechanism: str, where: str) -> None:
        found.setdefault(name, Operator(name, mechanism, where))

    source = root / "source" / "blender"
    for path in walk(source, (".cc", ".c", ".cpp", ".hh")):
        body = read(path)
        if "NODE_OT_" not in body:
            continue
        rel = str(path.relative_to(root))
        for name in CPP_IDNAME.findall(body):
            add(name, "cpp", rel)
        for name in CPP_MACRO.findall(body):
            add(name, "macro", rel)

    for path in walk(root / "scripts", (".py",)):
        body = read(path)
        if "bl_idname" not in body:
            continue
        rel = str(path.relative_to(root))
        core = rel.startswith("scripts/startup/")
        for stem in PY_IDNAME.findall(body):
            add("NODE_OT_" + stem, "python-core" if core else "python-addon", rel)

    for name, member, where in blender_hooks(root):
        add(f"{name}::{member}", name, where)
    return found


#: A C function pointer field, `void (*insert_link)(..)`.
HOOK_POINTER = re.compile(r"\(\*([a-zA-Z_0-9]+)\)\s*\(")
#: The same slot written as a `std::function`, which Blender is migrating to.
HOOK_FUNCTION = re.compile(r"^\s*std::function<.*>\s+([a-zA-Z_0-9]+)\s*;")

#: Blender's node-system extension surface: what a node type, a tree type and a
#: socket type may each answer.
BLENDER_HOOK_STRUCTS = ("bNodeType", "bNodeTreeType", "bNodeSocketType")


def blender_hooks(root: Path) -> list[tuple[str, str, str]]:
    """`(struct, member, where)` for every callback slot in those three structs.

    Brace-counted rather than regex-scoped: a struct's extent is a nesting
    question and a regex answering one is how a census acquires a hole.
    """
    where = "source/blender/blenkernel/BKE_node.hh"
    lines = read(root / where).split("\n")
    found: list[tuple[str, str, str]] = []
    for name in BLENDER_HOOK_STRUCTS:
        opening = f"struct {name} {{"
        start = next((k for k, line in enumerate(lines) if line.startswith(opening)), None)
        if start is None:
            continue
        depth = 0
        for index in range(start, len(lines)):
            line = lines[index]
            depth += line.count("{") - line.count("}")
            pointer = HOOK_POINTER.search(line)
            if pointer:
                found.append((name, pointer.group(1), where))
            boxed = HOOK_FUNCTION.match(line)
            if boxed:
                found.append((name, boxed.group(1), where))
            if depth == 0 and index > start:
                break
    return found


# ----------------------------------------------------------------- Unreal

UE_COMMAND = re.compile(r"TSharedPtr<\s*FUICommandInfo\s*>\s*([A-Za-z_0-9]+)")
UE_VIRTUAL = re.compile(r"\bvirtual\s+[A-Za-z_0-9:<>&*,\s]+?\b([A-Za-z_0-9]+)\s*\(")

#: Unreal's node-system extension surface, and the peer of Blender's three
#: structs: what a *graph* answers (the schema) and what a *node* answers.
UNREAL_HOOK_HEADERS = {
    "UEdGraphSchema": "Engine/Source/Runtime/Engine/Classes/EdGraph/EdGraphSchema.h",
    "UEdGraphNode": "Engine/Source/Runtime/Engine/Classes/EdGraph/EdGraphNode.h",
}


def census_unreal(root: Path) -> dict[str, Operator]:
    """Two surfaces.

    **Commands** — `FGraphEditorCommandsImpl`, the peer of Blender's operator
    list. Deliberately NOT `UK2Node_*`: those are node *types*, the analogue of
    the 429 registered node types this campaign already excludes as content.

    **Hooks** — the virtuals of `UEdGraphSchema` and `UEdGraphNode`. R1601 named
    this surface as uncounted and R1602 measured why it matters: the two things
    R1593 and R1594 closed are on it and on no operator list anywhere.
    """
    found: dict[str, Operator] = {}
    where = "Engine/Source/Editor/GraphEditor/Public/GraphEditorActions.h"
    for name in UE_COMMAND.findall(read(root / where)):
        found.setdefault(name, Operator(name, "graph-editor-command", where))
    for owner, header in UNREAL_HOOK_HEADERS.items():
        for member in UE_VIRTUAL.findall(read(root / header)):
            found.setdefault(f"{owner}::{member}", Operator(f"{owner}::{member}", owner, header))
    return found


# ------------------------------------------------------------------- pin


def load_pin() -> dict:
    if not PIN.is_file():
        return {"blender": {}, "unreal": {}}
    return json.loads(PIN.read_text())


def compare(live: dict[str, Operator], pinned: dict) -> dict[str, list[str]]:
    """What the pin and the live tree disagree about."""
    live_names = set(live)
    pin_names = set(pinned)
    reclassified = [
        name
        for name in sorted(live_names & pin_names)
        if pinned[name].get("mechanism") != live[name].mechanism
    ]
    unjudged = [
        name
        for name in sorted(pin_names)
        if pinned[name].get("verdict") not in VERDICTS
    ]
    return {
        "new": sorted(live_names - pin_names),
        "gone": sorted(pin_names - live_names),
        "reclassified": reclassified,
        "unjudged": unjudged,
    }


def surface_of(row: dict) -> str:
    """Which kind of question this row is. Unknown mechanisms answer `other`
    rather than being folded into a surface they might not belong to."""
    return SURFACE_OF.get(row.get("mechanism", ""), "other")


def coverage(pinned: dict, surface: str | None = None) -> tuple[int, int, list[str]]:
    """`(have, denominator, the absent ones)`, over one surface or over all.

    Only `have` and `absent` are in the denominator. Everything else is out of
    it **with its reason named in the pin**, which is what stops a coverage
    number from being inflated by excluding things quietly.
    """
    rows = {
        name: row
        for name, row in pinned.items()
        if surface is None or surface_of(row) == surface
    }
    have = [n for n, row in rows.items() if row.get("verdict") == "have"]
    absent = [n for n, row in rows.items() if row.get("verdict") == "absent"]
    return len(have), len(have) + len(absent), sorted(absent)


def report(census: Census, pin: dict, strict: bool) -> int:
    problems = 0
    for tree, live in census.all().items():
        pinned = pin.get(tree, {})
        present = bool(live)
        print(f"\n=== {tree} ===")
        if not present:
            print(
                f"  reference tree absent — census not run. "
                f"Set PINION_{tree.upper()}_REF to re-measure."
            )
        else:
            by_mechanism: dict[str, int] = {}
            for op in live.values():
                by_mechanism[op.mechanism] = by_mechanism.get(op.mechanism, 0) + 1
            print(f"  live: {len(live)} operator(s)")
            for mechanism, count in sorted(by_mechanism.items()):
                print(f"    {mechanism:<22} {count}")

        have, total, absent = coverage(pinned)
        if total:
            print(f"  pinned coverage: {have}/{total} = {100 * have // total}%")
            # Per surface, because merging them lets a fat one hide a starved
            # one — which is exactly what happened before this surface existed.
            for surface in sorted({surface_of(row) for row in pinned.values()}):
                s_have, s_total, s_absent = coverage(pinned, surface)
                if not s_total:
                    continue
                print(
                    f"    {surface:<10} {s_have}/{s_total} = "
                    f"{100 * s_have // s_total}%   ({len(s_absent)} absent)"
                )
            out = len(pinned) - total
            if out:
                reasons: dict[str, int] = {}
                for row in pinned.values():
                    verdict = row.get("verdict")
                    if verdict in ("composition", "app-content", "addon", "host-framework"):
                        reasons[verdict] = reasons.get(verdict, 0) + 1
                shape = ", ".join(f"{k} {v}" for k, v in sorted(reasons.items()))
                print(f"  out of the denominator: {out} ({shape})")
            if absent:
                print(f"  ABSENT ({len(absent)}): {', '.join(absent)}")

        if not present:
            continue
        diff = compare(live, pinned)
        for kind, names in diff.items():
            if not names:
                continue
            problems += len(names)
            print(f"  {kind.upper()} ({len(names)}): {', '.join(names)}")
    if problems:
        print(
            f"\n{problems} finding(s). A reference operator the pin does not "
            "judge is neither covered nor missing — it is unmeasured, and "
            "that is the state this tool exists to make visible."
        )
    return 1 if (problems and strict) else 0


def emit(census: Census, pin: dict) -> None:
    """A starter pin: every live operator, verdicts carried over where the pin
    already has one and left blank where it does not."""
    out = {}
    for tree, live in census.all().items():
        if not live:
            out[tree] = pin.get(tree, {})
            continue
        rows = {}
        for name in sorted(live):
            op = live[name]
            previous = pin.get(tree, {}).get(name, {})
            rows[name] = {
                "mechanism": op.mechanism,
                "verdict": previous.get("verdict", ""),
                "covered_by": previous.get("covered_by", ""),
                "proven_by": previous.get("proven_by", ""),
                "where": op.where,
            }
        out[tree] = rows
    print(json.dumps(out, indent=2, sort_keys=True))


# -------------------------------------------------------------- selftest

FAILURES: list[str] = []


def check(condition: bool, what: str) -> None:
    print(("  ok   " if condition else "  FAIL ") + what)
    if not condition:
        FAILURES.append(what)


def selftest() -> int:
    print("mechanism precedence")
    live = {
        "NODE_OT_a": Operator("NODE_OT_a", "cpp"),
        "NODE_OT_b": Operator("NODE_OT_b", "macro"),
    }
    pinned = {
        "NODE_OT_a": {"mechanism": "cpp", "verdict": "have"},
        "NODE_OT_b": {"mechanism": "python-core", "verdict": "composition"},
    }
    diff = compare(live, pinned)
    check(diff["reclassified"] == ["NODE_OT_b"], "a changed mechanism is a finding")
    check(diff["new"] == [] and diff["gone"] == [], "and nothing else is")

    print("new and gone")
    diff = compare(
        {"NODE_OT_a": Operator("NODE_OT_a", "cpp")},
        {"NODE_OT_z": {"mechanism": "cpp", "verdict": "have"}},
    )
    check(diff["new"] == ["NODE_OT_a"], "an operator the pin lacks is NEW")
    check(diff["gone"] == ["NODE_OT_z"], "an operator the tree lacks is GONE")

    print("an unjudged operator is neither covered nor missing")
    pinned = {
        "NODE_OT_a": {"mechanism": "cpp", "verdict": "have"},
        "NODE_OT_b": {"mechanism": "cpp", "verdict": "absent"},
        "NODE_OT_c": {"mechanism": "cpp", "verdict": ""},
    }
    have, total, absent = coverage(pinned)
    check((have, total) == (1, 2), "the unjudged one is OUT of the denominator")
    check(absent == ["NODE_OT_b"], "and the absent one is named")
    check(
        compare({}, pinned)["unjudged"] == ["NODE_OT_c"],
        "and it is reported as a finding rather than silently rounded",
    )

    print("the excluded classes leave the denominator, each by name")
    pinned = {
        "NODE_OT_a": {"mechanism": "cpp", "verdict": "have"},
        "NODE_OT_m": {"mechanism": "macro", "verdict": "composition"},
        "NODE_OT_x": {"mechanism": "python-addon", "verdict": "addon"},
        "NODE_OT_p": {"mechanism": "cpp", "verdict": "app-content"},
    }
    have, total, _ = coverage(pinned)
    check(
        (have, total) == (1, 1),
        "a macro is not a gap — counting one would be the R1577 false-gap error",
    )
    check(
        set(VERDICTS)
        == {"have", "absent", "composition", "app-content", "addon", "host-framework"},
        "and every out-of-denominator class has a stated reason",
    )

    print("the regexes read what the references actually write")
    body = (
        'ot->idname = "NODE_OT_join";\n'
        '  ot = WM_operatortype_append_macro("NODE_OT_join_named",\n'
        '  WM_operatortype_find("NODE_OT_swap_node", true);\n'
    )
    check(CPP_IDNAME.findall(body) == ["NODE_OT_join"], "a registration is found")
    check(CPP_MACRO.findall(body) == ["NODE_OT_join_named"], "a macro is found")
    check(
        "NODE_OT_swap_node" not in CPP_IDNAME.findall(body) + CPP_MACRO.findall(body),
        "★ and a LOOKUP is not — the R1598 attribution error, as an assertion",
    )
    check(
        PY_IDNAME.findall('    bl_idname = "node.swap_node"') == ["swap_node"],
        "a Python operator is found where the C++ census cannot see it",
    )
    check(
        UE_COMMAND.findall("TSharedPtr< FUICommandInfo > CollapseNodes;")
        == ["CollapseNodes"],
        "and Unreal's command list parses",
    )

    print("R1603 — a reference has more than one surface")
    pinned = {
        "NODE_OT_a": {"mechanism": "cpp", "verdict": "have"},
        "NODE_OT_b": {"mechanism": "cpp", "verdict": "absent"},
        "bNodeType::c": {"mechanism": "bNodeType", "verdict": "have"},
        "bNodeType::d": {"mechanism": "bNodeType", "verdict": "app-content"},
    }
    check(coverage(pinned, "operator")[:2] == (1, 2), "an operator surface counts operators")
    check(coverage(pinned, "hook")[:2] == (1, 1), "and the hook surface counts hooks")
    check(
        coverage(pinned)[:2] == (2, 3),
        "★ the whole is the sum, so no surface can be quietly left out of it",
    )
    check(
        surface_of({"mechanism": "nonesuch"}) == "other",
        "an unknown mechanism answers `other` rather than joining a surface",
    )
    check(
        set(SURFACE_OF.values()) <= set(SURFACES),
        "and every mechanism's surface is one this file states the meaning of",
    )

    print("the hook census reads what the reference actually writes")
    check(
        HOOK_POINTER.findall("  bool (*insert_link)(NodeInsertLinkParams &params) = nullptr;")
        == ["insert_link"],
        "a C function-pointer slot is found",
    )
    check(
        HOOK_FUNCTION.match("  std::function<void(bNode &)> ui_description_fn;") is not None,
        "and so is the std::function form Blender is migrating to",
    )
    check(
        HOOK_POINTER.findall("  node_type_storage(ntype, ...);") == [],
        "a call is not a slot",
    )
    check(
        UE_VIRTUAL.findall("\tvirtual bool TryCreateConnection(UEdGraphPin* A) const;")
        == ["TryCreateConnection"],
        "an Unreal virtual is found",
    )
    check(
        UE_VIRTUAL.findall("\tbool TryCreateConnection(UEdGraphPin* A) const;") == [],
        "and a non-virtual member is not — the surface is what the editor may OVERRIDE",
    )

    print("R1602 — a `have` names the test that runs it")
    here = Path(__file__).resolve().parent.parent
    row = {
        "mechanism": "cpp",
        "verdict": "have",
        "covered_by": "Document::group",
        "proven_by": "pinion-node-graph::blender_group_make",
    }
    check(
        proof_problems("blender", "NODE_OT_group_make", row, here) == [],
        "an address whose crate holds a census file resolves",
    )
    check(
        len(proof_problems("blender", "x", {**row, "proven_by": ""}, here)) == 1,
        "★ a `have` with no proof is a problem — the wrong verdict is the one "
        "that inflates the number, so it is the one that has to cost something",
    )
    check(
        len(proof_problems("blender", "x", {**row, "proven_by": "group_make"}, here)) == 1,
        "a bare test name is refused: it does not say which crate runs it",
    )
    check(
        len(proof_problems("blender", "x", {**row, "proven_by": "pinion-nope::t"}, here)) == 1,
        "and an address into a crate with no census file is refused",
    )
    check(
        proof_problems("blender", "x", {"verdict": "absent", "proven_by": ""}, here) == [],
        "an `absent` needs no proof",
    )
    check(
        len(proof_problems("blender", "x", {"verdict": "absent", "proven_by": "a::b"}, here)) == 1,
        "and may not carry one — evidence belongs to the verdict it is evidence for",
    )

    if FAILURES:
        print(f"\nreference census: {len(FAILURES)} failure(s)")
        return 1
    print("\nreference census: all checks passed")
    return 0


def proof_problems(tree: str, name: str, row: dict, repo: Path) -> list[str]:
    """Whether this row's `proven_by` resolves to a census file that exists.

    Deliberately only half the check. Whether the named *test* is there is a
    question about Rust, and asking it here would be a census over text — the
    failure this whole tool exists to stop. The other half is the bijection test
    inside each crate's own census file, where the compiler is the one reading.
    """
    verdict = row.get("verdict", "")
    proven_by = row.get("proven_by", "")
    if verdict != "have":
        if proven_by:
            return [
                f"{tree}/{name}: verdict {verdict!r} carries proven_by "
                f"{proven_by!r} — only a `have` is in the numerator, so only a "
                "`have` may claim evidence"
            ]
        return []
    if not proven_by:
        return [
            f"{tree}/{name}: `have` with no proof. Name the test that exercises "
            f"it, as <crate>::<test>, and add it to that crate's {PROOF_FILE}."
        ]
    match = PROOF_ADDRESS.match(proven_by)
    if not match:
        return [f"{tree}/{name}: proven_by {proven_by!r} is not <crate>::<test>"]
    crate = match.group(1)
    if not (repo / "crates" / crate / PROOF_FILE).is_file():
        return [
            f"{tree}/{name}: proven_by names {crate}, which has no "
            f"{PROOF_FILE} to hold the proof"
        ]
    return []


def check_pin(pin: dict, repo: Path = REPO) -> int:
    """The pin judges everything it holds, says why for each, and — for the ones
    that move the number up — names the test that runs it.

    Runnable with no reference tree, which is what makes it a gate: the trees are
    outside this repo and may be absent, but the JUDGEMENT is committed here and
    is the thing that rots. A row with no verdict is neither covered nor missing
    and would quietly leave the denominator.
    """
    problems: list[str] = []
    proven: dict[str, int] = {}
    for tree, rows in pin.items():
        for name, row in sorted(rows.items()):
            verdict = row.get("verdict", "")
            if verdict not in VERDICTS:
                problems.append(f"{tree}/{name}: verdict {verdict!r} is not one of "
                                f"{sorted(VERDICTS)}")
            elif not row.get("covered_by"):
                problems.append(
                    f"{tree}/{name}: verdict {verdict!r} with no reason. "
                    "`have` must name the API, and every other verdict must say "
                    "why it is out of the denominator."
                )
            if not row.get("mechanism"):
                problems.append(f"{tree}/{name}: no mechanism")
            found = proof_problems(tree, name, row, repo)
            problems.extend(found)
            if verdict == "have" and not found:
                crate = row["proven_by"].split("::", 1)[0]
                proven[crate] = proven.get(crate, 0) + 1
    for line in problems:
        print(f"  {line}")
    total = sum(len(rows) for rows in pin.values())
    print(f"reference census pin: {total} judged operator(s), {len(problems)} problem(s)")
    if proven:
        shape = ", ".join(f"{crate} {count}" for crate, count in sorted(proven.items()))
        print(f"  {sum(proven.values())} `have` verdict(s) name a proof: {shape}")
        print("  (that the named TEST exists is asserted by each crate's own "
              f"{PROOF_FILE})")
    return 1 if problems else 0


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--emit", action="store_true", help="print a starter pin")
    parser.add_argument("--selftest", action="store_true")
    parser.add_argument(
        "--check-pin",
        action="store_true",
        help="verify the committed judgement without needing a reference tree",
    )
    parser.add_argument(
        "--strict",
        action="store_true",
        help="exit non-zero on any finding (the pre-push reading)",
    )
    args = parser.parse_args(argv)
    if args.selftest:
        return selftest()
    if args.check_pin:
        return check_pin(load_pin())

    census = Census(
        blender=census_blender(BLENDER) if BLENDER.is_dir() else {},
        unreal=census_unreal(UNREAL) if UNREAL.is_dir() else {},
    )
    pin = load_pin()
    if args.emit:
        emit(census, pin)
        return 0
    return report(census, pin, args.strict)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
