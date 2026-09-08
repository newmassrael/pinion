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

    example  window  missed  broken  reconfigured  rebuilds  repeated  presenting  last_missed  why
    example  window  unanswered

`why` (R2099) is `name:count` pairs — every reason the producer knows, in its
order, zeros included — or `-` from a binary older than that field. It is the
column that answers WHY, which `last_missed` cannot: that field resets the
instant a frame reaches the screen, so a window that broke and RECOVERED
named nothing at all. Three hosted sweeps each reported twenty missed frames,
all twenty breakages, and not one reason.

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
        #: R2099 — `reason -> missed frames`, over every window that could
        #: answer, in the producer's own order (dicts keep insertion order).
        #:
        #: ★ The column this census was missing. Three hosted sweeps each
        #: reported twenty missed frames, all twenty breakages, and NOT ONE
        #: REASON — because the only reason published was `last_missed`,
        #: which resets when a frame reaches the screen, so a window that
        #: broke and recovered named nothing. The debt this census exists
        #: for pre-registered *a surviving device-lost* as its decisive
        #: evidence; a surviving one is by construction one that recovered.
        self.reasons: dict[str, int] = {}
        #: R2099 — `reason -> {where: count}` for the reasons that actually
        #: happened, so a non-zero row names a window to chase rather than
        #: only a number. Zero rows are in `reasons` and not here.
        self.reason_windows: dict[str, dict[str, int]] = {}
        #: R2099 — windows that answered the counts but carry no reason
        #: table (a binary older than the field). Counted, never folded into
        #: a zero: this census has already published one silence as a fact.
        self.reasonless = 0

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
        breakdown = parts[4 + len(FIELDS)] if len(parts) > 4 + len(FIELDS) else "-"
        self._add_reasons(where, breakdown)

    def _add_reasons(self, where: str, breakdown: str) -> None:
        """R2099 — fold one window's `name:count` pairs into the totals."""
        if breakdown == "-" or not breakdown:
            self.reasonless += 1
            return
        for pair in breakdown.split(","):
            name, _, raw = pair.partition(":")
            if not name or not raw.lstrip("-").isdigit():
                # A pair this reader cannot parse is a pair it must not
                # guess at: count the window as unable to say, exactly as an
                # absent column is counted.
                self.reasonless += 1
                return
            count = int(raw)
            # Every reason gets a row, zeros included — the zeros are what
            # let a reader say "it did not happen" instead of "nobody said".
            self.reasons[name] = self.reasons.get(name, 0) + count
            if count:
                seen = self.reason_windows.setdefault(name, {})
                seen[where] = seen.get(where, 0) + count

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
        out.extend(self._reason_lines())
        return out

    def _reason_lines(self) -> list[str]:
        """R2099 — WHY the missed frames missed, and where."""
        out: list[str] = []
        if self.reasons:
            named = ", ".join(f"{name} {count}" for name, count in self.reasons.items())
            out.append(f"[present-census] why: {named}")
            # ★ The invariant R2096 taught this file to state: a named list
            # published under a total must SUM to it. Here it also crosses
            # two independent counters — the miss tally and the per-reason
            # tally are written on the same path, so a disagreement means
            # the seam dropped a row rather than that a window misbehaved.
            summed = sum(self.reasons.values())
            if summed != self.totals["missed"]:
                out.append(
                    f"[present-census] ⚠ the reason rows sum to {summed} but "
                    f"{self.totals['missed']} frames were missed — "
                    f"{abs(self.totals['missed'] - summed)} unaccounted for"
                )
            for name, windows in self.reason_windows.items():
                worst = sorted(windows.items(), key=lambda kv: -kv[1])
                shown = ", ".join(f"{where} {count}" for where, count in worst[:NAMED])
                rest = len(worst) - NAMED
                out.append(
                    f"[present-census] {name}: {shown}"
                    + (f", and {rest} more" if rest > 0 else "")
                )
        if self.reasonless:
            out.append(
                f"[present-census] {self.reasonless} window(s) answered the counts "
                f"but named no reason (a binary older than the field) — NOT counted "
                f"as zero for any reason"
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
        # ⚠ R2099 — and every answering row now carries its `why` column, with
        # a DIFFERENT non-zero count per reason: equal counts would let a
        # reader that transposed two reasons pass. One row carries `-`, which
        # is the older-binary case, and it carries ZERO misses so the sum
        # invariant below still holds — the case where a `-` row DID miss
        # frames is its own fixture further down, because that is what makes
        # the unaccounted-for warning falsifiable.
        path.write_text(
            "hello-a\tmain\t0\t0\t0\t0\t0\tTrue\t-\t"
            "outdated:0,lost:0,validation:0,timeout:0,occluded:0,"
            "unconfigured:0,device_lost:0\n"
            "hello-b\tmain\t118\t118\t1\t1\t116\tFalse\tdevice_lost\t"
            "outdated:0,lost:2,validation:0,timeout:0,occluded:0,"
            "unconfigured:115,device_lost:1\n"
            "hello-b\ttorn\t2\t0\t0\t0\t0\tTrue\t-\t"
            "outdated:0,lost:0,validation:0,timeout:0,occluded:2,"
            "unconfigured:0,device_lost:0\n"
            "hello-b\tmain\t5\t5\t5\t0\t0\tTrue\t-\t"
            "outdated:5,lost:0,validation:0,timeout:0,occluded:0,"
            "unconfigured:0,device_lost:0\n"
            "hello-d\tmain\t0\t0\t0\t0\t0\tTrue\t-\n"
            "hello-c\tmain\tunanswered\n"
            "hello-c\tmain\tunanswered\n",
            encoding="utf-8",
        )
        census = total(path)
        assert census.windows == 7, census.windows
        assert len(census.examples) == 4, census.examples
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
        assert "4 example(s)" in text, text
        assert "could not answer" in text, text
        # A count of silent windows with no NAME is a number a reader cannot
        # act on — the shape this repository keeps repaying.
        assert "hello-c/main" in text, text
        assert "ENDED DARK" in text, text
        # ★ A window that missed frames and RECOVERED is still counted: that is
        # the whole point of the cumulative fields, and a summary that only saw
        # the dark ones would report the same number the old instrument did.
        assert "hello-b/torn 2" in text, text

        # ★★★★★ R2099 — WHY, which is the column three hosted sweeps did not
        # have. Asserted as the WHOLE table including the zeros: a reader
        # deciding whether a repair worked needs "device-lost happened once"
        # AND "validation happened zero times", and a summary that printed
        # only the non-zero rows could not tell the second from silence.
        assert census.reasons == {
            "outdated": 5,
            "lost": 2,
            "validation": 0,
            "timeout": 0,
            "occluded": 2,
            "unconfigured": 115,
            "device_lost": 1,
        }, census.reasons
        # The invariant R2096 taught this file, applied to the new list.
        assert sum(census.reasons.values()) == census.totals["missed"], census.reasons
        # A non-zero reason names WINDOWS, because a number no one can chase
        # is the shape this repository keeps repaying.
        assert census.reason_windows["device_lost"] == {"hello-b/main": 1}
        assert "validation" not in census.reason_windows, census.reason_windows
        assert census.reasonless == 1, census.reasonless
        assert "why: outdated 5, lost 2, validation 0" in text, text
        assert "device_lost: hello-b/main 1" in text, text
        assert "named no reason" in text, text
        # And a clean sweep must not report an unaccounted-for remainder.
        assert "unaccounted for" not in text, text

        # ★ THE DISCRIMINATING FIXTURE: a row that missed frames and named no
        # reason. Without this, the warning below could never fire and the
        # invariant above would be an assertion with no failing path — the
        # shape this session has already had to delete twice.
        older = Path(tmp) / "older.tsv"
        older.write_text("hello-e\tmain\t4\t4\t1\t1\t2\tTrue\t-\n", encoding="utf-8")
        stale = total(older)
        assert stale.reasonless == 1, stale.reasonless
        assert stale.reasons == {}, stale.reasons
        stale_text = "\n".join(stale.lines())
        assert "named no reason" in stale_text, stale_text
        # No reason rows at all ⟹ no "why" line to disagree with, so the
        # warning belongs to the MIXED case, which is the next one.
        assert "why:" not in stale_text, stale_text

        mixed = Path(tmp) / "mixed.tsv"
        mixed.write_text(
            "hello-e\tmain\t4\t4\t1\t1\t2\tTrue\t-\n"
            "hello-f\tmain\t1\t1\t1\t0\t0\tTrue\t-\t"
            "outdated:1,lost:0,validation:0,timeout:0,occluded:0,"
            "unconfigured:0,device_lost:0\n",
            encoding="utf-8",
        )
        mixed_text = "\n".join(total(mixed).lines())
        assert "the reason rows sum to 1 but 5 frames were missed" in mixed_text, mixed_text
        assert "4 unaccounted for" in mixed_text, mixed_text

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
