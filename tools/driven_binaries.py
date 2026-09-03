#!/usr/bin/env python3
"""R1984 — an example whose own source a push edits must have been DRIVEN.

## The round that paid for this

R1981, R1982 and R1983 each edited `examples/hello-node-lab/src/lib.rs`. Each
one ran what its round record calls "the blast radius" — eight, eleven and eight
walks — and every one of them was green. All of them launched
`hello-analyzer-shell`. **None launched `hello-node-lab`**, and the CI sweep,
three commits later, failed `r1651_the_node_lab_matches_the_reference`, which
launches exactly that. One screen has two binaries: the standalone lab and the
assembled shell, sharing one `lib.rs` and painting DIFFERENT things (the lab's
own app bar is never drawn in the shell). Green on one says nothing about the
other.

`blast_radius.py` already answered "which packages", and `pre-push` already RAN
both packages' tests — they passed, because the difference is what gets painted
in a window. `demo_radius.py` already answered "which demos", and pre-push
printed only the narrow pin subset of that answer, which for this change was
**empty**. So every instrument this repository owns was working and the round
still picked its walks by eye.

⇒ what was missing is not a computation. It is a gate that knows **which
binaries the round actually drove** and compares that with the ones it had to.

## What it answers

Given a change, the example packages whose binary must have a passing local walk
behind it:

1. the example packages that OWN a changed path under `examples/`, and
2. every example package that depends on one of those, transitively,
3. keeping only the ones some demo actually launches.

Step 2 is what makes the direction right: editing the lab's library changes the
shell too, and editing the shell changes nothing of the lab's.

**Measured before it was built**: across 231 example packages the largest such
set is THREE (`hello-topology-view`, whose library the shell and the sessions
view both build on) and the median is ONE. That is the number that made this
enforceable rather than reported — R1858 chose reporting for the pin axis
because the alternative was hundreds of demos, and here the alternative is one.

Owners are taken from paths under `examples/` ONLY, deliberately: for
`blast_radius.py` a change to `Cargo.lock` owns every member, which is the right
answer to *what can break* and the wrong one here — it would demand a walk per
example on any lockfile touch.

## What the evidence is

`tools/rpc_verify.py` writes `target/demo-runs/<package>.json` when a demo
PASSES, recording the binary it launched, that binary's identity on disk, and a
fingerprint of the workspace's Rust sources at the time. The evidence is
therefore produced by the act of driving, not declared by whoever remembers to.

A record is only accepted when BOTH still hold:

- the binary is byte-for-byte the one that was driven (size + mtime), so a
  rebuild after the run invalidates it; and
- the source fingerprint matches, so an edit after the run invalidates it too.

⚠ The fingerprint is `path|size|mtime_ns` over the tracked `*.rs` / `*.toml` /
`Cargo.lock` set, not their contents — measured 8 ms for 1,458 files, against
~150 ms for a content hash of the same 41 MB, and this runs on every demo
launch. The cost of the weaker form is that a `git checkout` which restores
identical bytes invalidates the records: the error direction is one redundant
4-second walk, never a missed one.

## The override, and why it is loud

`PINION_ALLOW_UNDRIVEN=1` publishes anyway. It exists because a session with no
display cannot drive anything, and a gate with no way past it stops the line
permanently. It prints what it let through.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import blast_radius  # noqa: E402
import demo_radius  # noqa: E402

REPO = Path(__file__).resolve().parent.parent
EXAMPLES_PREFIX = "examples/"
RECORD_DIRNAME = "demo-runs"
OVERRIDE_ENV = "PINION_ALLOW_UNDRIVEN"

#: The files whose state decides what a binary would do if it were rebuilt.
#: `Cargo.lock` is in for the reason `blast_radius.WORKSPACE_WIDE` has it: a
#: resolved dependency version changes every binary without any `.rs` moving.
FINGERPRINT_GLOBS = ("*.rs", "*.toml", "Cargo.lock")


def record_dir(root: Path | None = None) -> Path:
    return (root or REPO) / "target" / RECORD_DIRNAME


def source_fingerprint(root: Path | None = None) -> str:
    """A digest of the tracked Rust source state.

    `git ls-files` rather than a walk of the tree, so an untracked scratch file
    beside a crate cannot change the answer — and so the population is the one
    every other gate here uses.
    """
    base = root or REPO
    listed = subprocess.run(
        ["git", "ls-files", "--", *FINGERPRINT_GLOBS],
        cwd=base,
        capture_output=True,
        text=True,
        check=True,
    ).stdout.splitlines()
    digest = hashlib.sha256()
    for rel in sorted(line.strip() for line in listed if line.strip()):
        try:
            stat = (base / rel).stat()
        except FileNotFoundError:
            # A staged deletion: the path is in the index and gone from the
            # worktree. Recorded as absent rather than skipped, so deleting a
            # file is a change the fingerprint sees.
            digest.update(f"{rel}|absent\n".encode())
            continue
        digest.update(f"{rel}|{stat.st_size}|{stat.st_mtime_ns}\n".encode())
    return digest.hexdigest()


def binary_identity(path: Path) -> dict | None:
    try:
        stat = path.stat()
    except FileNotFoundError:
        return None
    return {"size": stat.st_size, "mtime_ns": stat.st_mtime_ns}


def write_record(package: str, binary: Path | None, demo: str) -> Path | None:
    """Record that `demo` drove `package`'s binary and passed.

    Returns the record path, or `None` when there was no binary to record — a
    demo that fell back to `cargo run` has no artifact whose identity a gate
    could compare, and a record with no identity would be evidence of nothing.
    """
    if binary is None:
        return None
    identity = binary_identity(Path(binary))
    if identity is None:
        return None
    out = record_dir()
    out.mkdir(parents=True, exist_ok=True)
    path = out / f"{package}.json"
    payload = {
        "package": package,
        "demo": demo,
        "binary": str(Path(binary)),
        "identity": identity,
        "sources": source_fingerprint(),
    }
    tmp = path.with_suffix(".json.tmp")
    tmp.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
    tmp.replace(path)
    return path


def read_record(package: str, root: Path | None = None) -> dict | None:
    path = record_dir(root) / f"{package}.json"
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (FileNotFoundError, json.JSONDecodeError):
        return None


def example_packages(metadata: dict, root: Path) -> set[str]:
    """The workspace members whose manifest lives under `examples/`."""
    out = set()
    for name, directory in blast_radius.package_dirs(metadata, root):
        if directory == "examples" or directory.startswith(EXAMPLES_PREFIX):
            out.add(name)
    return out


def edited_examples(changed: list[str], metadata: dict, root: Path) -> set[str]:
    """The example packages that own a changed path under `examples/`.

    Restricted to that prefix on purpose — see the module docstring on why
    `blast_radius.WORKSPACE_WIDE` must not widen this.
    """
    under = [p for p in changed if p.startswith(EXAMPLES_PREFIX)]
    if not under:
        return set()
    dirs = blast_radius.package_dirs(metadata, root)
    return blast_radius.owning_packages(under, dirs) & example_packages(metadata, root)


def required_packages(changed: list[str], metadata: dict, root: Path) -> list[str]:
    """The example binaries a walk must have driven for this change."""
    seeds = edited_examples(changed, metadata, root)
    if not seeds:
        return []
    examples = example_packages(metadata, root)
    reverse = blast_radius.consumers(metadata)
    seen = set(seeds)
    frontier = list(seeds)
    while frontier:
        current = frontier.pop()
        for consumer in reverse.get(current, ()):
            if consumer in examples and consumer not in seen:
                seen.add(consumer)
                frontier.append(consumer)
    return sorted(seen & launchable(root))


def launchable(root: Path | None = None) -> set[str]:
    """The packages some demo launches, read from the demo sources.

    Asked of `demo_radius` rather than re-derived, so "what a demo launches" has
    one home: a package with no demo cannot be required to have been driven, and
    which packages those are is exactly the question that tool answers.
    """
    del root
    out: set[str] = set()
    for path in sorted(demo_radius.DEMOS.glob("*.py")):
        out |= demo_radius.launched_by(path)
    return out


def demos_launching(package: str) -> list[Path]:
    return [
        path
        for path in sorted(demo_radius.DEMOS.glob("*.py"))
        if package in demo_radius.launched_by(path)
    ]


def verdict_for(package: str, sources: str, root: Path | None = None) -> tuple[bool, str]:
    """Whether `package`'s binary has a passing walk behind it right now.

    ⚠ The binary compared is the one the RECORD NAMES, not
    `target/release/<package>`. The first draft assumed the release profile and
    it was true — measured, ZERO demos in `tools/demos/` pass
    `release=False` — which is exactly the kind of true-today assumption that
    breaks silently: the first debug-driven walk would have been judged against
    a release artifact it never launched. Asking the record removes the
    assumption instead of documenting it.
    """
    base = root or REPO
    record = read_record(package, base)
    if record is None:
        return False, (
            "no walk has driven it — build it and run one of its demos "
            "(nothing is recorded under target/demo-runs/)"
        )
    identity = binary_identity(Path(record.get("binary", "")))
    if identity is None:
        return False, (
            f"the record names {record.get('demo', '?')} and the binary it "
            f"drove ({record.get('binary', '?')}) is not there now"
        )
    if record.get("identity") != identity:
        return False, (
            f"the last passing walk ({record.get('demo', '?')}) drove a DIFFERENT "
            "build — the binary has been rebuilt since"
        )
    if record.get("sources") != sources:
        return False, (
            f"the last passing walk ({record.get('demo', '?')}) ran before the "
            "current source state"
        )
    return True, f"driven by {record.get('demo', '?')}"


def check(
    changed: list[str],
    root: Path | None = None,
    *,
    metadata: dict | None = None,
    graph_root: Path | None = None,
) -> tuple[int, list[str]]:
    """`(exit status, lines to print)`.

    `root` is where the binaries and the records live; `graph_root` is what the
    metadata's manifest paths are relative to. They are the same directory in
    every real call and DIFFERENT in the selftest, which drives this over a
    synthetic graph rooted at `/w` while the records sit in a throwaway tree.
    Passing them separately is what lets the refusal itself be performed rather
    than described — the first draft took one root, so a selftest could not
    reach a single MISS line and the gate's refusal was asserted by nobody.
    """
    base = root or REPO
    if metadata is None:
        metadata = blast_radius.workspace_metadata(base)
    needed = required_packages(changed, metadata, graph_root or base)
    if not needed:
        return 0, []
    sources = source_fingerprint(base)
    lines = []
    undriven = []
    for package in needed:
        ok, why = verdict_for(package, sources, base)
        lines.append(f"{'ok  ' if ok else 'MISS'} {package} — {why}")
        if not ok:
            undriven.append(package)
    if not undriven:
        return 0, lines
    lines.append("")
    lines.append(
        "this change edits an example's own source, so every example binary it "
        "reaches must have a passing walk behind it. Run one of these and push "
        "again:"
    )
    for package in undriven:
        demos = demos_launching(package)
        shown = demos[0].name if demos else "(no demo launches it)"
        lines.append(f"  {package}: python3 tools/demos/{shown}")
    if os.environ.get(OVERRIDE_ENV) == "1":
        lines.append("")
        lines.append(f"{OVERRIDE_ENV}=1 — published anyway, with the misses above")
        return 0, lines
    lines.append("")
    lines.append(f"override with {OVERRIDE_ENV}=1 if there is no display here")
    return 1, lines


# ---------------------------------------------------------------------------
# Self-test. The graph assertions run against a SYNTHETIC metadata document,
# the same choice `blast_radius.py` made and for the same reason: the claim is
# about the derivation, not about whatever this tree contains today.
# ---------------------------------------------------------------------------

FIXTURE = {
    "packages": [
        {
            "name": "pinion-core",
            "manifest_path": "/w/crates/pinion-core/Cargo.toml",
            "dependencies": [],
        },
        {
            "name": "lab",
            "manifest_path": "/w/examples/lab/Cargo.toml",
            "dependencies": [{"name": "pinion-core"}],
        },
        {
            "name": "shell",
            "manifest_path": "/w/examples/shell/Cargo.toml",
            "dependencies": [{"name": "lab"}],
        },
        {
            "name": "elsewhere",
            "manifest_path": "/w/examples/elsewhere/Cargo.toml",
            "dependencies": [{"name": "pinion-core"}],
        },
    ]
}


def selftest() -> int:
    root = Path("/w")
    failures: list[str] = []

    def ok(label: str, got, want) -> None:
        if got != want:
            failures.append(f"{label}: want {want!r}, got {got!r}")

    launched = {"lab", "shell", "elsewhere"}
    original = globals()["launchable"]
    globals()["launchable"] = lambda root=None: launched
    try:
        ok(
            "the edited example and the example that builds on it",
            required_packages(["examples/lab/src/lib.rs"], FIXTURE, root),
            ["lab", "shell"],
        )
        # ⚠ THE DIRECTION. Editing the consumer must NOT demand the library's
        # own binary: nothing of `lab`'s behaviour changed. R1981's failure was
        # the other way round and this is the assertion that tells them apart.
        ok(
            "editing the consumer demands only the consumer",
            required_packages(["examples/shell/src/main.rs"], FIXTURE, root),
            ["shell"],
        )
        # ★★★★★ The reason owners are read off `examples/` only. Through
        # `blast_radius.owning_packages` a lockfile edit owns EVERY member, so
        # without the prefix restriction this gate would demand a walk per
        # example on any dependency bump.
        ok(
            "a lockfile edit demands nothing",
            required_packages(["Cargo.lock"], FIXTURE, root),
            [],
        )
        ok(
            "a crate edit demands nothing (no example source moved)",
            required_packages(["crates/pinion-core/src/lib.rs"], FIXTURE, root),
            [],
        )
        ok(
            "a tools edit demands nothing",
            required_packages(["tools/demos/r1.py"], FIXTURE, root),
            [],
        )
        # A package no demo launches cannot be required to have been driven.
        globals()["launchable"] = lambda root=None: {"shell"}
        ok(
            "a package no demo launches is not required",
            required_packages(["examples/lab/src/lib.rs"], FIXTURE, root),
            ["shell"],
        )
    finally:
        globals()["launchable"] = original

    # --- the record half, against real files in a throwaway tree ------------
    import tempfile

    with tempfile.TemporaryDirectory() as tmp:
        base = Path(tmp)
        # A git repository, because `source_fingerprint` asks git what is
        # tracked and a gate that fell back to walking the tree when git said
        # no would be a different gate. Empty is the right content here: what
        # is under test is the record half, not the digest.
        subprocess.run(["git", "init", "-q", str(base)], check=True)
        (base / "target" / "release").mkdir(parents=True)
        binary = base / "target" / "release" / "lab"
        binary.write_bytes(b"x" * 8)
        identity = binary_identity(binary)
        (base / "target" / RECORD_DIRNAME).mkdir(parents=True)
        record = base / "target" / RECORD_DIRNAME / "lab.json"

        def put(**over) -> None:
            payload = {
                "package": "lab",
                "demo": "r_probe.py",
                "binary": str(binary),
                "identity": identity,
                "sources": "FINGERPRINT",
            }
            payload.update(over)
            record.write_text(json.dumps(payload), encoding="utf-8")

        put()
        ok("a matching record passes", verdict_for("lab", "FINGERPRINT", base)[0], True)
        put(sources="SOMETHING-ELSE")
        ok(
            "a record from another source state fails",
            verdict_for("lab", "FINGERPRINT", base)[0],
            False,
        )
        put(identity={"size": 9, "mtime_ns": identity["mtime_ns"]})
        ok(
            "a record about another build fails",
            verdict_for("lab", "FINGERPRINT", base)[0],
            False,
        )
        record.unlink()
        ok(
            "no record fails",
            verdict_for("lab", "FINGERPRINT", base)[0],
            False,
        )
        # And the writer's own refusal: a demo that launched through `cargo run`
        # has no artifact, so there is nothing whose identity a gate could
        # compare and a record must NOT be written.
        ok("no binary writes no record", write_record("lab", None, "r_probe.py"), None)

        # ★★★★★ THE REFUSAL ITSELF, end to end. Everything above tests a part;
        # this drives `check` over the exact path class that produced this gate
        # (`examples/<name>/src/lib.rs`) in a tree where no record exists, and
        # over one that must demand nothing. A gate whose refusal nobody
        # performs is a report with an exit status.
        globals()["launchable"] = lambda root=None: {"lab", "shell"}
        try:
            edited = check(
                ["examples/lab/src/lib.rs"], base, metadata=FIXTURE, graph_root=root
            )
            crate_only = check(
                ["crates/pinion-core/src/lib.rs"],
                base,
                metadata=FIXTURE,
                graph_root=root,
            )
            ok("an edited example with no walk behind it is refused", edited[0], 1)
            ok(
                "and the refusal names both binaries it wanted",
                sum(1 for line in edited[1] if line.startswith("MISS ")),
                2,
            )
            ok("a crate-only change demands nothing", crate_only, (0, []))
            # The override releases it, and SAYS it did — an override that
            # publishes silently is the escape hatch standing rule (6) forbids.
            os.environ[OVERRIDE_ENV] = "1"
            try:
                released = check(
                    ["examples/lab/src/lib.rs"], base, metadata=FIXTURE, graph_root=root
                )
            finally:
                del os.environ[OVERRIDE_ENV]
            ok("the override releases the refusal", released[0], 0)
            ok(
                "and says so",
                sum(1 for line in released[1] if OVERRIDE_ENV in line),
                1,
            )
        finally:
            globals()["launchable"] = original

    # --- the fingerprint, against this repository ---------------------------
    # Not a value (it changes with every edit) but the properties a digest of a
    # file set has to have.
    first = source_fingerprint()
    ok("the fingerprint is stable across two reads", source_fingerprint(), first)
    ok("the fingerprint is a sha256 digest", len(first), 64)

    for line in failures:
        print(f"driven binaries selftest: {line}", file=sys.stderr)
    # ★ R1984 — the summary is spelled `selftest: FAILED` on purpose.
    # `tools/counterfactual.py`'s `python --selftest` harness recognises a
    # failure by that substring, and a selftest whose red the counterfactual
    # driver reports as UNREADABLE cannot be used to gate anything — which is
    # the state every `<tool> selftest: <line>` in this directory is in.
    if failures:
        print(
            f"driven binaries selftest: FAILED — {len(failures)} case(s)",
            file=sys.stderr,
        )
    print(
        f"driven binaries selftest: {len(launchable())} package(s) some demo launches"
    )
    return 1 if failures else 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--mode", choices=["staged", "range"], default="staged")
    parser.add_argument("--range", dest="rev_range")
    parser.add_argument("--selftest", action="store_true")
    parser.add_argument(
        "--required",
        action="store_true",
        help="print only the packages a walk must have driven",
    )
    args = parser.parse_args()

    if args.selftest:
        return selftest()

    changed = blast_radius.changed_paths(args.mode, args.rev_range, REPO)
    if args.required:
        metadata = blast_radius.workspace_metadata(REPO)
        for name in required_packages(changed, metadata, REPO):
            print(name)
        return 0

    status, lines = check(changed, REPO)
    for line in lines:
        print(line, file=sys.stderr if status else sys.stdout)
    return status


if __name__ == "__main__":
    raise SystemExit(main())
