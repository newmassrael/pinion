#!/usr/bin/env python3
"""R1615 — how many bindings paint from state **assembled** out of more than
one external, and how many of those publish the assembly.

`debt-scene-cannot-say-which-marks-it-painted` asked for this number and said
"do not estimate". The defect it names is not about hex dumps: whenever a
widget's appearance is decided by more than one external, the *composed* fact
belongs to no single oracle, so a client asking a derived question either gets
half an answer or supplies half the question itself. The view is the only place
the composition happens, so the view is the only place that can publish it.

# What is counted, and how it can be wrong

The population is mechanical and complete: a binding composes sibling externals
through exactly one substrate (`WidgetCore::create_extra_externals` returning
`ExtraExternal::new(TAG, …)`), so every multi-external binding declares itself
in its own source. Nothing here is curated.

The *verdict* per binding is a source scan, and a source scan has a reading
range. Two things are read as "this binding reads that external":

  * `find_external_with_tag(TAG)` / `_mut` — the direct form.
  * a typed sibling reader constructed with a tag constant, e.g.
    `Brush::new(BRUSH_TAG, …)`, whose `read(scene)` does the lookup one crate
    over. Measured: `hello-hex-dump` — the binding this census exists for —
    reads its brush *only* this way, so a scan that looked for the direct form
    alone would have reported the subject of the debt as not assembling.

Both directions of error are possible, so the tool picks the one that
self-corrects. Constructing a typed reader counts as reading, even if the value
is dropped unused: an over-reported binding is one someone looks at and clears,
while an under-reported one is invisible. Same reasoning as R1610's
`WindowLevel` table.

**Every declared tag is accounted for.** A tag that is declared and never read
is reported as such, and an argument the scan cannot resolve to a constant
(a variable, a method call) is reported as UNRESOLVED rather than dropped —
R1605's rule, because a census that silently skips what it cannot read is the
one that reports a wrong number confidently.

# R1618 — WHERE the read happens is the question, not whether it happens

The first cut counted a read anywhere in the file, and that is a different
question from the one the debt asks. The debt is about a widget's
**appearance** being a function of several externals. Measured, the two come
apart in both directions:

  * `hello-row-dissect` reads its second external inside
    `apply_access_action`. An action handler runs after the picture and cannot
    decide it.
  * `hello-grouped-grid` and `hello-grouped-list` read two in `read_state` and
    then paint with `view(state.selected, frame)`, dropping the second.

What makes this decidable rather than a judgment call is a STRUCTURAL fact
about the framework, not a convention: `WidgetCore::view(state, frame)` is
handed no `&Scene` at all, so a view **cannot** read an external. Every path
from an external to the picture therefore runs through `read_state`, and a read
outside it is a read that decided something else.

So each read is bucketed by the function body it sits in, and a read whose
function is not `read_state` — nor one `read_state` names — is reported under
that function's name rather than dropped. The `assembles` verdict counts only
the picture-deciding bucket. The whole-file count is still printed beside it,
because that is the upper bound and the difference between the two is the
finding.

The enclosing function is the nearest preceding `fn` declaration. That is
text, not a parse, and it is the same trade the rest of this scan makes: it is
right for every shape in this tree (a read is always inside some `fn`, and a
closure inside a `fn` still answers with that `fn`), and one level of call
following is applied so a `read_state` that delegates to a helper is not
under-reported.

Usage:
    python3 tools/assembled_state.py            # the census
    python3 tools/assembled_state.py --verbose  # per-binding rows
    python3 tools/assembled_state.py --selftest # the scan's own tests
"""

from __future__ import annotations

import argparse
import re
import sys
from dataclasses import dataclass, field
from pathlib import Path

WORKSPACE = Path(__file__).resolve().parent.parent

#: `fn tag() -> &'static str { CONST }` — the binding's primary external.
PRIMARY_RE = re.compile(r"fn\s+tag\s*\(\s*\)\s*->\s*&'static\s+str\s*\{\s*([A-Za-z0-9_:]+)\s*\}")

#: `ExtraExternal::new(TAG, …)` — the one substrate for a sibling external.
EXTRA_RE = re.compile(r"ExtraExternal::new\(\s*([^,\s][^,]*?)\s*,")

#: The direct read.
FIND_RE = re.compile(r"find_external_with_tag(?:_mut)?\(\s*([^)\s][^),]*?)\s*[,)]")

#: A typed sibling reader whose first argument is the tag it will look up.
#: Listed rather than inferred: the shape `Type::new(TAG, …)` is far too common
#: to treat as a read, so the types that actually do a scene lookup are named,
#: and `--selftest` asserts the list is not silently short.
TYPED_READERS = ("Brush::new(",)
TYPED_RE = re.compile(
    r"(?:" + "|".join(re.escape(t) for t in TYPED_READERS) + r")\s*([^,\s][^,]*?)\s*,"
)

#: A node that publishes the named runs behind its appearance (R1615).
PUBLISH_RE = re.compile(r"\.with_marks\(|\.with_marked_grid\(|\.named\(")

#: An argument the scan resolves to a *constant*: an upper-case identifier,
#: optionally path-qualified. Anything else is UNRESOLVED, on purpose.
CONST_RE = re.compile(r"^(?:[A-Za-z_][A-Za-z0-9_]*::)*[A-Z][A-Z0-9_]*$")


#: A `fn` declaration, for bucketing a read by the body it sits in.
FN_RE = re.compile(r"\bfn\s+([A-Za-z_][A-Za-z0-9_]*)\s*[(<]")

#: The one function whose result reaches the view. `view(state, frame)` is
#: handed no `&Scene`, so this is a structural fact rather than a convention.
PICTURE_FN = "read_state"


@dataclass
class Binding:
    """One multi-external binding and what its source says about it."""

    name: str
    primary: str | None
    declared: set[str] = field(default_factory=set)
    #: Read ANYWHERE in the file — the upper bound the first cut reported.
    read: set[str] = field(default_factory=set)
    #: Read on a path that can reach the picture: `read_state`, or a function
    #: `read_state` names. This is what the debt asks about.
    read_for_picture: set[str] = field(default_factory=set)
    #: Every read outside that path, by the function it sits in. Reported
    #: rather than dropped — a read this scan cannot place is a read someone
    #: has to look at.
    read_elsewhere: dict[str, set[str]] = field(default_factory=dict)
    unresolved: set[str] = field(default_factory=set)
    publishes: bool = False

    @property
    def assembles(self) -> bool:
        """The PICTURE is a function of more than one external's answer."""
        return len(self.read_for_picture) >= 2

    @property
    def reads_two_anywhere(self) -> bool:
        """The upper bound: two externals are read somewhere in the file."""
        return len(self.read) >= 2

    @property
    def unread(self) -> set[str]:
        """Declared and never read here — the framework routes input to it, but
        this binding's view never asks it anything."""
        return self.declared - self.read



def body_of(source: str, fn_start: int) -> str:
    """The braced body of the `fn` declared at `fn_start`, by brace matching.

    A window of N characters would be wrong in both directions — it truncates a
    long function and runs past a short one into the next — and this scan's
    whole job is to say which body a call is in.
    """
    open_at = source.find("{", fn_start)
    if open_at == -1:
        return ""
    depth = 0
    for i in range(open_at, len(source)):
        ch = source[i]
        if ch == "{":
            depth += 1
        elif ch == "}":
            depth -= 1
            if depth == 0:
                return source[open_at + 1 : i]
    return source[open_at + 1 :]


def scan(source: str, name: str) -> Binding:
    """Classify one binding's source. Every tag it mentions lands in exactly one
    of `read` / `unread` / `unresolved`."""
    primary_match = PRIMARY_RE.search(source)
    primary = primary_match.group(1) if primary_match else None
    binding = Binding(name=name, primary=primary)
    if primary:
        binding.declared.add(primary)

    def take(raw: str, into: set[str]) -> None:
        raw = raw.strip()
        if CONST_RE.match(raw):
            into.add(raw)
        else:
            binding.unresolved.add(raw)

    fn_spans = [(m.start(), m.group(1)) for m in FN_RE.finditer(source)]

    def enclosing(at: int) -> str:
        """The nearest preceding `fn` name, or `<top>` when there is none."""
        name = "<top>"
        for start, fn_name in fn_spans:
            if start > at:
                break
            name = fn_name
        return name

    # Call following to a FIXPOINT, not one level. Measured: the binding this
    # census exists for reaches its second external through
    # `read_state -> read_brush -> brush()`, so a one-level scan dropped the
    # subject of the debt out of its own population.
    bodies = {name: body_of(source, start) for start, name in fn_spans}
    picture_fns = {PICTURE_FN}
    while True:
        grown = set()
        for caller in picture_fns:
            body = bodies.get(caller, "")
            for callee in bodies:
                if callee not in picture_fns and re.search(
                    rf"\b{re.escape(callee)}\s*\(", body
                ):
                    grown.add(callee)
        if not grown:
            break
        picture_fns |= grown

    for match in EXTRA_RE.finditer(source):
        take(match.group(1), binding.declared)
    for pattern in (FIND_RE, TYPED_RE):
        for match in pattern.finditer(source):
            raw = match.group(1).strip()
            take(raw, binding.read)
            if not CONST_RE.match(raw):
                continue
            where = enclosing(match.start())
            if where in picture_fns:
                binding.read_for_picture.add(raw)
            else:
                binding.read_elsewhere.setdefault(where, set()).add(raw)
    binding.publishes = PUBLISH_RE.search(source) is not None
    # A tag read but never declared here is still an external this binding
    # reads (a shared constant, a helper's tag), so it counts toward assembly.
    binding.declared |= binding.read
    return binding


def population() -> list[Binding]:
    """Every binding that declares a sibling external, in name order."""
    out: list[Binding] = []
    for main in sorted(WORKSPACE.glob("examples/*/src/main.rs")):
        source = main.read_text(encoding="utf-8")
        if "ExtraExternal::new" not in source:
            continue
        out.append(scan(source, main.parent.parent.name))
    return out


#: R1618 — bindings whose `read_state` assembles more than one external and
#: whose PICTURE is nonetheless a function of one, with the test that proves it.
#:
#: The debt this census serves allows exactly two ways to answer for an
#: assembling binding: publish it, or record WHY it does not have to — and the
#: second only with a proof, because a verdict nobody checked is the thing this
#: whole census exists to replace. Each entry names a test in that binding's own
#: source, and `--selftest` fails when the named test is not there.
#:
#: A verdict is not permanent. The proof is behavioural — hold one external
#: still, vary the other, assert the painted scene is unchanged — so a binding
#: that starts painting the second contribution fails its own proof and owes
#: the publication.
VERDICTS: dict[str, tuple[str, str]] = {
    "hello-grouped-grid": (
        "the roving cursor steers the keyboard and the accessibility tree and "
        "is never painted: view(state.selected, frame) drops it",
        "r1618_the_second_external_does_not_decide_the_picture",
    ),
    "hello-grouped-list": (
        "the roving cursor steers the keyboard and the accessibility tree and "
        "is never painted: view(state.selected, frame) drops it",
        "r1618_the_second_external_does_not_decide_the_picture",
    ),
    "hello-selection-toolbar": (
        "the assembly is SPATIAL: the list rows are decided by the list's "
        "roving focus and the toolbar items by the toolbar's, so no single "
        "painted node is a function of both",
        "r1618_each_external_decides_a_disjoint_part_of_the_picture",
    ),
    "settings-panel": (
        "the assembly is SPATIAL: each settings row is one widget painted from "
        "its own external, so the composition is a layout rather than a node "
        "whose appearance is a function of several",
        "r1618_each_external_decides_a_disjoint_part_of_the_picture",
    ),
}


def report(bindings: list[Binding], verbose: bool) -> int:
    """Print the census. Non-zero when an assembling binding neither publishes
    nor carries a proven verdict — the debt's own closing condition, as a gate
    rather than as a number somebody reads."""
    assembling = [b for b in bindings if b.assembles]
    publishing = [b for b in assembling if b.publishes]
    unresolved = [b for b in bindings if b.unresolved]
    elsewhere = [b for b in bindings if b.read_elsewhere]
    answered = {b.name for b in publishing} | set(VERDICTS)
    open_ = [b for b in assembling if b.name not in answered]

    # A verdict is worth exactly as much as its proof. A named test that does
    # not exist is a verdict nobody checked, which is the shape this census was
    # built to replace -- so it is a REFUSAL, not a note.
    missing: list[str] = []
    for name, (_, proof) in sorted(VERDICTS.items()):
        source = WORKSPACE / "examples" / name / "src" / "main.rs"
        if not source.is_file() or f"fn {proof}(" not in source.read_text(encoding="utf-8"):
            missing.append(f"{name}: {proof}")
    # ...and a verdict for a binding that no longer assembles is a rule with
    # nothing under it, which goes stale in exactly the direction nobody looks.
    stale = sorted(set(VERDICTS) - {b.name for b in assembling})

    if verbose:
        for b in sorted(assembling, key=lambda b: b.name):
            if b.publishes:
                mark = "publishes"
            elif b.name in VERDICTS:
                mark = "verdict"
            else:
                mark = "OPEN"
            print(f"  {b.name:<28} picture-reads {len(b.read_for_picture):>2}  {mark}")
        if elsewhere:
            # R1605 — every read is accounted for. A read outside the path to
            # the picture is REPORTED under the function it sits in, because
            # "this scan dropped it" and "this read cannot decide the picture"
            # are different facts.
            print("\n  reads that cannot reach the picture (by function):")
            for b in sorted(elsewhere, key=lambda b: b.name):
                for fn, tags in sorted(b.read_elsewhere.items()):
                    print(f"    {b.name:<28} {fn}: {sorted(tags)}")
        if unresolved:
            print("\n  UNRESOLVED tag arguments (counted as neither):")
            for b in unresolved:
                print(f"    {b.name:<28} {sorted(b.unresolved)}")
        print()

    two_anywhere = [b for b in bindings if b.reads_two_anywhere]
    print(f"multi-external bindings        {len(bindings)}")
    print(f"  read two ANYWHERE            {len(two_anywhere)}   <- upper bound")
    print(f"  of those, ASSEMBLE           {len(assembling)}   <- the picture")
    print(f"    of those, PUBLISH          {len(publishing)}")
    print(f"    of those, VERDICT          {len(assembling) - len(open_) - len(publishing)}")
    print(f"    of those, OPEN             {len(open_)}")
    print(f"bindings with unresolved args  {len(unresolved)}")
    if publishing:
        print("\npublishing: " + ", ".join(sorted(b.name for b in publishing)))
    for name, (reason, proof) in sorted(VERDICTS.items()):
        print(f"verdict {name}: {reason} [proof: {proof}]")
    if open_:
        print(
            "\nOPEN — these paint from more than one external and neither "
            "publish the assembly nor say why they need not:"
        )
        for b in open_:
            print(f"  {b.name}")
    if missing:
        print("\nUNPROVEN — a verdict names a test that is not there:")
        for row in missing:
            print(f"  {row}")
    if stale:
        print("\nSTALE — a verdict for a binding that no longer assembles:")
        for name in stale:
            print(f"  {name}")
    return 1 if (open_ or missing or stale) else 0


# --- the scan's own tests --------------------------------------------------


def selftest() -> int:
    """Counterfactual-shaped: each case is a source the scan must classify a
    stated way, including the ones an earlier draft got wrong."""
    failures: list[str] = []

    def check(label: str, got: object, want: object) -> None:
        if got != want:
            failures.append(f"{label}: got {got!r}, want {want!r}")

    direct = """
        fn tag() -> &'static str { GRID_TAG }
        fn create_extra_externals() -> Vec<ExtraExternal> {
            vec![ExtraExternal::new(UNDO_TAG, Box::new(x))]
        }
        fn read_state(s: &Scene) -> S {
            let a = s.find_external_with_tag(GRID_TAG);
            let b = s.find_external_with_tag(UNDO_TAG);
        }
    """
    b = scan(direct, "direct")
    check("direct primary", b.primary, "GRID_TAG")
    check("direct reads", b.read, {"GRID_TAG", "UNDO_TAG"})
    check("direct assembles", b.assembles, True)
    check("direct publishes", b.publishes, False)

    # ★ The case a direct-form-only scan gets WRONG, and it is the very
    # binding this census exists for: the sibling is read through a typed
    # reader that hides the lookup one crate over.
    # R1618 — and it reaches them the way the real binding does:
    # `read_state -> read_brush -> brush()`, TWO levels of call. A one-level
    # scan dropped the subject of this census out of its own population.
    typed = """
        fn tag() -> &'static str { GRID_TAG }
        fn brush() -> Brush { Brush::new(BRUSH_TAG, (0.0, 1.0)) }
        fn read_brush(s: &Scene) { brush().read(s); }
        fn create_extra_externals() -> Vec<ExtraExternal> {
            vec![ExtraExternal::new(BRUSH_TAG, Box::new(x))]
        }
        fn read_selection(s: &Scene) { s.find_external_with_tag(GRID_TAG); }
        fn read_state(s: &Scene) { (read_brush(s), read_selection(s)) }
    """
    b = scan(typed, "typed")
    check("typed reads", b.read, {"GRID_TAG", "BRUSH_TAG"})
    check("typed assembles", b.assembles, True)
    check("typed picture reads", b.read_for_picture, {"GRID_TAG", "BRUSH_TAG"})

    # ★ R1618 — WHERE the read happens. An external read only from an action
    # handler cannot decide the picture, because `view(state, frame)` is handed
    # no `&Scene` and `read_state` is the only path from an external to a
    # painted node. Measured: `hello-row-dissect` reads its second external in
    # `apply_access_action`, and the first cut counted it as assembling.
    offstage = """
        fn tag() -> &'static str { ROOT_TAG }
        fn create_extra_externals() -> Vec<ExtraExternal> {
            vec![ExtraExternal::new(OTHER_TAG, Box::new(x))]
        }
        fn read_state(s: &Scene) { s.find_external_with_tag(ROOT_TAG); }
        fn apply_access_action(s: &mut Scene) {
            s.find_external_with_tag_mut(OTHER_TAG);
        }
    """
    b = scan(offstage, "offstage")
    check("offstage reads two anywhere", b.reads_two_anywhere, True)
    check("offstage does not assemble", b.assembles, False)
    check("offstage picture reads", b.read_for_picture, {"ROOT_TAG"})
    check(
        "offstage read is REPORTED, not dropped",
        b.read_elsewhere,
        {"apply_access_action": {"OTHER_TAG"}},
    )

    # ...and the same read INSIDE read_state does assemble, so the case above
    # is discriminating on the location rather than on the tag.
    onstage = offstage.replace("fn apply_access_action(s: &mut Scene) {", "fn read_state2(s: &mut Scene) {").replace(
        "fn read_state(s: &Scene) { s.find_external_with_tag(ROOT_TAG); }",
        "fn read_state(s: &Scene) { s.find_external_with_tag(ROOT_TAG); read_state2(s); }",
    )
    b = scan(onstage, "onstage")
    check("onstage assembles", b.assembles, True)

    # A binding whose only read is its own primary composes nothing, however
    # many siblings the framework routes input to.
    single = """
        fn tag() -> &'static str { LIST_TAG }
        fn create_extra_externals() -> Vec<ExtraExternal> {
            vec![ExtraExternal::new(BAR_TAG, Box::new(x))]
        }
        fn read_state(s: &Scene) -> S { s.find_external_with_tag(LIST_TAG); }
    """
    b = scan(single, "single")
    check("single assembles", b.assembles, False)
    check("single unread", b.unread, {"BAR_TAG"})

    # An argument that is not a constant is REPORTED, never dropped.
    variable = """
        fn tag() -> &'static str { A_TAG }
        fn create_extra_externals() -> Vec<ExtraExternal> {
            vec![ExtraExternal::new(tag, Box::new(x))]
        }
        fn f(s: &Scene, which: &str) { s.find_external_with_tag(which); }
    """
    b = scan(variable, "variable")
    check("variable unresolved", b.unresolved, {"tag", "which"})
    check("variable reads", b.read, set())

    # Publishing is any of the three spellings, and only those.
    for spelling, want in [
        (".with_marks(set)", True),
        (".with_marked_grid(painted)", True),
        ("StyleRun::new(0, 1, s).named(CLASS)", True),
        (".with_cells(cells)", False),
    ]:
        check(f"publishes {spelling}", scan(spelling, "p").publishes, want)

    # ★ The list of typed readers is the one curated thing here, so it is the
    # one thing that can silently go short. If a second type grows a
    # `read(&Scene)` that resolves a tag, this fails until it is listed.
    readers = WORKSPACE / "crates" / "pinion-chart" / "src" / "brush.rs"
    check(
        "Brush still resolves a tag through the scene",
        "find_external_with_tag" in readers.read_text(encoding="utf-8"),
        True,
    )
    # A typed reader is a type that STORES a tag and resolves it against a
    # scene it is handed: `scene.find_external_with_tag(self.<field>)`. The
    # first draft of this check asked for the two substrings anywhere in the
    # file and answered 5, four of them rustdoc prose and a free fn taking the
    # tag as a parameter -- a census counting its own documentation.
    lookups = []
    for rs in sorted(WORKSPACE.glob("crates/*/src/**/*.rs")):
        for line in rs.read_text(encoding="utf-8").splitlines():
            stripped = line.strip()
            if stripped.startswith("//"):
                continue
            if re.search(r"find_external_with_tag(?:_mut)?\(self\.", stripped):
                lookups.append(rs.name)
    check("crate-side typed tag readers", sorted(set(lookups)), ["brush.rs"])
    check("...and each is listed here", len(set(lookups)), len(TYPED_READERS))

    for line in failures:
        print(f"  FAIL {line}")
    print(f"selftest: {'FAIL' if failures else 'PASS'} ({len(failures)} failures)")
    return 1 if failures else 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--verbose", action="store_true", help="per-binding rows")
    parser.add_argument("--selftest", action="store_true", help="test the scan")
    args = parser.parse_args()
    if args.selftest:
        return selftest()
    return report(population(), args.verbose)


if __name__ == "__main__":
    sys.exit(main())
