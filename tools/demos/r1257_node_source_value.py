#!/usr/bin/env python3
"""R1257 §5.38 §2 #2 #7 — authorable SOURCE constants (the output-side twin of R899).

R1255 evaluated the graph but source nodes (Texture / Color / Scalar) emitted a
fixed port-type constant (all grey / 0.0) — sterile. R1257 gives each source an
*authorable* output constant: the output-side twin of the R899 input pin
default. It is AI-first over the §5.12 plane (§2 #2), headless, no pixels:

  * `query node.<id>.is_source`  — which nodes carry an authorable constant.
  * `intervene node.<id>.value`   — author a source's output (typed, undoable);
    a compute op / sink rejects it `ReadOnly` (its value is DERIVED).
  * the whole graph re-evaluates — `node.<id>.value` / `eval.output` follow.

Phases:
  (A) boot: Texture/Color are sources; Multiply/Output are not; terminal grey64.
  (B) author the two sources; Multiply-by-white passes the Texture through.
  (C) authoring a DERIVED node's value is ReadOnly; a mistyped source value is
      a TypeMismatch.
  (D) a Scalar (Float) source authors with a float, rejects a hex.
  (E) a source edit is one undoable step.

Run from the workspace root:
    cargo build -p hello-node-editor --release
    python3 tools/demos/r1257_node_source_value.py
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

UNDO = "/node_undo/external"


def expect_error(fn, label: str) -> None:
    """Assert `fn()` raises an `RpcError` — a rejected `intervene`
    (`ReadOnly` / `TypeMismatch`) surfaces as a JSON-RPC error, not a value."""
    try:
        fn()
    except RpcError:
        return
    raise AssertionError(f"expected an RpcError: {label}")


def q(tf: RpcSubprocess, key: str) -> Any:
    return tf.query(f"/external/{key}")


def inv(tf: RpcSubprocess, verb: str, args: Any) -> Any:
    return tf.invoke(f"/external/{verb}", args)


def rgb(v: Any) -> tuple[int, int, int]:
    return (v["r"], v["g"], v["b"])


def body() -> None:
    with RpcSubprocess("hello-node-editor", boot_grace=1.5) as tf:
        # ── (A) boot: which nodes are authorable sources ─────────────
        assert_eq(q(tf, "node.0.is_source"), True, "Texture is a source")
        assert_eq(q(tf, "node.1.is_source"), True, "Color is a source")
        assert_eq(q(tf, "node.2.is_source"), False, "Multiply is a compute op")
        assert_eq(q(tf, "node.3.is_source"), False, "Output is a sink")
        # is_source is orthogonal to op identity (R1256): both sources, distinct ops.
        assert_eq(q(tf, "node.0.op"), "Texture", "node 0 op = Texture")
        assert_eq(q(tf, "node.1.op"), "Color", "node 1 op = Color")
        # Sources start at their type constant (grey); terminal = grey*grey/255.
        assert_eq(rgb(q(tf, "node.0.value")), (0x80, 0x80, 0x80), "Texture default grey")
        assert_eq(rgb(q(tf, "node.1.value")), (0x80, 0x80, 0x80), "Color default grey")
        assert_eq(rgb(q(tf, "eval.output")), (64, 64, 64), "terminal grey64")
        assert_eq(q(tf, "eval.acyclic"), True, "the seed graph is a DAG")

        # ── (B) author the sources -> the graph re-evaluates ─────────
        tf.intervene("/external/node.0.value", "#ff8040")  # Texture (255,128,64)
        tf.intervene("/external/node.1.value", "#ffffff")  # Color white (identity)
        assert_eq(rgb(q(tf, "node.0.value")), (255, 128, 64), "Texture now the authored colour")
        assert_eq(rgb(q(tf, "node.1.value")), (255, 255, 255), "Color now white")
        # Multiply by white is the identity, so the terminal is the Texture colour.
        assert_eq(rgb(q(tf, "node.2.value")), (255, 128, 64), "Multiply(tex, white) = tex")
        assert_eq(rgb(q(tf, "eval.output")), (255, 128, 64), "terminal followed the source edits")
        assert_eq(q(tf, "node.0.is_source"), True, "authoring a source does not change its is_source")
        assert_eq(q(tf, "eval.acyclic"), True, "still a DAG after the source edits")

        # ── (C) DERIVED values are read-only; sources are typed ──────
        expect_error(lambda: tf.intervene("/external/node.2.value", "#00ff00"),
                      "authoring a compute op's value is ReadOnly")
        expect_error(lambda: tf.intervene("/external/node.3.value", "#00ff00"),
                      "authoring the sink's value is ReadOnly")
        expect_error(lambda: tf.intervene("/external/node.0.value", 0.5),
                      "a float against a Vector source is a TypeMismatch")

        # ── (D) a Scalar (Float) source ──────────────────────────────
        scalar = inv(tf, "add_node", "Scalar")
        assert_eq(scalar, 4, "add Scalar -> node 4")
        assert_eq(q(tf, "node_count"), 5, "one node added")
        assert_eq(q(tf, "node.4.op"), "Scalar", "op = Scalar")
        assert_eq(q(tf, "node.4.is_source"), True, "Scalar is a source")
        assert_eq(q(tf, "node.4.value"), 0.0, "Scalar default 0.0")
        tf.intervene("/external/node.4.value", 0.75)
        assert_eq(q(tf, "node.4.value"), 0.75, "the Float authored the Scalar")
        expect_error(lambda: tf.intervene("/external/node.4.value", "#ff0000"),
                     "a hex against a Float source is a TypeMismatch")
        # The Scalar is disconnected, so the terminal is untouched by it.
        assert_eq(rgb(q(tf, "eval.output")), (255, 128, 64), "a disconnected source leaves the terminal")

        # ── (E) a source edit is one undoable step ───────────────────
        tf.intervene("/external/node.0.value", "#000000")  # Texture -> black
        assert_eq(rgb(q(tf, "node.0.value")), (0, 0, 0), "Texture is black")
        assert_eq(rgb(q(tf, "eval.output")), (0, 0, 0), "terminal follows to black")
        assert_eq(tf.invoke(f"{UNDO}/undo", None), True, "undo the source edit")
        assert_eq(rgb(q(tf, "node.0.value")), (255, 128, 64), "the source reverted to #ff8040")
        assert_eq(rgb(q(tf, "eval.output")), (255, 128, 64), "and the terminal with it")
        assert_eq(tf.invoke(f"{UNDO}/redo", None), True, "redo re-applies the black")
        assert_eq(rgb(q(tf, "node.0.value")), (0, 0, 0), "the source is black again")


if __name__ == "__main__":
    sys.exit(run_demo("r1257_node_source_value", body))
