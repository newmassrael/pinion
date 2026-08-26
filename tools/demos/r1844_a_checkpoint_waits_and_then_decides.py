#!/usr/bin/env python3
"""R1844 §5.12 §5.16 §5.40 — **checkpoints and assertions with a timeout.**

# What this demo exists for

The analysis-tool census (`tools/analyzer_census.py`) carries `lab.t1.9` —
*checkpoints and assertions with a timeout* — as an **app** verdict whose
covering sentence is *expressible over run and tick; the predicate is the
application's*. That verdict named **no assembly**, which is what R1807's
`UNASSEMBLED` ratchet records: a claim about composition nobody had composed.
This is the composition, driven on the wire, on the node lab itself.

# The premise, measured before anything was built

The two halves were split, and one of them did not exist. `run` was already on
the wire and R1789 had given this screen a clock, a schedule of named lanes and
four acts. But `timeout` appeared **zero times** in every source file of this
crate, and there was no checking act at all: every one of R1789's four words
COMMANDS the graph, and none asks whether it obeyed. A plan could say *start
P-01 at two seconds and kill it at eight* and had no way to say *and it should
have been up in between*.

# ★★★★★ Why the timeout is the whole feature, and not a convenience

Checked only at the instant it is crossed, a checkpoint asserts something about
one moment of a DISCRETE clock — which is a fact about the step the caller
happened to advance by, not about the graph. Advance by 2s and it passes;
advance by 0.5s four times and the same plan fails. That is exactly the
machine-speed dependency `advance` was built to remove (R1600's division), let
back in through the front door.

With a deadline the assertion is about an INTERVAL, so it survives being
replayed at a different step. That is why the verdict is three-valued and why
section C is the one that matters: **`waiting`** is a real answer, and
collapsing it into `failed` would reintroduce the dependency.

# What is shown

  (A) the surface DECLARES which word waits — `act` is a discriminant, and
      choosing `check` brings a timeout the other four do not have.
  (B) a checkpoint whose card is down does not fail; it waits.
  (C) the card comes up inside the window and the verdict becomes `met`.
  (D) a deadline that passes with the card still down is `failed`.
  (E) a check commands nothing, so it never contradicts a command.
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


def verdicts(tf: RpcSubprocess) -> list[str]:
    """Every checkpoint's verdict, in the order the playhead raised them."""
    return [check["verdict"] for check in scenario(tf)["checks"]]


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        card = sorted(json.loads(tf.query(f"{EXT}/cards")))[0]

        # ── (A) the surface says which word waits ────────────────────
        banner("A — the act is a discriminant, and one word of it brings a deadline")
        fields = tf.query(f"{EXT}/$schema")
        place = next(f for f in fields if f["path"] == "schedule")
        domain = place["args"][1]["domain"] if place["args"][1]["name"] == "act" else next(
            a["domain"] for a in place["args"] if a["name"] == "act"
        )
        assert_eq(domain["kind"], "one_of_with", "choosing an act decides the rest of the call")
        brings = {case["value"]: [a["name"] for a in case["then"]] for case in domain["cases"]}
        assert_eq(
            brings,
            {"warmup": [], "start": [], "stop": [], "kill": [], "check": ["timeout"]},
            "★ exactly one word waits, and the DECLARATION says so — a client "
            "reads the timeout off the surface instead of discovering it from "
            "a refusal",
        )
        # ⚠ Both directions are refusals rather than one being ignored. An
        # argument a surface accepts and never reads is a lie it tells once and
        # then keeps telling.
        missing = assert_action_refused(
            lambda: tf.invoke(
                f"{EXT}/schedule", {"at": 1.0, "act": "check", "target": card}
            ),
            saying="needs a timeout",
        )
        ok("a check with no deadline is refused, and the refusal says why", bool(missing))
        spurious = assert_action_refused(
            lambda: tf.invoke(
                f"{EXT}/schedule",
                {"at": 1.0, "act": "kill", "target": card, "timeout": 3.0},
            ),
            saying="does not wait",
        )
        ok("and an act that does not wait refuses one, saying so", bool(spurious))
        print(f"    refusals: {missing!r} / {spurious!r}")

        # ── (B) a checkpoint waits rather than failing ───────────────
        banner("B — the card is down when the checkpoint is crossed, and it WAITS")
        tf.invoke(f"{EXT}/schedule", {"at": 0.0, "act": "stop", "target": card})
        tf.invoke(
            f"{EXT}/schedule",
            {"at": 1.0, "act": "check", "target": card, "timeout": 4.0},
        )
        tf.invoke(f"{EXT}/schedule", {"at": 3.0, "act": "start", "target": card})

        step = tf.invoke(f"{EXT}/advance", 2.0)
        print(f"    after 2s: {verdicts(tf)}")
        assert_eq(verdicts(tf), ["waiting"], "★ down at its moment, and not yet failed")
        # ★ The crossing log says the check changed nothing — `done` is about
        # the GRAPH moving, and an assertion's whole point is that it does not.
        crossed = {row["act"]: row["done"] for row in step["crossed"]}
        assert_eq(crossed.get("check"), False, "a check moves nothing, and says so")

        # ── (C) the card comes up inside the window ──────────────────
        banner("C — the card comes up before the deadline, and the verdict is met")
        tf.invoke(f"{EXT}/advance", 2.0)
        print(f"    after 4s: {verdicts(tf)}")
        assert_eq(verdicts(tf), ["met"], "★ the wait is what made this expressible")

        # ── (D) a deadline that passes ───────────────────────────────
        banner("D — a deadline the card never meets is failed, and stays failed")
        tf.invoke(f"{EXT}/advance", -99.0)  # back to the start; the run resets
        tf.invoke(f"{EXT}/unschedule", {"at": 3.0})
        tf.invoke(f"{EXT}/advance", 2.0)
        assert_eq(verdicts(tf), ["waiting"], "5s is the deadline and 2s is not past it")
        tf.invoke(f"{EXT}/advance", 4.0)
        print(f"    after 6s with no start: {verdicts(tf)}")
        assert_eq(verdicts(tf), ["failed"], "★ the half that makes a verdict worth reading")

        # ── (E) asking is not telling ────────────────────────────────
        banner("E — a check commands nothing, so it contradicts nothing")
        tf.invoke(
            f"{EXT}/schedule",
            {"at": 9.0, "act": "check", "target": card, "timeout": 1.0, "lane": "asserts"},
        )
        tf.invoke(f"{EXT}/schedule", {"at": 9.0, "act": "kill", "target": card})
        plan = scenario(tf)
        ok(
            "a checkpoint beside a command at one moment is not a conflict",
            all(
                "check" not in [a["act"] for a in row["acts"]]
                for row in plan["conflicts"]
            ),
        )

    print(f"\n{len(CHECKS)} named check(s) passed")


run_demo("r1844 a checkpoint waits, and then decides", body)
