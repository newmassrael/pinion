#!/usr/bin/env python3
"""R1264 §5.38 §2 #2 #7 — node-graph source-constant GUI authoring.

R1257 modelled an authorable SOURCE constant (`output_const`, the output-side
twin of the R899 input pin default) and made it writable over the AI-first
`intervene node.<id>.value` plane, but DEFERRED the GUI half: painting the
constant on the source card and editing it inline. R1264 lands that half by
reusing the R901 inline-editor machinery — the ONE shared field, its focus,
keymap, blur-commit, a11y, and (critically) the SAME
`apply_set_node_value` / `NodeValueTarget::OutputConst` write SSOT the AI path
uses, so the card editor and the RPC write can never drift.

Drives hello-node-editor over JSON-RPC (headless, no pixels beyond the paint
snapshot the framework already exposes):

  (A) boot — each SOURCE card paints its constant as an `oconst_<id>` value
      label; a compute op / sink paints none (its output is derived).
  (B) `invoke begin_edit_value <id>` opens the field seeded from the constant;
      `query editing` reports the `source_value` target; the field owns focus.
  (C) typing + Enter commits through the shared OutputConst SSOT (paint + value
      + `eval.output` follow); it is one undoable `Set source value` step.
  (D) a double-click on the `oconst_<id>` label re-opens the editor; the
      keystroke gate is the constant's own CellKind (a non-hex letter never
      reaches a Color source); Escape cancels without touching the value.
  (E) a Scalar (Float) source authors with a number; a letter is gated out.
  (F) the reject cases: a non-source node, an unknown node, a non-Int arg.

>= 30 assertions.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_until,
)

EXAMPLE = "hello-node-editor"
G = "node_graph"
EDIT = "node_edit"  # the shared inline edit field's tag
UNDO = "/node_undo/external"
VIEWPORT = (772, 420)


def editing(tf):
    return tf.query("/external/editing")


def value(tf, node_id: int):
    return tf.query(f"/external/node.{node_id}.value")


def editor_text(tf):
    return tf.query(f"/{EDIT}/external/text")


def const_label(tf, node_id: int):
    """The painted source-constant label's text for node `node_id`, or None."""
    node = find_by_tag(tf.snapshot(source="paint", viewport=VIEWPORT), f"{G}#oconst_{node_id}")
    if node is None:
        return None
    return node["children"][0]["content"]


def field_painted(tf) -> bool:
    return EDIT in abs_rects_of(tf.snapshot(source="paint", viewport=VIEWPORT))


def rejects(fn) -> bool:
    try:
        fn()
    except RpcError:
        return True
    return False


def retype(tf, new_text: str) -> None:
    """Erase the seeded value (caret parks at the end) and type a new one."""
    for _ in range(len(editor_text(tf))):
        tf.key(path=EDIT, name="Backspace")
    tf.text(new_text, path=EDIT)


def open_value(tf, node_id: int) -> None:
    assert_eq(tf.invoke("/external/begin_edit_value", node_id), True,
              f"begin_edit_value opens node {node_id}")
    wait_until(
        lambda: editing(tf) == {"kind": "source_value", "node": node_id, "surface": "card"},
        timeout=4.0, interval=0.03,
        desc=f"the editor opens on node {node_id}'s source value",
    )


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) boot — source cards paint their constant labels ──────
        assert_eq(editing(tf), None, "boot: no edit in flight")
        assert not field_painted(tf), "the shared field is unpainted while idle"
        assert_eq(const_label(tf, 0), "#808080", "the Texture source paints its grey constant")
        assert_eq(const_label(tf, 1), "#808080", "the Color source paints its grey constant")
        assert_eq(const_label(tf, 2), None, "the Multiply compute op paints no constant label")
        assert_eq(const_label(tf, 3), None, "the Output sink paints no constant label")
        assert_eq(tf.query("/external/node.1.is_source"), True, "node 1 is an authorable source")

        # ── (B) begin_edit_value opens the seeded field ──────────────
        open_value(tf, 1)  # the Color source
        wait_until(lambda: field_painted(tf), timeout=4.0, interval=0.03,
                   desc="the shared field paints over the source constant")
        assert_eq(editor_text(tf), "#808080", "seeded with the current constant's hex")
        assert_eq(tf.request("focus/get").result.get("focused"), EDIT,
                  "the field owns keyboard focus")

        # ── (C) typing + Enter commits (one undoable step) ──────────
        retype(tf, "#3366cc")
        wait_until(lambda: editor_text(tf) == "#3366cc", timeout=4.0, interval=0.03,
                   desc="keystrokes reach the shared field")
        tf.key(path=EDIT, name="Enter")
        wait_until(lambda: editing(tf) is None, timeout=4.0, interval=0.03,
                   desc="Enter leaves edit mode")
        v = value(tf, 1)
        assert_eq(v["r"], 0x33, "the commit authored the constant (r)")
        assert_eq(v["g"], 0x66, "the commit authored the constant (g)")
        assert_eq(v["b"], 0xCC, "the commit authored the constant (b)")
        assert not field_painted(tf), "the field unpaints after the commit"
        assert_eq(tf.request("focus/get").result.get("focused"), G, "focus returns to the canvas")
        assert_eq(const_label(tf, 1), "#3366cc", "the paint reflects the authored constant")
        # The Color source feeds Multiply -> Output; the whole graph re-evaluates
        # (the terminal is Multiply(grey, #3366cc), a Color) and stays a DAG.
        assert "hex" in tf.query("/external/eval.output"), "the terminal re-evaluated to a Color"
        assert_eq(tf.query("/external/eval.acyclic"), True, "still a DAG after the source edit")
        assert_eq(tf.query(f"{UNDO}/undo_label"), "Set source value", "journaled undoably")
        assert_eq(tf.invoke(f"{UNDO}/undo", None), True, "undo the source edit")
        assert_eq(value(tf, 1)["r"], 0x80, "undo restored the prior grey constant")
        assert_eq(const_label(tf, 1), "#808080", "the paint reverted with the undo")
        assert_eq(tf.invoke(f"{UNDO}/redo", None), True, "redo re-applies the authored constant")
        assert_eq(value(tf, 1)["r"], 0x33, "redo re-authored the constant")

        # ── (D) double-click re-opens; the keystroke gate is the type ─
        tf.double_click(path=f"{G}#oconst_1")
        wait_until(
            lambda: editing(tf) == {"kind": "source_value", "node": 1, "surface": "card"},
            timeout=4.0, interval=0.03, desc="double-clicking the constant label re-opens the editor",
        )
        assert_eq(editor_text(tf), "#3366cc", "re-seeded from the current constant")
        tf.key(path=EDIT, name="z")  # a non-hex letter is gated out of a Color field
        assert_eq(editor_text(tf), "#3366cc", "a non-hex keystroke never reaches a Color source")
        retype(tf, "#999999")
        tf.key(path=EDIT, name="Escape")
        wait_until(lambda: editing(tf) is None, timeout=4.0, interval=0.03, desc="Escape leaves edit mode")
        assert_eq(value(tf, 1)["r"], 0x33, "a cancel never touches the constant")

        # ── (E) a Scalar (Float) source authors with a number ────────
        scalar = tf.invoke("/external/add_node", "Scalar")
        assert_eq(scalar, 4, "add Scalar -> node 4")
        assert_eq(tf.query("/external/node.4.is_source"), True, "the Scalar is a source")
        assert_eq(const_label(tf, 4), "0", "the Scalar source paints its 0 constant")
        open_value(tf, 4)
        assert_eq(editor_text(tf), "0", "seeded with the Scalar's current value")
        tf.key(path=EDIT, name="x")  # a letter is gated out of a Float field
        assert_eq(editor_text(tf), "0", "a non-numeric keystroke never reaches a Float source")
        retype(tf, "0.5")
        tf.key(path=EDIT, name="Enter")
        wait_until(lambda: editing(tf) is None, timeout=4.0, interval=0.03, desc="Scalar commit leaves edit mode")
        assert_eq(value(tf, 4), 0.5, "the typed number authored the Scalar source")
        assert_eq(const_label(tf, 4), "0.5", "the Scalar label reflects the authored value")

        # ── (F) the reject cases ─────────────────────────────────────
        assert_eq(tf.invoke("/external/begin_edit_value", 2), False,
                  "a compute op (Multiply) has no constant to edit")
        assert_eq(tf.invoke("/external/begin_edit_value", 3), False,
                  "the sink (Output) has no constant to edit")
        assert_eq(tf.invoke("/external/begin_edit_value", 999), False,
                  "an unknown node is rejected")
        assert_eq(editing(tf), None, "no reject opened an editor")
        assert rejects(lambda: tf.invoke("/external/begin_edit_value", "1")), \
            "a non-Int arg is a type mismatch"


if __name__ == "__main__":
    sys.exit(run_demo("R1264 §5.38 §2 #2 #7 — node-graph source-constant GUI authoring", body))
