#!/usr/bin/env python3
"""R1866 §5.2 §5.7 — **two runs of one graph, compared on order and latency.**

# What this demo exists for

The analysis-tool census carries `lab.t2.17` — *scenario diff and regression:
two runs of one graph compared on order and latency distribution* — with the
verdict `app`, and its covering sentence said the substrate was already here:
`Run::trace` is comparable and the chart crate has the distributions, so the
comparison is the application's.

**Re-measured at R1866 before a line was written, the middle was missing.** A
run's `Step` carries which node ran and what it produced and *nothing about
when*; the lab's own value type is a locator string. Both named halves were
real and there was nothing to join them with — so the round built the join:
`pinion_core::regression` (what changed between two timelines) and this
screen's `record` / `regression`, which are that mechanism driven by a scenario
clock this screen already had.

**Order and latency are one axis at two scales.** An order is a sequence of
marks at logical times and a latency profile is the same sequence at physical
ones. Writing two comparators would be writing one rule twice, so a `Timeline`
carries its `Scale` and one comparison answers both — and two timelines at
different scales are REFUSED, because a shift of `2` is two steps or two seconds
and the number does not say which.

# What is shown

  (A) with nothing kept, the screen says so — `baseline: null` and a reason,
      which a client can tell from *nothing changed*.
  (B) a run is recorded, and comparing that run with itself is CLEAN: every
      mark held, nothing gained, lost or shifted.
  (C) ★ the same plan with one act moved reports exactly that act as shifted,
      by exactly the amount it moved, and everything else as held.
  (D) ★ an act ADDED is `gained` and an act REMOVED is `lost`, and the four
      groups partition both runs — recomputed here from the crossings alone,
      so this is a second opinion rather than a reading of the answer.
  (E) ★ the latency DISTRIBUTION: five landmarks over the shifts, which is the
      half of the census row a list of differences does not answer.
  (F) an empty tape cannot be recorded, because a baseline of nothing reports
      every later run as pure gain — a finding that would be an artefact of
      when somebody pressed the button.

Run from the workspace root:
    cargo build -p hello-node-lab --release
    python3 tools/demos/r1866_two_runs_of_one_graph_are_compared.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    run_demo,
)

EXAMPLE = "hello-node-lab"
EXT = "/external"

CHECKS: list[str] = []


def banner(text: str) -> None:
    print(f"\n=== {text} ===")


def ok(what: str, condition: bool) -> None:
    CHECKS.append(what)
    assert condition, what


def regression(tf: RpcSubprocess) -> dict[str, Any]:
    """What the screen says about this run against the kept one."""
    return tf.query(f"{EXT}/regression")


def plan_of(tf: RpcSubprocess) -> dict[str, Any]:
    return tf.query(f"{EXT}/scenario")


def cards(tf: RpcSubprocess) -> list[str]:
    """The graph's card names, READ rather than written down here.

    ⚠ `schedule` refuses a target the graph does not have — measured on this
    demo's first run, which is the check doing its job. Naming three cards in
    this file would have made the demo a claim about the opening graph as well
    as about the comparison, and the opening graph is another round's subject.
    """
    said = tf.query(f"{EXT}/spec")
    doc = json.loads(said) if isinstance(said, str) else said
    names = [n["id"] for n in doc["nodes"]]
    assert len(names) >= 4, f"the opening graph has {len(names)} card(s)"
    return names


def clear_plan(tf: RpcSubprocess) -> None:
    """Take every entry off, so each act of this demo starts from one place."""
    for lane in plan_of(tf)["lanes"]:
        for entry in list(lane["entries"]):
            tf.invoke(f"{EXT}/unschedule", {"lane": lane["lane"], "at": entry["at"]})


def schedule(tf: RpcSubprocess, act: str, target: str, at: float) -> None:
    tf.invoke(f"{EXT}/schedule", {"act": act, "target": target, "at": at})


def run_to(tf: RpcSubprocess, end: float) -> list[dict[str, Any]]:
    """Play the plan from the beginning to `end`, and answer what was crossed.

    ⚠ From the BEGINNING every time: `advance` restarts the tape when the
    playhead is at zero, and a comparison of two runs is only a comparison if
    each one is a whole run.
    """
    tf.invoke(f"{EXT}/advance", -1000.0)
    said = tf.invoke(f"{EXT}/advance", end)
    return said["crossed"]


def marks_of(crossed: list[dict[str, Any]]) -> list[tuple[str, float]]:
    """The run, rebuilt from the crossings — the demo's own second opinion."""
    return sorted(((f"{c['act']} {c['target']}", c["at"]) for c in crossed))


def a_nothing_kept(tf: RpcSubprocess) -> None:
    banner("A — with nothing kept, the screen says so")
    said = regression(tf)
    assert_eq(said["baseline"], None, "A: no run has been kept yet")
    ok(
        "A: ★ and it says WHY, so a client can tell 'nothing to compare with' "
        f"from 'nothing changed' — {said['why']!r}",
        isinstance(said.get("why"), str) and said["why"],
    )
    ok(
        "A: a run that has not been kept cannot be recorded either",
        "clean" not in said,
    )


def b_a_run_against_itself(tf: RpcSubprocess, seat: list[str]) -> list[dict[str, Any]]:
    banner("B — a run compared with itself is clean")
    clear_plan(tf)
    schedule(tf, "start", seat[0], 1.0)
    schedule(tf, "stop", seat[1], 4.0)
    schedule(tf, "kill", seat[2], 9.0)
    first = run_to(tf, 12.0)
    assert_eq(len(first), 3, "B: the plan's three acts were crossed")
    print(f"  [run 1] {marks_of(first)}")

    kept = tf.invoke(f"{EXT}/record", None)
    assert_eq(kept["kept"], 3, "B: three marks were kept")

    again = run_to(tf, 12.0)
    assert_eq(marks_of(again), marks_of(first), "B: the same plan ran the same way")
    said = regression(tf)
    ok(
        f"B: ★★ the same run against itself is CLEAN — {said['sentence']!r}",
        said["clean"] is True,
    )
    assert_eq(said["held"], 3, "B: every mark held")
    assert_eq(said["shifted"], [], "B: nothing shifted")
    return first


def c_one_act_moved(tf: RpcSubprocess, seat: list[str]) -> None:
    banner("C — one act moved reports exactly that act, by exactly that much")
    # The kill moves four seconds later; nothing else changes.
    tf.invoke(f"{EXT}/unschedule", {"lane": "main", "at": 9.0})
    schedule(tf, "kill", seat[2], 13.0)
    crossed = run_to(tf, 16.0)
    said = regression(tf)

    assert_eq(said["gained"], [], "C: nothing was added")
    assert_eq(said["lost"], [], "C: nothing was removed")
    assert_eq(said["held"], 2, "C: the two untouched acts held")
    assert_eq(len(said["shifted"]), 1, "C: exactly one act moved")
    moved = said["shifted"][0]
    assert_eq(moved["name"], f"kill {seat[2]}", "C: and it is the one that moved")
    ok(
        f"C: ★★★ by exactly the amount it was moved — {moved['by']:+.1f}s",
        abs(moved["by"] - 4.0) < 1e-6,
    )
    # ★ The second opinion: the amount, recomputed from the crossings.
    now = dict(marks_of(crossed))
    ok(
        "C: ★ and the crossings say the same, recomputed here rather than read "
        "off the answer",
        abs((now[f"kill {seat[2]}"] - 9.0) - moved["by"]) < 1e-6,
    )
    print(f"  [regression] {said['sentence']}")


def d_added_and_removed(tf: RpcSubprocess, seat: list[str]) -> None:
    banner("D — added is gained, removed is lost, and the groups partition")
    tf.invoke(f"{EXT}/unschedule", {"lane": "main", "at": 4.0})
    schedule(tf, "start", seat[3], 2.0)
    schedule(tf, "stop", seat[3], 6.0)
    crossed = run_to(tf, 16.0)
    said = regression(tf)

    gained = sorted(m["name"] for m in said["gained"])
    lost = sorted(m["name"] for m in said["lost"])
    assert_eq(
        gained,
        sorted([f"start {seat[3]}", f"stop {seat[3]}"]),
        "D: the two new acts",
    )
    assert_eq(lost, [f"stop {seat[1]}"], "D: the act that was taken off")
    ok(
        "D: ★★★★★ the four groups partition BOTH runs, which is what makes the "
        "totals a check rather than a summary",
        len(said["lost"]) + len(said["shifted"]) + said["held"] == said["baseline"]
        and len(said["gained"]) + len(said["shifted"]) + said["held"] == said["now"],
    )
    assert_eq(said["now"], len(crossed), "D: `now` is this run's mark count")
    print(f"  [regression] {said['sentence']}")


def e_the_distribution(tf: RpcSubprocess, seat: list[str]) -> None:
    banner("E — the latency distribution, which a list of differences is not")
    # Three acts, each moved by a different amount, so the summary has a shape.
    clear_plan(tf)
    schedule(tf, "start", seat[0], 1.0)
    schedule(tf, "start", seat[1], 2.0)
    schedule(tf, "start", seat[2], 3.0)
    run_to(tf, 8.0)
    tf.invoke(f"{EXT}/record", None)
    clear_plan(tf)
    schedule(tf, "start", seat[0], 2.0)
    schedule(tf, "start", seat[1], 4.0)
    schedule(tf, "start", seat[2], 9.0)
    run_to(tf, 12.0)
    said = regression(tf)

    shifts = sorted(m["by"] for m in said["shifted"])
    assert_eq([round(s, 3) for s in shifts], [1.0, 2.0, 6.0], "E: three moves")
    dist = said["distribution"]
    ok(
        f"E: ★★★★★ the shifts are summarised into five landmarks — "
        f"{dist['lower']:.2f} / {dist['q1']:.2f} / {dist['median']:.2f} / "
        f"{dist['q3']:.2f} / {dist['upper']:.2f}",
        dist["samples"] == 3 and abs(dist["median"] - 2.0) < 1e-6,
    )
    ok(
        "E: ★ and the landmarks are in ascending order, which is what makes "
        "them a summary rather than five numbers",
        dist["lower"] <= dist["q1"] <= dist["median"] <= dist["q3"] <= dist["upper"],
    )
    ok(
        "E: ★★ the worst move reaches the sentence a person reads",
        "kill" not in said["sentence"] and "+6.000s" in said["sentence"],
    )
    print(f"  [distribution] {dist}")


def f_an_empty_run_cannot_be_kept(tf: RpcSubprocess) -> None:
    banner("F — an empty tape cannot be recorded")
    tf.invoke(f"{EXT}/advance", -1000.0)
    refused = None
    try:
        tf.invoke(f"{EXT}/record", None)
    except Exception as why:  # noqa: BLE001 — the refusal is the assertion
        refused = str(why)
    ok(
        "F: ★★ recording nothing is REFUSED, because a baseline of nothing "
        f"reports every later run as pure gain — {refused!r}",
        refused is not None and "nothing has been crossed" in refused,
    )


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        seat = cards(tf)
        print(f"[demo] the opening graph's cards: {seat}")
        a_nothing_kept(tf)
        b_a_run_against_itself(tf, seat)
        c_one_act_moved(tf, seat)
        d_added_and_removed(tf, seat)
        e_the_distribution(tf, seat)
        f_an_empty_run_cannot_be_kept(tf)
    print(f"\n[demo] {len(CHECKS)} named check(s)")


run_demo("R1866 two runs of one graph are compared", body)
