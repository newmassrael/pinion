#!/usr/bin/env python3
"""R1601 — the reference census, computed rather than remembered.

The node-graph campaign's coverage claim rests on counting what the DCC and
the engine can do and checking it off. That count has been a hand-written `grep`,
re-typed each round, and it has been **wrong three times in recorded ways**
([[debt-DCC-census-by-field-name]]):

* R1587.1 — *one name, two spellings*: `no_mute_links` is read under that name
  and set through a builder called `no_muted_links`, so a field census reported
  **zero** users where there are 42.
* R1589 — *one spelling, two meanings*: `detach` (from a frame) and
  `links_detach` (a wire) are both "detach", so an operator we had for
  one was counted as covering the other.
* R1598 — *the name is not where the implementation is*: `swap_node`
  appears in `space_node/*.cc` only as a **string argument**, and is really a
  Python operator.

This tool is the mechanism that discipline never had. It is the same class of
fix as `tools/counterfactual.py` (R1600) and `tools/blast_radius.py` (R1582):
the rule was right and nobody had written the computation.

## What it computes

**the DCC registers node operators FIVE ways**, and the first census saw one:

| mechanism                | how it is written                                 |
|--------------------------|---------------------------------------------------|
| `cpp`                    | `<any>->idname = "x"`, or `= __func__`     |
| `macro`                  | `WM_operatortype_append_macro("x", ...)`   |
| `cpp-template`           | a `socket_items` maker, registering per accessor   |
| `cpp-template-instance`  | the id one such maker registers, in `operator_idnames` |
| `python-core`            | `idname = "node.x"` under `scripts/startup/`     |
| `python-addon`           | the same, anywhere else                            |

The distinction is not bookkeeping. A **macro** is a composition of other
operators (`translate_attach` is `TRANSFORM_OT_translate` then
`attach`), so counting one as a missing capability is a *false gap* —
the error R1577 already recorded for the DCC's 429 registered node types. An
**addon** operator is not the DCC's node system at all. And an **instance** is
the mirror: 69 ids that are four behaviours, so counting them would put one
capability in the denominator sixty-nine times.

## R1605 — and reading C++ as text is not enough, so the residue is measured

Three registrations were missed at once, each a different failure, and all three
were found by asking the census to account for **every** `` token rather
than to find the ones it knew how to find:

* `operator_type->idname = "…"` — the receiver is not always called `ot`, so a
  regex keyed on the variable name reported a real operator as unregistered.
  That is [[debt-DCC-census-by-field-name]] recurring inside the tool built
  to end it.
* `ot->idname = __func__` — the id is not in the source at all. No text census
  can read that one without knowing what the compiler substitutes.
* the `socket_items` templates — 69 ids, in a different directory, written as
  `static constexpr StringRefNull` members rather than as an assignment.

So the answer to "is a text scan good enough" is **no on its own**, and what
makes it sound anyway is that the classification is now exhaustive: every token
is an id, a template constant, a registration function's own symbol, or a string
naming a Python operator — and a token in none of those is a finding. The engine
side carries the same check as [`unreal_command_residue`]. Comments are removed
before matching ([`read_cxx`]), so a documented-but-absent virtual cannot be
counted.

**the engine's peer unit is not a node class.** `script node*` is content (113 of them,
the analogue of the DCC's registered node types); the surface that answers "what
can this editor DO" is a `TCommands<T>` subclass — and there is **not one of
them**. See R1605 below.

## R1605 — and the command surface is not one list either

R1603 read `FGraphEditorCommandsImpl` and nothing else, so the census measured
the *generic* node canvas and read every per-editor graph command as zero.
the engine ships at least nine graph-editor command classes: the generic one plus
the visual script, material, animation, sound-cue, sound-class and behaviour-tree
editors' own. `MaterialEditorActions.h` alone declares 68, and a material graph
is half of what this axis is named for.

Two measurement corrections came with reading them:

* **The unit is the `TCommands` CLASS, not the header.** `visual script editor commands.h`
  holds three classes and only one of them is the graph's;
  `BehaviorTreeEditorCommands.h` holds three. R1603's single list happened to be
  one class per file, so the difference was invisible.
* **Scope is declared rather than assumed.** Every `TCommands` class under
  `Engine/Source/Editor` is enumerated, and each must be either *read* by
  [`UNREAL_COMMAND_CLASSES`] or *excluded by name* in [`UNREAL_COMMANDS_OUT`]
  with a reason. A class in neither is a **finding** — so the next command list
  Epic adds cannot be quietly outside the denominator, which is the failure this
  round exists to stop repeating.

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
    python3 tools/reference_census.py --owed       # what is left, by the pin's own reason

The reference trees are GPL / EULA'd and live outside this repo, so they cannot
be vendored and the census cannot be a `cargo test`. Absent trees **fail open**
with a printed notice — infrastructure absence is not evidence of agreement —
which is the shape `.githooks/lib/ci-status.sh` already uses.
"""

from __future__ import annotations

import argparse
import json
import contextlib
import io
import os
import re
import sys
from dataclasses import dataclass, field, replace
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
    "instance": "one instantiation of a generic mechanism the pin judges once, by name",
}

#: R1605 — `instance` is the mirror of `composition`, and both exist because a
#: count of registered names is not a count of capabilities.
#:
#: `composition` came from R1601: one macro registers under its own name and IS
#: several operators, so counting it as a gap invents one. `instance` is the
#: other direction: the DCC's `socket_items` templates register **69** operator
#: ids that are four behaviours instantiated for twenty-three of its own node
#: types, and counting those 69 would put one capability in the denominator
#: sixty-nine times. Each `instance` row names the row that carries the verdict,
#: so the capability is judged exactly once and is still reachable from every
#: name the reference gives it.

#: R1603 — **a reference has more than one surface**, and a census that reads
#: only one is blind to whole capability classes rather than merely incomplete.
#:
#: R1593 (a link may convert) and R1594 (a value is authored on a socket) each
#: closed something a node substrate cannot host a material or visual script graph
#: without. Both are graph schema virtuals in the engine
#: (`CreateAutomaticConversionNodeAndConnections`, `TrySetDefaultValue`) and
#: node type / node tree type callbacks in the DCC — and **neither is an
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

#: The `TCommands` classes this census READS, and the short tag each row is
#: keyed by. Every one is the command list of an editor whose document is a node
#: graph; the tag drops the reference's own `F…Commands` furniture so a row reads
#: `MaterialEditor::BreakLink`.
#:
#: R1605 — the generic canvas is `GraphEditor` and the other eight are the
#: per-editor lists R1603 could not see. They are **not** disjoint from it: a
#: material graph re-declares `BreakLink`, a sound-cue graph re-declares it
#: again, and measuring that redundancy is the point — one pinion mechanism
#: answering three reference rows is the same "the reference writes it three
#: times and this derives it once" figure the proof fan-out already reports.
UNREAL_COMMAND_CLASSES = {
    "FGraphEditorCommandsImpl": "GraphEditor",
    "FBlueprintEditorCommands": "BlueprintEditor",
    "FMaterialEditorCommands": "MaterialEditor",
    "FAnimGraphCommands": "AnimGraph",
    "FSoundCueGraphEditorCommands": "SoundCueGraph",
    "FSoundClassEditorCommands": "SoundClassGraph",
    "FBTCommonCommands": "BehaviorTree",
    "FBTDebuggerCommands": "BehaviorTreeDebugger",
    "FBTBlackboardCommands": "BehaviorTreeBlackboard",
}

#: Every OTHER `TCommands` class under `Engine/Source/Editor`, by the reason its
#: commands are not a node graph's. Grouped by reason rather than by module so
#: the exclusion is per **class**: a module-wide rule would let a graph command
#: list hide inside a large module, and `UnrealEd` — which is where
#: `MaterialGraphSchema` lives — is exactly such a module.
#:
#: A class in neither table is a finding. That is the whole mechanism: this table
#: has to grow when the engine does, and forgetting is visible.
UNREAL_COMMANDS_OUT = {
    "the level editor, its viewport and its world — a scene, not a graph": [
        "FLevelEditorCommands",
        "FLevelEditorModesCommands",
        "FLevelViewportCommands",
        "FLightEditingCommands",
        "FLevelInstanceEditorModeCommands",
        "FLevelCollectionCommands",
        "FLayersViewCommands",
        "FActorBrowsingModeCommands",
        "FEditorCommands",
        "FHLODCompareCommands",
        "FWorldBookmarkCommands",
    ],
    "a 3D viewport's own camera, show-flags and visualisation modes": [
        "FEditorViewportCommands",
        "FViewportNavigationCommands",
        "FStandardToolModeCommands",
        "FGPUSkinCacheVisualizationMenuCommands",
        "FBufferVisualizationMenuCommands",
        "FGroomVisualizationMenuCommands",
        "FLumenVisualizationMenuCommands",
        "FMegaLightsVisualizationMenuCommands",
        "FNaniteVisualizationMenuCommands",
        "FRayTracingDebugVisualizationMenuCommands",
        "FShowFlagMenuCommands",
        "FSubstrateVisualizationMenuCommands",
        "FVirtualShadowMapVisualizationMenuCommands",
        "FVirtualTextureVisualizationMenuCommands",
        "FAdvancedPreviewSceneCommands",
        "FCommonEditorViewportToolbarCommands",
    ],
    "the application shell — main frame, asset editor chrome, source control": [
        "FMainFrameCommands",
        "FGlobalEditorCommonCommands",
        "FAssetEditorCommonCommands",
        "FSourceControlCommands",
        "FDerivedDataEditorMenuCommands",
        "FZenStausBarCommands",
        "FContentBrowserCommands",
        "FUserAssetTagCommands",
        "FPropertyEditorCommands",
        "FPlayWorldCommands",
    ],
    "a curve or timeline editor — keys on a track, not nodes on a canvas": [
        "FCurveEditorCommands",
        "FDistCurveEditorCommands",
        "FCurveTableEditorCommands",
        "FSequencerCommands",
        "FSequencerTrackFilterCommands",
        "FSimpleViewCommands",
        "FToolableTimelineCommands",
        "FSequenceRecorderCommands",
        "FAnimSequenceCurveEditorCommands",
        "FAnimSequenceTimelineCommands",
        "FAnimNotifyPanelCommands",
        "FAnimSegmentsPanelCommands",
        "FCurveViewerCommands",
    ],
    "a mesh, skeleton or physics asset editor — its document is geometry": [
        "FStaticMeshEditorCommands",
        "FStaticMeshViewportLODCommands",
        "FSkeletalMeshEditorCommands",
        "FSkeletonEditorCommands",
        "FSkeletonTreeCommands",
        "FPhysicsAssetEditorCommands",
        "FAnimationEditorCommands",
        "FAnimViewportShowCommands",
        "FAnimViewportMenuCommands",
        "FAnimViewportLODCommands",
        "FAnimViewportPlaybackCommands",
        "FPoseEditorCommands",
        "FPersonaCommonCommands",
        "FMeshPainterCommands",
        "FClothPainterCommands",
        "FClothPaintToolCommands_Gradient",
        "FClothingAssetListCommands",
        "FTextureEditorCommands",
    ],
    "a painting or terrain tool — a brush over a surface": [
        "FLandscapeEditorCommands",
        "FFoliageEditCommands",
        "FFoliagePaletteCommands",
    ],
    "the Blueprint asset's own panels — its variable browser, toolbar and "
    "component viewport, none of which act on the graph canvas": [
        "FMyBlueprintCommands",
        "FFullBlueprintEditorCommands",
        "FSCSEditorViewportCommands",
    ],
    "the UMG widget designer — a widget tree laid out on a design surface": [
        "FUMGEditorCommands",
        "FDesignerCommands",
        "FBindWidgetCommands",
    ],
    "a chord table that spawns one of the reference's OWN node types — the "
    "node-type content class this campaign already excludes": [
        "FBlueprintSpawnNodeCommands",
        "FMaterialEditorSpawnNodeCommands",
    ],
    "a debugger or inspector over data the node system does not own": [
        "FMassDebuggerCommands",
        "FDataHierarchyEditorCommands",
        "FPListEditorCommands",
    ],
}

#: `mechanism` says how the reference writes it; the surface is a property of
#: the mechanism rather than a second judgement.
#:
#: The command tags are folded in programmatically, so adding a command class
#: cannot leave its rows answering `other` — a second place to remember is a
#: second place to forget.
SURFACE_OF = {
    "cpp": "operator",
    "macro": "operator",
    "cpp-template": "operator",
    "cpp-template-instance": "operator",
    "python-core": "operator",
    "python-addon": "operator",
    "bNodeType": "hook",
    "bNodeTreeType": "hook",
    "bNodeSocketType": "hook",
    "UEdGraphSchema": "hook",
    "UEdGraphNode": "hook",
    **{tag: "command" for tag in UNREAL_COMMAND_CLASSES.values()},
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
    #: Operator names the tree MENTIONS and no mechanism registers. Empty is the
    #: claim that the five mechanisms are exhaustive; non-empty is a sixth.
    blender_unregistered: list[str] = field(default_factory=list)

    def all(self) -> dict[str, dict[str, Operator]]:
        return {"blender": self.blender, "unreal": self.unreal}


#: R1605 — a comment is not a declaration.
#:
#: Both references are read as TEXT, which is the standing hazard this file was
#: built for ([[debt-param-census-blind-to-variable-keys]]); a real C++ parse
#: would need each tree's whole include graph and its build configuration, so it
#: is out of reach rather than deferred. What IS in reach is to remove the parts
#: of the text that are prose, and to MEASURE the residue instead of assuming it
#: away — see [`residue`].
#:
#: Measured on `graph schema.h` / `graph node.h` at the pinned revision:
#: stripping changes neither header's virtual count, so today it corrects
#: nothing. It is here because "the reference happens not to document a virtual
#: it does not declare" is luck, and a census that depends on luck is the thing
#: this tool replaced.
COMMENT_BLOCK = re.compile(r"/\*.*?\*/", re.S)
COMMENT_LINE = re.compile(r"//[^\n]*")


def read(path: Path) -> str:
    try:
        return path.read_text(errors="replace")
    except OSError:
        return ""


def _blank(match: re.Match[str]) -> str:
    return "\n" * match.group(0).count("\n")


def read_cxx(path: Path) -> str:
    """A C or C++ file with its comments blanked, newlines preserved.

    Blanked rather than deleted so a brace count is unaffected: `/* } */` in a
    comment must not close a struct, and removing a line comment must not join
    two lines a line-oriented regex reads one at a time.

    Applied to the C/C++ scans **only**. `//` is floor division in Python, so
    running this over Blender's `scripts/` would be the same class of error it
    exists to prevent.
    """
    return COMMENT_LINE.sub("", COMMENT_BLOCK.sub(_blank, read(path)))


def walk(root: Path, suffixes: tuple[str, ...]) -> list[Path]:
    found: list[Path] = []
    for path in root.rglob("*"):
        if path.suffix in suffixes and path.is_file():
            found.append(path)
    return found


# ---------------------------------------------------------------- the DCC

#: R1605 — the receiver is NOT always called `ot`.
#:
#: `new_compositor_sequencer_node_group` writes
#: `operator_type->idname = "…"`, and a regex that hard-codes `ot->` reported it
#: as unregistered. That is [[debt-DCC-census-by-field-name]] exactly — a
#: census keyed on a *variable name* — recurring inside the tool built to end it.
CPP_IDNAME = re.compile(r'\b[A-Za-z_][A-Za-z_0-9]*->idname\s*=\s*"(NODE_OT_[a-z_0-9]+)"')
CPP_MACRO = re.compile(r'WM_operatortype_append_macro\(\s*"(NODE_OT_[a-z_0-9]+)"')
#: R1605.1 — **and Python has two string quotes**.
#:
#: Found by the closing audit, and it is the *fourth* instance in one round of
#: the same failure: a census that accepts exactly one spelling. `node_wrangler`
#: writes `idname = 'node.nw_swap_links'`, and six operators were invisible.
#:
#: All six are addon operators, so the coverage number does not move — but that
#: is luck, not a bound. A `scripts/startup/` operator written with single quotes
#: would have left the DENOMINATOR silently short, which is the direction that
#: inflates.
PY_IDNAME = re.compile(r"""bl_idname\s*=\s*(['"])node\.([a-z_0-9]+)\1""")

#: R1605 — and sometimes the string is not in the source at all.
#:
#: `deactivate_viewer` writes `ot->idname = __func__`, so its id exists
#: only after the compiler substitutes the enclosing function's name. **No
#: text census can read that** without knowing what `__func__` means, which is
#: the sharpest available answer to "is reading C++ as text good enough": not by
#: itself, and the residue is what says so.
CPP_IDNAME_FUNC = re.compile(
    r"\bvoid\s+(NODE_OT_[a-z_0-9]+)\s*\([^)]*\)\s*\{(?:[^{}]|\{[^{}]*\})*?->idname\s*=\s*__func__"
)

#: A registration FUNCTION's own symbol, which is not an operator id.
#:
#: `void collapse_toggle(wmOperatorType *ot)` sets
#: `ot->idname = "hide_toggle"` — the function is named after a command
#: the user sees and registers an operator with a different id. Counting the
#: symbol would have invented an operator that does not exist, which is the
#: `absent`-side twin of the error R1601 corrected.
CPP_OPERATOR_FUNCTION = re.compile(r"\bvoid\s+(NODE_OT_[a-z_0-9]+)\s*\(\s*wmOperatorType\s*\*")

#: R1605 — the FIFTH way the DCC registers a node operator, and the one that
#: made this census overstate the DCC coverage.
#:
#: `NOD_socket_items_ops.hh` declares four `template<typename Accessor>` makers
#: that call `WM_operatortype_append` with the idname taken from the accessor, so
#: **no registration site anywhere writes `ot->idname = "…"`**. The names
#: live as `static constexpr StringRefNull` members of a per-accessor
#: `struct operator_idnames`, in `source/the DCC/nodes/` rather than in
#: `editors/space_node/` — a different directory, a different spelling, and 69
#: operators the census read as zero.
#:
#: the DCC's own comment beside the maker says why the string is written out at
#: all: *"The idname is passed in explicitly, so that it is more searchable"* —
#: the reference anticipated a text census and made itself findable, and this one
#: still missed it, because it accepted exactly one spelling.
#:
#: The capability behind them is **variadic ports** — a node whose socket list is
#: authored per node rather than fixed by its kind — which is the same thing
#: the engine spells `AddOptionPin` / `RemoveOptionPin`, already `absent`.
CPP_TEMPLATE_IDNAMES = re.compile(r"struct\s+operator_idnames\s*\{(.*?)\};", re.S)
CPP_TEMPLATE_IDNAME = re.compile(
    r'static\s+constexpr\s+StringRefNull\s+[a-z_0-9]+\s*=\s*"(NODE_OT_[a-z_0-9]+)"'
)
#: The generic makers themselves — the unit the capability is judged at, because
#: 69 instantiations of three behaviours are three behaviours. Same correction as
#: R1601's on macros, in the opposite direction: there, one row hid several
#: operators; here, many rows hide one capability.
CPP_TEMPLATE_MAKER = re.compile(r"template<typename Accessor>\s+inline\s+void\s+([a-z_0-9]+)\(\)")
BLENDER_TEMPLATE_OPS = "source/blender/nodes/NOD_socket_items_ops.hh"

#: Every mention of an operator name, so the classification can be shown to be
#: exhaustive rather than assumed to be.
CPP_ANY_IDNAME = re.compile(r"\bNODE_OT_[a-z_0-9]+")

#: R1605.1 — the Python side's residue, the peer of [`unreal_command_residue`].
#:
#: A `idname` whose value is not a quoted literal is **unreadable by any text
#: census**: `idname = ANIM_KS_LOCATION_ID` names an operator only after the
#: module is imported. Measured at the pinned revision: 14, all of them keying
#: sets, addon preferences and key configurations — no node operator among them.
#: That is a fact about today's tree, not a property of the mechanism, so the
#: number is printed rather than assumed to stay zero.
PY_IDNAME_ANY = re.compile(r"^\s*bl_idname\s*=\s*(.+)$", re.M)
PY_IDNAME_LITERAL = re.compile(r"""^['"]""")


def census_blender(root: Path) -> tuple[dict[str, Operator], list[str]]:
    """The four mechanisms, in precedence order.

    A name found by more than one mechanism keeps the FIRST — an operator with a
    C++ registration is a C++ operator even if a Python file mentions it, which
    is the R1598 attribution error stated as a rule instead of a hazard.
    """
    found: dict[str, Operator] = {}
    #: Function symbols named `*` — not operator ids. See
    #: [`CPP_OPERATOR_FUNCTION`].
    symbols: set[str] = set()

    def add(name: str, mechanism: str, where: str) -> None:
        found.setdefault(name, Operator(name, mechanism, where))

    source = root / "source" / "blender"
    mentioned: set[str] = set()
    for path in walk(source, (".cc", ".c", ".cpp", ".hh")):
        body = read_cxx(path)
        if "NODE_OT_" not in body:
            continue
        rel = str(path.relative_to(root))
        mentioned.update(CPP_ANY_IDNAME.findall(body))
        symbols.update(CPP_OPERATOR_FUNCTION.findall(body))
        for name in CPP_IDNAME.findall(body):
            add(name, "cpp", rel)
        for name in CPP_IDNAME_FUNC.findall(body):
            add(name, "cpp", rel)
        for name in CPP_MACRO.findall(body):
            add(name, "macro", rel)
        for block in CPP_TEMPLATE_IDNAMES.findall(body):
            for name in CPP_TEMPLATE_IDNAME.findall(block):
                add(name, "cpp-template-instance", rel)

    for maker in CPP_TEMPLATE_MAKER.findall(read_cxx(root / BLENDER_TEMPLATE_OPS)):
        add(f"socket_items::{maker}", "cpp-template", BLENDER_TEMPLATE_OPS)

    for path in walk(root / "scripts", (".py",)):
        body = read(path)
        if "bl_idname" not in body:
            continue
        rel = str(path.relative_to(root))
        core = rel.startswith("scripts/startup/")
        for _quote, stem in PY_IDNAME.findall(body):
            add("NODE_OT_" + stem, "python-core" if core else "python-addon", rel)

    for name, member, where in blender_hooks(root):
        add(f"{name}::{member}", name, where)

    # ★ The completeness claim, computed rather than asserted. Every `*`
    # token the C++ contains is exactly one of:
    #
    #   * an id assigned at a registration site (`cpp` / `macro`),
    #   * a constant a `socket_items` template registers (`cpp-template-instance`),
    #   * a registration FUNCTION's own symbol, which is not an id at all,
    #   * a string referring to an operator Python registers (the R1598 case).
    #
    # A token in none of those is a registration mechanism nobody has read —
    # which is what `cpp-template`, `__func__` and the non-`ot` receiver each
    # turned out to be, all three found by asking this question rather than by
    # reasoning about the regexes.
    unregistered = sorted(mentioned - set(found) - symbols)
    return found, unregistered


#: A C function pointer field, `void (*insert_link)(..)`.
HOOK_POINTER = re.compile(r"\(\*([a-zA-Z_0-9]+)\)\s*\(")
#: The same slot written as a `std::function`, which the DCC is migrating to.
HOOK_FUNCTION = re.compile(r"^\s*std::function<.*>\s+([a-zA-Z_0-9]+)\s*;")

#: the DCC's node-system extension surface: what a node type, a tree type and a
#: socket type may each answer.
BLENDER_HOOK_STRUCTS = ("bNodeType", "bNodeTreeType", "bNodeSocketType")


def blender_computed_idnames(root: Path) -> int:
    """`bl_idname` assignments whose value is not a quoted literal.

    The Python peer of [`unreal_command_residue`]: the part of the reference no
    text scan can read, counted instead of wished away. If one of these ever
    resolves to a `node.` id, the operator denominator is short and nothing else
    would say so.
    """
    loose = 0
    for path in walk(root / "scripts", (".py",)):
        body = read(path)
        if "bl_idname" not in body:
            continue
        for value in PY_IDNAME_ANY.findall(body):
            if not PY_IDNAME_LITERAL.match(value.strip()):
                loose += 1
    return loose


def blender_hooks(root: Path) -> list[tuple[str, str, str]]:
    """`(struct, member, where)` for every callback slot in those three structs.

    Brace-counted rather than regex-scoped: a struct's extent is a nesting
    question and a regex answering one is how a census acquires a hole.
    """
    where = "source/blender/blenkernel/BKE_node.hh"
    lines = read_cxx(root / where).split("\n")
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


# ----------------------------------------------------------------- the engine

UE_COMMAND = re.compile(r"TSharedPtr<\s*FUICommandInfo\s*>\s*([A-Za-z_0-9]+)")
UE_VIRTUAL = re.compile(r"\bvirtual\s+[A-Za-z_0-9:<>&*,\s]+?\b([A-Za-z_0-9]+)\s*\(")

#: `class [MODULE_API] FX : public TCommands<FX>`, with the base clause allowed
#: to sit on the next line — which it does in a third of the tree, and a
#: single-line regex silently reported those classes as holding no commands.
UE_COMMANDS_CLASS = re.compile(
    r"\bclass\s+(?:[A-Z][A-Z_0-9]*_API\s+)?([A-Za-z_][A-Za-z_0-9]*)"
    r"\s*:\s*public\s+TCommands\s*<\s*\1\s*>"
)

#: Where the command classes are looked for. The hook headers are named exactly
#: because they are two known types; a command class is found by SEARCHING,
#: because the whole point is that nobody knows how many there are.
UNREAL_EDITOR = "Engine/Source/Editor"

#: the engine's node-system extension surface, and the peer of the DCC's three
#: structs: what a *graph* answers (the schema) and what a *node* answers.
UNREAL_HOOK_HEADERS = {
    "UEdGraphSchema": "Engine/Source/Runtime/Engine/Classes/EdGraph/EdGraphSchema.h",
    "UEdGraphNode": "Engine/Source/Runtime/Engine/Classes/EdGraph/EdGraphNode.h",
}


def unreal_command_classes(root: Path) -> list[tuple[str, str, list[str], str]]:
    """`(class, module, members, where)` for every `TCommands` subclass under
    `Engine/Source/Editor`.

    Brace-counted from the class's own opening brace, for the reason
    [`blender_hooks`] is: a class's extent is a nesting question, and one file
    holding three command classes is the case that makes the answer matter.
    """
    editor = root / UNREAL_EDITOR
    found: list[tuple[str, str, list[str], str]] = []
    for path in sorted(walk(editor, (".h",))):
        body = read_cxx(path)
        if "FUICommandInfo" not in body:
            continue
        module = path.relative_to(editor).parts[0]
        rel = str(path.relative_to(root))
        for match in UE_COMMANDS_CLASS.finditer(body):
            start = body.find("{", match.end())
            if start < 0:
                continue
            depth = 0
            index = start
            while index < len(body):
                if body[index] == "{":
                    depth += 1
                elif body[index] == "}":
                    depth -= 1
                    if depth == 0:
                        break
                index += 1
            found.append((match.group(1), module, UE_COMMAND.findall(body[start:index]), rel))
    return found


def unreal_command_residue(root: Path) -> list[str]:
    """`FUICommandInfo` declarations that are in no `TCommands` class at all.

    The honest answer to "is reading C++ as text good enough". A real parse is
    out of reach (it would need each tree's include graph and build config), so
    the residue is **measured** instead of assumed away: if these were command
    lists the census reads none of them, and if they are not, the number should
    stay small and the identifiers should look like parameters.

    Measured at the pinned revision: 56, whose identifiers are `InCommand`,
    `UICommand`, `InputCommand` and the like — function parameters and widget
    members holding one command, not lists declaring many.
    """
    editor = root / UNREAL_EDITOR
    spans: dict[Path, list[tuple[int, int]]] = {}
    for path in sorted(walk(editor, (".h",))):
        body = read_cxx(path)
        if "FUICommandInfo" not in body:
            continue
        found: list[tuple[int, int]] = []
        for match in UE_COMMANDS_CLASS.finditer(body):
            start = body.find("{", match.end())
            if start < 0:
                continue
            depth = 0
            index = start
            while index < len(body):
                if body[index] == "{":
                    depth += 1
                elif body[index] == "}":
                    depth -= 1
                    if depth == 0:
                        break
                index += 1
            found.append((start, index))
        spans[path] = found
    loose: list[str] = []
    for path, found in spans.items():
        body = read_cxx(path)
        for match in UE_COMMAND.finditer(body):
            if not any(start <= match.start() < end for start, end in found):
                loose.append(match.group(1))
    return sorted(loose)


#: `void UWhatever::GetNodeContextMenuActions(` — a node class's own menu.
UE_CONTEXT_MENU = re.compile(r"\bvoid\s+([A-Za-z_0-9]+)::GetNodeContextMenuActions\s*\(")
#: The menu entry reaching a `TCommands` list, versus being built on the spot.
UE_MENU_COMMAND = re.compile(r"F[A-Za-z_0-9]*Commands(?:Impl)?::Get\(\)")
UE_MENU_INLINE = ("FUIAction(", "FExecuteAction::Create")


def unreal_context_menu_units(root: Path) -> tuple[int, int, int]:
    """`(reaches a command list, builds the action inline, neither)`.

    R1603 registered "the per-node context menu might be a surface we do not
    count" and said to **measure the unit first**, because choosing the wrong one
    is the recorded failure ([[debt-blender-census-by-field-name]]). This is that
    measurement, computed rather than remembered so it cannot go stale quietly.

    The answer at the pinned revision is 17 / 16 / 3 of 36 overrides. The 17 name
    a `FUICommandInfo` from a `TCommands` class, so they are the **same unit** as
    the command surface and are already in the denominator now that the
    per-editor lists are read. The 16 build an `FUIAction` on the spot: those
    entries have **no name anywhere** — no id, no binding, nothing to key a row
    on — so they cannot be a census unit at all, and what they expose (add a pin,
    remove a pin, convert this node) is already named on the command lists.
    """
    reaches = inline = neither = 0
    for path in walk(root / "Engine" / "Source", (".cpp",)):
        body = read_cxx(path)
        if "::GetNodeContextMenuActions(" not in body:
            continue
        for match in UE_CONTEXT_MENU.finditer(body):
            start = body.find("{", match.end())
            if start < 0:
                continue
            depth = 0
            index = start
            while index < len(body):
                if body[index] == "{":
                    depth += 1
                elif body[index] == "}":
                    depth -= 1
                    if depth == 0:
                        break
                index += 1
            menu = body[start:index]
            if UE_MENU_COMMAND.search(menu):
                reaches += 1
            elif any(mark in menu for mark in UE_MENU_INLINE):
                inline += 1
            else:
                neither += 1
    return reaches, inline, neither


#: The CRTP-free spelling, matched without knowing the class's own name — an
#: independent way of asking "is there a command list here", so the structured
#: parse can be checked against something that does not share its assumptions.
UE_COMMANDS_BASE = re.compile(r"public\s+TCommands\s*<\s*([A-Za-z_][A-Za-z_0-9]*)\s*>")

#: Names that are legitimately not a command class, with the reason.
UNREAL_COMMANDS_NOT_A_CLASS = {
    "CommandContextType": "the type PARAMETER of `TInteractiveToolCommands`, "
    "a template base rather than a CRTP class",
}


#: A plugin module that declares its own graph type — i.e. an editor whose
#: document is a node graph.
UE_SCHEMA_SUBCLASS = re.compile(
    r"class\s+(?:[A-Z][A-Z_0-9]*\s+)?U[A-Za-z_0-9]*\s*:\s*public\s+U(?:EdGraph|RigVMEdGraph)Schema"
)


def unreal_plugin_graph_scope(root: Path) -> tuple[int, int, int]:
    """`(modules, command classes, commands)` under `Engine/Plugins` that this
    census does **not** read.

    R1605 found this and deliberately did not close it, so the size is computed
    at every run rather than written down once. Unreal's modern node graphs —
    Niagara, MetaSound, PCG, RigVM / Control Rig, StateTree, Optimus — ship as
    plugins, and a census of `Engine/Source/Editor` cannot see any of them.

    ★ It also found the reason the scope cannot simply be widened: a class NAME
    is not unique. `FBlueprintEditorCommands` exists in both `Kismet` and
    `SceneStateBlueprintEditor`, and `FEditorCommands` in both
    `WorldPartitionEditor` and `MetasoundEditor` — so the key has to carry the
    module before the denominator grows, or two different editors' commands
    would merge into one row. See [[debt-census-stops-at-engine-source-editor]].
    """
    plugins = root / "Engine" / "Plugins"
    if not plugins.is_dir():
        return (0, 0, 0)
    schema_modules: set[str] = set()
    for path in walk(plugins, (".h",)):
        parts = path.relative_to(plugins).parts
        if "Source" not in parts:
            continue
        body = read_cxx(path)
        if "Schema" in body and UE_SCHEMA_SUBCLASS.search(body):
            schema_modules.add(parts[parts.index("Source") + 1])
    classes = commands = 0
    for path in walk(plugins, (".h",)):
        parts = path.relative_to(plugins).parts
        if "Source" not in parts or parts[parts.index("Source") + 1] not in schema_modules:
            continue
        body = read_cxx(path)
        if "FUICommandInfo" not in body:
            continue
        for match in UE_COMMANDS_CLASS.finditer(body):
            start = body.find("{", match.end())
            if start < 0:
                continue
            depth = 0
            index = start
            while index < len(body):
                if body[index] == "{":
                    depth += 1
                elif body[index] == "}":
                    depth -= 1
                    if depth == 0:
                        break
                index += 1
            classes += 1
            commands += len(UE_COMMAND.findall(body[start:index]))
    return (len(schema_modules), classes, commands)


def unreal_command_classes_missed(root: Path) -> list[str]:
    """`public TCommands<X>` where the structured parse produced no class `X`.

    ★ This check exists because a counterfactual PASSED without it. Narrowing
    [`UE_COMMANDS_CLASS`] so it no longer matches a base clause on the next line
    made whole command classes **vanish**, and [`unreal_command_scope`] could not
    see it: that check iterates the classes the parser FOUND, so a class the
    parser stops recognising leaves no trace in it at all. A scope check whose
    input is the parser's own output cannot audit the parser.

    So the audit is a second, deliberately dumber reading — a substring that does
    not know the class's name — and a disagreement between the two is the
    finding.
    """
    editor = root / UNREAL_EDITOR
    missed: list[str] = []
    for path in sorted(walk(editor, (".h",))):
        body = read_cxx(path)
        if "TCommands" not in body:
            continue
        parsed = set(UE_COMMANDS_CLASS.findall(body))
        for name in UE_COMMANDS_BASE.findall(body):
            if name not in parsed and name not in UNREAL_COMMANDS_NOT_A_CLASS:
                missed.append(f"{name} ({path.relative_to(editor)})")
    return sorted(set(missed))


def unreal_command_scope(root: Path) -> list[str]:
    """Command classes the census neither reads nor excludes by name.

    The finding this returns is the one R1603 could not make: its scope was one
    header, chosen by hand, and nothing said what was outside it.
    """
    excluded = {name for names in UNREAL_COMMANDS_OUT.values() for name in names}
    return sorted(
        name
        for name, _module, _members, _where in unreal_command_classes(root)
        if name not in UNREAL_COMMAND_CLASSES and name not in excluded
    )


def census_unreal(root: Path) -> dict[str, Operator]:
    """Two surfaces.

    **Commands** — the `TCommands` classes of Unreal's node-graph editors, the
    peer of Blender's operator list. Deliberately NOT `UK2Node_*`: those are node
    *types*, the analogue of the 429 registered node types this campaign already
    excludes as content.

    **Hooks** — the virtuals of `UEdGraphSchema` and `UEdGraphNode`. R1601 named
    this surface as uncounted and R1602 measured why it matters: the two things
    R1593 and R1594 closed are on it and on no operator list anywhere.
    """
    found: dict[str, Operator] = {}
    for name, _module, members, where in unreal_command_classes(root):
        tag = UNREAL_COMMAND_CLASSES.get(name)
        if tag is None:
            continue
        for member in members:
            key = f"{tag}::{member}"
            found.setdefault(key, Operator(key, tag, where))
    for owner, header in UNREAL_HOOK_HEADERS.items():
        for member in UE_VIRTUAL.findall(read_cxx(root / header)):
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
        # R1614.1 -- the pin is keyed by the PUBLIC tree name and by the PUBLIC
        # operator id; `emit` learned that in R1612 and this reader did not, so
        # `pin.get(tree)` missed every time. The report then said `0/0` coverage
        # and called all 679 live operators NEW, while `--check-pin` -- which
        # reads the pin directly and never touches a live tree -- went on saying
        # 679 judged, 0 problems. Two readers of one census, one taught. The
        # same shape as R1612.3, in the tool that recorded it.
        pinned = pin.get(PUBLIC_TREE.get(tree, tree), {})
        # ★★★★★ R1919 — the WHOLE operator, not just its key. See `public_view`.
        live = public_view(tree, live)
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
            # And per mechanism under it, for the same reason one level down:
            # nine command lists summed into one percentage would let the
            # generic canvas hide a per-editor list nobody had read.
            for surface in sorted({surface_of(row) for row in pinned.values()}):
                s_have, s_total, s_absent = coverage(pinned, surface)
                if not s_total:
                    continue
                print(
                    f"    {surface:<10} {s_have}/{s_total} = "
                    f"{100 * s_have // s_total}%   ({len(s_absent)} absent)"
                )
                mechanisms = sorted(
                    {row.get("mechanism", "") for row in pinned.values()
                     if surface_of(row) == surface}
                )
                if len(mechanisms) < 2:
                    continue
                for mechanism in mechanisms:
                    rows = {n: r for n, r in pinned.items()
                            if r.get("mechanism") == mechanism}
                    m_have, m_total, _ = coverage(rows)
                    if m_total:
                        print(
                            f"      {mechanism:<24} {m_have}/{m_total} = "
                            f"{100 * m_have // m_total}%"
                        )
            out = len(pinned) - total
            if out:
                reasons: dict[str, int] = {}
                for row in pinned.values():
                    verdict = row.get("verdict")
                    # Every verdict that is not in the numerator or the
                    # denominator, derived from the vocabulary rather than
                    # listed again — a second list is a second place to forget
                    # a class, and `instance` was added by exactly that route.
                    if verdict in VERDICTS and verdict not in ("have", "absent"):
                        reasons[verdict] = reasons.get(verdict, 0) + 1
                shape = ", ".join(f"{k} {v}" for k, v in sorted(reasons.items()))
                print(f"  out of the denominator: {out} ({shape})")
            if absent:
                print(f"  ABSENT ({len(absent)}): {', '.join(absent)}")

        if not present:
            continue
        diff = compare(live, pinned)
        if tree == "unreal":
            diff["unscoped"] = unreal_command_scope(UNREAL)
            diff["unparsed"] = unreal_command_classes_missed(UNREAL)
            loose = unreal_command_residue(UNREAL)
            print(
                f"  {len(loose)} command declaration(s) in no TCommands class "
                f"({', '.join(sorted(set(loose))[:4])}…) — parameters and widget "
                "members, not lists"
            )
            modules, classes, commands = unreal_plugin_graph_scope(UNREAL)
            print(
                f"  OUT OF SCOPE: {commands} command(s) in {classes} class(es) "
                f"across {modules} Engine/Plugins module(s) that declare a graph "
                "schema (Niagara, MetaSound, PCG, RigVM, StateTree…) — "
                "debt-census-stops-at-engine-source-editor"
            )
            reaches, inline, neither = unreal_context_menu_units(UNREAL)
            print(
                f"  per-node context menus: {reaches} reach a command list "
                f"(same unit, counted), {inline} build the action inline "
                f"(no name to count), {neither} neither"
            )
        if tree == "blender":
            print(
                f"  {blender_computed_idnames(BLENDER)} Python bl_idname(s) are "
                "not a quoted literal — computed at import, unreadable by any "
                "text census (measured: keying sets and preferences, no node op)"
            )
            # Named `unregistered` rather than folded into `new`: these are not
            # rows the pin lacks, they are names no mechanism explains.
            diff["unregistered"] = [
                name
                for name in census.blender_unregistered
                if public_id(tree, name) not in pinned
                or pinned[public_id(tree, name)].get("mechanism", "").startswith("cpp")
            ]
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


# --- what the COMMITTED artifact is allowed to say (R1612) -----------------
#
# This file reads two other projects' source trees, so it cannot avoid naming
# their layout; the pin it writes is a different matter, because that IS a
# pushed artifact. The mapping below is what makes the census publishable, and
# it is a rename rather than an encoding: an operator id is
# `<vendor prefix>_<capability>` and the capability half is already the generic
# name, so `add_group` becomes `add_group` and nothing is lost. Hashing
# the ids would have been the other option and it would have made the census
# unreadable by the people who maintain it.
PUBLIC_TREE = {"blender": "dcc", "unreal": "engine"}

# An owner becomes exactly the stem `proof_name` already derived from it, so
# every proof identifier in the crates keeps the name it has.
PUBLIC_OWNER = {
    "UEdGraphSchema": "schema",
    "UEdGraphNode": "node",
    "bNodeType": "node",
    "bNodeSocketType": "node_socket",
    "bNodeTreeType": "node_tree",
    "BlueprintEditor": "script_editor",
}

PUBLIC_MECHANISM = {
    "UEdGraphSchema": "graph-schema",
    "UEdGraphNode": "graph-node",
    "bNodeType": "node-type",
    "bNodeSocketType": "socket-type",
    "bNodeTreeType": "tree-type",
    "BlueprintEditor": "script-editor",
}


def public_view(tree: str, live: dict[str, "Operator"]) -> dict[str, "Operator"]:
    """★★★★★ R1919 — a live census **wholly** in the spelling the pin uses.

    One function, because a half-translated census is what R1919 found: the
    reporting path re-keyed the operators by [`public_id`] and left every
    `Operator.mechanism` spelled the way the other project's headers spell it,
    while [`emit`] — the function that WRITES the pin — mapped both. So the
    pin's ids came from one translation and its mechanisms from another, and
    `compare`, which is handed both, reported all 34 of them RECLASSIFIED
    forever.

    Returning a translated census rather than mapping inside `compare` keeps
    `compare` pure over one vocabulary and makes *translated in one field and
    not the other* unrepresentable — the shape R1891 recorded as stronger than
    a rule saying the two must agree.
    """
    return {
        public_id(tree, name): replace(
            op, name=public_id(tree, op.name), mechanism=public_mechanism(op.mechanism)
        )
        for name, op in live.items()
    }


def public_mechanism(mechanism: str) -> str:
    """The mechanism as the committed pin spells it."""
    return PUBLIC_MECHANISM.get(mechanism, mechanism)


def public_id(tree: str, name: str) -> str:
    """The operator's id as the committed pin spells it.

    ★★★★★ R1919 — **the owner mapping applies to BOTH trees.** It did not: the
    `blender` arm stripped the operator prefix and returned, so a DCC name
    carrying an owner (`bNodeType::poll`) never met [`PUBLIC_OWNER`] and stayed
    spelled the way that project's own headers spell it.

    Measured at R1919's open, over the live trees: **34 NEW and 34 GONE, and
    they were the same 34 under two spellings** — every `bNodeType::` /
    `bNodeSocketType::` / `bNodeTreeType::` name paired with exactly one
    `node::` / `node_socket::` / `node_tree::` row of the pin, with no leftover
    on either side. So the tool reported, every run, that a third of the DCC
    census had appeared and the same third had vanished.

    ⚠ The half that made it a defect rather than noise: **six of those rows
    were ALSO reported ABSENT**, which is the tool saying two contradictory
    things about one row — *the pin judges something the tree does not have*
    and *the tree has something the pin says we lack*. And the campaign this
    census closes is closed by `absent` reaching zero, so six rows nothing
    could ever build were sitting in the number that has to reach it.

    ⚠ Two spellings is also a PUBLISHABILITY defect, which is what
    [`PUBLIC_OWNER`] exists for (R1612): the pin is a pushed artifact, and
    `bNodeType` is the other project's internal struct name rather than the
    neutral stem this repository publishes. The pin was right and this
    function was wrong, which is why the repair is here and not in the pin.

    This is R1614.1's class for the third time — two readers of one census and
    only one taught — and its comment is eleven lines above the site.
    """
    if tree == "blender":
        name = name.removeprefix("NODE_OT_")
    if "::" in name:
        owner, member = name.split("::", 1)
        return f"{PUBLIC_OWNER.get(owner, owner)}::{member}"
    return name


def emit(census: Census, pin: dict) -> None:
    """A starter pin: every live operator, verdicts carried over where the pin
    already has one and left blank where it does not.

    Ids, tree names and mechanisms come out in their PUBLIC spelling, and
    `where` -- a path inside a reference tree -- is left out entirely. It was
    provenance for a local audit and is regenerable by whoever has the tree; in
    the committed artifact it was 197 occurrences of a vendor's directory
    layout.
    """
    out = {}
    for tree, live in census.all().items():
        public_tree = PUBLIC_TREE.get(tree, tree)
        if not live:
            out[public_tree] = pin.get(public_tree, {})
            continue
        rows = {}
        for name in sorted(live):
            op = live[name]
            key = public_id(tree, name)
            previous = pin.get(public_tree, {}).get(key, {})
            rows[key] = {
                "mechanism": PUBLIC_MECHANISM.get(op.mechanism, op.mechanism),
                "verdict": previous.get("verdict", ""),
                "covered_by": previous.get("covered_by", ""),
                "proven_by": previous.get("proven_by", ""),
            }
        out[public_tree] = rows
    print(json.dumps(out, indent=2, sort_keys=True))


# -------------------------------------------------------------- selftest

FAILURES: list[str] = []


def check(condition: bool, what: str) -> None:
    print(("  ok   " if condition else "  FAIL ") + what)
    if not condition:
        FAILURES.append(what)


def selftest() -> int:
    print("the committed spelling")
    check(public_id("blender", "NODE_OT_add_group") == "add_group",
          "an operator id keeps its capability half and loses the prefix")
    check(public_id("unreal", "UEdGraphSchema::AddAction") == "schema::AddAction",
          "an owner becomes the stem the proof name already derived")
    check(public_id("unreal", "AnimGraph::AddBlendListPin")
          == "AnimGraph::AddBlendListPin",
          "an owner that names no vendor is left alone")
    check(len({public_id("blender", name) for name in
               ("NODE_OT_add_group", "NODE_OT_add_collection")}) == 2,
          "the rename is injective on the ids it changes")
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

    print("the public vocabulary reaches BOTH trees and BOTH fields (R1919)")
    # ★★★★★ The population is the MAPPING TABLES themselves, so a row added to
    # either one is demanded here without this block being edited — and there is
    # nowhere to write an exemption. R1919 found the tables applied to one tree
    # and one field: `public_id`'s blender arm returned before the owner map,
    # and `compare` read `Operator.mechanism` raw while `emit` mapped it. The
    # tool then reported 301 findings on every run — 34 NEW + 34 GONE on the DCC
    # tree and 233 RECLASSIFIED on the engine one — and every one of them was
    # this, not a reference that had moved.
    for owner, stem in PUBLIC_OWNER.items():
        for tree in PUBLIC_TREE:
            check(
                public_id(tree, f"{owner}::probe") == f"{stem}::probe",
                f"`{owner}::` becomes `{stem}::` on the `{tree}` tree too",
            )
    for mechanism, public in PUBLIC_MECHANISM.items():
        check(
            public_mechanism(mechanism) == public,
            f"the mechanism `{mechanism}` is published as `{public}`",
        )
    # ★ And the WHOLE operator travels, which is what `compare` is handed. A
    # translation that moved the key and left the mechanism is exactly what ran
    # for as long as this defect lasted, so the assertion is about both.
    translated = public_view(
        "blender", {"bNodeType::probe": Operator("bNodeType::probe", "bNodeType")}
    )
    check(list(translated) == ["node::probe"], "the key is translated")
    check(
        translated["node::probe"].mechanism == "node-type",
        "★ and so is the mechanism — a half-translated census is unrepresentable",
    )
    check(
        translated["node::probe"].name == "node::probe",
        "and the operator's own name, so nothing carries the internal spelling",
    )

    print("the report reads the pin under the PUBLIC names emit writes")
    # R1614.1 -- this runs `report` itself over a synthetic tree and pin,
    # because the round's FIRST version of this check asserted the PIN's shape
    # instead, and a counterfactual that restored the broken lookup passed it.
    # A test of the artifact is not a test of the reader.
    synthetic = Census(
        blender={
            "NODE_OT_probe": Operator("NODE_OT_probe", "cpp"),
            # ★★★★★ R1919 — an owner-carrying DCC name, which is the shape the
            # round found untranslated. Without one this check asks about the
            # engine tree only, and R1614.1's version did exactly that: it
            # passed for 300 rounds while a third of the DCC census and every
            # engine mechanism went through untranslated.
            "bNodeType::probe": Operator("bNodeType::probe", "bNodeType"),
        },
        unreal={"UEdGraphSchema::Probe": Operator("UEdGraphSchema::Probe", "UEdGraphSchema")},
    )
    synthetic_pin = {
        "dcc": {"probe": {"mechanism": "cpp", "verdict": "have",
                          "proven_by": "pinion-core::probe"},
                "node::probe": {"mechanism": "node-type", "verdict": "have",
                                "proven_by": "pinion-core::probe"}},
        "engine": {"schema::Probe": {"mechanism": "graph-schema", "verdict": "have",
                                     "proven_by": "pinion-core::probe"}},
    }
    buffer = io.StringIO()
    with contextlib.redirect_stdout(buffer):
        report(synthetic, synthetic_pin, strict=False)
    printed = buffer.getvalue()
    check(
        "2/2 = 100%" in printed and "1/1 = 100%" in printed,
        "★ report() finds the pin: a live id is mapped to its public spelling "
        "and the tree to its public name BEFORE the lookup",
    )
    check(
        "NEW (" not in printed,
        "and a judged operator is not reported as one the pin lacks",
    )
    check(
        "GONE (" not in printed,
        "nor is a pinned row reported as one the tree lacks",
    )
    check(
        "RECLASSIFIED (" not in printed,
        "★★★★★ nor is a judged operator reported as having changed mechanism — "
        "the finding 267 of this tool's 301 were, and none of them was a "
        "reference that had moved",
    )

    # R1614.1 -- the defect this closes: `report` looked the pin up under the
    # LIVE tree name and compared LIVE operator ids, while `emit` had written
    # both in their public spelling since R1612. Every lookup missed, coverage
    # printed 0/0, and 679 judged operators were reported as unjudged -- while
    # `--check-pin`, which never touches a live tree, kept saying 679 judged and
    # 0 problems. The two checks below are what tells the fixed reader from the
    # broken one.
    pin_now = load_pin()
    if pin_now:
        for tree, public in PUBLIC_TREE.items():
            _, total, _ = coverage(pin_now.get(public, {}))
            check(total > 0, f"the pin answers under `{public}`")
            check(
                not pin_now.get(tree),
                f"and NOT under `{tree}` -- a reader keyed on the live name "
                f"silently measures nothing",
            )
        check(
            not any(k.startswith("NODE_OT_") for k in pin_now.get("dcc", {})),
            "and its ids are public, so a live id must be mapped before lookup",
        )

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
        == {"have", "absent", "composition", "app-content", "addon",
            "host-framework", "instance"},
        "and every out-of-denominator class has a stated reason",
    )
    pinned = {
        "socket_items::make_add_item_operator": {"mechanism": "cpp-template",
                                                 "verdict": "absent"},
        "NODE_OT_repeat_zone_item_add": {"mechanism": "cpp-template-instance",
                                         "verdict": "instance"},
        "NODE_OT_simulation_zone_item_add": {"mechanism": "cpp-template-instance",
                                             "verdict": "instance"},
    }
    have, total, absent = coverage(pinned)
    check(
        (have, total) == (0, 1) and absent == ["socket_items::make_add_item_operator"],
        "★ 69 instantiations of one behaviour are ONE row in the denominator — "
        "`instance` is `composition` in the other direction",
    )

    print("what is OWED is a query over the pin, not a list anyone maintains (R1919)")
    # ★★★★★ The population is the pin's own `absent` rows and the grouping key
    # is its own `covered_by`, so this listing has nothing of its own to be
    # stale about — which is the whole point. Three hand-written versions of
    # this remainder were measured wrong before it became a query.
    owed_pin = {
        "dcc": {
            "a": {"mechanism": "cpp", "verdict": "absent", "covered_by": "one gap"},
            "b": {"mechanism": "cpp", "verdict": "absent", "covered_by": "one gap"},
            "c": {"mechanism": "cpp", "verdict": "have", "covered_by": "built",
                  "proven_by": "pinion-core::probe"},
        },
        "engine": {
            "d": {"mechanism": "graph-node", "verdict": "absent",
                  "covered_by": "one gap"},
            "e": {"mechanism": "graph-node", "verdict": "absent",
                  "covered_by": "another gap"},
        },
    }
    buffer = io.StringIO()
    with contextlib.redirect_stdout(buffer):
        rc = owed(owed_pin)
    printed = buffer.getvalue()
    check(rc == 0, "a pin whose absent rows all give a reason is reportable")
    check(
        "4 row(s) still absent, in 2 group(s)" in printed,
        "★ only `absent` rows are owed — a `have` is not a remainder",
    )
    check(
        printed.index("one gap") < printed.index("another gap"),
        "★ the biggest chunk is named first, because that is the work order",
    )
    check(
        printed.index("dcc     a") < printed.index("engine  d"),
        "★★★★★ and a chunk SPANS the trees — the six rows this round closed "
        "were one mechanism across both, and so is the largest one left",
    )
    # ★ An unclassified row is not a pass. R1919's standing rule: an escape
    # hatch that quietly absorbs a row would make this listing look complete
    # while the row it could not place is exactly the one nobody can plan.
    owed_pin["dcc"]["f"] = {"mechanism": "cpp", "verdict": "absent", "covered_by": ""}
    with contextlib.redirect_stdout(io.StringIO()):
        rc = owed(owed_pin)
    check(rc == 1, "★ an absent row with no reason is RED, not silently grouped")

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
        [stem for _q, stem in PY_IDNAME.findall('    bl_idname = "node.swap_node"')]
        == ["swap_node"],
        "a Python operator is found where the C++ census cannot see it",
    )
    check(
        [stem for _q, stem in PY_IDNAME.findall("    bl_idname = 'node.nw_del_unused'")]
        == ["nw_del_unused"],
        "★ and so is one written with the OTHER quote — Python has two, and "
        "accepting one is the same failure as accepting one C++ spelling",
    )
    check(
        PY_IDNAME.findall("""bl_idname = "node.x'""") == [],
        "while mismatched quotes are not a literal and are not read as one",
    )
    check(
        PY_IDNAME_LITERAL.match("ANIM_KS_LOCATION_ID") is None
        and PY_IDNAME_LITERAL.match("'node.x'") is not None,
        "and a COMPUTED bl_idname is told apart from a literal, so the part no "
        "text census can read is counted rather than assumed to be empty",
    )

    print("R1605 — and reading C++ as text is not enough")
    check(
        CPP_IDNAME.findall('  operator_type->idname = "NODE_OT_new_group";')
        == ["NODE_OT_new_group"],
        "★ the receiver is not always called `ot` — a census keyed on a VARIABLE "
        "name is the error this tool exists to end, and it had it",
    )
    check(
        CPP_IDNAME_FUNC.findall(
            "void NODE_OT_deactivate_viewer(wmOperatorType *ot)\n{\n"
            '  ot->name = "Deactivate";\n  ot->idname = __func__;\n}\n'
        )
        == ["NODE_OT_deactivate_viewer"],
        "★ and sometimes the id is `__func__`, so the string is not in the source",
    )
    check(
        CPP_OPERATOR_FUNCTION.findall("void NODE_OT_collapse_toggle(wmOperatorType *ot)")
        == ["NODE_OT_collapse_toggle"],
        "a registration FUNCTION's symbol is recognised as a symbol",
    )
    collapse = (
        "void NODE_OT_collapse_toggle(wmOperatorType *ot)\n{\n"
        '  ot->idname = "NODE_OT_hide_toggle";\n}\n'
    )
    check(
        CPP_IDNAME.findall(collapse) == ["NODE_OT_hide_toggle"]
        and CPP_IDNAME_FUNC.findall(collapse) == [],
        "★ and it is NOT the id: this function is named after one operator and "
        "registers another, so counting the symbol would invent one",
    )
    check(
        CPP_TEMPLATE_IDNAME.findall(
            '    static constexpr StringRefNull add_item = "NODE_OT_repeat_zone_item_add";'
        )
        == ["NODE_OT_repeat_zone_item_add"],
        "a template-registered id is found where no assignment exists",
    )
    check(
        CPP_TEMPLATE_MAKER.findall(
            "template<typename Accessor> inline void make_add_item_operator()"
        )
        == ["make_add_item_operator"],
        "and so is the maker that registers it, which is where the verdict goes",
    )
    check(
        CPP_IDNAME.findall(read_cxx.__doc__ or "") == []
        and COMMENT_BLOCK.sub(_blank, '/* ot->idname = "NODE_OT_ghost"; */').strip() == "",
        "★ a comment is not a declaration",
    )
    check(
        COMMENT_BLOCK.sub(_blank, "a{/*\n}\n*/}b").count("\n") == 2,
        "and blanking it preserves the newlines a brace count and a line regex need",
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

    print("R1605 — the command surface is not one list, and its unit is a CLASS")
    check(
        UE_COMMANDS_CLASS.findall("class FGraphEditorCommandsImpl : public "
                                  "TCommands<FGraphEditorCommandsImpl>")
        == ["FGraphEditorCommandsImpl"],
        "a command class is found",
    )
    check(
        UE_COMMANDS_CLASS.findall(
            "class ADVANCEDPREVIEWSCENE_API FAdvancedPreviewSceneCommands \n"
            "\t: public TCommands<FAdvancedPreviewSceneCommands>"
        )
        == ["FAdvancedPreviewSceneCommands"],
        "★ and so is one whose base clause is on the NEXT line, which a third of "
        "the tree writes and a single-line regex reports as holding no commands",
    )
    check(
        UE_COMMANDS_CLASS.findall("class FThing : public TCommands<FOther>") == [],
        "and a class parameterised by a DIFFERENT type is not one",
    )
    check(
        UE_COMMANDS_BASE.findall("class FThing\n\t: public TCommands<FThing>")
        == ["FThing"],
        "★ the audit reads the base clause WITHOUT the class's name, so it does "
        "not share the parse's assumptions — a check fed by the parser's own "
        "output cannot audit the parser, which a counterfactual proved",
    )
    check(
        set(UNREAL_COMMANDS_NOT_A_CLASS) == {"CommandContextType"},
        "and the one name that is a template parameter rather than a class is "
        "excluded by name, with its reason",
    )
    excluded = [name for names in UNREAL_COMMANDS_OUT.values() for name in names]
    check(
        len(excluded) == len(set(excluded)),
        "no command class is excluded twice, for two different reasons",
    )
    check(
        not (set(excluded) & set(UNREAL_COMMAND_CLASSES)),
        "★ and none is both read and excluded — the two tables partition, which "
        "is what makes 'in neither' a finding rather than an ambiguity",
    )
    check(
        all(SURFACE_OF.get(tag) == "command" for tag in UNREAL_COMMAND_CLASSES.values()),
        "every read command class answers the command surface, folded in rather "
        "than restated",
    )
    check(
        len(set(UNREAL_COMMAND_CLASSES.values())) == len(UNREAL_COMMAND_CLASSES),
        "and two command classes do not share a tag, which would merge their rows",
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


def owed(pin: dict) -> int:
    """★★★★★ R1919 — **every row still absent, grouped by the reason the PIN
    ITSELF gives**, largest group first. The campaign's remaining distance, as
    a query.

    # Why this is the tool's job and not a list in a memory file

    The campaign that this census closes kept its remainder as a hand-written
    table, and that table has now been measured wrong **three** times: it said
    `Blender 12` when the tool said 16 and named a different twelve; it said
    `Unreal 31` after four of those chunks had closed; and R1919's own audit
    found its *outputs* section naming two types (`Descent`, `through`) that
    the round had abandoned mid-flight and that exist nowhere in the code. Each
    time the list was written by someone who had just run the tool. ⇒ **a
    remainder kept as prose rots in the direction that hides work**, which is
    R1833's finding on the Phase B axes, in the tool one campaign further on.

    # Why `covered_by` is the right key, and not a classifier

    Because it is *already there and already load-bearing*: `check_pin` refuses
    a verdict with no reason, so every absent row carries one, and six rows
    closed in this very round precisely because their `covered_by` texts had
    said — long before anyone reached for them — that they were **one
    mechanism**. Grouping on it therefore surfaces the same fact for the rest
    without anyone inventing a taxonomy. A classifier written here would be a
    SECOND judgement about the same rows, free to disagree with the pin's; this
    has nothing of its own to be wrong about.

    ⚠ It groups by the reason **as written**. Two rows blocked on the same
    thing but worded differently sit apart, which understates a chunk and never
    overstates one — the safe direction, and the one a reader can repair by
    making the two texts agree.

    Returns non-zero if any absent row gives no reason at all: an unclassified
    row is not a pass, it is the one row this listing cannot be complete
    without. (`check_pin` refuses the same thing at the gate; here it is what
    makes the LISTING honest rather than what makes the pin valid.)
    """
    groups: dict[str, list[tuple[str, str]]] = {}
    for tree, rows in sorted(pin.items()):
        if not isinstance(rows, dict):
            continue
        for name, row in sorted(rows.items()):
            if not isinstance(row, dict) or row.get("verdict") != "absent":
                continue
            reason = (row.get("covered_by") or "").strip()
            groups.setdefault(reason, []).append((tree, name))
    total = sum(len(rows) for rows in groups.values())
    print(f"=== owed — {total} row(s) still absent, in {len(groups)} group(s), "
          "by the reason the pin itself gives ===")
    ordered = sorted(groups.items(), key=lambda kv: (-len(kv[1]), kv[0]))
    for reason, rows in ordered:
        shown = reason or "(NO REASON GIVEN — this row cannot be planned)"
        print(f"\n{len(rows):3d}  {shown}")
        for tree, name in rows:
            print(f"       {tree:7s} {name}")
    unclassified = len(groups.get("", []))
    if unclassified:
        print(f"\n{unclassified} absent row(s) give no reason — see `check_pin`")
    return 1 if unclassified else 0


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
        "--owed",
        action="store_true",
        help="what is still absent, grouped by the pin's own reason (tree-free)",
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
    if args.owed:
        return owed(load_pin())

    blender, unregistered = census_blender(BLENDER) if BLENDER.is_dir() else ({}, [])
    census = Census(
        blender=blender,
        unreal=census_unreal(UNREAL) if UNREAL.is_dir() else {},
        blender_unregistered=unregistered,
    )
    pin = load_pin()
    if args.emit:
        emit(census, pin)
        return 0
    return report(census, pin, args.strict)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
