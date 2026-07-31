#!/usr/bin/env python3
"""Phase B progress tally — the evidence is counted, the judgment is dated.

Why this exists (R1519). `CLAUDE.md` carried "Phase B ~56%" from a hand tally
made at **R931**. By R1518 — 587 rounds later — the tree had gone from 20 crates
/ 115 examples / 228 demos to 27 / 197 / 474, and the number had not moved once.
The percentage was not wrong so much as UNDATED: nothing said what evidence it
was judged against, so nothing could notice it no longer described the tree. A
progress figure that cannot go stale visibly is not a measurement, it is a
slogan.

**This tool does not compute the percentage.** "How complete is the DCC widget
axis against Qt" is a judgment, and a script that emitted a number for it would
be inventing precision (the workspace's own rule against fake metrics). What a
script CAN do is:

  1. count the evidence each axis rests on, mechanically and repeatably;
  2. hold the judgment NEXT TO the evidence it was made against, with the round
     it was made in; and
  3. shout when today's evidence has drifted far enough from that snapshot that
     the judgment should be re-made.

So the number stays human, and its staleness becomes mechanical.

It also reports what it CANNOT classify. An example matching no axis is not
silently dropped — it is listed, because a body of work with no axis is exactly
how the R1372-R1442 dataviz campaign became invisible to this tally.

**Evidence kinds (R1522).** R1519 counted one artifact — example directory
names — for every axis, and for six of the eight that is exactly right: a new
widget is a new example. For the performance axis it is structurally wrong, and
two consecutive rounds proved it. R1520 (scroll paint cache, 1360us -> 42us) and
R1521 (shape cache, 27.4ms -> 1.59ms) each closed the very gap this axis's
judgment named — "no measured large-scene hot-path opt" — and each moved its
evidence by ZERO, because an optimisation creates no example. Measured while
fixing it:

  * the 476 demos were counted in the report header and used for nothing;
  * the perf axis's four patterns were the names of the four examples that
    existed at R1519 — one pattern per match — so the axis could grow only if a
    future round happened to have been named in advance;
  * demo *names* do not rescue it. 63% of them match no axis at all (29% after
    normalising `_` to `-`, which the patterns need because they are written in
    example orthography), and the perf patterns still miss R1520/R1521 because
    they name example features rather than a category;
  * demo *bodies* do. What an optimisation leaves behind is a demo asserting on
    a cost counter, and that set contains R1520 and R1521 while excluding the
    six rounds that read `frame_timings` to verify focus, window identity or
    hover.

So an axis declares its evidence as (kind, patterns) pairs. The count is only
ever compared with that axis's OWN snapshot — never between axes — so axes may
legitimately count different artifacts; what the count must do is MOVE when work
lands on the axis, and that is the property the perf axis lacked.

Usage:
    python3 tools/phase_b_tally.py            # report
    python3 tools/phase_b_tally.py --selftest # check the tool's own logic
"""

from __future__ import annotations

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

#: Evidence drift, as a fraction of the snapshot count, past which an axis's
#: judgment is called STALE. 25% is not a principled constant — it is "a quarter
#: more evidence than you judged against is enough to look again". The point is
#: that SOME threshold fires; the R931 tally drifted +71% on examples and never
#: fired anything, because there was no threshold at all.
STALE_AT = 0.25

#: How an artifact is counted, and whether every artifact of that kind is
#: expected to belong somewhere.
#:
#: A *census* kind must account for all of its artifacts: an unmatched one is a
#: finding, and listing it is how a body of work with no axis surfaces at all.
#: A *probe* kind is consulted only by the axes that declare it, so an unmatched
#: artifact is not a signal and listing them would bury the report. Demos are a
#: probe and not a census on measurement, not preference: a demo is named for
#: the round it served and EVERY round has one whatever axis it advanced, so
#: "which axis owns this demo" is frequently unanswerable — 29% of demo names
#: match no axis even after separator normalisation.
CENSUS, PROBE = "census", "probe"

KINDS = {
    "example-name": CENSUS,  # examples/<name>/ — matched against the name
    "demo-body": PROBE,  # tools/demos/<name>.py — matched against the source
}

#: The axes, their weights in Phase B, the evidence each rests on, and the
#: judgment last recorded for each.
#:
#: `evidence` is a list of (kind, patterns). Patterns are substrings; the FIRST
#: axis whose pattern matches owns the artifact, so list order is the tie-break
#: for work that touches two axes (e.g. `hello-grid-sort` is Model/View before
#: it is catalog). `gated` marks an axis that cannot be advanced from this
#: machine — it is excluded from the "buildable" subtotal so that subtotal is a
#: target that can actually be reached.
AXES = [
    {
        "key": "dcc",
        "name": "Advanced DCC / IDE widgets",
        "weight": 20,
        "gated": False,
        "evidence": [
            ("example-name", [
                "property-grid", "data-grid", "node-editor", "inspector",
                "dock-", "tree-", "tree-view", "column-", "cell-select",
                "asset-browser", "file-manager", "undo", "grid-header-menu",
                "grid-frozen-col", "row-dissect", "hex-dump", "code-fold",
                "command-palette", "selection-toolbar", "tab-reorder",
                "dock-presets",
            ]),
        ],
        "judged_at": 1519,
        "completion": 85,
        "evidence_snapshot": 26,
    },
    {
        "key": "modelview",
        "name": "Model/View at scale",
        "weight": 16,
        "gated": False,
        "evidence": [
            ("example-name", [
                "virtual-", "lazy-", "million-row", "paged-stream",
                "async-data", "measured-list", "variable-list", "grouped-",
                "table", "grid-", "streaming-log", "tail-reveal", "live-data",
                "multi-select", "listbox", "flex-virtual",
            ]),
        ],
        "judged_at": 1519,
        "completion": 75,
        "evidence_snapshot": 36,
    },
    {
        "key": "catalog",
        "name": "Common widget catalog + interaction",
        "weight": 16,
        "gated": False,
        "evidence": [
            ("example-name", [
                "button", "checkbox", "radio", "toggle", "slider",
                "spinbutton", "number-input", "combobox", "tabs", "toolbar",
                "menu", "dialog", "tooltip", "popover", "accordion",
                "disclosure", "drawer", "snackbar", "badge", "fab", "rating",
                "chip", "card", "stepper", "nav-rail", "pagination",
                "breadcrumb", "segmented", "progress", "status-bar",
                "datepicker", "color-picker", "contextmenu", "hyperlink",
                "theme", "gradient", "path", "timeline", "transport",
                "scrubber", "image", "commands", "dnd", "range-slider",
                "popup", "gesture", "pinch-zoom", "smart-zoom", "raw-pointer",
                "crosshair", "settings-panel", "todomvc", "figma-",
            ]),
        ],
        "judged_at": 1519,
        "completion": 82,
        "evidence_snapshot": 73,
    },
    {
        "key": "dataviz",
        "name": "Charting / data visualisation",
        "weight": 10,
        "gated": False,
        # R1519 — this axis did not exist in the R931 tally, which is why the
        # entire R1372-R1442 campaign (22 examples, 72 demos, `pinion-chart` +
        # `pinion-graph`) could not move the Phase B number by a single point.
        # Qt ships QtCharts, so under the qt-parity directive it is in scope.
        "evidence": [
            ("example-name", [
                "chart", "scatter", "heatmap", "treemap", "donut", "histogram",
                "legend", "brush", "elevation", "market-map", "stat-tiles",
                "topology", "series-toggle", "rescale-toggle", "autoscale-y",
                "cross-filter", "live-data", "deviation-grid",
            ]),
        ],
        "judged_at": 1519,
        "completion": 65,
        "evidence_snapshot": 22,
    },
    {
        "key": "text",
        "name": "Rich-text editing / selection",
        "weight": 9,
        "gated": False,
        "evidence": [
            ("example-name", [
                "textfield", "textarea", "richtext", "find-replace",
                "syntax-highlight", "textgrid", "completer", "app-font",
            ]),
        ],
        "judged_at": 1519,
        "completion": 70,
        "evidence_snapshot": 9,
    },
    {
        "key": "perf",
        "name": "Pro-tool performance",
        "weight": 9,
        "gated": False,
        # R1522 — this axis is the reason `demo-body` exists. Its example
        # patterns name the four infrastructure demos (profiler, immediate-mode
        # canvas, replay), which is evidence of *tooling*; its completion is
        # gated on *optimisations*, which produce no example. So the axis's own
        # bottleneck was invisible to its own evidence, and two rounds of
        # exactly that work registered as +0%.
        #
        # The body patterns are cost-counter names. A demo that asserts on one
        # is what a landed optimisation leaves behind — deterministic counters
        # rather than wall-clock, so the guard is not flaky. Deliberately NOT
        # included: bare `frame_timings` / `render_us`, which any round may read
        # (measured: six do so to verify focus, window identity or hover, none
        # of them perf work).
        "evidence": [
            ("example-name", [
                "frame-profiler", "immediate-mode-canvas", "immediate-intent",
                "replay",
            ]),
            ("demo-body", [
                "cache_stats", "paint_cache", "frame_budget", "fixed_timestep",
            ]),
        ],
        # R1522 re-judgment. R1519 said 50% on "measurement infra mature
        # (R907 frame_timings + R925 jank profiler), measured hot-path opt 0".
        # That 0 is now 2, both with counter guards and recorded before/after:
        # R1520 scroll paint encode 1360us -> 42us, R1521 shape cache 27.4ms ->
        # 1.59ms at 1200 leaves. Still absent: GPU-timestamp render time, a
        # large-scene 60fps end-to-end measurement, and the paint walk's 2.6us
        # per text node (glyph-run walk + draw_glyphs encoding), which R1521
        # left as the dominant term.
        "judged_at": 1522,
        "completion": 60,
        "evidence_snapshot": 11,
    },
    {
        "key": "osnative",
        "name": "OS-native integration",
        "weight": 11,
        "gated": True,  # Mac/Win surfaces need those OSes' runners
        "evidence": [
            ("example-name", [
                "file-dialog", "file-open-dialog", "file-save-dialog",
                "file-browser", "filedrop", "print", "pdf-export", "tray",
                "window-", "multi-window", "no-primary", "modal-handoff",
                "modal-refocus",
            ]),
        ],
        "judged_at": 1519,
        "completion": 58,
        "evidence_snapshot": 13,
    },
    {
        "key": "api",
        "name": "§7 API stabilisation",
        "weight": 9,
        "gated": True,  # deliberately parked: freeze a mature surface, not a churning one
        "evidence": [
            ("example-name", [
                "ai-introspect", "answer-origin", "encoded-answer",
                "endpoint-identity", "viewport-question", "conn-lifecycle",
                "forge-counter",
            ]),
        ],
        "judged_at": 1519,
        "completion": 30,
        "evidence_snapshot": 7,
    },
]


#: Examples that are NOT Phase B evidence, and why. Listing them with a reason
#: is the difference between "excluded" and "invisible" — the dataviz campaign
#: was invisible for 587 rounds precisely because nothing named it.
NOT_PHASE_B = {
    "hello-audio": "Phase C — audio substrate",
    "hello-audio-device": "Phase C — audio substrate",
    "hello-audio-rt": "Phase C — audio substrate",
    "hello-narrative-walk": "cross-repo VN consumer axis (sprag)",
    "hello-place-map": "cross-repo VN consumer axis (sprag)",
    "hello-transcript": "cross-repo VN consumer axis (sprag)",
    "hello-vn-tide": "cross-repo VN consumer axis (sprag)",
}

#: Demos whose body matches an axis pattern for a reason that is not evidence.
#: Same idiom as NOT_PHASE_B, same reason: a documented exclusion can be argued
#: with, a silent one cannot. A body proxy will always admit some of these — the
#: alternative is a name proxy, which admits nothing and sees nothing.
NOT_EVIDENCE = {
    "r889_window_known_gate.py": (
        "exercises cache_stats only to prove it rejects a bogus window "
        "(window-identity round, not a cost measurement)"
    ),
}


def examples() -> list[str]:
    return sorted(
        p.name for p in (ROOT / "examples").iterdir() if (p / "Cargo.toml").is_file()
    )


def demos() -> list[str]:
    return sorted(p.name for p in (ROOT / "tools" / "demos").glob("*.py"))


#: `demo-body` reads ~475 files, and the report consults each universe more than
#: once. Cached because the tree does not change mid-run, and because this runs
#: on every push: uncached it measured 3.0s, which is a cost a reporter has no
#: business charging.
_UNIVERSE: dict[str, dict[str, str]] = {}


def universe(kind: str) -> dict[str, str]:
    """Artifacts of `kind`, as name -> the text patterns are matched against."""
    if kind in _UNIVERSE:
        return _UNIVERSE[kind]
    if kind == "example-name":
        got = {n: n for n in examples() if n not in NOT_PHASE_B}
    elif kind == "demo-body":
        got = {
            n: (ROOT / "tools" / "demos" / n).read_text(encoding="utf-8")
            for n in demos()
            if n not in NOT_EVIDENCE
        }
    else:
        raise KeyError(f"unknown evidence kind: {kind}")
    _UNIVERSE[kind] = got
    return got


def patterns_for(axis: dict, kind: str) -> list[str]:
    return [p for k, pats in axis["evidence"] if k == kind for p in pats]


def assign(kind: str, items: dict[str, str]) -> tuple[dict[str, list[str]], list[str]]:
    """Assign each artifact to the first axis whose pattern its text contains.

    Pure in `items` so the tool's own logic can be tested without the tree.
    """
    owned: dict[str, list[str]] = {a["key"]: [] for a in AXES}
    unmatched: list[str] = []
    for name, text in sorted(items.items()):
        for axis in AXES:
            pats = patterns_for(axis, kind)
            if pats and any(pat in text for pat in pats):
                owned[axis["key"]].append(name)
                break
        else:
            unmatched.append(name)
    return owned, unmatched


def evidence() -> tuple[dict[str, list[str]], dict[str, list[str]]]:
    """Per-axis evidence names, and the unmatched artifacts of each census kind."""
    counts: dict[str, list[str]] = {a["key"]: [] for a in AXES}
    unmatched: dict[str, list[str]] = {}
    for kind, coverage in KINDS.items():
        owned, missed = assign(kind, universe(kind))
        for key, names in owned.items():
            counts[key] += names
        if coverage == CENSUS:
            unmatched[kind] = missed
    return counts, unmatched


def drift(now: int, snapshot: int | None) -> tuple[bool, str]:
    if snapshot is None:
        return True, "no snapshot — never judged against counted evidence"
    if snapshot == 0:
        return (now > 0), f"{snapshot} -> {now}"
    delta = (now - snapshot) / snapshot
    return abs(delta) > STALE_AT, f"{snapshot} -> {now} ({delta:+.0%})"


def _sources(axis: dict) -> str:
    """Which artifacts this axis's count is made of — the `ev` column is not
    comparable between axes, so it must say what it counted."""
    short = {"example-name": "ex", "demo-body": "dm"}
    return "+".join(short.get(k, k) for k, _ in axis["evidence"])


def report() -> int:
    counts, unmatched = evidence()
    total_w = sum(a["weight"] for a in AXES)
    weighted = 0.0
    buildable_w = 0
    buildable_weighted = 0.0
    stale: list[str] = []

    print(f"Phase B tally — {len(examples())} examples, {len(demos())} demos\n")
    print(
        f"{'axis':38s} {'w':>3s} {'ev':>4s} {'src':>6s} {'done':>5s} "
        f"{'evidence drift':>22s}"
    )
    print("-" * 84)
    for axis in AXES:
        n = len(counts[axis["key"]])
        is_stale, how = drift(n, axis["evidence_snapshot"])
        done = axis["completion"]
        if is_stale:
            stale.append(axis["key"])
        if done is not None:
            weighted += axis["weight"] * done / 100
            if not axis["gated"]:
                buildable_w += axis["weight"]
                buildable_weighted += axis["weight"] * done / 100
        gate = " [gated]" if axis["gated"] else ""
        shown = f"{done}%" if done is not None else "  ?"
        print(
            f"{axis['name'][:36] + gate:38s} {axis['weight']:3d} {n:4d} "
            f"{_sources(axis):>6s} {shown:>5s} {how:>22s}"
        )
    print("-" * 84)
    print(
        f"{'weighted (all axes)':38s} {total_w:3d} {'':4s} {'':6s} {weighted:4.0f}%"
    )
    if buildable_w:
        print(
            f"{'weighted (buildable only)':38s} {buildable_w:3d} {'':4s} {'':6s} "
            f"{buildable_weighted / buildable_w * 100:4.0f}%"
        )

    # Leverage = weight x remaining. The answer to "what next" derived from the
    # evidence rather than from whichever axis was written first in a list — the
    # value order in CLAUDE.md predates two re-tallies and is not re-derived when
    # completions move.
    lev = sorted(
        (
            (a["weight"] * (100 - a["completion"]), a["name"])
            for a in AXES
            if not a["gated"] and a["completion"] is not None
        ),
        reverse=True,
    )
    if lev:
        print("\nLEVERAGE (buildable only, weight x remaining) — highest first:")
        for score, name in lev:
            print(f"  {score:5d}  {name}")

    for kind, missed in unmatched.items():
        if missed:
            print(
                f"\nUNCLASSIFIED — {len(missed)} {kind} artifact(s) belong to no "
                f"axis. Work with no axis is work this tally cannot see:"
            )
            for name in missed:
                print(f"  {name}")

    # A probe's reach has to be visible, or "no axis looked" is indistinguishable
    # from "nothing was there" — which is the failure this tool keeps finding.
    for kind, coverage in KINDS.items():
        if coverage != PROBE:
            continue
        of_kind = set(universe(kind))
        total = len(of_kind)
        drawn = sum(len(of_kind.intersection(counts[a["key"]])) for a in AXES)
        readers = [a["name"] for a in AXES if patterns_for(a, kind)]
        print(
            f"\nPROBE — {kind}: {drawn} of {total} counted, read only by "
            f"{', '.join(readers) if readers else '(no axis)'}. Unmatched "
            f"artifacts of a probe kind are not a finding."
        )

    print(f"\nEXCLUDED — {len(NOT_PHASE_B)} example(s) are not Phase B evidence:")
    for name, why in sorted(NOT_PHASE_B.items()):
        print(f"  {name:24s} {why}")
    if NOT_EVIDENCE:
        print(f"\nEXCLUDED — {len(NOT_EVIDENCE)} demo(s) match a pattern spuriously:")
        for name, why in sorted(NOT_EVIDENCE.items()):
            print(f"  {name:32s} {why}")

    if stale:
        print(
            f"\nSTALE — {len(stale)} axis judgment(s) rest on evidence that has "
            f"since moved more than {STALE_AT:.0%}: {', '.join(stale)}"
        )
        print("Re-judge them and update `judged_at` / `evidence_snapshot`.")
        return 1
    return 0


def selftest() -> int:
    """The tool's own logic, checked. A staleness detector that cannot report
    staleness is the very failure this round exists to fix."""
    fails = []

    def check(cond: bool, what: str) -> None:
        if not cond:
            fails.append(what)

    # drift()
    check(drift(100, 100) == (False, "100 -> 100 (+0%)"), "no drift is not stale")
    check(drift(126, 100)[0], "26% growth is stale")
    check(not drift(120, 100)[0], "20% growth is not stale")
    check(drift(70, 100)[0], "30% shrink is stale (evidence can be deleted)")
    check(drift(5, None)[0], "an unjudged axis is stale")
    check(drift(1, 0)[0], "first evidence against a zero snapshot is stale")

    # assign(): first-match-wins, and nothing is silently dropped
    names = ["hello-grid-sort", "hello-button", "hello-nothing-here"]
    owned, un = assign("example-name", {n: n for n in names})
    check(
        "hello-grid-sort" in owned["dcc"] or "hello-grid-sort" in owned["modelview"],
        "a grid example lands on a data axis",
    )
    check("hello-button" in owned["catalog"], "a button lands on the catalog axis")
    check(un == ["hello-nothing-here"], "an unmatched name is REPORTED, not dropped")
    total = sum(len(v) for v in owned.values()) + len(un)
    check(total == 3, "every input is accounted for exactly once")

    # R1522 — the property whose absence made this round necessary: an axis's
    # evidence must register work of the shape that axis actually receives. The
    # perf axis receives hot-path optimisations, which create no example, so
    # these two names are counted through `demo-body` or not at all. Under the
    # R1519 tool (example names only) this check FAILS.
    perf = next(a for a in AXES if a["key"] == "perf")
    counts, _ = evidence()
    for landed in ("r1520_scrolled_paint_cache.py", "r1521_shape_cache_working_set.py"):
        check(
            landed in counts["perf"],
            f"the perf axis counts {landed} (a measured hot-path optimisation)",
        )
    check(
        any(k == "demo-body" for k, _ in perf["evidence"]),
        "the perf axis draws on demo bodies, not only example names",
    )

    # demo-body matches the SOURCE, not the name — else it is a name proxy with
    # extra steps, and the six frame_timings readers would slip back in.
    body_owned, _ = assign(
        "demo-body",
        {
            "named_frame_budget_only.py": "nothing a cost counter would say\n",
            "r9999_unrelated_name.py": "resp = tf.cache_stats()\n",
        },
    )
    check(
        "named_frame_budget_only.py" not in body_owned["perf"],
        "a perf-sounding demo NAME with no counter in its body is not evidence",
    )
    check(
        "r9999_unrelated_name.py" in body_owned["perf"],
        "a counter in the BODY is evidence whatever the demo is named",
    )

    # every kind an axis declares must be a known kind with a coverage rule,
    # and every axis needs at least one census source or it is invisible to the
    # census that reports work belonging to no axis
    for a in AXES:
        for kind, _ in a["evidence"]:
            check(kind in KINDS, f"{a['key']} declares unknown kind {kind}")
        check(
            any(KINDS.get(k) == CENSUS for k, _ in a["evidence"]),
            f"{a['key']} has no census source",
        )
    check(
        CENSUS in KINDS.values() and PROBE in KINDS.values(),
        "both coverage rules are in use, else the distinction is decoration",
    )

    # weights are a whole
    check(sum(a["weight"] for a in AXES) == 100, "axis weights sum to 100")
    check(
        any(a["gated"] for a in AXES) and any(not a["gated"] for a in AXES),
        "both gated and buildable axes exist, else the subtotal is meaningless",
    )

    # leverage: a low-completion axis must outrank a high-completion one of the
    # same weight, else the ordering says nothing
    a = {"weight": 10, "completion": 20, "gated": False}
    b = {"weight": 10, "completion": 90, "gated": False}
    check(
        a["weight"] * (100 - a["completion"]) > b["weight"] * (100 - b["completion"]),
        "leverage ranks the less-complete axis higher at equal weight",
    )

    for f in fails:
        print(f"SELFTEST FAIL: {f}")
    print(f"selftest: {'PASS' if not fails else 'FAIL'} ({len(fails)} failure(s))")
    return 1 if fails else 0


if __name__ == "__main__":
    if "--selftest" in sys.argv:
        sys.exit(selftest())
    sys.exit(report())
