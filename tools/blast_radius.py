#!/usr/bin/env python3
"""R1582 — which packages a change can break, computed rather than remembered.

## The rule this replaces a memory of

The standing local-verification rule is "test the crates this round touched"
(2026-07-21 directive; the full workspace suite and the demo sweep are CI's).
The rule is right and the reading of it is where the defect lives: **"touched"
means the crates whose BEHAVIOUR changed, consumers included**, not the files
that were edited.

R1499 wrote that down after R1497 tested `pinion-runtime` and broke
`pinion-shell`. R1511 then knew the lesson and repeated the shape: it retired
`pinion-widget-paint::button`'s focus ring, ran the three crates it had edited,
and committed with `examples/hello-dialog`'s
`r694_focused_action_button_paints_ring_others_do_not` asserting that very ring.
`clippy --all-targets` **compiles** a test and does not run it, so every gate
was green and the assertion was false.

A lesson recorded twice and re-broken twice is not a lesson, it is a
computation nobody had written. This is the computation.

## What it answers

Given a set of changed paths, the workspace packages whose behaviour can
change: the packages that OWN those paths, plus every workspace package that
depends on them, transitively. `cargo metadata --no-deps` is the source (0.2s,
measured), so the answer comes from the manifests rather than from a list
somebody maintains.

Dev-dependencies count. A change that breaks only a consumer's test harness
still breaks that consumer's tests, which is the exact failure being closed.

## What it deliberately does not do

Decide. It prints the set and the `cargo test` line that covers it; whether
that is affordable is the caller's judgment, and `.githooks/lib/consumer-tests.sh`
is where the cost bound lives — with the measurement that chose it.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path


def workspace_metadata(root: Path) -> dict:
    """`cargo metadata --no-deps`, which lists only workspace members.

    `--no-deps` is what keeps this cheap: the full graph resolves every
    registry dependency, and the question here is only about packages this
    repository builds.
    """
    out = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"],
        cwd=root,
        capture_output=True,
        text=True,
        check=True,
    )
    return json.loads(out.stdout)


def package_dirs(metadata: dict, root: Path) -> list[tuple[str, str]]:
    """`(package name, repo-relative directory)`, longest directory first.

    Longest first so a nested package wins over an ancestor: matching a path
    against a shorter prefix that happens to also match would attribute the
    change to the wrong package.
    """
    pairs = []
    for pkg in metadata["packages"]:
        manifest = Path(pkg["manifest_path"]).parent
        try:
            rel = manifest.relative_to(root)
        except ValueError:
            continue
        pairs.append((pkg["name"], str(rel)))
    return sorted(pairs, key=lambda p: len(p[1]), reverse=True)


#: Paths that belong to no member package and yet decide what EVERY member
#: compiles against (R1635).
#:
#: The workspace manifest holds `[workspace.dependencies]`, so moving a `rev`
#: there changes the source of a crate half the tree links; the lockfile is the
#: resolved answer to it; and `vendor/` holds the submodules those pins name.
#: A change to any of the three owns the whole workspace.
#:
#: Found by an SCE pin bump that this tool reported a radius of **zero** for --
#: the docstring below used to say a path under no package "cannot break a
#: cargo test", which is true of `tools/` and `docs/` and false of exactly
#: these. A gate that answers "nothing" to the change class with the widest
#: reach is worse than no gate, because the zero reads as a clean bill.
WORKSPACE_WIDE: tuple[str, ...] = ("Cargo.toml", "Cargo.lock", "vendor")


def is_workspace_wide(path: str) -> bool:
    """Whether `path` is one of the workspace-wide inputs.

    The root manifest and lockfile are matched EXACTLY -- a member's own
    `crates/x/Cargo.toml` is owned by that member and must not widen to the
    tree -- while `vendor/` matches as a directory.
    """
    return path in ("Cargo.toml", "Cargo.lock") or path == "vendor" or path.startswith("vendor/")


def owning_packages(changed: list[str], dirs: list[tuple[str, str]]) -> set[str]:
    """The packages that own `changed`.

    A path under no package — `tools/`, `docs/`, a hook — owns nothing, which
    is the right answer rather than an error: those cannot break a cargo test.
    The exception is [`WORKSPACE_WIDE`], which owns every member instead.
    """
    owners: set[str] = set()
    for path in changed:
        if not path:
            continue
        if is_workspace_wide(path):
            owners.update(name for name, _ in dirs)
            continue
        for name, directory in dirs:
            if path == directory or path.startswith(directory + "/"):
                owners.add(name)
                break
    return owners


def consumers(metadata: dict) -> dict[str, set[str]]:
    """`package -> the workspace packages that depend on it directly`.

    Dev-dependencies included, deliberately: a change that breaks only a
    consumer's test harness still breaks that consumer's tests, and that is the
    failure this exists to catch.
    """
    members = {pkg["name"] for pkg in metadata["packages"]}
    reverse: dict[str, set[str]] = {name: set() for name in members}
    for pkg in metadata["packages"]:
        for dep in pkg["dependencies"]:
            if dep["name"] in members:
                reverse[dep["name"]].add(pkg["name"])
    return reverse


def radius(changed: list[str], metadata: dict, root: Path) -> list[str]:
    """Every package whose behaviour the change can alter, sorted.

    Breadth-first over the reverse dependency relation. A workspace with a
    dependency cycle is not buildable by cargo at all, so the `seen` set is
    about determinism rather than termination.
    """
    reverse = consumers(metadata)
    frontier = list(owning_packages(changed, package_dirs(metadata, root)))
    seen = set(frontier)
    while frontier:
        current = frontier.pop()
        for consumer in reverse.get(current, ()):
            if consumer not in seen:
                seen.add(consumer)
                frontier.append(consumer)
    return sorted(seen)


def changed_paths(mode: str, rev_range: str | None, root: Path) -> list[str]:
    if mode == "staged":
        args = ["git", "diff", "--cached", "--name-only"]
    elif mode == "range":
        assert rev_range, "range mode needs a revision range"
        args = ["git", "diff", "--name-only", rev_range]
    else:
        raise SystemExit(f"unknown mode {mode!r}")
    out = subprocess.run(args, cwd=root, capture_output=True, text=True, check=True)
    return [line.strip() for line in out.stdout.splitlines() if line.strip()]


# ---------------------------------------------------------------------------
# Self-test. Runs against a SYNTHETIC metadata document rather than this
# workspace's, so the assertions state the graph they are about instead of
# inheriting whatever the tree happens to contain today — and so they keep
# meaning the same thing after a crate is added.
# ---------------------------------------------------------------------------

FIXTURE = {
    "packages": [
        {
            "name": "leaf",
            "manifest_path": "/w/crates/leaf/Cargo.toml",
            "dependencies": [],
        },
        {
            "name": "mid",
            "manifest_path": "/w/crates/mid/Cargo.toml",
            "dependencies": [{"name": "leaf"}, {"name": "serde"}],
        },
        {
            "name": "app",
            "manifest_path": "/w/examples/app/Cargo.toml",
            "dependencies": [{"name": "mid"}],
        },
        {
            "name": "app-tests-only",
            "manifest_path": "/w/examples/app-tests-only/Cargo.toml",
            # A dev-dependency, which `cargo metadata` reports in the same list.
            "dependencies": [{"name": "leaf", "kind": "dev"}],
        },
        {
            "name": "unrelated",
            "manifest_path": "/w/examples/unrelated/Cargo.toml",
            "dependencies": [],
        },
        {
            "name": "nested",
            "manifest_path": "/w/examples/app/nested/Cargo.toml",
            "dependencies": [],
        },
    ]
}


def selftest() -> int:
    root = Path("/w")
    failures: list[str] = []

    def ok(label: str, got, want) -> None:
        if got != want:
            failures.append(f"{label}: want {want!r}, got {got!r}")

    ok(
        "a leaf change reaches everything above it",
        radius(["crates/leaf/src/lib.rs"], FIXTURE, root),
        ["app", "app-tests-only", "leaf", "mid"],
    )

    # R1635 — the change class this tool answered ZERO for until an SCE pin
    # bump exposed it. The workspace manifest holds the `rev` half the tree
    # links against, so it owns every member; the lockfile is the resolved
    # answer to it; `vendor/` is the checkout those pins name.
    ok(
        "the workspace manifest owns every member",
        radius(["Cargo.toml"], FIXTURE, root),
        ["app", "app-tests-only", "leaf", "mid", "nested", "unrelated"],
    )
    ok(
        "and so does the lockfile",
        radius(["Cargo.lock"], FIXTURE, root),
        ["app", "app-tests-only", "leaf", "mid", "nested", "unrelated"],
    )
    ok(
        "and so does a vendored submodule",
        radius(["vendor/sce"], FIXTURE, root),
        ["app", "app-tests-only", "leaf", "mid", "nested", "unrelated"],
    )
    # ★ And a MEMBER's own manifest still owns only that member and its
    # consumers. Without this the widening would swallow every package
    # manifest in the tree and the radius would be "everything" forever,
    # which is the same uselessness as zero with the opposite sign.
    ok(
        "a member's own manifest does not widen to the tree",
        radius(["crates/leaf/Cargo.toml"], FIXTURE, root),
        ["app", "app-tests-only", "leaf", "mid"],
    )
    ok(
        "and a path under no package still owns nothing",
        radius(["tools/blast_radius.py", "docs/SEED_PROMPT.md"], FIXTURE, root),
        [],
    )
    ok(
        "a dev-dependency is a consumer",
        "app-tests-only" in radius(["crates/leaf/src/lib.rs"], FIXTURE, root),
        True,
    )
    ok(
        "a mid change does not reach below it",
        radius(["crates/mid/src/lib.rs"], FIXTURE, root),
        ["app", "mid"],
    )
    ok(
        "an example change reaches only itself",
        radius(["examples/app/src/main.rs"], FIXTURE, root),
        ["app"],
    )
    ok(
        "a nested package wins over its ancestor's directory",
        radius(["examples/app/nested/src/main.rs"], FIXTURE, root),
        ["nested"],
    )
    ok(
        "a path under no package owns nothing",
        radius(["tools/demos/x.py", "docs/SEED.md"], FIXTURE, root),
        [],
    )
    ok(
        "several changes union",
        radius(["crates/mid/src/lib.rs", "examples/unrelated/src/main.rs"], FIXTURE, root),
        ["app", "mid", "unrelated"],
    )
    ok("an empty change set is empty", radius([], FIXTURE, root), [])
    ok("a blank path is ignored", radius([""], FIXTURE, root), [])
    # The manifest itself is part of the package: a dependency added there
    # changes what the package builds against.
    ok(
        "a manifest change owns its package",
        radius(["crates/mid/Cargo.toml"], FIXTURE, root),
        ["app", "mid"],
    )

    for failure in failures:
        print(f"  FAIL {failure}")
    print(f"blast_radius selftest: {'FAIL' if failures else 'PASS'} "
          f"({len(failures)} failure(s))")
    return 1 if failures else 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--mode", choices=("staged", "range"), default="staged")
    parser.add_argument("--range", dest="rev_range", default=None)
    parser.add_argument("--command", action="store_true",
                        help="print the cargo test line instead of the names")
    parser.add_argument("--count", action="store_true", help="print the size only")
    parser.add_argument("--selftest", action="store_true")
    args = parser.parse_args()

    if args.selftest:
        return selftest()

    root = Path(__file__).resolve().parent.parent
    metadata = workspace_metadata(root)
    names = radius(changed_paths(args.mode, args.rev_range, root), metadata, root)

    if args.count:
        print(len(names))
    elif args.command:
        if names:
            print("cargo test " + " ".join(f"-p {n}" for n in names))
    else:
        for name in names:
            print(name)
    return 0


if __name__ == "__main__":
    sys.exit(main())
