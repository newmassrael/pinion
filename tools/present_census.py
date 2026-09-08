#!/usr/bin/env python3
"""R2090 — total the present-health census a sweep collected.

`rpc_verify.RpcSubprocess` asks every window, at every demo's teardown, what
its rendering went through (`scene/render_fidelity`'s cumulative counts) and
appends one row per window when `PINION_PRESENT_CENSUS` names a file. This
turns that file into the number the open rendering defect needs: **how often
anything went wrong at all**, across a whole sweep nobody was watching.

Why it is a separate tool rather than a few lines in `sweep_headless.sh`: the
sweep's runner body is one single-quoted shell string, so an `awk` program
cannot be written there at all (its own comment says so), and a summary that
cannot be tested is a summary nobody can trust. This has a `--selftest`, run
as a `pre-push` step — a wrong total here is a wrong *denominator* for the
open rendering defect, reported with a straight face.

    usage: present_census.py <census.tsv> [--selftest]

Row shape, written by the harness:

    example  window  missed  broken  reconfigured  rebuilds  repeated  presenting  last_missed
    example  window  unanswered

`unanswered` is a window that never painted (so it has no record) or a binary
older than the fields. It is counted separately rather than as zero: reading
an absent answer as "nothing went wrong" is the inference this whole line of
work exists to stop.
"""

from __future__ import annotations

import sys
from pathlib import Path

FIELDS = ("missed", "broken", "reconfigured", "rebuilds", "repeated")

#: How many affected demos to name in the summary. The rest are counted.
NAMED = 8


class Census:
    """What a sweep's windows went through, totalled."""

    def __init__(self) -> None:
        self.windows = 0
        #: Distinct EXAMPLES, not walks: two walks driving one example are one
        #: application, and the counts are the application's. Naming it after
        #: the walk would double-count a shell every second demo drives.
        self.examples: set[str] = set()
        self.unanswered = 0
        #: Which windows could not answer, so a reader can chase one instead
        #: of being told only how many there were. DISTINCT, in first-seen
        #: order: one example driven by forty demos is one thing to chase.
        self.silent: list[str] = []
        self.totals = dict.fromkeys(FIELDS, 0)
        #: `example/window -> missed`, ACCUMULATED across every demo that drove
        #: that example.
        #:
        #: 🟥 R2096 — this used to assign rather than add, and the first real
        #: sweep showed the defect in its own output: the totals said 20 missed
        #: frames while the named list summed to 7, because 726 demos drive 220
        #: examples and a repeated `example/window` key overwrote its earlier
        #: count. The TOTALS were right (they always added); only the list a
        #: reader acts on was wrong, which is the worse half to get wrong.
        self.affected: dict[str, int] = {}
        #: Windows whose last reading was not presenting, mapped to the reason.
        #: A dict for the same reason `affected` is one: the same window seen
        #: dark in six demos is one window to chase, not six lines.
        self.dark: dict[str, str] = {}

    def add(self, line: str) -> None:
        parts = line.rstrip("\n").split("\t")
        if len(parts) < 3:
            return
        example, window = parts[0], parts[1]
        self.windows += 1
        self.examples.add(example)
        where = f"{example}/{window}"
        if parts[2] == "unanswered" or len(parts) < 3 + len(FIELDS):
            self.unanswered += 1
            if where not in self.silent:
                self.silent.append(where)
            return
        counts = [int(n) if n.lstrip("-").isdigit() else 0 for n in parts[2 : 2 + len(FIELDS)]]
        for field, count in zip(FIELDS, counts, strict=True):
            self.totals[field] += count
        if counts[0]:
            self.affected[where] = self.affected.get(where, 0) + counts[0]
        presenting = parts[2 + len(FIELDS)] if len(parts) > 2 + len(FIELDS) else "True"
        reason = parts[3 + len(FIELDS)] if len(parts) > 3 + len(FIELDS) else "-"
        if presenting == "False":
            self.dark[where] = reason

    def lines(self) -> list[str]:
        out = [
            f"[present-census] {self.windows} window(s) over {len(self.examples)} "
            f"example(s) — missed {self.totals['missed']} "
            f"({self.totals['broken']} broken), rungs {self.totals['reconfigured']}/"
            f"{self.totals['rebuilds']}/{self.totals['repeated']}"
        ]
        if self.unanswered:
            named = ", ".join(self.silent[:NAMED])
            rest = len(self.silent) - NAMED
            out.append(
                f"[present-census] {self.unanswered} window(s) could not answer "
                f"(never painted, or a binary older than the fields) — NOT counted "
                f"as zero: {named}" + (f", and {rest} more" if rest > 0 else "")
            )
        if self.affected:
            worst = sorted(self.affected.items(), key=lambda kv: -kv[1])
            named = ", ".join(f"{where} {missed}" for where, missed in worst[:NAMED])
            rest = len(worst) - NAMED
            out.append(
                f"[present-census] windows that missed frames: {named}"
                + (f", and {rest} more" if rest > 0 else "")
            )
        else:
            out.append(
                "[present-census] no window missed a frame — this sweep produced "
                "no denominator for the rendering defect"
            )
        if self.dark:
            out.append(
                "[present-census] ENDED DARK: "
                + ", ".join(f"{where} ({reason})" for where, reason in self.dark.items())
            )
        return out


def total(path: Path) -> Census:
    census = Census()
    if not path.exists():
        return census
    for line in path.read_text(encoding="utf-8").splitlines():
        if line.strip():
            census.add(line)
    return census


def selftest() -> int:
    import tempfile

    with tempfile.TemporaryDirectory() as tmp:
        path = Path(tmp) / "census.tsv"
        # ⚠ R2096 — the fixture now has an example driven TWICE, because that is
        # the shape a real sweep has (726 demos over 220 examples) and it is the
        # shape the first version got wrong: the repeated key overwrote instead
        # of adding, so the totals and the named list disagreed in the census's
        # own output. A fixture without a repeat could not have caught it.
        path.write_text(
            "hello-a\tmain\t0\t0\t0\t0\t0\tTrue\t-\n"
            "hello-b\tmain\t118\t118\t1\t1\t116\tFalse\tdevice_lost\n"
            "hello-b\ttorn\t2\t0\t0\t0\t0\tTrue\t-\n"
            "hello-b\tmain\t5\t5\t5\t0\t0\tTrue\t-\n"
            "hello-c\tmain\tunanswered\n"
            "hello-c\tmain\tunanswered\n",
            encoding="utf-8",
        )
        census = total(path)
        assert census.windows == 6, census.windows
        assert len(census.examples) == 3, census.examples
        assert census.unanswered == 2, census.unanswered
        # Counted twice, listed once: a window to chase is one window however
        # many demos met it.
        assert census.silent == ["hello-c/main"], census.silent
        assert census.totals["missed"] == 125, census.totals
        assert census.totals["broken"] == 123, census.totals
        assert census.totals["repeated"] == 116, census.totals
        # ★ THE ROW THAT CAUGHT THE DEFECT: 118 + 5, not 5.
        assert census.affected == {"hello-b/main": 123, "hello-b/torn": 2}, census.affected
        assert census.dark == {"hello-b/main": "device_lost"}, census.dark
        # And the named list must ADD UP TO the total it is listed under —
        # the property whose absence was visible in the first real sweep.
        assert sum(census.affected.values()) == census.totals["missed"], census.affected
        text = "\n".join(census.lines())
        assert "125 (123 broken)" in text, text
        assert "3 example(s)" in text, text
        assert "could not answer" in text, text
        # A count of silent windows with no NAME is a number a reader cannot
        # act on — the shape this repository keeps repaying.
        assert "hello-c/main" in text, text
        assert "ENDED DARK" in text, text
        # ★ A window that missed frames and RECOVERED is still counted: that is
        # the whole point of the cumulative fields, and a summary that only saw
        # the dark ones would report the same number the old instrument did.
        assert "hello-b/torn 2" in text, text

        empty = total(Path(tmp) / "absent.tsv")
        assert empty.windows == 0
        assert "no denominator" in "\n".join(empty.lines())
    # The count is DERIVED from THIS FUNCTION's source rather than typed: a
    # hand-kept "N checks OK" is a number that goes stale the first time an
    # assertion is added, and this repository has repaid that shape at every
    # scale it appears. Scoped to `selftest` rather than to the file, so an
    # assertion added anywhere else cannot inflate it.
    import inspect

    checks = sum(
        1 for line in inspect.getsource(selftest).splitlines() if line.strip().startswith("assert ")
    )
    print(f"present_census selftest: {checks} checks OK")
    return 0


def main(argv: list[str]) -> int:
    if "--selftest" in argv:
        return selftest()
    if len(argv) != 2:
        print(__doc__.strip().splitlines()[0])
        print("usage: present_census.py <census.tsv> [--selftest]")
        return 2
    for line in total(Path(argv[1])).lines():
        print(line)
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
