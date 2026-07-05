#!/usr/bin/env python3
"""R1258 §5.38 §2 #2 — set_graph structural validation (trust-boundary hardening).

`set_graph` is the AI-first WRITE path (§2 #2) — the inverse of `query
serialized`. Before R1258 it installed any schema-matched blob verbatim, so an
AI (or a corrupt file) could inject a structurally-invalid graph — an ill-typed
edge, a wrong-arity op, a duplicate id, a mistyped default — that the evaluator
then read to the WRONG value or a permanent `null`. R1258 validates the blob and
rejects a malformed one LOUD: `set_graph` returns false, the graph unchanged
(the AI sees the rejection, not a silent corruption). Driven over §5.12, no pixels:

  (A) round-trip: `query serialized` -> `set_graph` the same blob -> accepted.
  (B) a modified-but-VALID blob (a renamed node) is accepted and applied.
  (C) a STRUCTURALLY-INVALID blob (a Multiply with one input port -> a wrong
      arity that would evaluate to a permanent null) is REJECTED, graph unchanged.
  (D) a blob whose id counter is behind a stored id is rejected.
  (E) malformed JSON is rejected. The graph still evaluates after every reject.

Run from the workspace root:
    cargo build -p hello-node-editor --release
    python3 tools/demos/r1258_node_set_graph_validation.py
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    assert_eq,
    run_demo,
)


def q(tf: RpcSubprocess, key: str) -> Any:
    return tf.query(f"/external/{key}")


def set_graph(tf: RpcSubprocess, blob: str) -> Any:
    return tf.invoke("/external/set_graph", blob)


def reject(tf: RpcSubprocess, blob: str, label: str) -> None:
    """A rejected set_graph fails LOUD — `Rejected` surfaces as an RpcError
    (the graph unchanged), the §2 #2 fail-loud contract for the AI write path."""
    try:
        set_graph(tf, blob)
    except RpcError:
        return
    raise AssertionError(f"expected set_graph to reject: {label}")


def rgb(v: Any) -> tuple[int, int, int]:
    return (v["r"], v["g"], v["b"])


def body() -> None:
    with RpcSubprocess("hello-node-editor", boot_grace=1.5) as tf:
        assert_eq(q(tf, "node_count"), 4, "4 seed nodes")
        valid = q(tf, "serialized")
        assert isinstance(valid, str) and "schema_version" in valid, "serialized is the JSON blob"

        # ── (A) round-trip: a live blob reloads ──────────────────────
        assert_eq(set_graph(tf, valid), True, "set_graph accepts a serialized live graph")
        assert_eq(q(tf, "node_count"), 4, "the round-trip preserved the graph")
        assert_eq(q(tf, "edge_count"), 3, "edges preserved")
        assert_eq(q(tf, "node.0.op"), "Texture", "op preserved")
        assert_eq(q(tf, "node.2.op"), "Multiply", "op preserved")
        assert_eq(rgb(q(tf, "eval.output")), (64, 64, 64), "it still evaluates (grey64)")

        # ── (B) a modified but VALID blob is applied ─────────────────
        renamed = valid.replace('"title":"Texture"', '"title":"Albedo"')
        assert renamed != valid, "the rename edit changed the blob"
        assert_eq(set_graph(tf, renamed), True, "a valid modification is accepted")
        assert_eq(q(tf, "node.0.title"), "Albedo", "the rename applied")
        assert_eq(q(tf, "node.0.op"), "Texture", "op is unchanged by a rename (R1256 identity)")

        # ── (C) a STRUCTURALLY-INVALID blob is rejected ──────────────
        # Drop one of Multiply's two input ports: a wrong-arity op that would
        # evaluate to a permanent null. Only Multiply has [Vector,Vector] in the
        # seed graph, so this edit is unambiguous.
        bad_arity = renamed.replace('"input_ports":["Vector","Vector"]', '"input_ports":["Vector"]')
        assert bad_arity != renamed, "the arity edit changed the blob"
        reject(tf, bad_arity, "a wrong-arity op")
        assert_eq(q(tf, "node.0.title"), "Albedo", "the graph is unchanged after the reject")
        assert_eq(q(tf, "node_count"), 4, "no nodes were installed from the bad blob")
        assert_eq(q(tf, "node.2.inputs"), 2, "the surviving Multiply still has 2 inputs")
        assert_eq(rgb(q(tf, "eval.output")), (64, 64, 64), "and it still evaluates")

        # A duplicate node id: give the Output node (id 3) id 0 as well. The
        # node object serializes `"id":N,"title":...` (edges are `"id":N,
        # "from_node"`), so this edit hits exactly node 3.
        dup_id = renamed.replace('"id":3,"title"', '"id":0,"title"')
        assert dup_id != renamed, "the duplicate-id edit changed the blob"
        reject(tf, dup_id, "a duplicate node id")
        assert_eq(q(tf, "node_count"), 4, "no duplicate-id graph was installed")
        assert_eq(q(tf, "node.0.op"), "Texture", "node 0 is still the Texture source")

        # ── (D) an id counter behind a stored id is rejected ─────────
        stale = renamed.replace('"next_node_id":4', '"next_node_id":1')
        assert stale != renamed, "the counter edit changed the blob"
        reject(tf, stale, "a counter behind a stored id")
        assert_eq(q(tf, "node_count"), 4, "graph unchanged")
        assert_eq(q(tf, "node.0.title"), "Albedo", "still the last valid state")

        # ── (E) malformed JSON is rejected ───────────────────────────
        reject(tf, "{not json", "malformed JSON")
        reject(tf, '{"schema_version":999}', "a schema mismatch")
        assert_eq(q(tf, "node_count"), 4, "still unchanged after every reject")
        assert_eq(q(tf, "node.0.title"), "Albedo", "still the last VALID state (B)")
        assert_eq(q(tf, "eval.acyclic"), True, "the surviving graph is a DAG")

        # ── a final valid write proves the path still accepts good input ─
        assert_eq(set_graph(tf, valid), True, "a valid blob is still accepted after the rejects")
        assert_eq(q(tf, "node.0.title"), "Texture", "restored to the original valid graph")
        assert_eq(rgb(q(tf, "eval.output")), (64, 64, 64), "and it evaluates to the seed terminal")


if __name__ == "__main__":
    sys.exit(run_demo("r1258_node_set_graph_validation", body))
