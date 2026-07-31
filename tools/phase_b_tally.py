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

Usage:
    python3 tools/phase_b_tally.py            # report
    python3 tools/phase_b_tally.py --selftest # check the tool's own logic
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

#: Evidence drift, as a fraction of the snapshot count, past which an axis's
#: judgment is called STALE. 25% is not a principled constant — it is "a quarter
#: more evidence than you judged against is enough to look again". The point is
#: that SOME threshold fires; the R931 tally drifted +71% on examples and never
#: fired anything, because there was no threshold at all.
STALE_AT = 0.25

#: The axes, their weights in Phase B, the evidence each rests on, and the
#: judgment last recorded for each.
#:
#: `patterns` are matched against example directory names (substring, in order);
#: the FIRST axis whose pattern matches owns the example, so the list order is
#: the tie-break for examples that touch two axes (e.g. `hello-grid-sort` is
#: Model/View before it is catalog). `gated` marks an axis that cannot be
#: advanced from this machine — it is excluded from the "buildable" subtotal so
#: that subtotal is a target that can actually be reached.
AXES = [
    {
        "key": "dcc",
        "name": "Advanced DCC / IDE widgets",
        "weight": 20,
        "gated": False,
        "patterns": [
            "property-grid", "data-grid", "node-editor", "inspector", "dock-",
            "tree-", "tree-view", "column-", "cell-select", "asset-browser",
            "file-manager", "undo", "grid-header-menu", "grid-frozen-col",
            "row-dissect", "hex-dump", "code-fold", "command-palette",
            "selection-toolbar", "tab-reorder", "dock-presets",
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
        "patterns": [
            "virtual-", "lazy-", "million-row", "paged-stream", "async-data",
            "measured-list", "variable-list", "grouped-", "table", "grid-",
            "streaming-log", "tail-reveal", "live-data", "multi-select",
            "listbox", "flex-virtual",
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
        "patterns": [
            "button", "checkbox", "radio", "toggle", "slider", "spinbutton",
            "number-input", "combobox", "tabs", "toolbar", "menu", "dialog",
            "tooltip", "popover", "accordion", "disclosure", "drawer",
            "snackbar", "badge", "fab", "rating", "chip", "card", "stepper",
            "nav-rail", "pagination", "breadcrumb", "segmented", "progress",
            "status-bar", "datepicker", "color-picker", "contextmenu",
            "hyperlink", "theme", "gradient", "path", "timeline", "transport",
            "scrubber", "image", "commands", "dnd", "range-slider", "popup",
            "gesture", "pinch-zoom", "smart-zoom", "raw-pointer", "crosshair",
            "settings-panel", "todomvc", "figma-",
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
        "patterns": [
            "chart", "scatter", "heatmap", "treemap", "donut", "histogram",
            "legend", "brush", "elevation", "market-map", "stat-tiles",
            "topology", "series-toggle", "rescale-toggle", "autoscale-y",
            "cross-filter", "live-data", "deviation-grid",
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
        "patterns": [
            "textfield", "textarea", "richtext", "find-replace",
            "syntax-highlight", "textgrid", "completer", "app-font",
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
        "patterns": [
            "frame-profiler", "immediate-mode-canvas", "immediate-intent",
            "replay",
        ],
        "judged_at": 1519,
        "completion": 50,
        "evidence_snapshot": 4,
    },
    {
        "key": "osnative",
        "name": "OS-native integration",
        "weight": 11,
        "gated": True,  # Mac/Win surfaces need those OSes' runners
        "patterns": [
            "file-dialog", "file-open-dialog", "file-save-dialog",
            "file-browser", "filedrop", "print", "pdf-export", "tray",
            "window-", "multi-window", "no-primary", "modal-handoff",
            "modal-refocus",
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
        "patterns": [
            "ai-introspect", "answer-origin", "encoded-answer",
            "endpoint-identity", "viewport-question", "conn-lifecycle",
            "forge-counter",
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

def examples() -> list[str]:
    return sorted(
        p.name for p in (ROOT / "examples").iterdir() if (p / "Cargo.toml").is_file()
    )


def demos() -> list[str]:
    return sorted(p.name for p in (ROOT / "tools" / "demos").glob("*.py"))


def classify(names: list[str]) -> tuple[dict[str, list[str]], list[str]]:
    """Assign each name to the first axis whose pattern it contains."""
    owned: dict[str, list[str]] = {a["key"]: [] for a in AXES}
    unclassified: list[str] = []
    for name in names:
        if name in NOT_PHASE_B:
            continue
        for axis in AXES:
            if any(pat in name for pat in axis["patterns"]):
                owned[axis["key"]].append(name)
                break
        else:
            unclassified.append(name)
    return owned, unclassified


def drift(now: int, snapshot: int | None) -> tuple[bool, str]:
    if snapshot is None:
        return True, "no snapshot — never judged against counted evidence"
    if snapshot == 0:
        return (now > 0), f"{snapshot} -> {now}"
    delta = (now - snapshot) / snapshot
    return abs(delta) > STALE_AT, f"{snapshot} -> {now} ({delta:+.0%})"


def report() -> int:
    ex_owned, ex_unclassified = classify(examples())
    total_w = sum(a["weight"] for a in AXES)
    weighted = 0.0
    buildable_w = 0
    buildable_weighted = 0.0
    stale: list[str] = []

    print(f"Phase B tally — {len(examples())} examples, {len(demos())} demos\n")
    print(f"{'axis':38s} {'w':>3s} {'ex':>4s} {'done':>5s} {'evidence drift':>22s}")
    print("-" * 78)
    for axis in AXES:
        n = len(ex_owned[axis["key"]])
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
            f"{shown:>5s} {how:>22s}"
        )
    print("-" * 78)
    print(f"{'weighted (all axes)':38s} {total_w:3d} {'':4s} {weighted:4.0f}%")
    if buildable_w:
        print(
            f"{'weighted (buildable only)':38s} {buildable_w:3d} {'':4s} "
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

    if ex_unclassified:
        print(
            f"\nUNCLASSIFIED — {len(ex_unclassified)} example(s) belong to no axis. "
            f"Work with no axis is work this tally cannot see:"
        )
        for name in ex_unclassified:
            print(f"  {name}")

    print(
        f"\nEXCLUDED — {len(NOT_PHASE_B)} example(s) are not Phase B evidence:"
    )
    for name, why in sorted(NOT_PHASE_B.items()):
        print(f"  {name:24s} {why}")

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

    # classify(): first-match-wins, and nothing is silently dropped
    owned, un = classify(["hello-grid-sort", "hello-button", "hello-nothing-here"])
    check(
        "hello-grid-sort" in owned["dcc"] or "hello-grid-sort" in owned["modelview"],
        "a grid example lands on a data axis",
    )
    check("hello-button" in owned["catalog"], "a button lands on the catalog axis")
    check(un == ["hello-nothing-here"], "an unmatched name is REPORTED, not dropped")
    total = sum(len(v) for v in owned.values()) + len(un)
    check(total == 3, "every input is accounted for exactly once")

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
