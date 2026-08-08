#!/usr/bin/env python3
"""R1600 §5.38 §5.52 §2 #7 — a graph that remembers, per instance.

R1599 made a control **loop** authorable and left the reason it could not yet
mean anything: `NodeKind::evaluate` is a pure function of the resolved inputs,
so a node that ran twice computed the same thing twice. Nothing accumulated,
nothing counted, nothing converged — the loop in `hello-node-flow`'s seed ran
until the step budget ran out, every time, and no edit short of rewiring could
change that.

What a second pass needs is a value **one step behind**: SSA's φ at a loop
header, Lustre's `pre`, Simulink's Unit Delay, a hardware register. That is
`NodeBody::Delay`, and everything below follows from two facts about it.

**It is the only node a value cycle may pass through.** Its output this step is
not a function of its input this step, so it CUTS the dependency graph — which
is exactly Lustre's causality rule ("every cycle must be broken by a `pre`"),
and here it is one predicate that `connect`, `cycle_nodes` and `validate` all
read, so they cannot disagree about which cycles are legal.

**Its register belongs to an INSTANCE, not to a node.** A definition's tree is
shared by every instance of it, so two instances of one counting group must
count separately. That is the same key the evaluator's memo has used since
R1577 — and it is what let control finally descend into a group this round,
because entering an instance and remembering inside one need the same address.

What this script checks, and why each check discriminates:

* **The register is a node with a type, and the graph is the same size however
  long it runs.** Blender's Repeat Zone materialises one copy of the body per
  iteration (`geometry_nodes_repeat_zone.cc`: *"the graph is built with as many
  body copies as there are iterations"*), so its count has to be known before
  the graph is built and a data-dependent exit is inexpressible there.
* **A value cycle closes through the register and nowhere else.**
* **A run READS the machine and a tick MOVES it.** Four runs leave the
  registers where they were; one tick does not. That split is what keeps a
  tick's outcome a function of the document and the registers rather than of
  the walk — Unreal takes the other road, where state is a Blueprint *variable*
  written by an execution wire, so which value a read sees depends on where
  control happened to go.
* **The trace changes between ticks with no edit at all**, which is the whole
  point: the same graph terminates once the world has moved.
* **Control descends into a group instance and comes back out**, and the steps
  inside are attributed to the instance. Unreal has no equivalent because it
  has no instance: `FKismetCompilerContext` expands a macro by calling
  `FEdGraphUtilities::CloneGraph`, so its N uses are N copies before anything
  runs.
* **Flattening the instances collides ids across trees** — asserted, not
  described, because it is the argument for having the instance-keyed reading.
* **"How many ticks until this finishes" is answerable without taking them**,
  because a machine is a value.
* **A register is FORCED** — the debugger's verb — and the scenario jumps to
  its end with no ticks taken.
* **Halting and converging are different questions**, and both are answerable.

Run from the workspace root:
    cargo build -p hello-node-flow --release
    python3 tools/demos/r1600_a_graph_remembers_per_instance.py
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
#: `STAGE` is the group instance the collapse minted, so its id is the highest.
BEGIN, FORK, WARM, BRANCH, SETTLE, FINISH = 0, 1, 2, 3, 4, 6
BUMP, OVER, ELAPSED, STAGE = 7, 8, 9, 10

#: The limit authored onto `Over budget?`'s own Limit port, and therefore how
#: many ticks the loop runs for before the branch takes its exit arm.
LIMIT = 3

#: The instance control descends into. `/` is the root; one segment is
#: `<host tree>:<instance node>`.
INSIDE = f"/0:{STAGE}"


def q(tf: RpcSubprocess, path: str):
    return tf.query(f"{EXT}/{path}")


def inv(tf: RpcSubprocess, path: str, args):
    return tf.invoke(f"{EXT}/{path}", args)


def refused(tf: RpcSubprocess, path: str, args) -> None:
    """A verb that must not succeed."""
    try:
        inv(tf, path, args)
    except AssertionError:
        pass
    except Exception:  # noqa: BLE001 - any refusal shape is fine here
        pass
    else:
        raise AssertionError(f"{path}({args!r}) was expected to be refused")


def ids(reply: str) -> list[str]:
    return [piece for piece in str(reply).split(",") if piece]


def body() -> None:
    with RpcSubprocess("hello-node-flow", boot_grace=1.5) as tf:
        # ── (A) the register is a node, and it is the only one of its kind ──
        assert_eq(q(tf, "valid"), "ok", "A: the seed is a valid document")
        assert_eq(
            q(tf, "delays"),
            str(ELAPSED),
            "A: one register, and it is enumerable — Blender's simulation state "
            "lives in a private Map on the modifier with no accessor at all",
        )
        assert_eq(
            dict(piece.split(":", 1) for piece in ids(q(tf, "port_flows")))[
                str(ELAPSED)
            ],
            "v>v",
            "A: its signature is DERIVED from the type it holds — one in, one "
            "out, both that type — so it cannot narrow or widen what passes",
        )
        assert_eq(
            dict(piece.split(":", 1) for piece in ids(q(tf, "port_flows")))[
                str(STAGE)
            ],
            "c>c",
            "A: and the group instance's interface carries CONTROL, derived "
            "from the control links that crossed the boundary",
        )
        assert_eq(q(tf, "ticks"), 0, "A: nothing has happened yet")
        assert_eq(
            q(tf, "registers"),
            "",
            "A: and nothing is held, so the register reads its AUTHORED initial "
            "value — R1594's mechanism, which is Lustre's `->`",
        )

        # ── (B) a value cycle closes through the register and nowhere else ──
        assert_eq(
            q(tf, "cycle_nodes"),
            "",
            "B: `elapsed -> bump -> elapsed` is a cycle in the wires and NOT a "
            "dependency cycle, because the register cuts it",
        )
        assert_eq(
            q(tf, "control_loops"),
            f"{WARM},{BRANCH}",
            "B: the control loop is a different question, answered separately",
        )
        # Route the loop AROUND the register — `bump -> over` — and then try to
        # close it. Both ends are ordinary nodes now, so there is no cut on the
        # cycle and the very same shape the seed holds legally is refused.
        assert_eq(
            inv(tf, "wire", f"{BUMP}.0,{OVER}.0"),
            f"linked, displacing node {ELAPSED}.0->node {OVER}.0",
            "B: a value INPUT takes one producer, so the register's wire to "
            "`over` gave way — and what it displaced is named",
        )
        refused(tf, "wire", f"{OVER}.0,{BUMP}.0")
        assert_eq(
            q(tf, "last_refusal"),
            f"that link would close a cycle: {BUMP} -> {OVER}",
            "B: THE ARGUMENT. `bump -> elapsed -> bump` is legal and "
            f"`{BUMP} -> {OVER} -> {BUMP}` is not, and the only difference is "
            "which node is on the cycle. Lustre's causality rule, as a refusal",
        )
        assert_eq(q(tf, "valid"), "ok", "B: so the document never got there")
        assert_eq(inv(tf, "reset", 0), "reset", "B: back to the seed")

        # ── (C) a run READS the machine; a tick MOVES it ────────────────────
        assert_eq(q(tf, "stop"), "budget_exhausted", "C: the loop spins at tick 0")
        for _ in range(4):
            q(tf, "trace")
        assert_eq(
            q(tf, "ticks"),
            0,
            "C: four runs and the clock has not moved — a run is a pure read, "
            "which is what makes a tick's outcome reproducible",
        )
        assert_eq(
            inv(tf, "tick", 0),
            "tick 1: 1 moved, 0 dropped",
            "C: and a tick advances every register at ONE instant, saying how "
            "many moved and how many were dropped as no longer reachable",
        )
        assert_eq(
            q(tf, "registers"),
            f"/@{ELAPSED}=1",
            "C: `/` is the root instance, `@` separates it from the node — a "
            "composite address needs its own separator",
        )

        # ── (D) the trace changes with NO edit at all ───────────────────────
        assert_eq(
            q(tf, "ticks_to_finish"),
            LIMIT,
            "D: three more ticks, answered WITHOUT taking them — a machine is a "
            "value, so the question is asked on a copy",
        )
        assert_eq(q(tf, "ticks"), 1, "D: and asking did not move the world")
        assert_eq(q(tf, "stop"), "budget_exhausted", "D: still spinning")
        assert_eq(
            inv(tf, "settle", 12),
            "12 tick(s), converged: false",
            "D: the counter never reaches a fixed point — it counts forever, "
            "and the last tick still moving IS 'did not converge'",
        )
        assert_eq(
            q(tf, "stop"),
            "halted",
            "D: but the SCENARIO finished. Halting and converging are different "
            "questions, and both are answerable",
        )
        assert_eq(
            q(tf, "at_fixed_point"),
            "no",
            "D: the machine says so itself, without advancing",
        )

        # ── (E) control descended into an instance ──────────────────────────
        assert_eq(
            q(tf, "entered"),
            INSIDE,
            "E: control crossed a group boundary. Unreal has no instance to "
            "name here: a macro is EXPANDED by cloning its graph, so its N uses "
            "are N copies of the nodes before anything runs",
        )
        trace = ids(q(tf, "trace_instances"))
        assert_eq(
            trace,
            [
                f"/@{BEGIN}",
                f"/@{FORK}",
                f"/@{WARM}",
                f"/@{BRANCH}",
                f"/@{SETTLE}",
                f"{INSIDE}@6",
                f"{INSIDE}@5",
                f"{INSIDE}@7",
                f"/@{FINISH}",
            ],
            "E: in, three steps inside, out — the tunnel-in node, the step, the "
            "tunnel-out node — and arm 0 SETTLED before arm 1 was entered, "
            "which is a stack property rather than an ordering convention",
        )
        assert_eq(
            ids(q(tf, "trace"))[5],
            "6",
            "E: flattened, that step is 'node 6' — and node 6 in the ROOT tree "
            "is Finish. A NodeId is unique within its tree, so the instance is "
            "not decoration: without it the two are one row",
        )
        assert_eq(
            q(tf, "never_ran"),
            f"{OVER},{ELAPSED},{STAGE}",
            "E: the two PURE nodes are pulled rather than run — and so is the "
            "group instance, because it is not a computation either: entering "
            "one shows up as the first step INSIDE it",
        )

        # ── (F) the register is forced, and the scenario jumps ──────────────
        assert_eq(inv(tf, "rewind", 0), "rewound", "F: back to tick zero")
        assert_eq(q(tf, "ticks"), 0, "F: the clock too")
        assert_eq(q(tf, "registers"), "", "F: and the registers are empty")
        assert_eq(q(tf, "stop"), "budget_exhausted", "F: so the loop spins again")
        assert_eq(
            inv(tf, "force", f"{ELAPSED},{LIMIT + 1}"),
            "was unset",
            "F: the debugger's verb — write a register directly, answering what "
            "was there. Simulink calls this FORCING a signal",
        )
        assert_eq(
            q(tf, "ticks"),
            0,
            "F: forcing is not ticking, and the difference is stated",
        )
        assert_eq(
            q(tf, "stop"),
            "halted",
            "F: and the scenario is at its end with no ticks taken at all — "
            "reproducing a state a capture caught, without replaying it",
        )
        refused(tf, "force", f"{WARM},1")
        assert_eq(
            q(tf, "last_refusal"),
            f"node {WARM} in tree 0 is not a register",
            "F: and forcing a node that holds nothing is refused BY NAME — the "
            "refusal is the FRAMEWORK's since R1601.1, and it names the tree "
            "too, because a NodeId is unique within one and nowhere else",
        )

        # ── (G) the machine survives an edit, and says what it lost ─────────
        assert_eq(inv(tf, "reset", 0), "reset", "G: back to the seed")
        assert_eq(inv(tf, "settle", 5), "5 tick(s), converged: false", "G: run it")
        assert_eq(q(tf, "registers"), f"/@{ELAPSED}=5", "G: five ticks, five")
        refused(tf, "bypass", ELAPSED)
        assert_eq(
            q(tf, "last_refusal").split(":")[0],
            f"bypassing delay {ELAPSED} in tree 0 would make a value cycle live",
            "G: bypassing the register makes it a plain WIRE, so the cycle it "
            "was breaking becomes live — refused, and the path is named. The "
            "same state `connect` refuses to author, reached by taking the cut "
            "away instead of adding the wire",
        )
        assert_eq(q(tf, "valid"), "ok", "G: so the document never got there")
        assert_eq(
            q(tf, "registers"),
            f"/@{ELAPSED}=5",
            "G: and a refused edit left the machine alone",
        )


run_demo("R1600 a graph remembers, per instance", body)
