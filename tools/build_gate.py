#!/usr/bin/env python3
"""Trustworthy build-then-run gate — the one blessed way to get a runnable pinion
example binary that is GUARANTEED to be current with its source.

Why this exists (the rig traps it closes, all hit in one R1327-R1329 session):

  1. `cargo build 2>&1 | grep -c '^error'` DISCARDS cargo's exit code, so a
     deny-warnings failure (an unused import, an unused `mut`) reads as a
     successful build — and the demo then runs the PREVIOUS binary. Every check
     here keys on the real subprocess exit status, never on grepping output.

  2. Editing a source file and running `./target/release/<x>` directly runs
     whatever was built last — `git checkout` of the source, or a `python`
     in-place edit whose `.replace()` silently did not match, both leave a stale
     binary on disk. `cargo build` is the authoritative freshness check (its
     fingerprint graph knows the crate AND its path-deps); this module always
     runs it before handing back a path, so "did my edit land" is answered by
     cargo, not by hope.

     R1510 — with one hole, MEASURED. Cargo's fingerprint keys on mtimes, so a
     restore that moves an mtime BACKWARDS defeats it: reverting a counterfactual
     with `mv file.bak file` hands back the backup's older timestamp, cargo calls
     the unit fresh, and the next `cargo test` measures the counterfactual build
     while its operator reads the result as the fix. That happened in R1510 and
     nearly went into the record as "the fix does not work". Two defences, below:
     `restore_file` keeps mtimes monotonic, and `ensure_built(expect_rebuild=)`
     stops trusting the fingerprint and ASKS cargo whether it recompiled.
     (Cargo's own content-hash freshness, `-Z checksum-freshness`, would close
     this at the source but is nightly-only; `rust-toolchain.toml` pins stable.)

  3. A "same code passes with a probe / fails without it" result — the signature
     of a stale binary OR a codegen bug — wasted hours of source bisection. With
     a build that cannot silently no-op, that whole failure class disappears: if
     it still reproduces after this gate, it is genuinely the compiler (then use
     the debug-vs-release / never-taken-branch tests, not more source edits).

Use it two ways:

  * From Python (the demo harness does this automatically):
        from build_gate import ensure_built, BuildError
        binary = ensure_built("hello-dock-panels-editor")   # raises BuildError, loud

  * From a shell, instead of a raw binary path:
        bin="$(python3 tools/build_gate.py hello-dock-panels-editor)" || exit 1
        "$bin"                                               # provably fresh
    The path goes to STDOUT only on success; cargo's diagnostics go to STDERR;
    the exit code is cargo's, so `|| exit 1` actually fires on a broken build.
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import time
from pathlib import Path

WORKSPACE_ROOT = Path(__file__).resolve().parent.parent


class BuildError(RuntimeError):
    """`cargo build` exited non-zero. Carries the tail of cargo's output so the
    caller can surface WHY (deny-warnings line, type error, …) rather than a bare
    'build failed'."""

    def __init__(self, package: str, returncode: int, output: str) -> None:
        self.package = package
        self.returncode = returncode
        self.output = output
        tail = "\n".join(output.splitlines()[-25:])
        super().__init__(
            f"cargo build -p {package} failed (rc={returncode}):\n{tail}"
        )


def _default_jobs_env() -> dict[str, str]:
    """Respect an explicit `CARGO_BUILD_JOBS`; otherwise cap at 2.

    The workspace OOMs under unbounded parallel codegen on constrained boxes
    ([[env-cargo-oom-jobs-cap]]); 2 is the value every build in this repo already
    uses. A caller that has more headroom sets `CARGO_BUILD_JOBS` explicitly and
    this leaves it alone.
    """
    env = dict(os.environ)
    env.setdefault("CARGO_BUILD_JOBS", "2")
    return env


def _binary_path(package: str, *, release: bool) -> Path:
    flavor = "release" if release else "debug"
    return WORKSPACE_ROOT / "target" / flavor / package


def restore_file(path: "str | Path", content: str) -> None:
    """Write `content` back to `path` AND bump its mtime.

    R1510 — the blessed way to revert a counterfactual injection. `mv`, `cp -p`
    and `git checkout` all restore a file with a timestamp that can be OLDER
    than the artifact built from the injected version, which makes cargo call
    that artifact fresh; the next run then measures the counterfactual. Writing
    the content through this function cannot do that, because a write always
    moves the mtime forward.

    (`git checkout` has a second, worse failure mode this avoids by construction:
    on a file carrying uncommitted work it discards ALL of it, not just the
    injection. R1510 lost a round's worth of edits to exactly that.)
    """
    target = Path(path)
    target.write_text(content)
    now = time.time()
    os.utime(target, (now, now))


def ensure_built(
    package: str,
    *,
    release: bool = True,
    quiet: bool = True,
    expect_rebuild: bool = False,
) -> Path:
    """Build `package` and return the path to its (now-current) binary.

    `cargo build` is incremental: when nothing changed this is a sub-second
    fingerprint check, so calling it on every run is cheap AND correct — the only
    way a stale binary can run is to skip the build, which is exactly the bug this
    prevents.

    Raises [`BuildError`] on any non-zero cargo exit (compile error, or a
    deny-warnings violation under the workspace lint baseline). The binary on
    disk is then whatever last succeeded, but this function refuses to point at
    it — the caller gets an exception, not a silent stale path.

    Raises `FileNotFoundError` if cargo reports success yet the expected binary
    is absent (a package that is a lib, or a renamed bin target) — a
    misconfiguration the caller should see immediately, not at launch.

    R1510 — `expect_rebuild=True` asserts that cargo actually RECOMPILED the
    package rather than reporting it fresh. Pass it whenever the source was just
    changed (a counterfactual injected or reverted): a `fresh` verdict then means
    the mtime fingerprint was defeated, and the run that follows would measure
    the wrong binary. This is the difference between hoping the artifact matches
    the source and checking it — the same move the hook libraries make for the
    Mnemosyne pin, where a build's revision is verified instead of inferred from
    the path it sits in.
    """
    cmd = ["cargo", "build", "-p", package]
    if release:
        cmd.append("--release")
    if expect_rebuild:
        # `--quiet` and `--message-format=json` do not compose usefully: the
        # verdict this needs is in the JSON stream.
        cmd.append("--message-format=json")
    elif quiet:
        cmd.append("--quiet")
    completed = subprocess.run(
        cmd,
        cwd=WORKSPACE_ROOT,
        env=_default_jobs_env(),
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        check=False,
    )
    if completed.returncode != 0:
        raise BuildError(package, completed.returncode, completed.stdout or "")
    if expect_rebuild:
        _assert_recompiled(package, completed.stdout or "")
    binary = _binary_path(package, release=release)
    if not binary.exists():
        raise FileNotFoundError(
            f"cargo build -p {package} succeeded but {binary} is missing — "
            "is this package a binary target with a bin name matching the package?"
        )
    return binary


def _assert_recompiled(package: str, cargo_json: str) -> None:
    """Raise unless cargo reported compiling (not reusing) a unit of `package`.

    R1510 — cargo emits one `compiler-artifact` message per unit with a `fresh`
    flag: `false` means it recompiled, `true` means it reused what was on disk.
    After a source change the package's units must be `false`; an all-`true`
    answer is the mtime fingerprint having been defeated, and the only honest
    thing to do is refuse rather than let the caller measure a stale binary.

    Unknown lines are ignored on purpose — cargo's stream carries diagnostics
    and build-script messages too, and a parser that died on those would turn a
    freshness check into a build breaker.
    """
    seen = False
    for line in cargo_json.splitlines():
        line = line.strip()
        if not line.startswith("{"):
            continue
        try:
            msg = json.loads(line)
        except json.JSONDecodeError:
            continue
        if msg.get("reason") != "compiler-artifact":
            continue
        name = (msg.get("target") or {}).get("name")
        pkg_id = msg.get("package_id") or ""
        if name != package and f"{package}#" not in pkg_id and package not in pkg_id:
            continue
        seen = True
        if msg.get("fresh") is False:
            return
    if not seen:
        raise BuildError(
            package,
            0,
            f"cargo reported no compiler-artifact for {package}; cannot tell "
            "whether it was rebuilt",
        )
    raise BuildError(
        package,
        0,
        f"cargo reused the existing {package} artifacts (every unit `fresh`) "
        "even though a rebuild was expected — an mtime moved backwards (a `mv` "
        "or `cp -p` restore), so the binary does NOT match the source and the "
        "next run would measure the wrong build. Touch the changed files and "
        "rebuild.",
    )


def _main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(
        prog="build_gate.py",
        description="Build a pinion example and print its fresh binary path.",
    )
    parser.add_argument("package", help="workspace package / example name")
    parser.add_argument(
        "--debug",
        action="store_true",
        help="build the debug profile (default: release, what ships)",
    )
    args = parser.parse_args(argv)
    try:
        binary = ensure_built(args.package, release=not args.debug)
    except (BuildError, FileNotFoundError) as exc:
        # Diagnostics to STDERR; STDOUT stays clean so `$(...)` capture in a shell
        # yields nothing on failure and the non-zero exit fires `|| exit 1`.
        print(str(exc), file=sys.stderr)
        return 1
    print(binary)
    return 0


if __name__ == "__main__":
    raise SystemExit(_main(sys.argv[1:]))
