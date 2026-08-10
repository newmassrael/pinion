#!/usr/bin/env python3
"""pin-sync — one pinion rev, a DERIVED SCE rev, every consumer repo in step.

Why this exists. sprag's `[workspace.dependencies]` carries the procedure as a
comment:

    TO TAKE A NEW PINION: push pinion, then set `rev` on every pinion-* line
    below to the new commit AND set the SCE rev ... to pinion@<rev>'s own SCE
    rev, so the single shared SCE instance is preserved

Two properties make that a poor thing to leave to a human.

  1. **The SCE rev is derived, not chosen.** It is whatever the selected pinion
     already pins. A human re-deciding it is a human getting the chance to be
     wrong, and being wrong here does not fail loudly — it puts two SCE
     instances in one dependency graph.
  2. **The edit is ~10 near-identical lines per consumer.** That is the shape
     of edit where exactly one line gets missed, and a single missed line costs
     a second full checkout of pinion on disk plus a split dependency graph.

A second consumer repo makes both worse, which is what prompted this.

## Two invariants, two modes

**`--check-dual-pin` is about pinion alone** and needs no consumer checked out,
which is what makes it a gate that runs the same everywhere:

  * `Cargo.toml`'s SCE rev == this repo's own `Cargo.lock` == the `vendor/sce`
    submodule gitlink recorded in the index == that submodule's checked-out
    HEAD when it is initialised.

That pair-of-pairs is `Cargo.toml`'s stated DUAL-PIN DISCIPLINE, which until
R1641 existed only as a comment carrying a shell one-liner for whoever
remembered to run it. `.githooks/lib/mnemosyne-tool.sh` calls its own version of
this "the same dual discipline `vendor/sce` has, made mechanical" — naming, in
passing, that this one was not. A prose warning is not a gate (R1470, R1495).

It matters to the other mode too: everything below derives the expected SCE rev
from a pinion commit's `Cargo.lock`, so pinion's own two SCE statements
disagreeing would be propagated to every consumer and reported as "aligned".
The derivation is only as good as the pin it derives from.

**`--check` is about the consumers**, and runs the dual-pin first. The invariant
is NOT "every consumer tracks pinion HEAD". A consumer is pinned deliberately,
and sitting behind HEAD is the normal, intended state. What must hold is:

  * every consumer names the SAME pinion rev R; and
  * every consumer's SCE rev equals pinion@R's own SCE rev; and
  * every consumer's Cargo.lock agrees with its own Cargo.toml.

`--check` verifies exactly that and nothing more. Moving to a new R is a
deliberate act: pass the rev, or `--head`.

The third clause catches the case the first two cannot: a manifest edited but
never resolved. Manifests can agree with each other while the lock still holds
the previous commit, and the lock is what the build actually uses. A consumer
with no `Cargo.lock` at all is ANNOUNCED rather than passed silently — a clause
that quietly stopped being checked reads exactly like a clause that passed.

## Writing to other repositories

The rewrite modes edit **other repositories' manifests**, which this project's
`CLAUDE.md` forbids doing from a pinion session:

    from a pinion session, NEVER directly modify any repository other than
    pinion itself

That rule is about agent sessions, and this tool is a CLI a human runs. The rule
is restated here because the file lives in pinion's `tools/`, and a tool sitting
in the repo reads as sanctioned by it. It is not: an agent working in pinion may
run the read-only modes and must not run the rewrite. Nothing here can enforce
that, which is why it is written down at the top instead.

Read-only is the default in the weak sense that a bare invocation writes
nothing — a rewrite requires naming a rev or passing `--head`.

## Why this rewrites lines rather than the TOML document

Both consumer manifests are heavily commented — sprag's pinion block is a dozen
lines of rationale that no round-tripping TOML writer is worth trusting with.
So this only ever substitutes the 40-hex value inside `rev = "..."` on lines
that already name the exact git URL, and then ASSERTS that the old and new
lines are identical once every 40-hex token is removed from both. A change that
touched anything else cannot pass that assertion, so it aborts before writing.

Both URLs are substituted in ONE pass over each manifest. Two passes would make
that abort a per-URL guarantee rather than a per-file one: a manifest whose
pinion lines were already rewritten when the SCE pass aborted would be left
half-updated on disk.

The lock is read the same way, by line rather than by TOML parser. `tomllib`
would raise this file's floor to Python 3.11 while `tools/README.md` states 3.9
stdlib only, and the only datum needed out of a lock is the resolved commit on
one `source = "git+…"` line — the parser bought nothing the scan does not
already do for the manifest.
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable

PINION_URL = "https://github.com/newmassrael/pinion.git"
SCE_URL = "https://github.com/newmassrael/scxml-core-engine.git"

#: Where pinion vendors the SCE checkout the `rev` above must agree with.
SCE_SUBMODULE = "vendor/sce"

#: A full git object name. Deliberately not accepting short revs in file
#: content — the manifests and the lock both spell them in full, and matching
#: a short form would let a 7-hex substring inside a comment look like a rev.
SHA_RE = re.compile(r"\b[0-9a-f]{40}\b")

#: The value slot this tool is allowed to touch: the 40-hex inside `rev = "…"`.
REV_VALUE_RE = re.compile(r'(?<=rev = ")[0-9a-f]{40}(?=")')

#: A `Cargo.lock` git source: `git+<url>?rev=<asked>#<resolved>`. The URL is
#: captured to be compared for EQUALITY, not prefix — a prefix test lets a
#: sibling repository whose URL starts the same way answer for this one.
LOCK_SOURCE_RE = re.compile(
    r'^\s*source = "git\+(?P<url>[^"?#]+)'
    r'(?:\?(?P<query>[^"#]*))?'
    r'#(?P<resolved>[0-9a-f]{40})"\s*$',
    re.MULTILINE,
)

#: The `rev=` inside a lock source's query string, when it is a full object name.
LOCK_QUERY_REV_RE = re.compile(r"(?:^|&)rev=([0-9a-f]{40})(?:&|$)")

DEFAULT_CONSUMERS_FILE = "pin-consumers.txt"


class PinError(Exception):
    """A condition the caller must fix; reported without a traceback."""


@dataclass(frozen=True)
class RepoPins:
    """What one repository names, read from its manifest and its lock."""

    path: Path
    manifest_pinion: str | None
    manifest_sce: str | None
    lock_pinion: str | None
    lock_sce: str | None
    has_lock: bool

    @property
    def name(self) -> str:
        return self.path.name


def run_git(repo: Path, *args: str) -> str:
    """Run git in `repo` and return stdout, raising PinError on failure."""
    proc = subprocess.run(
        ["git", "-C", str(repo), *args],
        capture_output=True,
        text=True,
        check=False,
    )
    if proc.returncode != 0:
        raise PinError(
            f"git {' '.join(args)} failed in {repo}: {proc.stderr.strip()}"
        )
    return proc.stdout


def resolve_rev(repo: Path, rev: str) -> str:
    """Expand any revision spelling to a full 40-hex object name."""
    full = run_git(repo, "rev-parse", f"{rev}^{{commit}}").strip()
    if not SHA_RE.fullmatch(full):
        raise PinError(f"{rev!r} did not resolve to a commit in {repo}")
    return full


def lock_source_revs(text: str, url: str) -> set[str]:
    """Every commit a Cargo.lock resolved for the git source `url`.

    A set, not a first match: one lock CAN carry two sources for the same URL
    at different revs, and that is precisely the drift worth reporting. The
    pre-R1641 reader returned whichever came first, which made the LOCK — the
    thing this tool calls the authority, because it is what the build uses —
    the one place drift could hide.

    The commit after `#` is what cargo resolved; the `rev=` in the query is what
    the manifest asked for. They can only differ when the manifest named a
    non-immutable spelling, which this tool refuses to write, so a disagreement
    here is a state nobody intended and is raised rather than picked between.
    """
    revs: set[str] = set()
    for match in LOCK_SOURCE_RE.finditer(text):
        if match.group("url") != url:
            continue
        resolved = match.group("resolved")
        asked = LOCK_QUERY_REV_RE.search(match.group("query") or "")
        if asked and asked.group(1) != resolved:
            raise PinError(
                f"lock source for {url} asks for {asked.group(1)[:7]} but "
                f"resolved {resolved[:7]}"
            )
        revs.add(resolved)
    return revs


def lock_rev(text: str, url: str, *, where: str) -> str | None:
    """The single commit a lock resolved for `url`, or None if it names none."""
    revs = lock_source_revs(text, url)
    if not revs:
        return None
    if len(revs) > 1:
        raise PinError(
            f"{where}: lock resolves {len(revs)} different revs for {url}: "
            + ", ".join(sorted(r[:7] for r in revs))
        )
    return revs.pop()


def manifest_rev(manifest_text: str, url: str) -> str | None:
    """The single rev every `url` dependency line names, or None if absent.

    Raises when the lines disagree with each other: that is drift INSIDE one
    manifest, and silently picking one of them would hide it.
    """
    found: set[str] = set()
    for line in manifest_text.splitlines():
        if f'git = "{url}"' not in line:
            continue
        match = REV_VALUE_RE.search(line)
        if match:
            found.add(match.group(0))
    if not found:
        return None
    if len(found) > 1:
        raise PinError(
            f"manifest names {len(found)} different revs for {url}: "
            + ", ".join(sorted(r[:7] for r in found))
        )
    return found.pop()


def index_gitlink(repo: Path, submodule: str) -> str | None:
    """The commit the superproject's index records for `submodule`.

    This — not the submodule's checked-out HEAD — is what a commit publishes
    and what a fresh clone will get, and it is readable whether or not the
    submodule was ever initialised here.
    """
    for row in run_git(repo, "ls-files", "--stage", "--", submodule).splitlines():
        meta, _, _path = row.partition("\t")
        parts = meta.split()
        if len(parts) >= 2 and parts[0] == "160000" and SHA_RE.fullmatch(parts[1]):
            return parts[1]
    return None


def submodule_head(repo: Path, submodule: str) -> str | None:
    """The submodule's checked-out HEAD, or None when it is not initialised."""
    if not (repo / submodule / ".git").exists():
        return None
    return resolve_rev(repo / submodule, "HEAD")


def check_dual_pin(pinion_root: Path) -> list[str]:
    """Verify pinion's own SCE pin agrees with itself. Returns problem lines.

    Four statements of one fact, and every pair that can be compared is:
    manifest, this repo's lock, the index gitlink, and the checked-out
    submodule. The last is skipped — loudly — when the submodule was never
    initialised, because an absent checkout is absence, not drift.
    """
    problems: list[str] = []

    manifest_path = pinion_root / "Cargo.toml"
    if not manifest_path.is_file():
        raise PinError(f"no Cargo.toml in {pinion_root}")
    manifest = manifest_rev(manifest_path.read_text(encoding="utf-8"), SCE_URL)
    if manifest is None:
        raise PinError(
            f"{manifest_path} names no {SCE_URL} dependency; the dual pin has "
            "no manifest side to check"
        )

    lock_path = pinion_root / "Cargo.lock"
    if lock_path.is_file():
        resolved = lock_rev(
            lock_path.read_text(encoding="utf-8"), SCE_URL, where=str(lock_path)
        )
        if resolved is None:
            problems.append(
                f"{lock_path.name} resolves no SCE source, but Cargo.toml pins "
                f"{manifest[:7]}"
            )
        elif resolved != manifest:
            problems.append(
                f"Cargo.lock has SCE {resolved[:7]} but Cargo.toml says "
                f"{manifest[:7]} (run cargo fetch)"
            )
    else:
        print(
            f"pin-sync: {pinion_root} has no Cargo.lock; the lock side of the "
            "dual pin is NOT verified",
            file=sys.stderr,
        )

    gitlink = index_gitlink(pinion_root, SCE_SUBMODULE)
    if gitlink is None:
        problems.append(
            f"{SCE_SUBMODULE} is not a submodule gitlink in the index; the "
            "vendored side of the dual pin cannot be read"
        )
    elif gitlink != manifest:
        problems.append(
            f"{SCE_SUBMODULE} gitlink is {gitlink[:7]} but Cargo.toml pins "
            f"{manifest[:7]} — bump both in the same commit"
        )

    head = submodule_head(pinion_root, SCE_SUBMODULE)
    if head is None:
        print(
            f"pin-sync: {SCE_SUBMODULE} is not checked out here; comparing the "
            "index gitlink only",
            file=sys.stderr,
        )
    elif gitlink is not None and head != gitlink:
        problems.append(
            f"{SCE_SUBMODULE} is checked out at {head[:7]} but the index "
            f"records {gitlink[:7]} (run git submodule update)"
        )

    return problems


def report_dual_pin(pinion_root: Path) -> int:
    problems = check_dual_pin(pinion_root)
    if problems:
        print("pinion dual-pin drift:", file=sys.stderr)
        for problem in problems:
            print(f"  - {problem}", file=sys.stderr)
        return 1
    manifest = manifest_rev(
        (pinion_root / "Cargo.toml").read_text(encoding="utf-8"), SCE_URL
    )
    assert manifest is not None  # check_dual_pin raises when it is not
    print(f"dual pin: SCE@{manifest[:7]} (manifest = lock = {SCE_SUBMODULE})")
    return 0


def read_repo(path: Path) -> RepoPins:
    """Read one consumer's pins.

    A path that is not checked out here is NOT drift — it is absence, and a
    gate that refuses a push because a sibling repo is missing on this machine
    would be reporting the wrong thing. Callers filter those out via
    [`present_consumers`]; reaching this function means the repo is expected
    to be readable, so a missing manifest at this point is a real error.
    """
    manifest_path = path / "Cargo.toml"
    lock_path = path / "Cargo.lock"
    if not manifest_path.is_file():
        raise PinError(f"no Cargo.toml in {path}")

    manifest_text = manifest_path.read_text(encoding="utf-8")
    has_lock = lock_path.is_file()
    lock_text = lock_path.read_text(encoding="utf-8") if has_lock else ""

    return RepoPins(
        path=path,
        manifest_pinion=manifest_rev(manifest_text, PINION_URL),
        manifest_sce=manifest_rev(manifest_text, SCE_URL),
        lock_pinion=lock_rev(lock_text, PINION_URL, where=str(lock_path))
        if has_lock
        else None,
        lock_sce=lock_rev(lock_text, SCE_URL, where=str(lock_path))
        if has_lock
        else None,
        has_lock=has_lock,
    )


def only_shas_differ(before: str, after: str) -> bool:
    """True when `before` and `after` are identical once 40-hex tokens are gone.

    The whole safety argument for rewriting lines instead of the TOML document
    rests on this predicate, so it is a named function rather than an inline
    comparison: a property nothing can call is a property nothing can test.
    """
    return SHA_RE.sub("", before) == SHA_RE.sub("", after)


def rewrite_manifest(path: Path, revs: dict[str, str], *, dry_run: bool) -> int:
    """Set every listed URL's `rev` in one pass. Returns lines changed.

    Only the 40-hex inside `rev = "…"` is substituted, and each rewritten line
    must be identical to the original once all 40-hex tokens are stripped from
    both. Anything else aborts before the file is touched — and because every
    URL is handled in this single pass, that abort leaves the file untouched
    rather than carrying some other URL's completed rewrite.
    """
    manifest_path = path / "Cargo.toml"
    original = manifest_path.read_text(encoding="utf-8")
    out_lines: list[str] = []
    changed = 0

    for lineno, line in enumerate(original.splitlines(keepends=True), start=1):
        new_line = line
        for url, new_rev in revs.items():
            if f'git = "{url}"' in new_line:
                new_line = REV_VALUE_RE.sub(new_rev, new_line)
        if new_line != line:
            if not only_shas_differ(line, new_line):
                raise PinError(
                    f"{manifest_path}:{lineno} would change more than the rev; "
                    "refusing to write"
                )
            changed += 1
        out_lines.append(new_line)

    if changed and not dry_run:
        manifest_path.write_text("".join(out_lines), encoding="utf-8")
    return changed


def dedupe(paths: Iterable[Path]) -> list[Path]:
    """Order-preserving unique. A repo listed twice is read and rewritten twice."""
    seen: set[Path] = set()
    unique: list[Path] = []
    for path in paths:
        if path not in seen:
            seen.add(path)
            unique.append(path)
    return unique


def load_consumers(tools_dir: Path, explicit: list[str]) -> list[Path]:
    if explicit:
        paths = [Path(p).expanduser().resolve() for p in explicit]
    else:
        listing = tools_dir / DEFAULT_CONSUMERS_FILE
        if not listing.is_file():
            raise PinError(
                f"no consumer list at {listing} and no --repo given. "
                "Add one path per line, or pass --repo."
            )
        paths = [
            Path(raw.split("#", 1)[0].strip()).expanduser().resolve()
            for raw in listing.read_text(encoding="utf-8").splitlines()
            if raw.split("#", 1)[0].strip()
        ]
    if not paths:
        raise PinError("consumer list is empty")
    return dedupe(paths)


def present_consumers(paths: list[Path]) -> list[Path]:
    """Drop consumers that are not checked out on this machine.

    The list is shared across machines but a clone is not. A repo that is
    absent cannot have drifted, and failing a gate over it would report the
    wrong thing entirely. Absences are announced rather than swallowed —
    silence would make a check that verified nothing look like a check that
    passed.
    """
    present = [path for path in paths if (path / "Cargo.toml").is_file()]
    for path in paths:
        if path not in present:
            print(f"pin-sync: skipping {path} (not checked out here)", file=sys.stderr)
    if not present:
        raise PinError(
            "none of the listed consumers are checked out here (--check-dual-pin "
            "verifies pinion's own pin without them)"
        )
    return present


def expected_sce_for(pinion_root: Path, target: str) -> str:
    """The SCE rev pinion@`target` itself resolved."""
    try:
        lock_text = run_git(pinion_root, "show", f"{target}:Cargo.lock")
    except PinError as exc:
        raise PinError(
            f"cannot read pinion@{target[:7]}'s Cargo.lock — is that commit "
            f"fetched in {pinion_root}? ({exc})"
        ) from exc
    resolved = lock_rev(lock_text, SCE_URL, where=f"pinion@{target[:7]}:Cargo.lock")
    if resolved is None:
        raise PinError(f"pinion@{target[:7]} does not pin SCE; cannot derive")
    return resolved


def report_check(pinion_root: Path, consumers: list[RepoPins]) -> int:
    """Verify the three-clause invariant. Returns a process exit code."""
    problems: list[str] = []

    named = {c.manifest_pinion for c in consumers if c.manifest_pinion}
    if not named:
        raise PinError("no consumer names a pinion dependency")
    if len(named) > 1:
        # Returning here rather than carrying on: every clause below is stated
        # relative to ONE pinion rev, and with the consumers disagreeing there
        # is no non-arbitrary choice of which. The pre-R1641 code picked the
        # lexicographically smallest sha and then attributed SCE mismatches to
        # consumers that were correct relative to their own pin.
        print("pin drift:", file=sys.stderr)
        print(
            "  - consumers disagree on the pinion rev: "
            + ", ".join(
                f"{c.name}@{c.manifest_pinion[:7]}"
                for c in consumers
                if c.manifest_pinion
            ),
            file=sys.stderr,
        )
        print(
            "    (the SCE and lock clauses are stated against one pinion rev "
            "and were not evaluated)",
            file=sys.stderr,
        )
        return 1
    target = named.pop()

    expected_sce = expected_sce_for(pinion_root, target)

    for consumer in consumers:
        if consumer.manifest_sce and consumer.manifest_sce != expected_sce:
            problems.append(
                f"{consumer.name}: SCE rev {consumer.manifest_sce[:7]} != "
                f"pinion@{target[:7]}'s {expected_sce[:7]}"
            )
        if not consumer.has_lock:
            problems.append(
                f"{consumer.name}: no Cargo.lock, so the manifest was not "
                "checked against what the build would resolve"
            )
            continue
        for label, in_manifest, in_lock in (
            ("pinion", consumer.manifest_pinion, consumer.lock_pinion),
            ("SCE", consumer.manifest_sce, consumer.lock_sce),
        ):
            if in_manifest and in_lock is None:
                problems.append(
                    f"{consumer.name}: Cargo.toml pins {label} "
                    f"{in_manifest[:7]} but Cargo.lock resolves no such source "
                    "(run cargo fetch)"
                )
            elif in_manifest and in_lock and in_manifest != in_lock:
                problems.append(
                    f"{consumer.name}: Cargo.lock has {label} "
                    f"{in_lock[:7]} but Cargo.toml says {in_manifest[:7]} "
                    "(run cargo fetch)"
                )

    if problems:
        print("pin drift:", file=sys.stderr)
        for problem in problems:
            print(f"  - {problem}", file=sys.stderr)
        return 1

    print(f"aligned: pinion@{target[:7]}  SCE@{expected_sce[:7]}")
    for consumer in consumers:
        print(f"  {consumer.name}")
    return 0


def apply_sync(
    pinion_root: Path, consumers: list[Path], target: str, *, dry_run: bool
) -> int:
    expected_sce = expected_sce_for(pinion_root, target)

    verb = "would set" if dry_run else "set"
    print(f"pinion@{target[:7]}  ->  SCE@{expected_sce[:7]} (derived)")

    total = 0
    for path in consumers:
        if path.resolve() == pinion_root.resolve():
            raise PinError(
                "pinion is the SCE reference, not a consumer; remove it from "
                "the consumer list"
            )
        changed = rewrite_manifest(
            path, {PINION_URL: target, SCE_URL: expected_sce}, dry_run=dry_run
        )
        total += changed
        status = "up to date" if not changed else f"{verb} {changed} line(s)"
        print(f"  {path.name}: {status}")

    if total and not dry_run:
        print("\nCargo.lock is now stale in the repos above. Run `cargo fetch` "
              "in each, then re-run with --check.")
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        prog="pin-sync",
        description="Keep every pinion consumer on one rev, with the SCE rev "
        "derived from that pinion rather than chosen again.",
    )
    parser.add_argument(
        "rev",
        nargs="?",
        help="pinion revision to adopt (any spelling git understands)",
    )
    parser.add_argument(
        "--head",
        action="store_true",
        help="adopt the pinion working tree's current HEAD",
    )
    parser.add_argument(
        "--check",
        action="store_true",
        help="verify the dual pin, then that consumers agree with each other "
        "and with their locks; does not compare against HEAD, so it is safe "
        "as a pre-push gate",
    )
    parser.add_argument(
        "--check-dual-pin",
        action="store_true",
        help="verify pinion's own SCE pin only (manifest = lock = vendor/sce); "
        "needs no consumer checked out",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="report what would change without writing",
    )
    parser.add_argument(
        "--repo",
        action="append",
        default=[],
        metavar="PATH",
        help="a consumer repo (repeatable); overrides the consumer list file",
    )
    parser.add_argument(
        "--pinion",
        metavar="PATH",
        help="path to the pinion repo (default: this script's grandparent)",
    )
    args = parser.parse_args(argv)

    tools_dir = Path(__file__).resolve().parent
    pinion_root = (
        Path(args.pinion).expanduser().resolve() if args.pinion else tools_dir.parent
    )

    try:
        checking = args.check or args.check_dual_pin
        if checking and (args.rev or args.head):
            raise PinError("--check verifies the existing pins; it takes no rev")
        if checking and args.dry_run:
            # Accepting it would let a caller believe the write path had been
            # rehearsed when no write path ran at all.
            raise PinError("--dry-run describes a rewrite; --check performs none")
        if args.check and args.check_dual_pin:
            raise PinError("--check already runs the dual-pin check")
        if args.rev and args.head:
            raise PinError("pass a rev or --head, not both")

        if args.check_dual_pin:
            return report_dual_pin(pinion_root)

        if args.check:
            if report_dual_pin(pinion_root) != 0:
                # The consumer clauses derive the expected SCE rev from
                # pinion's own lock. Reporting them against a pin that
                # disagrees with itself would state a conclusion the
                # derivation cannot support.
                print(
                    "pin-sync: consumer clauses not evaluated — pinion's own "
                    "pin is what they derive from",
                    file=sys.stderr,
                )
                return 1
            consumer_paths = present_consumers(load_consumers(tools_dir, args.repo))
            return report_check(
                pinion_root, [read_repo(p) for p in consumer_paths]
            )

        if not (args.rev or args.head):
            raise PinError(
                "nothing to do: pass a rev, --head, --check, or --check-dual-pin"
            )

        consumer_paths = present_consumers(load_consumers(tools_dir, args.repo))
        target = resolve_rev(pinion_root, "HEAD" if args.head else args.rev)
        return apply_sync(
            pinion_root, consumer_paths, target, dry_run=args.dry_run
        )
    except PinError as exc:
        print(f"pin-sync: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
