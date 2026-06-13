#!/usr/bin/env python3
"""R916 §5.38 §5.40 — node-editor Details panel (select -> inspect -> edit).

Drives `hello-node-editor` via JSON-RPC. The node-graph editor gains the Unreal
"Details panel": a right sidebar that reflects the *single selected* node's
editable properties (title / position / per-port defaults) as rows. Addressing
is selection-relative — `detail.<field>` resolves against the selected node (the
R909 inspector pattern, now inside the graph editor) and is `Null` when the
selection is not exactly one node. Each `detail.<field>` is also intervene-able:
a write routes through the identical `node.<selected>.<field>` funnel the
absolute path uses (rename / move), so a card edit, a drag, an `intervene
node.<id>`, and an `intervene detail.<field>` are one mutation path.

Witness (§2 #7 scene-as-data): the panel's rows + the selected node's property
values are observable through `/external/{detail.*, node.<id>.*, selected}`. The
panel re-reflects live as the model changes (the reactive `use_nodes` /
`use_selection` Signals drive the view).

  (A) boot — panel present; nothing selected -> detail.* Null + placeholder.
  (B) select a node — detail.* reflects it; the property rows paint.
  (C) selection-relative alias — detail.<field> == node.<selected>.<field>.
  (D) edit via the panel — intervene detail.title / detail.x writes the node.
  (E) re-select — the panel follows the selection (different arity).
  (F) multi-select / clear — detail.* Null; the placeholder returns.

>= 30 assertions.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    find_by_tag,
    run_demo,
)

# The window is palette (132) + canvas (640) + details (192) = 964 wide.
VIEWPORT = (964, 420)
G = "node_graph"
DETAIL = "node_details"


def gq(tf, slot):
    return tf.query(f"/external/{slot}")


def select(tf, ids: str) -> None:
    tf.intervene("/external/selected_ids", ids)


def has_tag(tf, tag) -> bool:
    return find_by_tag(tf.snapshot(source="paint", viewport=VIEWPORT), tag) is not None


def body() -> None:
    with RpcSubprocess("hello-node-editor", boot_grace=1.5) as tf:
        # ── (A) boot — panel present, nothing selected ──────────────
        assert_eq(gq(tf, "node_count"), 4, "4 nodes")
        assert has_tag(tf, DETAIL), "the Details panel is present"
        assert_eq(gq(tf, "selected"), None, "nothing selected at boot")
        assert_eq(gq(tf, "detail.node"), None, "detail.node Null with no selection")
        assert_eq(gq(tf, "detail.title"), None, "detail.title Null with no selection")
        assert_eq(gq(tf, "detail.x"), None, "detail.x Null with no selection")
        assert not has_tag(tf, f"{DETAIL}#title"), "no property rows while nothing is selected"

        # ── (B) select node 2 (Multiply, 2 Vector inputs at x=250) ──
        select(tf, "2")
        assert_eq(gq(tf, "selected"), 2, "node 2 is the single selection")
        assert_eq(gq(tf, "detail.node"), 2, "detail.node is the selected id")
        assert_eq(gq(tf, "detail.title"), "Multiply", "detail.title reflects node 2")
        assert_eq(gq(tf, "detail.x"), 250, "detail.x reflects node 2")
        assert_eq(gq(tf, "detail.y"), 110, "detail.y reflects node 2")
        assert_eq(gq(tf, "detail.inputs"), 2, "detail.inputs reflects node 2's arity")
        assert has_tag(tf, f"{DETAIL}#title"), "the Title row paints"
        assert has_tag(tf, f"{DETAIL}#x"), "the Position X row paints"
        assert has_tag(tf, f"{DETAIL}#y"), "the Position Y row paints"
        assert has_tag(tf, f"{DETAIL}#in_0"), "the input-0 default row paints"
        assert has_tag(tf, f"{DETAIL}#in_1"), "the input-1 default row paints"

        # ── (C) selection-relative alias == absolute address ────────
        assert_eq(gq(tf, "detail.title"), gq(tf, "node.2.title"), "detail.title == node.2.title")
        assert_eq(gq(tf, "detail.x"), gq(tf, "node.2.x"), "detail.x == node.2.x")
        assert_eq(gq(tf, "detail.inputs"), gq(tf, "node.2.inputs"), "detail.inputs == node.2.inputs")
        assert_eq(gq(tf, "detail.input_default.0"), gq(tf, "node.2.input_default.0"),
                  "detail.input_default.0 == node.2.input_default.0")

        # ── (D) edit via the panel — intervene detail.<field> ───────
        tf.intervene("/external/detail.title", "Albedo")
        assert_eq(gq(tf, "node.2.title"), "Albedo", "intervene detail.title renamed node 2")
        assert_eq(gq(tf, "detail.title"), "Albedo", "the panel reflects the rename")
        tf.intervene("/external/detail.x", 300)
        assert_eq(gq(tf, "node.2.x"), 300, "intervene detail.x moved node 2")
        assert_eq(gq(tf, "detail.x"), 300, "the panel reflects the move")

        # ── (E) re-select node 0 (Texture, 0 inputs) ────────────────
        select(tf, "0")
        assert_eq(gq(tf, "detail.node"), 0, "detail.node follows the selection")
        assert_eq(gq(tf, "detail.title"), "Texture", "detail.title now reflects node 0")
        assert_eq(gq(tf, "detail.inputs"), 0, "node 0 has no input ports")
        assert not has_tag(tf, f"{DETAIL}#in_0"), "no input-default rows for a source node"
        assert has_tag(tf, f"{DETAIL}#title"), "the Title row still paints"

        # ── (F) multi-select then clear — detail.* Null + placeholder ─
        select(tf, "0, 2")
        assert_eq(gq(tf, "detail.node"), None, "a multi-selection has no single detail node")
        assert_eq(gq(tf, "detail.title"), None, "detail.title Null under multi-select")
        assert not has_tag(tf, f"{DETAIL}#title"), "no property rows under multi-select"
        assert has_tag(tf, DETAIL), "the panel itself stays present (placeholder)"
        select(tf, "")
        assert_eq(gq(tf, "detail.node"), None, "cleared selection -> detail.node Null")
        assert_eq(gq(tf, "selected"), None, "selection cleared")


if __name__ == "__main__":
    sys.exit(run_demo("R916 §5.38 — node-editor Details panel", body))
