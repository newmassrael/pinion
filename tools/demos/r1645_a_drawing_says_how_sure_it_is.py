#!/usr/bin/env python3
"""R1645 §5.38 §5.52 §2 #7 — two layers in one model, and what the drawing can be trusted about.

`hello-graph-diff` (R1575) drew a link graph in two layers — what a user drew,
what a source reported — and derived each link's layer from set membership
rather than storing it. That derivation was right. What it could not be was a
**node graph**: it kept two `Vec` of name pairs of its own, 801 lines, because
there was nowhere in `pinion-node-graph` to put a reported link. So the
capability existed in an example and an application wanting it had to copy the
example, which is a fork.

R1645 puts both layers in the model, and three things follow that a pair of
name-pair sets cannot produce:

* **An observation is not in the graph.** It lives beside `Tree::links`, not in
  it, so no derivation in the crate can reach one by accident — the same
  placement argument R1644 made for keeping breakpoints out of the run. Adding
  reports leaves `evaluate`, `validate`, `cycle_nodes` and the link count
  untouched, which this script asserts on the wire.
* **Observation is admitted where authoring is refused.** The world is under no
  obligation to obey this model, so a report that closes a value cycle is
  *recorded*; a capture that dropped what it saw would be the tool lying. Where
  it shows up is `adopt`, which runs the authoring rules and **names** the
  refusal — "this exists out there and your drawing cannot express it". The
  `impossible` scenario exists to reach that, and the old binding could not have
  produced it: nothing in a set of name pairs knows what a cycle is.
* **A drawing known to be incomplete says so.** `standing` is `partial` when
  auto-discovery is on **or when anything has drifted**, and the second is
  derived rather than switched: one reported link nobody drew is proof the
  drawing is not the whole topology. So `reaches` answers about BOTH layers and
  carries the standing, and the case that matters is the disagreement — a static
  rule says blocked and the world says otherwise, which is what field experience
  with a lab built this way records happening.

Run from the workspace root:
    cargo build -p hello-graph-diff --release
    python3 tools/demos/r1645_a_drawing_says_how_sure_it_is.py
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

EXT = "/external"

#: The scenarios the binding declares. `partial` differs from the drawing in
#: both directions, `converged` agrees with it, and `impossible` reports a link
#: the model refuses to hold.
PARTIAL, CONVERGED, IMPOSSIBLE = "partial", "converged", "impossible"


def q(tf: RpcSubprocess, path: str):
    return tf.query(f"{EXT}/{path}")


def inv(tf: RpcSubprocess, path: str, args):
    return tf.invoke(f"{EXT}/{path}", args)


def scenario(tf: RpcSubprocess, name: str) -> None:
    tf.intervene(f"{EXT}/scenario", name)
    assert_eq(q(tf, "scenario"), name, f"loaded {name}")


def refused(tf: RpcSubprocess, path: str, args) -> str:
    try:
        inv(tf, path, args)
    except Exception as why:  # noqa: BLE001 - any refusal shape is fine here
        return str(why)
    raise AssertionError(f"{path}({args!r}) was expected to be refused")


def body() -> None:
    with RpcSubprocess("hello-graph-diff", boot_grace=1.5) as tf:
        counted = assert_declared_channels_are_true(tf)
        assert counted["read"] >= 12, f"A: the walk reaches the surface: {counted}"

        # ── (A) the drawing and the reports disagree, in both directions ──
        assert_eq(q(tf, "scenario"), PARTIAL, "A: the binding opens partial")
        assert_eq(q(tf, "matched"), 4, "A: four links drawn and reported")
        assert_eq(q(tf, "missing"), 1, "A: one drawn and not reported")
        assert_eq(q(tf, "drift"), 1, "A: one reported and not drawn")
        assert_eq(q(tf, "missing_ids"), "leaf-3>peer-b", "A: which one is missing")
        assert_eq(q(tf, "drift_ids"), "leaf-2>hub", "A: and which drifted")
        assert_eq(q(tf, "link_count"), 6, "A: six links across the two layers")
        assert_eq(
            inv(tf, "link_kind", "leaf-3,peer-b"),
            "missing",
            "A: and one link's layer, by name",
        )

        # ── (B) ★ the drawing says how sure it is ────────────────────────
        assert_eq(q(tf, "discovery"), "off", "B: discovery is OFF by default")
        assert_eq(
            q(tf, "standing"),
            "partial (discovery off, 1 undrawn link(s) reported)",
            "B: ★ and the drawing is PARTIAL anyway — one reported link nobody "
            "drew is proof it is not the whole topology, whatever the switch "
            "says. The switch is the reference condition; drift is the derived "
            "one, and it is the stronger",
        )
        assert_eq(q(tf, "certain"), "no", "B: so nothing static is a fact yet")

        scenario(tf, CONVERGED)
        assert_eq(q(tf, "drift"), 0, "B: nothing drifted here")
        assert_eq(q(tf, "missing"), 0, "B: and nothing is missing")
        assert_eq(
            q(tf, "standing"),
            "certain",
            "B: so the drawn links ARE the topology and a static answer is an "
            "answer about the world",
        )
        assert_eq(q(tf, "certain"), "yes", "B: said in one word too")
        tf.intervene(f"{EXT}/discovery", "on")
        assert_eq(q(tf, "discovery"), "on", "B: the determinism switch is writable")
        assert_eq(
            q(tf, "standing"),
            "partial (discovery on, 0 undrawn link(s) reported)",
            "B: ★ and turning it on makes the SAME graph partial with nothing "
            "drifted — the two conditions are independent, which a single "
            "counter could not show",
        )
        assert_eq(q(tf, "certain"), "no", "B: so the answer is about the drawing")
        tf.intervene(f"{EXT}/discovery", "off")
        assert_eq(q(tf, "certain"), "yes", "B: and off again restores it")

        # ── (C) ★ the two layers disagree about what is reachable ────────
        scenario(tf, PARTIAL)
        assert_eq(
            inv(tf, "reaches", "leaf-2,hub"),
            "drawn=true observed=true disagrees=false standing=partial",
            "C: leaf-2 reaches the hub on both layers — through peer-a as "
            "drawn, and directly as reported",
        )
        assert_eq(
            inv(tf, "reaches", "leaf-3,hub"),
            "drawn=true observed=false disagrees=true standing=partial",
            "C: ★ and here the drawing is OPTIMISTIC: it says leaf-3 reaches "
            "the hub and the reports say that link never came up",
        )
        scenario(tf, IMPOSSIBLE)
        assert_eq(
            inv(tf, "reaches", "hub,peer-a"),
            "drawn=false observed=true disagrees=true standing=partial",
            "C: ★★ and here it is PESSIMISTIC — the static rule says blocked "
            "and the world says otherwise. That is the case field experience "
            "with a lab built this way records happening, and it is one read",
        )
        assert_eq(
            inv(tf, "reaches", "leaf-1,leaf-3"),
            "drawn=false observed=false disagrees=false standing=partial",
            "C: two leaves reach each other on neither layer",
        )
        assert "no node named" in refused(tf, "reaches", "hub,nowhere"), "C: unknown node"
        assert "is not <from>,<to>" in refused(tf, "reaches", "hub"), "C: malformed"

        # ── (D) ★ a report the model cannot hold is NAMED ────────────────
        assert_eq(q(tf, "drift_ids"), "hub>peer-a", "D: the world reports a loop back")
        assert_eq(
            inv(tf, "link_kind", "hub,peer-a"),
            "drift",
            "D: which is drift — reported and not drawn",
        )
        why = refused(tf, "adopt", "")
        assert "would close a cycle" in why, f"D: ★ adopting it is refused BY THE AUTHORING RULE and the refusal names the cycle it would have closed — 'this exists out there and your drawing cannot express it' is a finding, not something to swallow. The binding that kept two sets of name pairs assigned one to the other, and nothing in a set of pairs knows what a cycle is: {why}"
        assert "hub>peer-a" in why, "D: and it says WHICH report it refused"
        assert_eq(
            q(tf, "drift"),
            1,
            "D: so it stays visible as drift rather than being forgotten",
        )
        assert_eq(q(tf, "authored_ids").count(","), 4, "D: five drawn links, unchanged")

        # ── (E) an observation is not in the graph ───────────────────────
        scenario(tf, PARTIAL)
        drawn = q(tf, "authored_ids")
        assert_eq(q(tf, "node_count"), 6, "E: six nodes")
        scenario(tf, IMPOSSIBLE)
        assert_eq(
            q(tf, "authored_ids"),
            drawn,
            "E: ★ a completely different set of reports left the DRAWN links "
            "identical. An observation lives beside the tree's links rather "
            "than in them, so no derivation in the crate reaches one by "
            "accident — the same placement R1644 used to keep breakpoints out "
            "of the run",
        )
        assert_eq(q(tf, "node_count"), 6, "E: and the nodes are the same")

        # ── (F) adopting is still one gesture where the model allows it ──
        scenario(tf, PARTIAL)
        before = q(tf, "missing") + q(tf, "drift")
        assert_eq(before, 2, "F: two differences")
        assert_eq(inv(tf, "adopt", ""), 2, "F: adopted, and it says how many")
        assert_eq(q(tf, "missing"), 0, "F: nothing drawn is unreported now")
        assert_eq(q(tf, "drift"), 0, "F: and nothing reported is undrawn")
        assert_eq(q(tf, "matched"), 5, "F: five links, drawn and reported")
        assert_eq(
            q(tf, "standing"),
            "certain",
            "F: ★ so the drawing is the topology again — and the standing said "
            "so without anyone asserting it, because it is derived from the "
            "same difference the counts are",
        )
        assert_eq(q(tf, "certain"), "yes", "F: in one word")


run_demo("R1645 a drawing says how sure it is", body)
