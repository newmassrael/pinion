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


@dataclass
class Binding:
    """One multi-external binding and what its source says about it."""

    name: str
    primary: str | None
    declared: set[str] = field(default_factory=set)
    read: set[str] = field(default_factory=set)
    unresolved: set[str] = field(default_factory=set)
    publishes: bool = False

    @property
    def assembles(self) -> bool:
        """Paints from more than one external's answer."""
        return len(self.read) >= 2

    @property
    def unread(self) -> set[str]:
        """Declared and never read here — the framework routes input to it, but
        this binding's view never asks it anything."""
        return self.declared - self.read


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

    for match in EXTRA_RE.finditer(source):
        take(match.group(1), binding.declared)
    for match in FIND_RE.finditer(source):
        take(match.group(1), binding.read)
    for match in TYPED_RE.finditer(source):
        take(match.group(1), binding.read)
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


def report(bindings: list[Binding], verbose: bool) -> None:
    assembling = [b for b in bindings if b.assembles]
    publishing = [b for b in assembling if b.publishes]
    unresolved = [b for b in bindings if b.unresolved]

    if verbose:
        for b in sorted(assembling, key=lambda b: b.name):
            mark = "publishes" if b.publishes else "-"
            print(f"  {b.name:<28} reads {len(b.read):>2}  {mark}")
        if unresolved:
            print("\n  UNRESOLVED tag arguments (counted as neither):")
            for b in unresolved:
                print(f"    {b.name:<28} {sorted(b.unresolved)}")
        print()

    print(f"multi-external bindings        {len(bindings)}")
    print(f"  of those, ASSEMBLE           {len(assembling)}")
    print(f"    of those, PUBLISH          {len(publishing)}")
    print(f"  read exactly one external    {len(bindings) - len(assembling)}")
    print(f"bindings with unresolved args  {len(unresolved)}")
    if publishing:
        print("\npublishing: " + ", ".join(sorted(b.name for b in publishing)))


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
    typed = """
        fn tag() -> &'static str { GRID_TAG }
        fn brush() -> Brush { Brush::new(BRUSH_TAG, (0.0, 1.0)) }
        fn create_extra_externals() -> Vec<ExtraExternal> {
            vec![ExtraExternal::new(BRUSH_TAG, Box::new(x))]
        }
        fn read_selection(s: &Scene) { s.find_external_with_tag(GRID_TAG); }
    """
    b = scan(typed, "typed")
    check("typed reads", b.read, {"GRID_TAG", "BRUSH_TAG"})
    check("typed assembles", b.assembles, True)

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
    report(population(), args.verbose)
    return 0


if __name__ == "__main__":
    sys.exit(main())
