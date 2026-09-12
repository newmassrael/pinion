#!/usr/bin/env python3
"""★★★★★ R2161 — **what a walk ASKS the paint for, recorded in order.**

# Why this exists

The address campaign converts a walk by replacing every spelled painted address
with one composed from the screen's own declaration. The obvious check on such a
conversion is that the walk still passes. **It is not a check.**

R2154 measured why, on `r1553_distribution_datum.py`: that walk carries four
`assert_eq(find_by_tag(...), None, ...)` checks, and a WRONG composed address
passes every one of them — `find_by_tag` answers `None` for an address nothing
paints, which is exactly what those four assert. So a conversion that quietly
started asking for the wrong mark is green, and the walk now proves less than it
did while reading as if it proved the same.

What does discriminate is the SEQUENCE OF ADDRESSES THE WALK ASKS FOR. Record it
before the edit and after; a faithful conversion changes nothing, and a
deliberate change shows up as exactly the lines the round meant to change.
R2154 did that by hand and measured 3,202 lookups with one intended difference;
R2156 did it again. Both wrote the harness in a scratchpad and both scratchpads
are gone, so the carry that says *measure every conversion this way* has been
asking for a tool that does not exist. This is that tool.

# Using it

    python3 tools/address_trace.py r1553_distribution_datum --out before.trace
    # ... convert the walk ...
    python3 tools/address_trace.py r1553_distribution_datum --out after.trace
    python3 tools/address_trace.py --compare before.trace after.trace

The compare exits non-zero when the traces differ and prints the difference, so
a round can state *this conversion changed these lookups and no others* rather
than *the walk still passes*.

# ⚠ What it can see, and what it cannot

It wraps two populations and says so on every run:

* the shared readers in `rpc_verify` that take a tag — this is where a walk asks
  the paint about an address at all;
* every module-level function of the walk itself, because a walk's own helper
  (`count_prefix(snap, "chart.box.")`) is where a PREFIX is used and no shared
  reader ever sees that string.

What is recorded from a call is every string argument that LOOKS LIKE a painted
address — dotted, lower-case, no whitespace. That rule is derived from the
value's shape rather than from a list of parameter names, so a helper nobody
told this tool about is still covered.

⚠⚠ It cannot see an address a walk builds and compares itself without passing it
to any function — `if node["tag"] == "chart.bar.0"` is invisible here. That is a
real hole and it is stated rather than left to be discovered: a round whose
conversion touches such a site must say so, because this tool's silence about it
is not evidence. The census (`tools/painted_addresses.py`) is what counts those;
this tool is about whether a conversion preserved MEANING, not about finding
sites.
"""

from __future__ import annotations

import argparse
import difflib
import importlib
import itertools
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

#: A string that looks like a painted address: two lower-case words joined by a
#: dot, then anything.
#:
#: ⚠⚠ **The tail is deliberately permissive, and the selftest is what taught it
#: so.** The first draft required the whole string to be lower-case and caught
#: `chart.series.0` while silently missing `lab.node.P-01` and
#: `lab.node.Group Input` — this tree addresses marks by NODE NAME, so an
#: address carries capitals and spaces. A needle that drops those records
#: nothing for the families that use them, and two traces that record nothing
#: compare equal: the vacuous pass this whole tool exists to stop, inside the
#: tool.
#:
#: ⚠ The first segment must be at least two characters. That is what keeps
#: ordinary prose out — `e.g. something` matched the loose form, and an
#: assertion message in a trace is noise a reader has to step over. The
#: direction of the remaining error is chosen: a false positive appears
#: identically in both runs and cancels in the diff, while a false negative is
#: a lookup the comparison cannot see at all.
LOOKS_LIKE_ADDRESS = re.compile(r"^[a-z][a-z0-9_]+\.[A-Za-z0-9_][\w .#*{}:/-]*$")

#: The shared readers a walk asks the paint through. Named rather than derived
#: because `rpc_verify` is 6,000 lines and most of it is not a tag reader; what
#: makes the list honest is that the walk's OWN functions are wrapped
#: generically beside it, so a helper this list forgets is still covered from
#: the other side.
SHARED_READERS = ("find_by_tag", "access_node_by_tag", "find_node", "rect_of")


def _addresses_in(args: tuple, kwargs: dict) -> list[str]:
    """Every argument of one call that looks like a painted address."""
    found: list[str] = []
    for value in (*args, *kwargs.values()):
        if isinstance(value, str) and LOOKS_LIKE_ADDRESS.match(value):
            found.append(value)
    return found


def _wrap(where, name: str, log: list[str]) -> bool:
    """Wrap one callable so its address-shaped arguments are recorded."""
    original = getattr(where, name, None)
    if not callable(original) or getattr(original, "_address_traced", False):
        return False

    def traced(*args, **kwargs):
        for address in _addresses_in(args, kwargs):
            log.append(f"{name} {address}")
        return original(*args, **kwargs)

    traced._address_traced = True  # noqa: SLF001 — our own marker
    setattr(where, name, traced)
    return True


def trace(walk: str, out: Path) -> int:
    """Run one walk with its address lookups recorded, and write them to `out`."""
    sys.path.insert(0, str(ROOT / "tools"))
    sys.path.insert(0, str(ROOT / "tools" / "demos"))
    import rpc_verify  # noqa: PLC0415 — the path has to be set first

    log: list[str] = []
    wrapped = [name for name in SHARED_READERS if _wrap(rpc_verify, name, log)]
    module = importlib.import_module(walk)
    # ⚠ The walk's own module namespace is wrapped AFTER it is imported, because
    # a walk does `from rpc_verify import find_by_tag` — its module holds its own
    # reference, and wrapping the source afterwards would leave that reference
    # pointing at the original. Wrapping the walk's namespace catches both its
    # imported readers and its own helpers in one pass.
    own = [
        name
        for name in dir(module)
        if not name.startswith("_") and _wrap(module, name, log)
    ]
    if not hasattr(module, "body"):
        print(
            f"address-trace: {walk} has no `body()` to drive. 702 of this "
            "tree's 726 walks do; one that does not needs its entry point "
            "named before it can be traced.",
            file=sys.stderr,
        )
        return 2
    try:
        module.body()
    finally:
        out.write_text("\n".join(log) + "\n", encoding="utf-8")
    print(
        f"address-trace: {len(log)} lookup(s) -> {out} "
        f"({len(wrapped)} shared reader(s) and {len(own)} of the walk's own "
        "callables wrapped)"
    )
    if not log:
        print(
            "address-trace: NOTHING was recorded, which is not the same as "
            "nothing being asked — a trace this tool cannot see is a "
            "comparison that passes vacuously. Check the walk reaches the "
            "screen before trusting a diff of this.",
            file=sys.stderr,
        )
        return 1
    return 0


def runs(lines: list[str]) -> list[str]:
    """One entry per RUN of the same lookup repeated back to back.

    ★★★★★ R2162 — **the comparison unit is the run, not the lookup, and using
    this tool on a harder walk is what found that out.** `r1389_frame_timeline`
    polls: `wait_until` re-snapshots and asks the same address again until the
    screen catches up. How many times it spun is TIMING. Measured at R2162, two
    runs of an IDENTICAL tree differed by 8 lookups and 1,436 diff lines — so
    the raw sequence called a faithful conversion broken, which is the failure
    that teaches a round to stop believing its instrument.

    Collapsed, the same two runs are equal at 22 entries, and so is the
    conversion's before against its after. ⚠ Consecutive duplicates only —
    ORDER is preserved and a lookup that comes back later is its own run, so
    this removes repetition without removing sequence.
    """
    return [line for line, _ in itertools.groupby(lines)]


def addresses(lines: list[str]) -> list[str]:
    """The ADDRESS half of each line, collapsed by run.

    ★★★★★ R2177 — a trace line is `<reader> <address>`, and the comparison is
    over the pair. That is right: which door a walk came in at is part of what
    it did. But it means **adding a named reader on the path of a lookup that
    is otherwise unchanged reads as a change of meaning**, and this campaign's
    conversions do exactly that — moving a composition out of an unwrapped
    helper and onto a shared door is the repair.

    Measured three rounds running: R2174's `r1591` went 0 -> 628 lookups,
    R2176's `r1442` gained one `fill_template` line, and R2177's `r1395`
    produced a 334-line diff whose `find_by_tag` multiset was **identical**
    (5,236 rows both sides). Each time the round verified that by hand.

    ⇒ this is the second half of the verdict, so the tool says which of the two
    situations a reader is in instead of a round re-deriving it. The verdict
    does NOT weaken: a pair-level difference still exits non-zero, because a new
    reader CAN be a change of meaning and only a person can say.
    """
    return runs([line.partition(" ")[2] for line in lines])


def compare(before: Path, after: Path) -> int:
    """Diff two traces; non-zero when they differ in MEANING.

    The verdict is on the collapsed sequences (see [`runs`]); a difference in
    how many times a poll spun is reported and is not a failure.

    ★ R2177 — when the pairs differ, [`addresses`] is asked too and the answer
    is printed. An unchanged address sequence under changed readers is what a
    conversion onto a shared door looks like; it is still reported as a
    difference, and the line says which kind it is.
    """
    raw_left = before.read_text(encoding="utf-8").splitlines()
    raw_right = after.read_text(encoding="utf-8").splitlines()
    left, right = runs(raw_left), runs(raw_right)
    if left == right:
        spun = ""
        if len(raw_left) != len(raw_right):
            spun = (
                f" (raw {len(raw_left)} vs {len(raw_right)} — a poll spun a "
                "different number of times, which is timing and not meaning)"
            )
        print(
            f"address-trace: identical — {len(left)} lookup run(s), and the "
            f"conversion asked the paint for exactly what it asked before{spun}"
        )
        return 0
    changed = [
        line
        for line in difflib.unified_diff(left, right, "before", "after", lineterm="")
        if line.startswith(("+", "-")) and not line.startswith(("+++", "---"))
    ]
    print(
        f"address-trace: {len(left)} -> {len(right)} lookup run(s), "
        f"{len(changed)} line(s) differ"
    )
    for line in changed[:80]:
        print(f"  {line}")
    if len(changed) > 80:
        print(f"  … {len(changed) - 80} more")
    # ★★★★★ R2177 — the second half of the verdict. See [`addresses`].
    left_addr, right_addr = addresses(raw_left), addresses(raw_right)
    if left_addr == right_addr:
        print(
            f"address-trace: ★ but the ADDRESSES are unchanged — {len(left_addr)} "
            "run(s), the same sequence on both sides. Only which reader asked "
            "changed, which is what moving a composition onto a shared door "
            "looks like. Still reported: a new reader CAN mean something, and "
            "only a person can say."
        )
    else:
        print(
            f"address-trace: and the ADDRESSES differ too — {len(left_addr)} -> "
            f"{len(right_addr)} run(s). This is not a reader moving; the walk "
            "is asking the paint for something else."
        )
    print(
        "address-trace: a faithful conversion changes NOTHING here. Every line "
        "above is a change of meaning, and a round that meant it should say so "
        "in as many words.",
        file=sys.stderr,
    )
    return 1


def selftest() -> int:
    """The pure halves, exercised. Runs no walk and opens no window.

    ★★★★★ R2154's lesson applied to this tool on the day it was written: a
    helper with no test is a second unchecked copy of the thing it replaced,
    and `chart_family` shipped that way and was wrong. What can be wrong here
    is the needle (a trace that records nothing compares clean against another
    trace that records nothing — the vacuous pass this whole tool exists to
    stop) and the comparison (one that never reports a difference would call
    every conversion faithful).
    """
    failed = 0
    ran = 0

    def check(ok: bool, label: str) -> None:
        nonlocal failed, ran
        ran += 1
        if not ok:
            failed += 1
            print(f"FAIL: {label}", file=sys.stderr)

    for address in (
        "chart.series.0",
        "lab.form.control.id",
        "card.packet#0.cell.1",
        "chart.grid.minor.y.",
        # ⚠ The two that the first draft of the needle MISSED — this tree
        # addresses marks by node name, so capitals and spaces are ordinary.
        "lab.node.P-01",
        "lab.node.Group Input",
        "lab.palette.role.Router",
    ):
        check(bool(LOOKS_LIKE_ADDRESS.match(address)), f"needle matches {address!r}")
    # ⚠ The other arm, and it is the one that keeps a trace readable: an
    # ordinary sentence, a number and a bare word are not addresses, and a
    # needle that took them would bury the lookups in prose.
    for other in ("the chart is painted", "1.5", "dashboard", "", "A.B", "e.g. this"):
        check(
            not LOOKS_LIKE_ADDRESS.match(other), f"needle declines {other!r}"
        )
    # ★ And a call's arguments are read for shape, not for position — a helper
    # taking `(snap, prefix)` and one taking `(tag)` are both covered.
    picked = _addresses_in(({"tag": "x"}, "chart.grid.y."), {"where": "lab.node.P-01"})
    check(
        picked == ["chart.grid.y.", "lab.node.P-01"],
        f"address-shaped arguments are picked out of a call: {picked}",
    )
    # ★★ The comparison reports a difference and reports sameness, and the two
    # are different exit codes because a round reads this as a verdict.
    import tempfile  # noqa: PLC0415 — the selftest's own need

    with tempfile.TemporaryDirectory() as box:
        one, two = Path(box) / "a", Path(box) / "b"
        one.write_text("find chart.series.0\n", encoding="utf-8")
        two.write_text("find chart.series.0\n", encoding="utf-8")
        check(compare(one, two) == 0, "identical traces compare equal")
        two.write_text("find chart.series.1\n", encoding="utf-8")
        check(compare(one, two) == 1, "a changed address is reported")
        # ★★★★★ R2162 — a POLL that spun a different number of times is timing,
        # not meaning. Two runs of an identical tree measured 8 lookups apart
        # and 1,436 diff lines, which called a faithful conversion broken.
        two.write_text("find chart.series.0\n" * 4, encoding="utf-8")
        check(
            compare(one, two) == 0,
            "a repeated lookup is one run, so a poll's spin count is not a diff",
        )
        # ⚠ And the other arm: collapsing repeats must not collapse ORDER. A
        # lookup that comes back after a different one is its own run.
        one.write_text("find a.b\nfind c.d\nfind a.b\n", encoding="utf-8")
        two.write_text("find a.b\nfind c.d\n", encoding="utf-8")
        check(compare(one, two) == 1, "a lookup that returns later is its own run")
        # ★★★★★ R2177 — the ADDRESS half of the verdict. A reader added on the
        # path of an unchanged lookup is what a conversion onto a shared door
        # looks like, and three rounds verified that by hand before this.
        check(
            addresses(["find a.b", "helper a.b", "find c.d"]) == ["a.b", "c.d"],
            "a reader added beside a lookup leaves the address sequence alone",
        )
        check(
            addresses(["find a.b", "find c.d"]) != addresses(["find a.b", "find e.f"]),
            "★ and a different address still differs — the discriminating case",
        )
    print(f"address_trace selftest: {ran - failed} of {ran} checks OK")
    return 1 if failed else 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("walk", nargs="?", help="the walk module name, no .py")
    parser.add_argument("--out", type=Path, help="where to write the trace")
    parser.add_argument(
        "--compare",
        nargs=2,
        type=Path,
        metavar=("BEFORE", "AFTER"),
        help="diff two traces instead of running a walk",
    )
    parser.add_argument(
        "--selftest", action="store_true", help="exercise the needle and the compare"
    )
    args = parser.parse_args()
    if args.selftest:
        return selftest()
    if args.compare:
        return compare(*args.compare)
    if not args.walk or not args.out:
        parser.error("a walk and --out, or --compare BEFORE AFTER")
    return trace(args.walk, args.out)


if __name__ == "__main__":
    raise SystemExit(main())
