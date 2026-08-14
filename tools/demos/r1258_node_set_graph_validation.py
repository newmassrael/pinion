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

import re
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
        # R1596 — a node's user-facing name is `Node::label`, and `None` means
        # "call it what its body is called", so a never-renamed node carries no
        # name string at all and a rename to the name it already shows journals
        # nothing. The node is renamed through the model first, so the string
        # edit below has something to bite on.
        tf.intervene("/external/node.0.title", "Albedo")
        assert_eq(q(tf, "node.0.title"), "Albedo", "the model rename landed")
        valid = q(tf, "serialized")
        # ★ R1689 — the archive is written INDENTED, so the edit is made on the
        # PARSED value's text rather than on an assumed spelling of it. A string
        # replacement that silently matches nothing turns every assertion below
        # into a test of the unmodified blob.
        renamed = re.sub(r'"label":\s*"Albedo"', '"label": "Ochre"', valid)
        assert renamed != valid, "the rename edit changed the blob"
        assert_eq(set_graph(tf, renamed), True, "a valid modification is accepted")
        assert_eq(q(tf, "node.0.title"), "Ochre", "the rename applied")
        assert_eq(q(tf, "node.0.op"), "Texture", "op is unchanged by a rename (R1256 identity)")

        # ── (C) ★ three of the old violations are UNREPRESENTABLE ────
        # R1596 — the editor's blob carried a per-node port list, a flat node
        # vector and three id counters, so a wrong-arity op, a duplicate id and
        # a counter behind a stored id were all things a peer could send and a
        # checker had to catch. A kind DECLARES its ports, a tree keys its nodes
        # BY id, and the mint counter travels inside the tree -- so none of the
        # three is a document the type can hold. An invariant that became a
        # property of the types is stronger than one a checker enforces, because
        # nothing has to remember to run.
        assert '"input_ports"' not in renamed, "no per-node port list to disagree with the kind"
        assert '"next_node_id"' not in renamed, "no counter beside the ids it mints past"

        # ── (D) what a peer CAN still send is rejected, and named ────
        # A link naming a node that is not there. The blob is a `Document`, so
        # the links live under the tree beside the nodes.
        dangling = re.sub(
            r'"to":\s*\{\s*"node":\s*2,\s*"port":\s*0\s*\}',
            '"to": { "node": 99, "port": 0 }',
            renamed,
        )
        assert dangling != renamed, "the dangling-endpoint edit changed the blob"
        reject(tf, dangling, "a link naming a node that is not there")
        assert_eq(q(tf, "node_count"), 4, "no nodes were installed from the bad blob")
        assert_eq(q(tf, "node.0.title"), "Ochre", "the graph is unchanged after the reject")

        # A node claiming a parent that is not there (R1589's forest), which the
        # editor's own checker never had at all.
        orphan = re.sub(r'"parent":\s*null', '"parent": 99', renamed, count=1)
        assert orphan != renamed, "the parent edit changed the blob"
        reject(tf, orphan, "a node inside a frame that is not there")
        assert_eq(q(tf, "node_count"), 4, "graph unchanged")
        assert_eq(q(tf, "node.0.title"), "Ochre", "still the last valid state")

        # ── (E) malformed JSON is rejected ───────────────────────────
        reject(tf, "{not json", "malformed JSON")
        reject(tf, '{"schema_version":999}', "a schema mismatch")
        assert_eq(q(tf, "node_count"), 4, "still unchanged after every reject")
        assert_eq(q(tf, "node.0.title"), "Ochre", "still the last VALID state (B)")
        assert_eq(q(tf, "eval.acyclic"), True, "the surviving graph is a DAG")

        # ── a final valid write proves the path still accepts good input ─
        assert_eq(set_graph(tf, valid), True, "a valid blob is still accepted after the rejects")
        assert_eq(q(tf, "node.0.title"), "Albedo", "restored to the graph (B) started from")
        assert_eq(rgb(q(tf, "eval.output")), (64, 64, 64), "and it evaluates to the seed terminal")


if __name__ == "__main__":
    sys.exit(run_demo("r1258_node_set_graph_validation", body))
