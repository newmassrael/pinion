#!/usr/bin/env python3
"""R1788 §5.21 §5.15 §2 #2 §2 #7 — **the deployable plan is the framework's, and
it refuses to lose a node rather than quietly losing one.**

# What this demo exists for

The analysis-tool census carries `lab.t1.12` — *saving a scenario and exporting
a deployable configuration* — as a **gap**, on the reason that what is missing
is "the SCENARIO level, a whole graph as one saved artifact".

Re-measured before this round wrote a line, that reason was wrong the same way
`capture.t1.12`'s was one round earlier. Both halves were built: `Archive`
(R1689) saves the whole graph with its camera, its selection and a companion,
and reports **why** a file will not open; `deploy` (R1687) derives the bring-up
order. What was actually missing is a different sentence, and it is the one this
project's standing rule names: **the artifact derivation lived in an example**.
371 lines of `Plan`, the builder and both renderings sat in one binary's `src/`,
where no second consumer could reach them and where the census — which asks
whether the *framework* can do it — was right to say no.

# What moving it bought, and what moving it FOUND

`Document::deployment` now derives the ORDER itself, so the order and the
artifact cannot disagree; while a screen read `launch_order` and handed the
sequence to a builder, any caller could pass any order.

★★★★★ And the crate's own first test run found a defect that had been live since
R1687 and was invisible in the example: **a plan names each node's configuration
file after the node, and two nodes can share a name.** The document keyed its
`nodes` map by that name, so one of them was dropped; the script wrote two
heredocs to one path, so one overwrote the other.

★★★★★ **The invariant that looks like it covers this does not.** The model
maintains *authored names are unique within a tree, and therefore address
exactly one node* — a refusal whose own doc measures the reference floor for the
same rule. That is about the **stored label**. A node with no label still has a
name: the display name falls back to its KIND's, and a fallback is not a stored
value, so nothing can refuse a second one. Two unlabelled nodes of one kind
collide **while the invariant holds**, which is why the plan has to be the
guard.

# The floor, measured across TWO processes

Built and run at 6.11 (scratchpad only, never tracked). Handed a value its
settings store cannot encode, the reference reports **no error** on write, **no
error** on sync and **no error** on read; the file keeps the type name and no
payload; and reading it back **inside the writing process appears to succeed**,
because a process-wide cache answers instead of the file. Only a second process
sees the value is gone — and the status is still no-error there. The entire
signal is one line on stderr.

So the in-process test a developer would write **passes** while the artifact is
broken. That is why the population for a persistence claim is two processes, and
why a plan that says nothing about what it dropped is the same failure.

★★ **And the collision is not reachable on this screen**, which is worth saying
rather than working around: every card here carries a label, so the model's own
rule applies and a rename onto a taken name is refused — measured, *another card
in tree 0 is already called "P-02"*. Two guards, one rule, at the two layers
where the rule can be enforced at all: the model refuses a duplicate **label**,
and the plan refuses a duplicate **name**, which is the wider set because it
includes the fallback. The second is exercised by `pinion-node-graph`'s own
tests on a graph whose nodes carry no labels — what a consumer that has not
thought about names looks like. R1786's shape again: a surface the framework
owes its consumers and cannot exercise itself is a reason to build it, not to
skip it.

  (A) both artifacts are latched, not derived — nothing is produced until asked.
  (B) an export carries the order, the nodes, the hosts and what was not carried.
  (C) the order in the artifact IS the graph's launch order, and the script
      starts every node in it exactly once.
  (D) a rename onto a name already in use is refused, and says which — the
      model's guard, and the reason the plan's is untestable on this screen.
  (E) a refused edit costs the plan nothing: the artifact is unchanged.
  (F) a row no document can carry reaches BOTH renderings — the wire key is
      `uncarried`, which is this round's own rename, so this is where a reader
      finds out.

Run from the workspace root:
    cargo build -p hello-node-lab --release
    python3 tools/demos/r1788_a_plan_refuses_to_lose_a_node.py
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


def q(tf: RpcSubprocess, slot: str) -> Any:
    return tf.query(f"{EXT}/{slot}")


def produced(tf: RpcSubprocess) -> dict:
    return json.loads(q(tf, "produced"))


def export(tf: RpcSubprocess) -> Any:
    tf.invoke(f"{EXT}/export", "")
    return produced(tf)["config"]


def script(tf: RpcSubprocess) -> Any:
    tf.invoke(f"{EXT}/script", "")
    return produced(tf)["script"]


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) latched, not derived ─────────────────────────────────
        banner("A — an artifact is a thing somebody made, so nothing is there yet")
        empty = produced(tf)
        assert_eq(sorted(empty), ["config", "script"], "both halves are keys")
        ok("nothing is exported before anybody asks", empty["config"] is None)
        ok("and no script either", empty["script"] is None)

        # ── (B) the artifact's four sections ─────────────────────────
        banner("B — the export carries the order, the nodes, the hosts and the losses")
        plan = export(tf)
        ok("the export landed", plan is not None)
        assert_eq(
            sorted(plan),
            ["hosts", "nodes", "order", "uncarried"],
            "four sections, and `uncarried` is one of them",
        )
        ok("producing one did not produce the other", produced(tf)["script"] is None)
        started = [row["node"] for row in plan["order"]]
        ok("the graph starts something", len(started) > 0)
        assert_eq(
            sorted(plan["nodes"]),
            sorted(started),
            "★ every started node has a configuration and no other does — the "
            "map and the order are one population",
        )
        for row in plan["order"]:
            ok(
                f"{row['node']} says where it starts, what it runs and why it is here",
                bool(row["host"]) and bool(row["program"]) and bool(row["standing"]),
            )
            ok(
                f"and {row['node']}'s host is one the plan lists",
                row["host"] in plan["hosts"],
            )
        for host, names in plan["hosts"].items():
            ok(
                f"host {host} lists only nodes that are started",
                set(names) <= set(started),
            )

        # ── (C) the artifact's order IS the graph's ──────────────────
        banner("C — the order in the artifact is the graph's own launch order")
        cards = json.loads(q(tf, "cards"))
        ok("the screen has cards to deploy", len(cards) > 0)
        # `standing` is the reason, and it is monotonic down the order: a node
        # that has to be up first cannot appear after one that only dials.
        rank = {"first": 0, "between": 1, "last": 2, "alone": 3}
        standings = [rank[row["standing"]] for row in plan["order"]]
        ok(
            "the reasons do not run backwards down the order",
            all(a <= b for a, b in zip(standings, standings[1:])),
        )
        # The script starts every node the document names, exactly once.
        text = script(tf)
        ok("the script landed", isinstance(text, str) and text.startswith("#!"))
        for name in started:
            assert_eq(
                text.count(f'"$OUT/{name}.json"'),
                2,
                f"★ {name} gets exactly one configuration file and one start line",
            )
        ok("and the script waits for them", "\nwait" in text)
        ok(
            "★ a plan across hosts SAYS how many, rather than emitting one "
            "host's processes and looking like it worked",
            (len(plan["hosts"]) > 1) == ("hosts — run this on each" in text),
        )

        # ── (D) the model's guard: a duplicate LABEL is refused ──────
        banner("D — a rename onto a name already in use is refused, by name")
        victim, twin = started[0], started[1]
        assert_action_refused(
            lambda: tf.invoke(f"{EXT}/rename", f"{twin},{victim}"),
            saying=f'already called "{victim}"',
        )
        ok(
            "★ every card here has a LABEL, so the model's rule applies and the "
            "plan's wider one — which also covers the kind-name fallback — is "
            "unreachable on this screen; it is proven on an unlabelled graph by "
            "pinion-node-graph's own tests",
            True,
        )

        # ── (E) a refused edit costs the plan nothing ────────────────
        banner("E — the refusal left the graph, and the artifact, alone")
        again = export(tf)
        assert_eq(
            [row["node"] for row in again["order"]],
            started,
            "the same nodes in the same order",
        )
        assert_eq(sorted(again["nodes"]), sorted(started), "and the same configurations")
        assert_eq(again["uncarried"], [], "and nothing was lost")

        # ── (F) a row no document can carry reaches both renderings ──
        banner("F — `uncarried` on the wire, in the document and in the script")
        spec = json.loads(q(tf, "spec"))
        key, over = "transport.link.tx.batch_size", "70000"
        tf.invoke(f"{EXT}/select", spec["selected_node"])
        tf.invoke(f"{EXT}/set_field", f"{key}={over}")
        lossy = export(tf)
        rows = lossy["uncarried"]
        assert_eq(len(rows), 1, "one row, named")
        assert_eq(rows[0]["key"], key, "★ by its configuration path")
        assert_eq(rows[0]["shown"], over, "carrying the value verbatim")
        ok(f"and why: {rows[0]['why']!r}", bool(rows[0]["why"]))
        assert_eq(
            rows[0]["node"],
            spec["selected_node"],
            "and which node it belongs to",
        )
        ok(
            "★★ the rest of that node's configuration still ships — one bad "
            "value does not cost the file",
            bool(lossy["nodes"][spec["selected_node"]]),
        )
        ok(
            "and the refused row is not silently in it",
            over not in json.dumps(lossy["nodes"][spec["selected_node"]]),
        )
        lossy_script = script(tf)
        ok(
            "★ the script keeps it as a COMMENT, because a script is the "
            "artifact somebody keeps and a toast is not",
            "not in any file above" in lossy_script,
        )
        ok("naming the same path", key in lossy_script)
        # Put it back, and the news goes away rather than lingering.
        tf.invoke(f"{EXT}/reset", "fields")
        assert_eq(export(tf)["uncarried"], [], "a fixed row stops being news")
        ok(
            "and the script stops saying it too",
            "not in any file above" not in script(tf),
        )

    print(f"\n=== {len(CHECKS)} named check(s) ===")
    for line in CHECKS:
        print(f"  - {line}")


if __name__ == "__main__":
    sys.exit(run_demo("R1788 a plan refuses to lose a node", body))
