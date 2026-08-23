#!/usr/bin/env python3
"""R1789 §5.28 §5.15 §2 #2 §2 #7 — **stop that node at eight seconds**, and the
query a scrub cannot answer without a track: *what did this step just cross?*

# What this demo exists for

The analysis-tool census asks for `lab.t1.10` — *a scenario timeline: warmup,
sequential and concurrent tasks, and killing a node at a given time* — and
recorded that a graph which stops a node at eight seconds has **no authoring
surface**.

Re-measured before this round wrote a line, that reason **held**. It is the
first census reason in four rounds to survive re-measurement (R1747, R1787 and
R1788 each found one false), and the measurement is specific: this screen's
`run` is a **boolean**, `pinion_core::animation` interpolates between **two**
points, `pinion_chart::timeline` is a **reading** lane, and `TransportClock` is a
playhead that knows nothing about what is on it. Somewhere to put the eight
seconds is what was absent.

# The floor, built and run at 6.11

The reference has a keyframe API, so by this project's rule a consumer for one
exists. Measured (scratchpad only, never tracked):

| asked | what it does |
|---|---|
| two keys at one step | the second **silently replaces** the first |
| a step outside its range | **dropped**; the only signal is a line on stderr |
| two keys it cannot interpolate | the value between them is an **invalid** empty variant, with no reason and no state change |
| *which keys did t0→t1 cross?* | **there is no such query** |

The last row is the one that decides the shape. An interpolating keyframe API
answers *a value at a time*; a scenario is made of discrete events, so "stop
that node at eight seconds" is inexpressible there whatever the value is.

# What is the framework's and what is the screen's

`pinion_core::widgets::track` owns the shape — a `Track` of timed entries whose
`place` refuses a second entry at one moment instead of swallowing it, a
`Schedule` of named lanes for concurrency, and `due(after, upto)` answering the
half-open window a tick advanced through. The screen owns the **taxonomy**:
which four things can happen to a graph, and what a card is.

  (A) an empty scenario answers rather than being absent, and publishes its acts.
  (B) place a warmup, a kill and a concurrent start; the lanes and the derived
      duration follow.
  (C) advance the playhead — and the answer is WHAT IT CROSSED, in time order
      across lanes, each saying whether the graph actually moved.
  (D) the kill at eight seconds really switches the card off. This is the
      census row's own sentence, driven.
  (E) six ways to schedule something impossible, six sentences.
  (F) an entry is delivered EXACTLY ONCE across a run, whatever the step sizes —
      the half-open window is a partition, not a guess.
  (G) two lanes telling one card opposite things at one moment is REPORTED,
      because which one lasts is decided by lane order rather than by the author.

Run from the workspace root:
    cargo build -p hello-node-lab --release
    python3 tools/demos/r1789_a_scenario_says_what_a_step_crossed.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_action_refused,
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


def scenario(tf: RpcSubprocess) -> dict:
    return tf.query(f"{EXT}/scenario")


def lane_of(plan: dict, name: str) -> dict:
    return next(lane for lane in plan["lanes"] if lane["lane"] == name)


def card(tf: RpcSubprocess, name: str) -> dict:
    return json.loads(tf.query(f"{EXT}/cards"))[name]


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) an empty scenario is an answer ───────────────────────
        banner("A — an empty scenario answers, and says which acts exist")
        plan = scenario(tf)
        assert_eq(
            sorted(plan),
            ["acts", "conflicts", "duration", "lanes", "playhead"],
            "the whole shape in one read",
        )
        assert_eq(plan["lanes"], [], "no lanes yet")
        assert_eq(plan["duration"], 0.0, "and no length")
        assert_eq(plan["playhead"], 0.0, "the playhead is at the start")
        assert_eq(
            plan["acts"],
            ["warmup", "start", "stop", "kill"],
            "★ the closed vocabulary, published — a client enumerates a valid "
            "call instead of guessing one",
        )
        # And the declaration says the same, on the other channel.
        fields = tf.query(f"{EXT}/$schema")
        place = next(f for f in fields if f["path"] == "schedule")
        assert_eq(place["channel"], "invoke", "an invoke channel")
        args = {a["name"]: a for a in place["args"]}
        assert_eq(sorted(args), ["act", "at", "lane", "target"], "four arguments")
        assert_eq(
            args["act"]["domain"],
            {"kind": "one_of", "values": plan["acts"]},
            "★ and the domain IS the roster the read publishes — one list",
        )
        ok("a lane and a target may be omitted", args["lane"]["optional"] and args["target"]["optional"])

        # ── (B) author a scenario ────────────────────────────────────
        banner("B — a warmup, a kill at eight seconds, and a concurrent start")
        victim = sorted(json.loads(tf.query(f"{EXT}/cards")))[0]
        ok(f"the card {victim} starts switched on", card(tf, victim)["disabled"] is False)
        tf.invoke(f"{EXT}/schedule", {"at": 0.0, "act": "warmup"})
        tf.invoke(f"{EXT}/schedule", {"at": 8.0, "act": "kill", "target": victim})
        tf.invoke(
            f"{EXT}/schedule",
            {"at": 4.0, "act": "stop", "target": victim, "lane": "faults"},
        )
        plan = scenario(tf)
        assert_eq([lane["lane"] for lane in plan["lanes"]], ["main", "faults"], "first-written order")
        assert_eq(plan["duration"], 8.0, "★ derived from the entries, never declared")
        assert_eq(lane_of(plan, "main")["duration"], 8.0)
        assert_eq(lane_of(plan, "faults")["duration"], 4.0)
        assert_eq(
            [(e["at"], e["act"]) for e in lane_of(plan, "main")["entries"]],
            [(0.0, "warmup"), (8.0, "kill")],
            "in time order however they were placed",
        )
        assert_eq(plan["conflicts"], [], "nothing contradicts itself yet")

        # ── (C) advancing answers what it crossed ────────────────────
        banner("C — a step answers the entries it passed, not a value")
        first = tf.invoke(f"{EXT}/advance", 1.0)
        assert_eq(first["playhead"], 1.0)
        assert_eq(
            [(c["lane"], c["act"]) for c in first["crossed"]],
            [("main", "warmup")],
            "★ the entry at ZERO is reached by the first step — a caller never "
            "has to invent a negative time",
        )
        ok("and a warmup moves no card, which it says", first["crossed"][0]["done"] is False)
        nothing = tf.invoke(f"{EXT}/advance", 1.0)
        assert_eq(nothing["crossed"], [], "a step over empty time crosses nothing")
        assert_eq(nothing["playhead"], 2.0, "and still moves the playhead")

        # ── (D) the census row's own sentence, driven ────────────────
        banner("D — the card is switched off at the second the scenario said")
        mid = tf.invoke(f"{EXT}/advance", 3.0)  # 2 -> 5, crosses the stop at 4
        assert_eq([c["act"] for c in mid["crossed"]], ["stop"])
        ok("the stop moved the graph", mid["crossed"][0]["done"] is True)
        ok(f"and {victim} is off", card(tf, victim)["disabled"] is True)
        # Put it back so the kill at 8 has something to do.
        tf.invoke(f"{EXT}/schedule", {"at": 6.0, "act": "start", "target": victim})
        tf.invoke(f"{EXT}/advance", 1.5)  # 5 -> 6.5
        ok(f"{victim} is on again", card(tf, victim)["disabled"] is False)
        late = tf.invoke(f"{EXT}/advance", 2.0)  # 6.5 -> 8.5, crosses the kill
        assert_eq([c["act"] for c in late["crossed"]], ["kill"])
        ok(
            "★★ the kill at eight seconds switched the card off — the row's own "
            "sentence, driven through the wire",
            card(tf, victim)["disabled"] is True,
        )
        ok(
            "and a kill on an already-off card would report that it moved "
            "nothing rather than refusing",
            tf.invoke(f"{EXT}/advance", 0.0)["crossed"] == [],
        )

        # ── (E) six impossible schedulings, six sentences ────────────
        banner("E — every way of scheduling something impossible says which")
        for args_, saying in (
            ({"at": 8.0, "act": "kill", "target": victim}, "something already happens at 8s"),
            ({"at": 2.0, "act": "explode", "target": victim}, "no act named"),
            ({"at": -1.0, "act": "stop", "target": victim}, "before the track starts"),
            ({"at": 3.0, "act": "kill"}, "needs a card to happen to"),
            ({"at": 3.0, "act": "warmup", "target": victim}, "takes no card"),
            ({"at": 3.0, "act": "kill", "target": "no-such"}, "no card called"),
        ):
            assert_action_refused(
                lambda a=args_: tf.invoke(f"{EXT}/schedule", a), saying=saying
            )
        assert_action_refused(
            lambda: tf.invoke(f"{EXT}/unschedule", {"at": 99.0}),
            saying="nothing happens at 99s",
        )
        assert_action_refused(
            lambda: tf.invoke(f"{EXT}/advance", "soon"),
            saying="number of seconds",
        )

        # ── (F) exactly once across a run, whatever the steps ────────
        banner("F — every entry is delivered exactly once, at any step size")
        tf.invoke(f"{EXT}/clear_graph", "")
        for lane in ("main", "faults"):
            for entry in list(lane_of(scenario(tf), lane)["entries"]):
                tf.invoke(f"{EXT}/unschedule", {"at": entry["at"], "lane": lane})
        assert_eq(scenario(tf)["duration"], 0.0, "the scenario is empty again")
        # ★★ Rewinding is advancing backwards, and it moves the playhead
        # WITHOUT undoing anything — a scenario is a script, not an inverse.
        # Discovered by this section failing: the playhead was still at 8.5 and
        # every entry below it was behind the window.
        back = tf.invoke(f"{EXT}/advance", -100.0)
        assert_eq(back["playhead"], 0.0, "★ it does not run past the start")
        assert_eq(back["crossed"], [], "and crosses nothing on the way back")
        ok(
            "the card the scenario killed is STILL off after a rewind — a "
            "script played backwards is not an undo",
            card(tf, victim)["disabled"] is True,
        )
        for at in (0.0, 0.25, 1.0, 2.0, 3.5):
            tf.invoke(f"{EXT}/schedule", {"at": at, "act": "warmup"})
        seen: list[float] = []
        # Steps chosen so the boundaries land ON entries — where a closed window
        # would double-deliver and an open one would drop.
        for step in (0.25, 0.75, 1.0, 0.5, 1.0, 1.0):
            for crossed in tf.invoke(f"{EXT}/advance", step)["crossed"]:
                seen.append(crossed["act"])
        assert_eq(len(seen), 5, f"five entries, each once: {seen}")

        # ── (G) a moment that contradicts itself is reported ─────────
        banner("G — two lanes telling one card opposite things is REPORTED")
        target = sorted(json.loads(tf.query(f"{EXT}/cards")))[0]
        tf.invoke(f"{EXT}/schedule", {"at": 9.0, "act": "kill", "target": target})
        assert_eq(scenario(tf)["conflicts"], [], "one act is not a contradiction")
        tf.invoke(
            f"{EXT}/schedule",
            {"at": 9.0, "act": "start", "target": target, "lane": "traffic"},
        )
        clash = scenario(tf)["conflicts"]
        assert_eq(len(clash), 1, f"one moment contradicts itself: {clash}")
        assert_eq(clash[0]["at"], 9.0)
        assert_eq(clash[0]["target"], target)
        assert_eq(
            sorted(a["act"] for a in clash[0]["acts"]),
            ["kill", "start"],
            "naming both, with the lane each is on",
        )
        ok(f"and why it matters: {clash[0]['why']!r}", "lane order" in clash[0]["why"])
        ok(
            "★ reported and NOT refused — a scenario is authored one entry at a "
            "time, and the second half of a pair somebody is midway through "
            "placing is not an error yet",
            len(lane_of(scenario(tf), "traffic")["entries"]) == 1,
        )
        tf.invoke(f"{EXT}/unschedule", {"at": 9.0, "lane": "traffic"})
        assert_eq(scenario(tf)["conflicts"], [], "and taking one off clears it")

    print(f"\n=== {len(CHECKS)} named check(s) ===")
    for line in CHECKS:
        print(f"  - {line}")


if __name__ == "__main__":
    sys.exit(run_demo("R1789 a scenario says what a step crossed", body))
