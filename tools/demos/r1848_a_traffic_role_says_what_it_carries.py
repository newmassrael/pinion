#!/usr/bin/env python3
"""R1848 §5.11 §5.7 — **a traffic role declares which parameters its nodes
carry, and every card is judged against its own role's declaration.**

# What this demo exists for

The analysis-tool census carries `lab.t1.8` — *traffic nodes carrying rate,
payload, priority, congestion and reliability* — with the verdict `app`: the
framework owns what a node IS (`pinion_node_graph`'s `NodeKind`, which
`rests_on` names) and declines to own which parameters a domain's traffic has,
because every domain's are different. This screen is where that taxonomy is
declared, and this script is what makes the claim checkable on the wire.

**What made it worth declaring rather than assuming.** The opening graph already
PAINTS traffic parameters — as free-text key/value rows on a card, keyed by
whatever the row was written with. Nothing said which keys belong to a role, so
"does this node state its priority?" had nowhere to be asked, and a node stating
a parameter its role does not carry would have read exactly like one that does.
A closed vocabulary plus a per-role declaration turns both into questions with
answers, and the answers are DERIVED from the two rather than recorded a third
time.

# What is shown

  (A) the vocabulary is published as a CLOSED list, so a client enumerates what
      can be carried instead of inferring it from whichever roles this document
      happens to contain.
  (B) every role's `carries` is drawn from that vocabulary, and the division is
      real: infrastructure carries none, and the traffic roles do not all carry
      the same thing.
  (C) ★ each node's `traffic_stated` is exactly the declared parameters its own
      rows state — recomputed here from `rows` and the role table alone, and
      asserted equal to what the application published.
  (D) `traffic_stated` and `traffic_unstated` PARTITION what the role carries,
      so neither can drift without the other saying so.
  (E) ★ the measurement the screen could not previously make about itself: how
      much of the declared taxonomy the reference's own opening graph puts in
      front of a reader. Reported, and asserted to be a strict subset — the
      screen reproduces a reference, so this is a fact about that screen rather
      than a defect list.

Run from the workspace root:
    cargo build -p hello-node-lab --release
    python3 tools/demos/r1848_a_traffic_role_says_what_it_carries.py
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


def spec_of(tf: RpcSubprocess) -> dict[str, Any]:
    """The screen's whole specification, as the wire publishes it."""
    return json.loads(tf.query(f"{EXT}/spec"))


def derive_stated(node: dict, roles: dict[str, dict]) -> list[str]:
    """What this node states, recomputed from its rows and its role alone.

    ★ The point of section C: a demo that read `traffic_stated` and checked it
    against itself would agree with whatever the application said. This is the
    second opinion.
    """
    carries = roles[node["role"]]["carries"]
    keys = {key for key, _ in node["rows"]}
    return [p for p in carries if p in keys]


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        spec = spec_of(tf)

        # ── (A) the vocabulary, published and closed ────────────────────
        banner("A — the vocabulary is published, so a client need not infer it")
        vocabulary = spec["traffic_parameters"]
        ok("the screen publishes a traffic vocabulary", len(vocabulary) > 1)
        ok("and no word in it is repeated", len(set(vocabulary)) == len(vocabulary))
        print(f"[demo] vocabulary: {vocabulary}")

        # ── (B) the roles, and the division between them ────────────────
        banner("B — a role declares what it carries, and the declaration divides")
        roles = {r["name"]: r for r in spec["roles"]}
        for role in spec["roles"]:
            for parameter in role["carries"]:
                ok(
                    f"{role['name']} carries {parameter!r}, a published word",
                    parameter in vocabulary,
                )
            assert_eq(
                bool(role["carries"]),
                role["group"] == "traffic",
                f"B: {role['name']} is {role['group']} and carries "
                f"{len(role['carries'])}",
            )
        traffic = [r for r in spec["roles"] if r["group"] == "traffic"]
        widths = sorted({len(r["carries"]) for r in traffic})
        ok(
            "★ the traffic roles are not all told the same thing — a taxonomy "
            f"that divides nothing is a label: widths {widths}",
            len(widths) > 1,
        )
        ok(
            "and something carries the WHOLE vocabulary, so the widest case is "
            "exercised rather than merely declared",
            max(len(r["carries"]) for r in traffic) == len(vocabulary),
        )
        for role in traffic:
            print(f"[demo] {role['name']:<11} carries {role['carries']}")

        # ── (C) ★ what a card states, recomputed ────────────────────────
        banner("C — ★ a node's stated parameters are DERIVED, and recomputed here")
        for node in spec["nodes"]:
            assert_eq(
                node["traffic_stated"],
                derive_stated(node, roles),
                f"C: {node['id']} states what its rows and its role say it does",
            )
            for parameter in node["traffic_stated"]:
                ok(
                    f"C: {node['id']} states {parameter!r} and has a row for it",
                    any(key == parameter for key, _ in node["rows"]),
                )

        # ── (D) the two halves partition the declaration ────────────────
        banner("D — stated and unstated partition what the role carries")
        for node in spec["nodes"]:
            carries = roles[node["role"]]["carries"]
            assert_eq(
                sorted(node["traffic_stated"] + node["traffic_unstated"]),
                sorted(carries),
                f"D: {node['id']}'s two halves are exactly what {node['role']} carries",
            )
            ok(
                f"D: {node['id']} does not both state and omit one parameter",
                not set(node["traffic_stated"]) & set(node["traffic_unstated"]),
            )

        # ── (E) ★ what the reference's own graph actually shows ─────────
        banner("E — ★ how much of the taxonomy the opening graph puts on screen")
        declared = sum(len(roles[n["role"]]["carries"]) for n in spec["nodes"])
        stated = sum(len(n["traffic_stated"]) for n in spec["nodes"])
        ok("some node declares traffic at all, so this measures something", declared > 0)
        ok(
            "★ and the cards state a STRICT subset of it — this screen "
            "reproduces a reference, so the gap is a fact about that screen "
            "rather than a list of defects",
            0 < stated < declared,
        )
        for node in spec["nodes"]:
            if roles[node["role"]]["carries"]:
                print(
                    f"[demo] {node['id']:<6} {node['role']:<11} "
                    f"states {node['traffic_stated']} "
                    f"omits {node['traffic_unstated']}"
                )
        print(f"[demo] {stated} of {declared} declared parameter(s) are on a card")

        print(f"\n=== {len(CHECKS)} named check(s) ===")
        for what in CHECKS:
            print(f"  - {what}")


if __name__ == "__main__":
    sys.exit(run_demo("R1848 a traffic role says what it carries", body))
