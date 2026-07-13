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
import os
import subprocess
import sys
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


def ensure_built(package: str, *, release: bool = True, quiet: bool = True) -> Path:
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
    """
    cmd = ["cargo", "build", "-p", package]
    if release:
        cmd.append("--release")
    if quiet:
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
    binary = _binary_path(package, release=release)
    if not binary.exists():
        raise FileNotFoundError(
            f"cargo build -p {package} succeeded but {binary} is missing — "
            "is this package a binary target with a bin name matching the package?"
        )
    return binary


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
