#!/usr/bin/env python3
"""measure-ink-budgets — regenerate the two ink ratchet files, reproducibly.

## Why this exists

`docs/containment-budget.tsv` (R1656) and `docs/text-smear-budget.tsv` (R1654)
are ratchets: a per-example count of painted marks that left their box, and of
text runs painted on top of each other. Every demo pays them at boot.

**They had no producer.** R1656 measured 223 surfaces by hand, wrote the
non-zero rows, and the numbers were then a fact about one machine on one
afternoon. Two things followed, and both cost a round:

  * The numbers were **host-dependent** — ink is a function of the shaped face
    and the face of the host's font database — so CI disagreed and 33 sweep
    runs went red on 8 examples that pass locally. R1660 fixed the *measurement*
    by pinning the font database (`rpc_verify.pinned_fontconfig`), and left the
    *files* holding pre-pin numbers.
  * A surface that could not be measured left **no row**, and no row reads as
    `0` — the strictest budget there is, chosen by nobody. `hello-audio-device`
    cannot boot without `snd-dummy`, so it was silently budgeted at 0 and CI,
    which does load that card, measures 1.

A ratchet whose numbers cannot be re-derived is a ratchet nobody can re-judge.
This is that derivation, run under the pin, writing every surface — including
the ones it could not measure, and why.

## The three states, and why the third one has to exist

    <example>\t<count>                  measured, and this is the number
    <example>\tunmeasured\t<reason>     could not be measured here, stated
    (no row)                            AN ERROR — the census is total

`unmeasured` passes the gate and REPORTS. It is not an exemption: the reason
travels with it, so a surface that becomes measurable (a CI runner that loads
the card this host lacks) is visibly owed a number rather than quietly clean.

Usage:

    python3 tools/measure_ink_budgets.py            # measure every example
    python3 tools/measure_ink_budgets.py --only X   # one, for iterating
    python3 tools/measure_ink_budgets.py --add X    # register a NEW surface
    python3 tools/measure_ink_budgets.py --dry-run  # print, write nothing

## Why `--add` exists, and why it cannot re-measure

A `--only` run writes nothing, because a producer that writes a partial file is
how a census loses rows. But the census is `total`, so a round that ADDS an
example must be able to register it — and re-measuring 223 unrelated surfaces to
do that is a full local sweep, which this project does not run locally.

`--add` measures only the named examples and MERGES them into the files. It
refuses an example that already has a row in any of the three, so it can never
overwrite a measured number: registering a new surface and re-measuring an old
one are different operations, and only the second one needs the whole census.
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import rpc_verify  # noqa: E402
from rpc_verify import WORKSPACE_ROOT, RpcSubprocess  # noqa: E402

CONTAINMENT = WORKSPACE_ROOT / "docs" / "containment-budget.tsv"
SMEAR = WORKSPACE_ROOT / "docs" / "text-smear-budget.tsv"
REACH = WORKSPACE_ROOT / "docs" / "scroll-reach-budget.tsv"

#: Value in the count column for a surface this host could not measure.
UNMEASURED = "unmeasured"


def examples() -> list[str]:
    """Every example package, from the tree rather than from a list.

    Derived, because a hand-written population is how a census reports a sample
    as coverage — the shape this project has paid for repeatedly.
    """
    root = WORKSPACE_ROOT / "examples"
    return sorted(p.name for p in root.iterdir() if (p / "Cargo.toml").is_file())


class BuildFailed(RuntimeError):
    """The example did not compile, so nothing about it was measured.

    ★ R1662 — its own type because the catch-all below turned a build failure
    into `unmeasured`, which is a LEGAL budget value. Measured while it was
    happening: a concurrent `cargo` run took the target-directory lock, 24
    consecutive examples failed to build, and each was about to be written into
    the ratchet as "this host cannot measure it" with a reason nobody would
    read again. A tool that degrades an environment problem into a permanent
    exemption is worse than one that stops.
    """


def measure(example: str) -> tuple[object, object, object, str]:
    """`(containment, smear, lost, reason)` for one example.

    Any count is [`UNMEASURED`] when the surface could not be booted or the
    binary is too old to answer; `reason` says which, and is empty on success.

    # Raises

    [`BuildFailed`] when the example did not compile — never recorded as a
    measurement, because it is not one.
    """
    try:
        with RpcSubprocess(example, measuring=True) as app:
            try:
                out = app.request("scene/containment").result
                escapes = len(out.get("escapes", []))
            except Exception as exc:  # noqa: BLE001 — the reason is the payload
                return UNMEASURED, UNMEASURED, UNMEASURED, f"scene/containment: {exc}"[:120]
            try:
                runs = app.request("scene/text_painted").result.get("runs", [])
            except Exception as exc:  # noqa: BLE001
                return escapes, UNMEASURED, UNMEASURED, f"scene/text_painted: {exc}"[:120]
            try:
                reach = app.request("scene/scroll_reach").result
                lost = sum(
                    1 for o in reach.get("out_of_sight", []) if o.get("reach") == "lost"
                )
            except Exception as exc:  # noqa: BLE001
                return escapes, _smear_pairs(runs), UNMEASURED, f"scene/scroll_reach: {exc}"[:120]
            return escapes, _smear_pairs(runs), lost, ""
    except AssertionError as exc:
        # A boot gate other than these refused (pointer-reach, for instance).
        # The surface booted, so this is a real defect on it and NOT an
        # unmeasured one — but this tool cannot read past the refusal.
        return UNMEASURED, UNMEASURED, UNMEASURED, f"a boot gate refused: {exc}"[:120]
    except Exception as exc:  # noqa: BLE001
        if "failed before launch" in str(exc):
            raise BuildFailed(f"{example}: {exc}"[:200]) from exc
        return UNMEASURED, UNMEASURED, UNMEASURED, f"{type(exc).__name__}: {exc}"[:120]


def _smear_pairs(runs: list[dict]) -> int:
    """Overlapping run pairs, grouped by owner — the shape the gate counts."""
    by_owner: dict[object, list[dict]] = {}
    for r in runs:
        by_owner.setdefault(r.get("owner"), []).append(r)
    pairs = 0
    for group in by_owner.values():
        for i, a in enumerate(group):
            for b in group[i + 1 :]:
                if _overlaps(a, b):
                    pairs += 1
    return pairs


def _overlaps(a: dict, b: dict) -> bool:
    ax, ay, aw, ah = a.get("x", 0), a.get("y", 0), a.get("w", 0), a.get("h", 0)
    bx, by, bw, bh = b.get("x", 0), b.get("y", 0), b.get("w", 0), b.get("h", 0)
    return ax < bx + bw and bx < ax + aw and ay < by + bh and by < ay + ah


HEADER = """\
# {title}
#
# census: total
#
# GENERATED by `python3 tools/measure_ink_budgets.py` under the R1660 font pin
# (`rpc_verify.PINNED_FACES`). Do not hand-edit a number: a count with no
# reproducible measurement behind it is the one use that turns this ratchet back
# into a suggestion. Re-run the tool instead.
#
# `<example>\\t<count>` measured; `<example>\\t{unmeasured}\\t<reason>` could not be
# measured on the machine that ran the tool, with the reason travelling so a
# host that CAN measure it is visibly owed a number. A missing row is an error:
# the census is total.
"""


def existing(path: Path) -> "list[tuple[str, object, str]]":
    """The rows a budget file already holds, in file order.

    Parsed rather than pattern-matched so `--add` merges into exactly what the
    last full run wrote, including its `unmeasured` rows and their reasons.
    """
    rows: list[tuple[str, object, str]] = []
    if not path.exists():
        return rows
    for line in path.read_text(encoding="utf-8").splitlines():
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        parts = line.split("\t")
        if len(parts) >= 3 and parts[1] == UNMEASURED:
            rows.append((parts[0], UNMEASURED, parts[2]))
        elif len(parts) >= 2:
            rows.append((parts[0], int(parts[1]), ""))
    return rows


def merged(
    path: Path, added: "list[tuple[str, object, str]]"
) -> "list[tuple[str, object, str]]":
    """`path`'s rows plus `added`, sorted the way a full run writes them."""
    return sorted(existing(path) + added, key=lambda row: row[0])


def write(path: Path, title: str, rows: list[tuple[str, object, str]]) -> None:
    lines = [HEADER.format(title=title, unmeasured=UNMEASURED)]
    for example, count, reason in rows:
        if count == UNMEASURED:
            lines.append(f"{example}\t{UNMEASURED}\t{reason}")
        else:
            lines.append(f"{example}\t{count}")
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--only", action="append", help="measure just this example")
    ap.add_argument(
        "--add",
        action="append",
        help="measure a NEW example and merge it in; refuses one already present",
    )
    ap.add_argument("--dry-run", action="store_true", help="print, write nothing")
    args = ap.parse_args()

    if args.add and args.only:
        print("--add and --only are different operations; pick one", file=sys.stderr)
        return 1
    if args.add:
        known = {row[0] for path in (CONTAINMENT, SMEAR, REACH) for row in existing(path)}
        already = sorted(set(args.add) & known)
        if already:
            print(
                f"refusing to --add {already}: they already have rows. Registering a "
                "new surface and RE-MEASURING an existing one are different "
                "operations, and only the second one is allowed to move a number "
                "somebody already measured — run the tool with no arguments for that.",
                file=sys.stderr,
            )
            return 1

    if rpc_verify.pinned_fontconfig() is None:
        print(
            "refusing to measure: FONTCONFIG_FILE is already set, so the pin is "
            "off and these numbers would be a fact about this host again",
            file=sys.stderr,
        )
        return 1

    targets = args.add or args.only or examples()
    if args.add:
        unknown = sorted(set(args.add) - set(examples()))
        if unknown:
            print(f"no such example: {unknown}", file=sys.stderr)
            return 1
    cont: list[tuple[str, object, str]] = []
    smear: list[tuple[str, object, str]] = []
    reach: list[tuple[str, object, str]] = []
    for i, example in enumerate(targets, 1):
        try:
            c, s, l, reason = measure(example)
        except BuildFailed as exc:
            print(
                f"STOPPING at [{i}/{len(targets)}]: {exc}\n"
                "A build failure is not a measurement. Nothing was written — "
                "fix the build (or stop the concurrent cargo run holding the "
                "target lock) and start again.",
                file=sys.stderr,
            )
            return 1
        cont.append((example, c, reason))
        smear.append((example, s, reason))
        reach.append((example, l, reason))
        print(
            f"[{i}/{len(targets)}] {example}: containment={c} smear={s} lost={l} {reason}",
            file=sys.stderr,
            flush=True,
        )

    if args.dry_run or args.only:
        print("(dry run / partial — not writing)", file=sys.stderr)
        return 0
    if args.add:
        cont = merged(CONTAINMENT, cont)
        smear = merged(SMEAR, smear)
        reach = merged(REACH, reach)
    write(CONTAINMENT, "the measured backlog of painted marks that left their box.", cont)
    write(SMEAR, "the measured backlog of text runs painted on top of each other.", smear)
    write(REACH, "the measured backlog of marks no gesture can bring into view.", reach)
    print(f"wrote {CONTAINMENT}, {SMEAR} and {REACH}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
