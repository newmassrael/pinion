#!/usr/bin/env python3
"""R1731 §5.27 §5.38 §5.40 §2 #2 §2 #7 — **the log section is built, and with it
the analysis tool's rail reproduces the reference completely.**

# What this demo exists for

R1728 wrote the tool's navigation down as a reviewed artifact and made something
fail when the application stopped matching it. It began with **five** of eight
seats reproduced and a declared remainder of three. R1729 mounted the capture
viewer, R1730 built the key-pattern section, and this round builds the log
section — the last one owed.

So the claim is one a demo can state exactly: `docs/analyzer-rail-spec.json`'s
`owed` list is **empty**, and the running application says so itself.

What this drives:

* **A** — the log section's own three surfaces, compared with
  `docs/analyzer-logs-spec.json` read here from the repository. Both sides of
  every comparison come from different places, or the comparison is the
  application agreeing with itself.
* **B** — the two narrowings, which is the shape this section has and its
  sibling does not: a severity choice that is **exclusive and ordered**, a live
  filter, and a hidden event that says WHICH of the two dropped it.
* **C** — an event whose frame never arrived. The reference draws that case; a
  byte pane that simply went blank would be indistinguishable from a decode that
  failed.
* **D** — the machine's own pointer, pressing every event and every severity.
* **E** — and the rail, closed: the shell reproduces 8 of 8, its declared
  remainder is empty, and the section paints inside the host's chrome.

# Floor, measured by building a probe against 6.11.1 and running it

The conformance rows are R1730's and unchanged — a column has no key, no member
of the table view, the header or the model names a specification, and a check
written against the model passes while the reader is looking at a different
order. The row this section adds is the severity choice: a button group there is
exclusive by a bool on the group, and *which* member is on is an integer id with
no ordering among the members — so "warnings and worse" is a rule the widget
cannot hold and every consumer re-implements.

Run from the workspace root (a real pointer needs a display):
    cargo build --release -p hello-log-view -p hello-analyzer-shell
    DISPLAY=:97 python3 tools/demos/r1731_a_log_section_closes_the_rail.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from analyzer_spec import DOCS, rail_spec, surfaces  # noqa: E402
from rpc_verify import (  # noqa: E402
    RealPointer,
    RealPointerUnavailable,
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    run_demo,
)

SECTION = "hello-log-view"
SHELL = "hello-analyzer-shell"
EXT = "/external"
LOGS_SPEC_PATH = DOCS / "analyzer-logs-spec.json"

CHECKS: list[str] = []
REAL_POINTER_RUNS = 0


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def ok(what: str, condition: bool) -> None:
    CHECKS.append(what)
    assert condition, what


def q(app: RpcSubprocess, path: str):
    return app.query(f"{EXT}/{path}")


def logs_spec() -> dict:
    """The section's own reviewed artifact, read from the repository."""
    return json.loads(LOGS_SPEC_PATH.read_text(encoding="utf-8"))


def pointer(app: RpcSubprocess):
    global REAL_POINTER_RUNS
    try:
        driver = RealPointer(app)
    except RealPointerUnavailable as exc:
        print(f"[real-pointer] UNAVAILABLE — section D is not driven: {exc}")
        return None
    REAL_POINTER_RUNS += 1
    return driver


def section_a(app: RpcSubprocess, spec: dict) -> None:
    banner("A — the section says how much of its specification it is")
    verdict = q(app, "conformance")
    # ★ R1758 — the slot publishes the whole verdict, qualifier first.
    ok(
        "A: ★★★★★ and it says what it was read from -- this section judges the "
        "frame it painted, not the tables it holds",
        verdict["evidence"] == "paint",
    )
    conformance = verdict["surfaces"]
    ok(
        "A: it reports every surface the specification fixes",
        sorted(conformance) == surfaces(spec),
    )
    for surface in surfaces(spec):
        canon = spec[surface]["canon"]
        owed = spec[surface]["owed"]
        row = conformance[surface]
        assert_eq(
            row["specified"], len(canon), f"A: the {surface} surface counts its parts"
        )
        assert_eq(
            [d["says"] for d in row["divergences"]],
            [entry["sentence"] for entry in owed],
            f"A: ★★ the {surface} surface's difference from the reference is "
            "EXACTLY what somebody wrote down -- so a divergence quietly paid "
            "off fails here too",
        )
        assert_eq(row["unreconciled"], [], f"A: the {surface} surface reconciles")

    published = q(app, "spec")
    assert_eq(
        [c["key"] for c in published["columns"]],
        [p["key"] for p in spec["columns"]["canon"]],
        "A: the columns the SCREEN publishes are the specified columns, in the "
        "specified order",
    )
    assert_eq(
        [p["key"] for p in published["detail"]],
        [p["key"] for p in spec["detail"]["canon"]],
        "A: and so are the decode pane's six parts",
    )
    ok(
        "A: ★ the severity VOCABULARY is published beside the controls -- an "
        "agent given only the controls could not tell whether a severity it saw "
        "on a row is one it can filter to",
        published["severity_vocabulary"] == ["info", "warn", "error"],
    )


def section_b(app: RpcSubprocess) -> None:
    banner("B — two narrowings, and a hidden event says which one dropped it")
    app.invoke(f"{EXT}/choose_severity", "all")
    app.invoke(f"{EXT}/filter", "")
    app.tick(8)
    everything = q(app, "kept_rows")
    assert_eq(len(everything), int(q(app, "row_count")), "B: everything is shown")

    app.invoke(f"{EXT}/choose_severity", "warn")
    app.tick(8)
    warn = q(app, "kept_rows")
    app.invoke(f"{EXT}/choose_severity", "error")
    app.tick(8)
    error = q(app, "kept_rows")
    ok(
        "B: ★★★ a severity choice keeps that severity AND WORSE -- errors are a "
        "subset of warnings, which is what an ordering means and what three "
        "independent toggles could not say",
        set(error) < set(warn) < set(everything),
    )
    assert_eq(q(app, "severity"), "error", "B: and the choice is exclusive")

    app.invoke(f"{EXT}/choose_severity", "warn")
    app.invoke(f"{EXT}/filter", "source in (P-03)")
    app.tick(8)
    kept = q(app, "kept_rows")
    ok("B: both narrowings apply, not only the last one set", len(kept) == 1)
    hidden = q(app, "why_hidden")
    ok(
        "B: ★★ some events went for the severity, and the reader is told so",
        any(row["severity"] for row in hidden),
    )
    ok(
        "B: ★★ and others for the query, which is a different thing to undo -- "
        "the question the floor answers with an invalid index and nothing else",
        any(row["clause"] for row in hidden),
    )
    app.invoke(f"{EXT}/choose_severity", "all")
    app.invoke(f"{EXT}/filter", "")
    app.tick(8)


def section_c(app: RpcSubprocess) -> None:
    banner("C — an event whose frame never arrived says so")
    rows = q(app, "spec")["rows"]
    empty = next(n for n, row in enumerate(rows) if row["bytes"] == 0)
    app.invoke(f"{EXT}/select_event", empty)
    app.tick(8)
    record = q(app, "record")
    assert_eq(record["bytes"], 0, "C: the selected event carries no frame")
    assert_eq(record["severity"], "warn", "C: and it is the warning that timed out")
    tree = {n["tag"]: n for n in app.request("scene/access").result["nodes"]}
    reading = str(tree["lv.detail.bytes"].get("value"))
    ok(
        "C: ★★ a reader is TOLD the frame never arrived rather than handed an "
        "empty block -- which would be indistinguishable from a decode that "
        "failed",
        "no frame" in reading,
    )
    # And a frame that did arrive reads as bytes.
    full = next(n for n, row in enumerate(rows) if row["bytes"] > 0)
    app.invoke(f"{EXT}/select_event", full)
    app.tick(8)
    tree = {n["tag"]: n for n in app.request("scene/access").result["nodes"]}
    ok(
        "C: and an event that did carry one reads as bytes",
        "bytes" in str(tree["lv.detail.bytes"].get("value")),
    )


def section_d(app: RpcSubprocess, spec: dict) -> None:
    banner("D — the list and the choice, pressed by the machine's own pointer")
    rects = abs_rects_of(app.snapshot(source="paint"))
    lefts = []
    for column in spec["columns"]["canon"]:
        tag = f"lv.column.{column['key']}"
        ok(f"D: the {column['key']} column is painted", tag in rects)
        lefts.append(rects[tag][0])
    ok(
        "D: ★ and they run left to right in the specified order",
        lefts == sorted(lefts) and len(set(lefts)) == len(lefts),
    )

    driver = pointer(app)
    if driver is None:
        return
    with driver as hand:
        app.invoke(f"{EXT}/choose_severity", "all")
        app.tick(8)
        rects = abs_rects_of(app.snapshot(source="paint"))
        pressed = 0
        for n in range(int(q(app, "row_count"))):
            tag = f"lv.list.row.{n}"
            if tag not in rects:
                continue
            rect = rects[tag]
            hand.move((rect[0] + rect[2] / 2, rect[1] + rect[3] / 2))
            hand.press()
            hand.release()
            app.tick(16)
            assert_eq(int(q(app, "selected_row")), n, f"D: a real press decodes event {n}")
            pressed += 1
        ok(f"D: all {pressed} painted events took a real press", pressed >= 8)

        for choice in q(app, "spec")["severities"]:
            rect = rects[f"lv.severity.{choice['key']}"]
            hand.move((rect[0] + rect[2] / 2, rect[1] + rect[3] / 2))
            hand.press()
            hand.release()
            app.tick(16)
            assert_eq(
                q(app, "severity"),
                choice["key"],
                f"D: a real press on {choice['key']} chooses it",
            )
        app.invoke(f"{EXT}/choose_severity", "all")
        app.tick(8)


def section_e(spec: dict) -> None:
    banner("E — the rail, closed")
    rail = rail_spec()
    ok(
        "E: ★★★★★ the rail's declared remainder is EMPTY -- every section the "
        "reference opens is one this application opens. It was three at R1728",
        rail["owed"] == [],
    )
    with RpcSubprocess(SHELL, boot_grace=1.5) as shell:
        conformance = shell.query(f"{EXT}/conformance")
        assert_eq(
            conformance["reproduced"],
            len(rail["canon"]),
            "E: ★★★ the shell reproduces every seat of the specified rail",
        )
        assert_eq(conformance["divergences"], [], "E: with no difference at all")
        # ★ What is still shut is shut because the REFERENCE defers it.
        shut = [s["key"] for s in rail["canon"] if s["standing"] == "closed"]
        rows = {
            row["tag"].rsplit(".", 1)[1]: row
            for row in shell.request("scene/disabled", {}).result["disabled"]
            if row["tag"].startswith("shell.rail.")
        }
        assert_eq(
            sorted(rows),
            sorted(shut),
            "E: ★★ and the only shut seats are the ones the reference draws "
            "locked itself",
        )
        ok(
            "E: ★★★★★ every one of them says `reserved` -- this rail can no "
            "longer SPELL 'specified and not built', because nothing "
            "constructs that arm and the compiler said so",
            {row["reason"] for row in rows.values()} == {"reserved"},
        )

        shell.intervene(f"{EXT}/nav", "logs")
        shell.tick(16)
        assert_eq(shell.query(f"{EXT}/nav"), "logs", "E: the fourth seat opens")
        rects = abs_rects_of(shell.snapshot(source="paint"))
        ok(
            "E: arriving paints the section inside the host",
            any(tag.startswith("lv.") for tag in rects),
        )
        for chrome in ("shell.appbar", "shell.rail", "shell.rail.logs"):
            ok(f"E: and the host's {chrome} survives -- a page, not a takeover", chrome in rects)
        ok(
            "E: ★ every column of the specified list is painted in the host too",
            all(f"lv.column.{c['key']}" in rects for c in spec["columns"]["canon"]),
        )
        shell.intervene(f"{EXT}/nav", "dashboard")
        shell.tick(16)
        rects = abs_rects_of(shell.snapshot(source="paint"))
        ok(
            "E: ★★ and leaving takes it away",
            not any(tag.startswith("lv.") for tag in rects),
        )


def body() -> None:
    spec = logs_spec()
    named = surfaces(spec)
    ok("the specification fixes three surfaces", len(named) == 3)
    ok(
        "and every one of them declares an ordered roster of named parts",
        all(
            [p["ordinal"] for p in spec[s]["canon"]] == list(range(1, len(spec[s]["canon"]) + 1))
            for s in named
        ),
    )

    with RpcSubprocess(SECTION, boot_grace=1.5, visible_window=True) as app:
        section_a(app, spec)
        section_b(app)
        section_c(app)
        section_d(app, spec)

    section_e(spec)

    banner("what was checked")
    for line in CHECKS:
        print(f"  · {line}")
    print(
        f"\n[coverage] {REAL_POINTER_RUNS} real-pointer session(s) contributed to "
        f"this run; {len(CHECKS)} named check(s) plus the assert_eq comparisons above."
    )
    if REAL_POINTER_RUNS == 0:
        print(
            "[coverage] ⚠ section D's presses did NOT run on this host. The run "
            "is shorter than it looks and this line is the only evidence."
        )


if __name__ == "__main__":
    run_demo("R1731 a log section closes the rail", body)
