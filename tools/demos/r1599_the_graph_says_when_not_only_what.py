#!/usr/bin/env python3
"""R1599 §5.38 §5.52 §2 #7 — the graph says WHEN, not only what.

A node graph has two kinds of edge and until this round `pinion-node-graph` had
one. A **value** link says what a node is built out of; a **control** link says
what runs next. They are not variations on each other — they obey *opposite*
laws, and the whole round falls out of that:

|             | input      | output     | cycle         |
|-------------|------------|------------|---------------|
| **value**   | at most 1  | unbounded  | a contradiction |
| **control** | unbounded  | at most 1  | a **loop**    |

A value has one producer and many readers. A control transfer has one successor
and many ways to arrive at it. That is `def`/`use` against
`terminator`/`predecessors` — SSA's own duality, and the reason a control-flow
graph has join points where a dataflow graph has fan-out.

`hello-node-flow` is the composition. It declares a taxonomy and paints it; it
contains no link law, no acyclicity test, no scheduler and no branch machinery,
because the framework owns all four.

What this script checks, and why each check discriminates:

* **The flow is declared per port**, and a node's shape is readable as such
  (`port_flows`), so "is this pin control?" is a fact rather than a convention.
  the engine 5.8.1 spells the same distinction as the *string* `"exec"` sitting in
  the pin's type slot, compared literally in 40 places outside the `IsExecPin`
  helper that exists for it.
* **The multiplicity INVERTS.** Wiring a second successor onto a control output
  displaces the first, and the displaced link is *named*. The engine displaces here
  too (`CONNECT_RESPONSE_BREAK_OTHERS_A`) and `TryCreateConnection` answers a
  bare `bool`, so there what it broke is gone.
* **A control input JOINS.** Two predecessors converge and both survive — a
  state the pre-R1599 model could not hold at all.
* **The planes never mix**, and the refusal says which end is control.
* **A control cycle is a LOOP**: `valid` stays `ok`, `cycle_nodes` stays empty,
  and `control_loops` names the members — statically, before anything runs.
  Nothing in the engine answers this: an exec loop compiles (exec pins are excluded
  from the dependency sort by `PinIsImportantForDependancies`), and a runaway
  one is found at *run time* by counting to `GMaximumScriptLoopIterations`.
* **The execution ORDER is derived**, and the run says why it stopped.
* **A branch takes one arm and the other never runs** — the fact a dataflow
  evaluator cannot express, because there every node has a value. Change the
  *datum* and the trace changes, so the control choice reads the value plane.
* **A pure node is never in the trace and its value is still there**, which is
  the engine's impure/pure split.
* **A fork runs each arm to completion before the next**, which is a stack
  property, not an ordering convention.

Run from the workspace root:
    cargo build -p hello-node-flow --release
    python3 tools/demos/r1599_the_graph_says_when_not_only_what.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    run_demo,
)

#: A widget's primary External is addressed by the framework path, not the tag.
EXT = "/external"

#: `hello-node-flow`'s seed, mirrored rather than imported — a demo that read
#: the fixture out of the code under test could not catch it changing.
#:
#: R1603.1 — and it DID change: R1600 gave the seed a register, a second task
#: and a group, so `drain` moved into a definition and the ids after `branch`
#: all shifted. This demo was not updated with it and went red in CI for two
#: rounds. The numbers below are the same ones `r1600_a_graph_remembers_per_
#: instance.py` mirrors, re-derived here from the live surface rather than read
#: off the source.
BEGIN, FORK, WARM, BRANCH, SETTLE, FINISH = 0, 1, 2, 3, 4, 6
BUMP, OVER, ELAPSED, STAGE = 7, 8, 9, 10


def q(tf: RpcSubprocess, path: str):
    return tf.query(f"{EXT}/{path}")


def inv(tf: RpcSubprocess, path: str, args):
    return tf.invoke(f"{EXT}/{path}", args)


def refused(tf: RpcSubprocess, path: str, args) -> None:
    """A verb that must not succeed. The refusal's *wording* is asserted
    separately, through `last_refusal`, so this only checks the outcome."""
    try:
        inv(tf, path, args)
    except AssertionError:
        pass
    except Exception:  # noqa: BLE001 - any refusal shape is fine here
        pass
    else:
        raise AssertionError(f"{path}({args!r}) was expected to be refused")


def ids(reply: str) -> list[str]:
    text = str(reply)
    return [piece for piece in text.split(",") if piece]


def body() -> None:
    with RpcSubprocess("hello-node-flow", boot_grace=1.5) as tf:
        # ── (A) the flow is a per-port declaration ──────────────────────────
        flows = dict(
            piece.split(":", 1) for piece in ids(q(tf, "port_flows"))
        )
        assert_eq(flows[str(BEGIN)], ">c", "A: Begin takes nothing and emits control")
        assert_eq(
            flows[str(WARM)],
            "c>cv",
            "A: a Task takes control and emits BOTH — control and a value — "
            "which is what makes the two planes one graph rather than two",
        )
        assert_eq(flows[str(FORK)], "c>cc", "A: a fork emits two control successors")
        assert_eq(flows[str(BRANCH)], "cv>cc", "A: a branch reads a VALUE to choose")
        assert_eq(
            flows[str(OVER)],
            "vv>v",
            "A: and a pure node has no control port at all — Unreal's impure/"
            "pure split, here readable off the signature",
        )
        assert_eq(
            q(tf, "pure_nodes"),
            f"{BUMP},{OVER},{ELAPSED}",
            "A: derived, not listed",
        )
        assert_eq(q(tf, "valid"), "ok", "A: the seed is a valid document")

        # ── (B) the seed holds a LOOP, and that is not a defect ─────────────
        assert_eq(
            q(tf, "control_loops"),
            f"{WARM},{BRANCH}",
            "B: the loop's members are named, statically, before anything runs",
        )
        assert_eq(
            q(tf, "cycle_nodes"),
            "",
            "B: and it is NOT a dependency cycle — the value plane is acyclic",
        )
        assert_eq(
            q(tf, "valid"),
            "ok",
            "B: so it is not a violation either. An execution loop is a legal "
            "graph, which is the single thing a dataflow model cannot say",
        )
        assert_eq(q(tf, "entries"), str(BEGIN), "B: one entry, derived from arity")

        # ── (C) the order is DERIVED ────────────────────────────────────────
        assert_eq(q(tf, "budget"), 24, "C: the caller's bound, not a constant")
        assert_eq(
            q(tf, "trace"),
            f"{BEGIN},{FORK},{WARM},{BRANCH},{WARM},{BRANCH},{WARM},{BRANCH},"
            f"{WARM},{BRANCH},{WARM},{BRANCH},{WARM},{BRANCH},{WARM},{BRANCH},"
            f"{WARM},{BRANCH},{WARM},{BRANCH},{WARM},{BRANCH},{WARM},{BRANCH}",
            "C: the loop runs, and a node that ran twice appears twice",
        )
        assert_eq(q(tf, "steps"), 24, "C: exactly the budget")
        assert_eq(
            q(tf, "stop"),
            "budget_exhausted",
            "C: and WHY it stopped is a value. Unreal reaches this condition at "
            "run time, in a shipped build, as an InfiniteLoop exception",
        )
        assert_eq(
            q(tf, "never_ran"),
            f"{SETTLE},{FINISH},{BUMP},{OVER},{ELAPSED},{STAGE}",
            "C: arm 1 never ran because arm 0 never completed — 'to completion, "
            "then the next' is a stack property, and it is observable",
        )

        # ── (D) the budget is the caller's ──────────────────────────────────
        assert_eq(inv(tf, "set_budget", 5), 5, "D: shorten it")
        assert_eq(q(tf, "steps"), 5, "D: and the run is shorter")
        assert_eq(q(tf, "stop"), "budget_exhausted", "D: still running when cut")
        assert_eq(inv(tf, "set_budget", 24), 24, "D: put it back")

        # ── (E) the branch reads the VALUE plane ────────────────────────────
        assert_eq(
            inv(tf, "set_reading", f"{OVER},3"),
            3,
            "E: make the branch's condition a constant under the limit — a "
            "datum, not a wire",
        )
        assert_eq(
            q(tf, "stop"),
            "halted",
            "E: and the SAME graph now terminates. The control choice is a "
            "function of what arrived, resolved through the value plane",
        )
        assert_eq(
            q(tf, "trace"),
            f"{BEGIN},{FORK},{WARM},{BRANCH},{SETTLE},{FINISH},5,{BUMP},{FINISH}",
            "E: arm 0 completed, THEN arm 1 ran — the sequence semantics, now "
            "visible because the loop exits. The bare 5 and 7 are INSIDE the "
            "Stage definition, where ids are that tree's own (R1600)",
        )
        assert_eq(
            q(tf, "never_ran"),
            f"{OVER},{ELAPSED},{STAGE}",
            "E: and what never ran are the PURE nodes — pulled, not run, so "
            "they are not in an order at all — plus the group INSTANCE, which "
            "takes no turn of its own: entering it shows as the first step "
            "inside it",
        )
        assert_eq(
            q(tf, "control_loops"),
            f"{WARM},{BRANCH}",
            "E: the loop is still THERE — a loop that is not taken is still a "
            "loop, and the static answer does not depend on the run",
        )

        assert_eq(
            q(tf, "links"),
            10,
            "E: and the swap COST a wire, which is the honest price: a Reading "
            "has no inputs, so the value feeding the condition had nowhere to "
            "land and set_kind severed it rather than dropping it in silence",
        )

        # ── (F) a control output takes ONE successor ────────────────────────
        assert_eq(inv(tf, "reset", 0), "reset", "F: back to the seed first")
        assert_eq(q(tf, "links"), 11, "F: the seed's wires")
        assert_eq(
            inv(tf, "wire", f"{BEGIN}.0,{FINISH}.0"),
            f"linked, displacing node {BEGIN}.0->node {FORK}.0",
            "F: THE INVERSION. Begin already had a successor, and a control "
            "output has exactly one — so the first gave way, and it is NAMED. "
            "Unreal displaces here too and returns a bare bool",
        )
        assert_eq(q(tf, "links"), 11, "F: one went as one came")
        assert_eq(
            q(tf, "trace"),
            f"{BEGIN},{FINISH}",
            "F: and the order changed to match",
        )
        assert_eq(inv(tf, "reset", 0), "reset", "F: back to the seed")

        # ── (G) a control input JOINS ───────────────────────────────────────
        #
        # The seed ALREADY contains a join: `Task warm`'s control input is fed
        # by both `Fork.0` and the loop's `Branch.True`. That is a state the
        # pre-R1599 model could not hold, so it is asserted before anything is
        # added — a capability the fixture exercises by existing.
        assert_eq(
            inv(tf, "wire", f"{SETTLE}.0,{FINISH}.0"),
            "linked",
            "G: nothing gives way — `Task settle`'s control output is FREE, and "
            "`Finish`'s control input already has the Stage on it, so the "
            "second predecessor JOINS. The exact mirror of the value rule",
        )
        assert_eq(q(tf, "links"), 12, "G: so the count GREW, where F's did not")
        assert_eq(q(tf, "valid"), "ok", "G: two predecessors is a legal state")
        assert_eq(
            inv(tf, "set_reading", f"{OVER},3"),
            3,
            "G: and now the loop exits, so both paths are walked",
        )
        assert_eq(
            q(tf, "trace"),
            f"{BEGIN},{FORK},{WARM},{BRANCH},{SETTLE},{FINISH},{FINISH},5,"
            f"{BUMP},{FINISH}",
            "G: `Finish` runs more than once, reached down different paths — "
            "which is what a join is for, and what a value input could never do",
        )
        assert_eq(inv(tf, "reset", 0), "reset", "G: back to the seed")

        # ── (H) the planes never mix ────────────────────────────────────────
        refused(tf, "wire", f"{BEGIN}.0,{OVER}.0")
        assert_eq(
            q(tf, "last_refusal"),
            f"node {BEGIN}.0 carries control and node {OVER}.0 carries a value",
            "H: an execution wire cannot feed a number, and the refusal says "
            "WHICH end is control — its own arm, because there is no type on "
            "the control end to report as mismatched",
        )
        refused(tf, "wire", f"{OVER}.0,{WARM}.0")
        assert_eq(
            q(tf, "last_refusal"),
            f"node {WARM}.0 carries control and node {OVER}.0 carries a value",
            "H: and the other direction, named the same way",
        )
        assert_eq(q(tf, "links"), 11, "H: neither refusal touched the document")

        # ── (I) a VALUE cycle is still refused ──────────────────────────────
        refused(tf, "wire", f"{OVER}.0,{OVER}.0")
        assert_eq(q(tf, "valid"), "ok", "I: the value plane keeps its old law")
        assert_eq(
            q(tf, "cycle_nodes"), "", "I: and stays free of dependency cycles"
        )

        # ── (J) bypass passes CONTROL through ───────────────────────────────
        assert_eq(inv(tf, "bypass", WARM), "was false", "J: take a step out")
        assert_eq(
            q(tf, "stop"),
            "budget_exhausted",
            "J: the loop still loops — a bypassed node is the identity as far "
            "as its signature allows, and that now includes the flow",
        )
        assert_eq(
            q(tf, "trace").split(",")[:3],
            [str(BEGIN), str(FORK), str(WARM)],
            "J: and control still passes THROUGH it rather than stopping",
        )
        assert_eq(inv(tf, "reset", 0), "reset", "J: back to the seed")
        assert_eq(q(tf, "steps"), 24, "J: and the seed's run is restored")


if __name__ == "__main__":
    run_demo("r1599_the_graph_says_when_not_only_what", body)
