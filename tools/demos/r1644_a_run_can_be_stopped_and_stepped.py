#!/usr/bin/env python3
"""R1644 §5.38 §5.52 §2 #2 §2 #7 — a run that can be stopped, stepped and watched.

R1599 gave the graph an execution order and R1600 gave it registers and a clock.
Both answer in one gulp: a run derives a whole order and a tick moves every
register at one instant, and **neither can be interrupted**. So the two questions
a person actually asks of a graph that misbehaves — *stop before this node* and
*what is this port holding* — had no mechanism at all, and nothing observed
`run` or `tick` from outside. The engine's debugging surface is twenty-three
commands over exactly those two.

The design decision this script is mostly about:

**A breakpoint cannot change the run.** The debugger never suspends anything. It
computes the whole run — once, without reference to any breakpoint — and then
moves about inside it. That is bought by R1600's decision that state is a delay
and nothing else, which makes a run a pure function of the document and the
registers. Nothing weaker would do: a debugger that halts a running machine
observes a *different* execution from the one the program has without it, and
then reverse-stepping has to replay recorded frames — which is why the
reference's tree debugger has `CurrentValues` and `SavedValues` as two separate
commands over two data sources. Here going backwards is the same arithmetic as
going forwards, on the same object.

What each check discriminates:

* **The trace is byte-identical with three breakpoints armed and with none.**
  Asserted rather than described, because it is the whole design.
* **A breakpoint stops BEFORE the node runs**, which is where the reference
  stops too, and the step about to run is *readable* — an exact prediction,
  which no debugger over a mutable-state graph can offer.
* **Disabled is not removed.** The place is remembered and stops nothing, which
  is why the reference has five breakpoint commands and not three.
* **Toggling is about presence**, not about the enabled flag — the same line the
  reference draws.
* **A breakpoint that could never fire is refused**: a pure node is *pulled*,
  never run, and a group instance takes no turn of its own. A silently inert
  mark reads as a mark that never fired.
* **An occurrence narrows a mark to one instance.** The reference cannot express
  this: it expands a macro into a copy per use before anything runs.
* **Five stepping commands are two words.** `strides` publishes the 2x3 product,
  and the sixth cell — stepping back *out* — is the one the reference's naming
  left unwritten.
* **A watch reads per occurrence and says whether that occurrence ran**, because
  a value that is not on the trace and a value the run never reached look alike
  and are not. Watching a control port is refused: control is not a value.
* **A mark that outlives what it marked is named, not dropped** — with the
  reason, on the same channel.

Run from the workspace root:
    cargo build -p hello-node-flow --release
    python3 tools/demos/r1644_a_run_can_be_stopped_and_stepped.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_declared_channels_are_true,
    assert_eq,
    run_demo,
)

#: A widget's primary External is addressed by the framework path, not the tag.
EXT = "/external"

#: `hello-node-flow`'s seed, mirrored rather than imported — a demo that read
#: the fixture out of the code under test could not catch it changing.
BEGIN, FORK, WARM, BRANCH, SETTLE, DRAIN, FINISH = 0, 1, 2, 3, 4, 5, 6
BUMP, OVER, ELAPSED, STAGE = 7, 8, 9, 10

#: The definition the collapse minted, and the one instance of it.
DEFINITION = 1
INSIDE = f"/0:{STAGE}"

#: How many ticks it takes for the register to carry the loop past its limit.
#: Settling first is what gives the run a finite trace to debug.
TICKS = 5

#: The settled trace: five steps at the root, three inside the instance, one
#: back at the root. Depths `0,0,0,0,0,1,1,1,0` — a run that never left the root
#: would let `into`, `over` and `out` be one function with nothing noticing.
SETTLED = "0,1,2,3,4,6,5,7,6"


def q(tf: RpcSubprocess, path: str):
    return tf.query(f"{EXT}/{path}")


def inv(tf: RpcSubprocess, path: str, args):
    return tf.invoke(f"{EXT}/{path}", args)


def refused(tf: RpcSubprocess, path: str, args) -> str:
    """A verb that must not succeed, and the reason it gave."""
    try:
        inv(tf, path, args)
    except Exception as why:  # noqa: BLE001 - any refusal shape is fine here
        return str(why)
    raise AssertionError(f"{path}({args!r}) was expected to be refused")


def body() -> None:
    with RpcSubprocess("hello-node-flow", boot_grace=1.5) as tf:
        # ── (A) the vocabulary, published rather than described ──────────
        assert_eq(
            q(tf, "strides"),
            "forward,into forward,over forward,out "
            "back,into back,over back,out",
            "A: ★ the reference names FIVE stepping commands "
            "(forward-into, forward-over, back-into, back-over, out). They are "
            "a direction and a stride, which is 2x3 = six — and the sixth cell, "
            "stepping back OUT, is not a feature added on top but the one its "
            "naming left unwritten. This list is the product computed at "
            "compile time, so a new arm in either vocabulary cannot leave it "
            "short",
        )
        assert_eq(q(tf, "paused_at"), 0, "A: a fresh session has run nothing")
        assert_eq(q(tf, "halt"), "entry", "A: and says so")
        assert_eq(
            q(tf, "occurrences"),
            f"0@/,{DEFINITION}@{INSIDE}",
            "A: every occurrence of every tree — the root once, the definition "
            "once per instance. This is the axis a breakpoint can be narrowed "
            "to, and the reference has none: it expands a macro into a copy "
            "per use before anything runs",
        )

        # ── (A2) every declared path answers on the channel it declares ───
        # R1644 added ten reads and fourteen actions to this surface, and the
        # walk that holds a declaration to its channel is what keeps that from
        # being twenty-four claims nobody checked. It now probes the WRITE
        # channel of each read too: a read declared in `$schema` and missing
        # from the surface's `intervene` arm answers "no such path", which is a
        # surface reporting a name it publishes as one it does not have.
        counted = assert_declared_channels_are_true(tf)
        assert counted["read"] >= 20 and counted["invoke"] >= 14, (
            f"A2: the walk must reach the whole surface, not the easy half: "
            f"{counted}"
        )

        # ── (B) settle, so there is a finite run to debug ─────────────────
        assert_eq(q(tf, "stop"), "budget_exhausted", "B: at tick 0 it loops")
        inv(tf, "settle", TICKS)
        assert_eq(q(tf, "stop"), "halted", "B: once the register carries it past")
        assert_eq(q(tf, "trace"), SETTLED, "B: nine steps, three of them inside")
        assert_eq(
            q(tf, "trace_instances"),
            f"/@{BEGIN},/@{FORK},/@{WARM},/@{BRANCH},/@{SETTLE},"
            f"{INSIDE}@6,{INSIDE}@{DRAIN},{INSIDE}@7,/@{FINISH}",
            "B: and the three inside are attributed to the instance",
        )

        # ── (C) ★ a breakpoint cannot change the run ──────────────────────
        for site in (f"0:{WARM}@*", f"0:{FINISH}@*", f"{DEFINITION}:{DRAIN}@*"):
            assert_eq(inv(tf, "arm", site), f"{site} armed", f"C: armed {site}")
        assert_eq(
            q(tf, "breakpoints"),
            f"0:{WARM}@*=on 0:{FINISH}@*=on {DEFINITION}:{DRAIN}@*=on",
            "C: three marks, each live, each printed in the form it is sent in",
        )
        assert_eq(
            q(tf, "trace"),
            SETTLED,
            "C: ★★ and the run is IDENTICAL. Not equivalent — the same steps in "
            "the same order. The timeline is computed without reference to any "
            "breakpoint, which is what a pure run (R1600: state is a delay and "
            "nothing else) buys, and it is why reverse stepping here needs no "
            "recorded frames",
        )
        assert_eq(
            inv(tf, "resume", None),
            f"2 — breakpoint at 0:{WARM}@*",
            "C: it stops at the first mark the run reaches",
        )
        assert_eq(
            q(tf, "next_step"),
            f"/@{WARM}",
            "C: and the marked node is the one ABOUT to run — the reference "
            "stops in the same place. The step is readable before it happens, "
            "which is an exact prediction rather than a guess, because a run is "
            "a pure function of the document and the registers",
        )
        assert_eq(q(tf, "paused_at"), 2, "C: two steps taken, not three")
        assert_eq(
            inv(tf, "resume", None),
            f"6 — breakpoint at {DEFINITION}:{DRAIN}@*",
            "C: on to the next, INSIDE the instance — and a resume always "
            "leaves where it stands, or it could never get past a mark. Six "
            "steps, not five: entering a group tunnels through the "
            "definition's inside-input node, and that node takes a turn of "
            "its own",
        )
        assert_eq(
            q(tf, "stack"),
            f"0:{STAGE}",
            "C: with the call stack naming the instance control is in",
        )
        assert_eq(inv(tf, "resume", None), f"8 — breakpoint at 0:{FINISH}@*", "C: and out again")
        assert_eq(q(tf, "stack"), "", "C: back at the root, no frame")
        assert_eq(inv(tf, "resume", None), "9 — halted", "C: then to the end")
        assert_eq(
            q(tf, "trace"),
            SETTLED,
            "C: having changed nothing at all",
        )

        # ── (D) disabled is not removed ───────────────────────────────────
        assert_eq(
            inv(tf, "disable_all_breaks", None), "3 changed", "D: and it counts"
        )
        assert_eq(
            q(tf, "breakpoints"),
            f"0:{WARM}@*=off 0:{FINISH}@*=off {DEFINITION}:{DRAIN}@*=off",
            "D: every place remembered, none of them live — which is why the "
            "reference has five breakpoint commands and not three",
        )
        inv(tf, "restart", None)
        assert_eq(inv(tf, "resume", None), "9 — halted", "D: so nothing stops it")
        assert_eq(
            inv(tf, "enable_break", f"0:{WARM}@*"), "was off", "D: one back on"
        )
        inv(tf, "restart", None)
        assert_eq(
            inv(tf, "resume", None),
            f"2 — breakpoint at 0:{WARM}@*",
            "D: and its place was exactly where it had been left",
        )
        assert_eq(inv(tf, "enable_all_breaks", None), "2 changed", "D: two moved")
        assert_eq(
            inv(tf, "enable_all_breaks", None),
            "0 changed",
            "D: and it reports what CHANGED, not what is armed",
        )

        # ── (E) toggling is about presence ────────────────────────────────
        inv(tf, "disable_break", f"0:{WARM}@*")
        assert_eq(
            inv(tf, "toggle_break", f"0:{WARM}@*"),
            f"0:{WARM}@* gone",
            "E: a DISABLED mark toggles away rather than back to enabled — "
            "toggling is presence, and the reference draws the same line",
        )
        assert_eq(
            inv(tf, "toggle_break", f"0:{WARM}@*"),
            f"0:{WARM}@* armed",
            "E: and a fresh one comes back live",
        )
        assert_eq(inv(tf, "clear_breaks", None), "3 forgotten", "E: and clear says how many")
        assert_eq(q(tf, "breakpoints"), "", "E: nothing armed")

        # ── (F) a mark that could never fire is refused ───────────────────
        assert (
            "has no control port" in refused(tf, "arm", f"0:{OVER}@*")
        ), "F: a pure node is PULLED by whoever reads it, never run — so a mark there could never fire, and one that could never fire reads as one that never fired"
        assert (
            "takes no turn of its own" in refused(tf, "arm", f"0:{STAGE}@*")
        ), "F: a group instance is not a computation; entering one shows up as the first step inside it"
        assert "no tree 77" in refused(tf, "arm", f"77:{WARM}@*"), "F: no such tree"
        assert "has no node 9999" in refused(tf, "arm", "0:9999@*"), "F: no such node"
        assert (
            "no such instance" in refused(tf, "arm", f"0:{WARM}@/0:4242")
        ), "F: an occurrence that resolves nowhere"
        assert (
            "lands in tree" in refused(tf, "arm", f"0:{WARM}@{INSIDE}")
        ), "F: ★ and an occurrence that resolves SOMEWHERE ELSE is the opposite mistake, so it is a different refusal"
        assert "is not a site" in refused(tf, "arm", "nonsense"), "F: and a malformed address"
        assert_eq(q(tf, "breakpoints"), "", "F: not one of those armed anything")

        # ── (G) an occurrence narrows a mark ──────────────────────────────
        assert_eq(
            inv(tf, "arm", f"{DEFINITION}:{DRAIN}@{INSIDE}"),
            f"{DEFINITION}:{DRAIN}@{INSIDE} armed",
            "G: a mark scoped to ONE occurrence of the definition",
        )
        inv(tf, "restart", None)
        assert_eq(
            inv(tf, "resume", None),
            f"6 — breakpoint at {DEFINITION}:{DRAIN}@{INSIDE}",
            "G: which stops in that instance, and the address it reports back "
            "is the scoped one rather than the node",
        )
        assert_eq(q(tf, "stack"), f"0:{STAGE}", "G: inside the frame")
        inv(tf, "clear_breaks", None)

        # ── (H) six strides, on a run with a boundary in it ───────────────
        inv(tf, "restart", None)
        assert_eq(inv(tf, "step", "forward,into"), "1 — stepped", "H: one step is one step")
        for _ in range(3):
            inv(tf, "step", "forward,into")
        assert_eq(q(tf, "paused_at"), 4, "H: at the last step before the boundary")
        assert_eq(
            inv(tf, "step", "forward,over"),
            "8 — stepped",
            "H: ★ OVER skips the whole instance — three steps inside pass in one "
            "command, and the next thing at this depth is step 8",
        )
        assert_eq(
            inv(tf, "step", "back,over"),
            "4 — stepped",
            "H: and back over it lands where it set off",
        )
        assert_eq(
            inv(tf, "step", "forward,into"),
            "5 — stepped",
            "H: INTO the instance instead — the same position, a different verb",
        )
        assert_eq(q(tf, "stack"), f"0:{STAGE}", "H: one frame deep")
        assert_eq(
            inv(tf, "step", "forward,out"),
            "8 — stepped",
            "H: OUT runs the frame to completion and stops where control returns",
        )
        assert_eq(q(tf, "stack"), "", "H: back at the root")
        assert_eq(inv(tf, "step", "back,into"), "7 — stepped", "H: back one, inside again")
        assert_eq(q(tf, "stack"), f"0:{STAGE}", "H: in the frame")
        assert_eq(
            inv(tf, "step", "back,out"),
            "4 — stepped",
            "H: ★ and the cell the reference has no command for: back out of the "
            "frame control is in, landing before it was entered. It falls out of "
            "the product rather than being added to it",
        )
        assert_eq(
            inv(tf, "step", "forward,out"),
            "9 — halted",
            "H: out of the OUTERMOST frame runs to the end, because there is "
            "nothing shallower to arrive at — which is what every debugger "
            "does. And landing at the end is reported as the END rather than as "
            "a stride, because why a run finished and how far a command went "
            "are different facts",
        )
        assert_eq(inv(tf, "step", "back,into"), "8 — stepped", "H: and back one")
        assert "no stride" in refused(tf, "step", "forward,sideways"), "H: a closed vocabulary"
        assert "no direction" in refused(tf, "step", "diagonal,into"), "H: on both segments"

        # ── (I) a watch reads per occurrence ──────────────────────────────
        assert_eq(
            inv(tf, "watch", f"0:{WARM}.out1@*"),
            f"0:{WARM}.out1@* watched",
            "I: the cost port of a node that runs",
        )
        assert_eq(
            q(tf, "readings"),
            f"0:{WARM}.out1@*@/=4 ran@2",
            "I: one reading per occurrence, with the value and the step the "
            "node ran at. Per OCCURRENCE and not per step because a port's "
            "value is a pure function of the document and the registers — "
            "within one run it cannot differ between two moments, so 'the "
            "value now' and 'the value at step 4' are one question. The "
            "reference needs two commands for them",
        )
        assert_eq(
            inv(tf, "watch", f"0:{ELAPSED}.out0@*"),
            f"0:{ELAPSED}.out0@* watched",
            "I: and the register itself",
        )
        assert (
            f"0:{ELAPSED}.out0@*@/={TICKS} pure" in str(q(tf, "readings"))
        ), "I: ★ whose value is NOT on the trace — a delay has no control port, so it is pulled and never run. Reported rather than left to be inferred, because 'a value off the trace' and 'a value the run never reached' look alike and are not"
        inv(tf, "tick", None)
        assert (
            f"0:{ELAPSED}.out0@*@/={TICKS + 1} pure" in str(q(tf, "readings"))
        ), "I: and it moves when the world does"
        assert (
            "carries control, not a value"
            in refused(tf, "watch", f"0:{WARM}.out0@*")
        ), "I: watching control is refused — control is not a value, so there is nothing to report. The reference refuses the same thing, by asking whether the pin's category is an execution one"
        assert "has no port out9" in refused(tf, "watch", f"0:{WARM}.out9@*"), "I: no such port"
        assert_eq(inv(tf, "unwatch", f"0:{ELAPSED}.out0@*"), f"0:{ELAPSED}.out0@* true", "I: dropped")
        assert_eq(inv(tf, "clear_watches", None), "1 dropped", "I: and the rest")
        assert_eq(q(tf, "readings"), "", "I: nothing watched, nothing reported")

        # ── (J) a mark can outlive what it marked ─────────────────────────
        inv(tf, "arm", f"0:{WARM}@*")
        inv(tf, "watch", f"0:{WARM}.out1@*")
        assert_eq(q(tf, "stale_marks"), "", "J: both hold, for now")
        assert_eq(
            inv(tf, "set_reading", f"{WARM},5"),
            5,
            "J: the document is editable while it is being debugged, and this "
            "turns the marked node into a PURE one",
        )
        assert_eq(
            q(tf, "stale_marks"),
            f"0:{WARM}@*: node {WARM} in tree 0 has no control port, so no run "
            f"arrives at it | 0:{WARM}.out1@*: node {WARM} in tree 0 has no "
            "port out1",
            "J: ★ so both marks are reported, with the reason each no longer "
            "holds, and NEITHER is silently dropped — a debugger that forgets "
            "the place a person marked is worse than one that says the mark no "
            "longer holds",
        )
        assert_eq(
            q(tf, "breakpoints"),
            f"0:{WARM}@*=on",
            "J: the mark is still there to be seen and removed",
        )
        assert_eq(inv(tf, "reset", None), "reset", "J: and a reset starts over")
        assert_eq(q(tf, "breakpoints"), "", "J: with nothing armed")
        assert_eq(q(tf, "paused_at"), 0, "J: at the entry")
        assert_eq(q(tf, "valid"), "ok", "J: on a valid document")


run_demo("R1644 a run can be stopped, stepped and watched", body)
