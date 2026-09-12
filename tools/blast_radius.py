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
import copy
import json
import subprocess
import sys
import tomllib
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


#: The lockfile, by name — the one [`WORKSPACE_WIDE`] input whose CONTENTS say
#: which members a change to it reaches. See [`lock_owners`].
LOCKFILE = "Cargo.lock"


def lock_owners(
    old: dict | None, new: dict | None, members: set[str]
) -> set[str] | None:
    """The members a change between two parsed lockfiles reaches, or `None`
    when the change cannot be read that narrowly and every member is assumed.

    ★★★★★ R2182 — **the lockfile is not opaque, and treating it as if it were
    hid a red.** R2176 added `serde_json` to one example's manifest; the lock
    changed by one line, inside that example's own `[[package]]` entry;
    [`WORKSPACE_WIDE`] made the radius all 262 members. `demo_radius.py` then
    answered 725 demos, the consumer gate said *over the local cap, CI covers
    them* and tested nothing, and the round read "everything" as "CI's" and ran
    one of the three walks that boot the example it had changed. The second of
    them was red in CI on every push after.

    Measured over the last 60 commits that touched the lockfile: 38 changed
    only member entries, reaching one or two members each, and 22 were engine
    pin bumps reaching 244-253. *The lockfile owns every member* was right 22
    times and wrong 38.

    A lock is a list of resolved `[[package]]` entries. An entry that changed
    and is a MEMBER reaches that member. An entry that changed and is not — a
    version, a source, a checksum, its own dependency list — reaches every
    member whose resolved dependency closure contains its name, in EITHER lock
    (an added crate is resolved in the new one, a removed one in the old). By
    name rather than by version, which errs towards reaching more.

    `None` for anything this cannot vouch for: a side that is absent or does
    not parse, or a change outside the package list (the format `version`, a
    `[metadata]` or `[patch]` table), because what those alter is not a
    property of any one entry.
    """
    if old is None or new is None:
        return None
    if {k: v for k, v in old.items() if k != "package"} != {
        k: v for k, v in new.items() if k != "package"
    }:
        return None

    def entries(lock: dict) -> dict[tuple, dict]:
        return {
            (entry.get("name"), entry.get("version"), entry.get("source")): entry
            for entry in lock.get("package", [])
        }

    before, after = entries(old), entries(new)
    changed = {
        key[0] for key in set(before) | set(after) if before.get(key) != after.get(key)
    }
    reached = changed & members
    external = changed - members
    if external:
        graph: dict[str, set[str]] = {}
        for lock in (old, new):
            for entry in lock.get("package", []):
                graph.setdefault(entry.get("name"), set()).update(
                    dependency.split(" ")[0]
                    for dependency in entry.get("dependencies", [])
                )
        for member in members - reached:
            seen: set[str] = set()
            stack = [member]
            while stack:
                for dependency in graph.get(stack.pop(), ()):
                    if dependency not in seen:
                        seen.add(dependency)
                        stack.append(dependency)
            if seen & external:
                reached.add(member)
    return reached


def lock_at(spec: str, root: Path) -> dict | None:
    """The lockfile `git show <spec>` names, parsed — `None` when git has no
    such blob or it does not parse."""
    done = subprocess.run(["git", "show", spec], cwd=root, capture_output=True, text=True)
    if done.returncode != 0:
        return None
    try:
        return tomllib.loads(done.stdout)
    except tomllib.TOMLDecodeError:
        return None


def lock_sides(
    mode: str, rev_range: str | None, root: Path
) -> tuple[dict | None, dict | None]:
    """The lockfile before and after the change [`changed_paths`] diffs.

    The same two trees that diff compares: `HEAD` and the index for `staged`;
    `A` and `B` for `A..B`; their merge base and `B` for `A...B`. A range of
    any other shape answers `(None, None)`, which [`lock_owners`] reads as
    every member.
    """
    if mode == "staged":
        return lock_at(f"HEAD:{LOCKFILE}", root), lock_at(f":{LOCKFILE}", root)
    if not rev_range:
        return None, None
    if "..." in rev_range:
        left, right = rev_range.split("...", 1)
        base = subprocess.run(
            ["git", "merge-base", left or "HEAD", right or "HEAD"],
            cwd=root,
            capture_output=True,
            text=True,
        )
        if base.returncode != 0:
            return None, None
        return (
            lock_at(f"{base.stdout.strip()}:{LOCKFILE}", root),
            lock_at(f"{right or 'HEAD'}:{LOCKFILE}", root),
        )
    if ".." in rev_range:
        left, right = rev_range.split("..", 1)
        return (
            lock_at(f"{left or 'HEAD'}:{LOCKFILE}", root),
            lock_at(f"{right or 'HEAD'}:{LOCKFILE}", root),
        )
    return None, None


def owning_packages(
    changed: list[str],
    dirs: list[tuple[str, str]],
    lock_reach: set[str] | None = None,
) -> set[str]:
    """The packages that own `changed`.

    A path under no package — `tools/`, `docs/`, a hook — owns nothing, which
    is the right answer rather than an error: those cannot break a cargo test.
    The exception is [`WORKSPACE_WIDE`], which owns every member instead — and
    the lockfile, when `lock_reach` says which members its change reaches, owns
    those (R2182, [`lock_owners`]).
    """
    owners: set[str] = set()
    for path in changed:
        if not path:
            continue
        if path == LOCKFILE and lock_reach is not None:
            owners.update(lock_reach)
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


def radius(
    changed: list[str],
    metadata: dict,
    root: Path,
    lock: tuple[dict | None, dict | None] | None = None,
) -> list[str]:
    """Every package whose behaviour the change can alter, sorted.

    Breadth-first over the reverse dependency relation. A workspace with a
    dependency cycle is not buildable by cargo at all, so the `seen` set is
    about determinism rather than termination.

    `lock` is the lockfile before and after, when the caller has them. Without
    it a lockfile change keeps [`WORKSPACE_WIDE`]'s answer (R2182).
    """
    reverse = consumers(metadata)
    reach = None
    if lock is not None and LOCKFILE in changed:
        members = {pkg["name"] for pkg in metadata["packages"]}
        reach = lock_owners(lock[0], lock[1], members)
    frontier = list(owning_packages(changed, package_dirs(metadata, root), reach))
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
        "and so does the lockfile, while nothing says what changed inside it",
        radius(["Cargo.lock"], FIXTURE, root),
        ["app", "app-tests-only", "leaf", "mid", "nested", "unrelated"],
    )

    # ★★★★★ R2182 — and when the lockfile's two sides ARE known, its own entries
    # say whom the change reaches. The fixture lock resolves the workspace above
    # plus two registry crates, `serde` (which `mid` resolves) and `itoa` (which
    # nothing does).
    fixture_lock = {
        "version": 4,
        "package": [
            {"name": "leaf", "version": "0.1.0"},
            {"name": "mid", "version": "0.1.0", "dependencies": ["leaf", "serde"]},
            {"name": "app", "version": "0.1.0", "dependencies": ["mid"]},
            {"name": "app-tests-only", "version": "0.1.0", "dependencies": ["leaf"]},
            {"name": "unrelated", "version": "0.1.0"},
            {"name": "nested", "version": "0.1.0"},
            {"name": "serde", "version": "1.0.0", "source": "registry+x", "checksum": "a"},
            {"name": "itoa", "version": "1.0.0", "source": "registry+x", "checksum": "b"},
        ],
    }
    everyone = ["app", "app-tests-only", "leaf", "mid", "nested", "unrelated"]

    def entry(lock: dict, name: str) -> dict:
        return next(item for item in lock["package"] if item["name"] == name)

    def edited(mutate) -> tuple[dict, dict]:
        new = copy.deepcopy(fixture_lock)
        mutate(new)
        return fixture_lock, new

    def drop_serde(lock: dict) -> None:
        lock["package"].remove(entry(lock, "serde"))
        entry(lock, "mid")["dependencies"].remove("serde")

    ok(
        "★ the R2176 shape: a member's own entry reaches that member and its "
        "consumers, not the workspace",
        radius(
            ["Cargo.lock", "crates/mid/Cargo.toml"],
            FIXTURE,
            root,
            edited(lambda lock: entry(lock, "mid")["dependencies"].append("itoa")),
        ),
        ["app", "mid"],
    )
    ok(
        "an external version reaches every member that resolves it",
        radius(
            ["Cargo.lock"],
            FIXTURE,
            root,
            edited(lambda lock: entry(lock, "serde").update(version="1.0.1", checksum="c")),
        ),
        ["app", "mid"],
    )
    ok(
        "an external nothing resolves reaches nothing",
        radius(
            ["Cargo.lock"],
            FIXTURE,
            root,
            edited(lambda lock: entry(lock, "itoa").update(checksum="z")),
        ),
        [],
    )
    ok(
        "a removed external reaches the member that resolved it, read in the old lock",
        radius(["Cargo.lock"], FIXTURE, root, edited(drop_serde)),
        ["app", "mid"],
    )
    ok(
        "⚠ a change outside the package list cannot be read narrowly",
        radius(["Cargo.lock"], FIXTURE, root, edited(lambda lock: lock.update(version=3))),
        everyone,
    )
    ok(
        "⚠ nor can a side that is absent or does not parse",
        radius(["Cargo.lock"], FIXTURE, root, (None, fixture_lock)),
        everyone,
    )
    ok(
        "an identical lock reaches nothing",
        radius(["Cargo.lock"], FIXTURE, root, (fixture_lock, copy.deepcopy(fixture_lock))),
        [],
    )
    ok(
        "a lock pair beside a change that does not touch the lockfile is ignored",
        radius(
            ["examples/app/src/main.rs"],
            FIXTURE,
            root,
            edited(lambda lock: lock.update(version=3)),
        ),
        ["app"],
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

    # ★★★★★ R2028 — THE TWO ORACLES, against the real repository.
    #
    # Every case above hands `radius` a FIXTURE workspace, which is the whole
    # design: the rule is pure and can be tested without cargo or git. But a
    # pure rule does not choose what it looks at, and until now nothing watched
    # the two functions that do — `tools/oracle_census.py` counts that class.
    # A `workspace_metadata` that named no package, or a `changed_paths` that
    # returned nothing, would make this tool answer *nothing is affected* for
    # every change while every case above stayed green.
    real = Path(__file__).resolve().parent.parent
    meta = workspace_metadata(real)
    names = {p.get("name") for p in meta.get("packages", [])}
    if "pinion-core" not in names:
        failures.append(
            "workspace_metadata does not name this workspace's own crates: "
            f"{sorted(names)[:5]}"
        )
    if not all(p.get("manifest_path") for p in meta.get("packages", [])):
        failures.append("a package with no manifest path cannot be located by `radius`")
    # ★★★★★ R2182 — the lock rule against THIS tree's lockfile, and without
    # history: a shallow checkout has HEAD and nothing before it, so the arm
    # mutates the real lock rather than replaying a commit. The fixture cases
    # above prove the discrimination; this proves the reader hands it a real
    # lock and that the rule still separates on a graph of this size.
    head_lock = lock_at(f"HEAD:{LOCKFILE}", real)
    if not head_lock or not head_lock.get("package"):
        failures.append("lock_at could not read this tree's own lockfile at HEAD")
    else:
        if None in lock_sides("staged", None, real):
            failures.append("lock_sides could not read the staged lockfile's two sides")
        member = next(
            (item["name"] for item in head_lock["package"]
             if item["name"] in names and item["name"] != "pinion-core"),
            None,
        )
        if member is None:
            failures.append("this tree's lock names no member besides pinion-core")
        else:
            mutated = copy.deepcopy(head_lock)
            entry(mutated, member).setdefault("dependencies", []).append("serde")
            ok(
                f"this tree's lock: an edit inside {member}'s own entry reaches "
                f"{member} alone",
                lock_owners(head_lock, mutated, names),
                {member},
            )
        core = entry(head_lock, "pinion-core")
        outside = next(
            (dependency.split(" ")[0] for dependency in core.get("dependencies", [])
             if dependency.split(" ")[0] not in names),
            None,
        )
        if outside is None:
            failures.append(
                "pinion-core resolves no external crate, so the external arm "
                "cannot discriminate anything"
            )
        else:
            mutated = copy.deepcopy(head_lock)
            for item in mutated["package"]:
                if item["name"] == outside:
                    item["checksum"] = "0" * 64
            reach = lock_owners(head_lock, mutated, names) or set()
            ok(
                f"this tree's lock: a change to {outside}, which pinion-core "
                "resolves, reaches it and most of the tree",
                "pinion-core" in reach and len(reach) > len(names) // 2,
                True,
            )
    # ★ `staged`, because it is the mode the hooks use and it answers whatever
    # the index holds — including nothing, which is a legitimate answer and the
    # reason this asserts the SHAPE rather than a count.
    staged = changed_paths("staged", None, real)
    if not isinstance(staged, list) or any(not isinstance(p, str) for p in staged):
        failures.append(f"changed_paths must answer a list of paths, got {staged!r}")
    if any(p.startswith("/") for p in staged):
        failures.append("changed_paths answers repository-relative paths, and did not")

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
    changed = changed_paths(args.mode, args.rev_range, root)
    lock = lock_sides(args.mode, args.rev_range, root) if LOCKFILE in changed else None
    names = radius(changed, metadata, root, lock)

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
