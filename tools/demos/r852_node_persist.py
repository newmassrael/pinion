#!/usr/bin/env python3
"""R852 §5.38 §5.15 — node-graph serialization (save / load + AI-first blob).

Drives hello-node-editor over JSON-RPC. R851 made the structural edits
reversible; R852 makes the graph *persistent* — the other table-stakes a
self-hosted editor needs. The graph saves / loads through the §3 §5.15 Storage
substrate (a real `FileStorage` when the platform offers a data dir, the
in-memory fallback otherwise — the todomvc runtime-selection idiom, so the
binding genuinely reaches the file backend). The same `SerializedGraph` snapshot
is the AI-first `query serialized` / `invoke set_graph` read-write pair. Stable
ids (R841) + the persisted id counters mean a reload resumes minting cleanly.

  (A) cross-launch persistence — launch 1 grows the graph and `save`s to a real
      file; a *fresh* launch 2 (same storage dir) `load`s the 6-node graph back.
  (B) AI-first serialize-as-data — `query serialized` is the whole graph as one
      JSON blob; `invoke set_graph` restores it (the read-write twin); a
      malformed blob is rejected and changes nothing.
  (C) the Ctrl+S / Ctrl+O keyboard pairing saves and opens within a session.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    assert_eq,
    isolated_storage_dir,
    run_demo,
)

EXAMPLE = "hello-node-editor"
G = "node_graph"
STORAGE_KEY = "node_graph.state"


def ncount(tf) -> int:
    return tf.query("/external/node_count")


def ecount(tf) -> int:
    return tf.query("/external/edge_count")


def node_ids(tf) -> list[int]:
    csv = tf.query("/external/node_ids")
    return [int(x) for x in csv.split(",")] if csv else []


def body() -> None:
    with isolated_storage_dir("r852_node_persist") as sdir:
        # ── (A) cross-launch persistence (the real FileStorage) ──────
        with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
            assert_eq(ncount(tf), 4, "launch 1 boot: 4 seed nodes")
            assert_eq(ecount(tf), 3, "launch 1 boot: 3 seed edges")
            a = tf.invoke("/external/add_node", "Multiply")
            b = tf.invoke("/external/add_node", "Texture")
            assert_eq(ncount(tf), 6, "two nodes added before the save")
            # Connect the two new nodes so an edge persists too.
            assert_eq(tf.invoke("/external/add_edge", f"{b},0,{a},0"), True, "wire the new nodes")
            assert_eq(ecount(tf), 4, "the new edge landed")
            assert_eq(tf.invoke("/external/save", None), True, "save the graph to storage")

        # The Storage backend wrote a real file under the isolated dir.
        saved = sdir / STORAGE_KEY
        assert saved.exists(), f"FileStorage wrote {STORAGE_KEY} under {sdir}"
        assert saved.stat().st_size > 0, "the saved blob is non-empty"
        blob_text = saved.read_text(encoding="utf-8")
        assert "schema_version" in blob_text, "the on-disk blob is the snapshot JSON"

        # A fresh process, same storage dir: boot is the seed (no auto-load),
        # then `load` restores the saved 6-node / 4-edge graph.
        with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
            assert_eq(ncount(tf), 4, "launch 2 boot: fresh seed (no surprise auto-load)")
            assert_eq(tf.invoke("/external/load", None), True, "load the persisted graph")
            assert_eq(ncount(tf), 6, "the 6-node graph survived the relaunch")
            assert_eq(ecount(tf), 4, "the persisted edge survived too")
            ids = node_ids(tf)
            assert a in ids and b in ids, "the added stable ids survived the relaunch"
            assert_eq(tf.query(f"/external/node.{a}.title"), "Multiply", "node identity persisted")
            # The id counter resumed: a fresh add never reuses a persisted id.
            reborn = tf.invoke("/external/add_node", "Add")
            assert reborn not in ids, f"minted id {reborn} resumes past the persisted ids"

        # ── (B) AI-first serialize-as-data round-trip ────────────────
        with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
            blob = tf.query("/external/serialized")
            assert "schema_version" in blob, "serialized is the snapshot JSON"
            assert '"nodes"' in blob and '"edges"' in blob, "the blob carries the model"
            assert_eq(ncount(tf), 4, "boot graph for the round-trip")
            tf.invoke("/external/add_node", "Add")
            tf.invoke("/external/add_node", "Color")
            assert_eq(ncount(tf), 6, "mutated away from the snapshot")
            assert_eq(tf.invoke("/external/set_graph", blob), True, "set_graph restores the blob")
            assert_eq(ncount(tf), 4, "the wire round-trip reverted the graph")
            assert_eq(ecount(tf), 3, "edges reverted too")
            assert_eq(tf.query("/external/selected"), None, "selection dropped on load")
            # A malformed blob is rejected; the graph is untouched.
            rejected = False
            try:
                tf.invoke("/external/set_graph", "this is not json")
            except RpcError:
                rejected = True
            assert rejected, "a malformed set_graph is rejected"
            assert_eq(ncount(tf), 4, "the rejected set_graph changed nothing")

        # ── (C) Ctrl+S save / Ctrl+O open keyboard ───────────────────
        with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
            tf.request("focus/set", {"tag": G})
            tf.invoke("/external/add_node", "Multiply")
            assert_eq(ncount(tf), 5, "added a node to save")
            # Ctrl+S saves the 5-node graph (overwriting the section-A blob).
            tf.modifiers(ctrl=True)
            tf.key(path=G, name="s")
            tf.modifiers()
            # Mutate, then Ctrl+O reverts to the saved graph.
            tf.invoke("/external/add_node", "Color")
            assert_eq(ncount(tf), 6, "mutated after the Ctrl+S save")
            tf.modifiers(ctrl=True)
            tf.key(path=G, name="o")
            tf.modifiers()
            assert_eq(ncount(tf), 5, "Ctrl+O loaded the saved 5-node graph")


if __name__ == "__main__":
    sys.exit(run_demo("R852 §5.38 §5.15 — node-graph serialization (save / load)", body))
