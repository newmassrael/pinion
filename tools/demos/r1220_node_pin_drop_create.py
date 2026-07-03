#!/usr/bin/env python3
"""R1220 §5.52 §5.38 §5.49 — node-graph pin-drop node-creation menu (auto-wire).

The signature Unreal / Blender blueprint authoring gesture: drag a wire off an
OUTPUT pin and release on empty canvas, and a TYPE-FILTERED menu of the node
kinds that output can feed opens at the drop point; pick one and it is created
there AND auto-wired from the source pin to its first compatible input — as ONE
undo step. Before R1220 a wire released in empty space just cancelled; the graph
could only grow from the fixed palette (never contextually, from a pin).

Every half is AI-observable and AI-drivable over the §5.12 RPC plane (§2 #2):
`query pin_create` reads the open menu (candidates + drop point + highlight — the
introspection twin of the painted floating menu), and the verbs `open_pin_create`
/ `pin_create_filter` / `pin_create_highlight` / `commit_pin_create` /
`cancel_pin_create` are the AI-first peer of the live drag + keyboard, funnelling
through the SAME coordinator (invoke-funnel discipline).

The seed graph is `Texture x Color -> Multiply -> Output` (all `Vector` ports),
nodes 0=Texture 1=Color 2=Multiply 3=Output; the palette kinds are Texture(0)
Color(1) Multiply(2) Add(3) Output(4) Scalar(5) Lerp(6). Only the input-bearing
kinds (Multiply/Add/Output/Lerp) can consume a wire, so they are the candidates.

  (A) boot taxonomy — 4 nodes / 3 edges / menu closed / verbs schema-declared.
  (B) LIVE GESTURE — drag Texture.out onto empty canvas: the menu opens AT the
      drop point (graph coords), candidates are the type-compatible kinds, and
      the floating menu + its cards paint. Then cancel closes it.
  (C) type-to-narrow FILTER (RPC) — "add" -> [Add]; "" -> all four; "zzz" -> [].
  (D) roving HIGHLIGHT (RPC) — arrow-delta moves + wraps the active item.
  (E) GUI COMMIT by CLICKING a menu card — creates the node at the menu + wires
      it; ONE undo removes BOTH node and wire (atomic create+wire).
  (F) RPC COMMIT by kind NAME — returns the new node id; auto-wired.
  (G) KEYBOARD — type-to-filter narrows, Enter commits the sole match; Escape
      cancels an open menu (the menu is modal over the graph shortcuts).
  (H) a NON-candidate kind (a sourceless node) is REJECTED; the menu stays open.

Run from the workspace root:
    cargo build -p hello-node-editor --release
    python3 tools/demos/r1220_node_pin_drop_create.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_until,
)

VIEWPORT = (132 + 640, 420)
G = "node_graph"
UNDO = "/node_undo/external"


def node_count(tf) -> int:
    return tf.query("/external/node_count")


def edge_count(tf) -> int:
    return tf.query("/external/edge_count")


def edge_ids(tf) -> list[int]:
    csv = tf.query("/external/edge_ids")
    return [int(x) for x in csv.split(",")] if csv else []


def conns(tf) -> set[str]:
    """The live set of "from:fp->to:tp" connection strings."""
    return {tf.query(f"/external/edge.{eid}") for eid in edge_ids(tf)}


def menu(tf):
    """The open `pin_create` menu JSON, or None when closed."""
    return tf.query("/external/pin_create")


def candidates(tf) -> list[str]:
    m = menu(tf)
    return list(m["candidates"]) if m else []


def undo_count(tf) -> int:
    return tf.query(f"{UNDO}/count")


def undo(tf) -> bool:
    return tf.invoke(f"{UNDO}/undo", None)


def body() -> None:
    with RpcSubprocess("hello-node-editor", boot_grace=1.5) as tf:
        # ── (A) boot taxonomy ────────────────────────────────────────
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert find_by_tag(snap, G) is not None, "graph canvas present"
        assert_eq(node_count(tf), 4, "4 seed nodes")
        assert_eq(edge_count(tf), 3, "3 seed edges")
        assert_eq(menu(tf), None, "no menu open at boot")
        # `query pin_create` on the closed menu is a well-formed Null (not an
        # error): the schema declares it, the coordinator answers "closed".
        assert_eq(tf.query("/external/pin_create"), None, "pin_create reads Null when closed")

        # ── (B) LIVE GESTURE: drag Texture.out onto empty canvas ──────
        # Drop at window (432, 340) -> canvas-relative (300, 340) -> graph
        # (300, 340) at zoom 1 / no pan: a clearly empty region below the nodes.
        tf.drag(from_path=f"{G}#oport_0_0", to_at=(432.0, 340.0), steps=12)
        wait_until(lambda: menu(tf) is not None, timeout=4.0,
                   desc="the empty-canvas drop opened the create menu")
        m = menu(tf)
        assert_eq(m["from_node"], 0, "menu remembers the source node")
        assert_eq(m["from_port"], 0, "... and the source port")
        assert_eq(m["at"], {"x": 300, "y": 340}, "the node will land at the drop point")
        assert_eq(candidates(tf), ["Multiply", "Add", "Output", "Lerp"],
                  "only the input-bearing kinds a Vector can feed")
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert find_by_tag(snap, f"{G}#pin_menu") is not None, "the floating menu paints"
        assert find_by_tag(snap, f"{G}#create_2") is not None, "a Multiply card paints"
        assert_eq(tf.invoke("/external/cancel_pin_create", None), True, "cancel closes it")
        assert_eq(menu(tf), None, "the menu is closed after cancel")
        assert_eq(node_count(tf), 4, "the gesture + cancel created nothing")

        # ── (C) type-to-narrow FILTER (over RPC) ─────────────────────
        assert_eq(tf.invoke("/external/open_pin_create", "0.0"), True, "open for Texture.out")
        assert_eq(candidates(tf), ["Multiply", "Add", "Output", "Lerp"], "unfiltered")
        assert_eq(tf.invoke("/external/pin_create_filter", "add"), True, "filter 'add'")
        assert_eq(candidates(tf), ["Add"], "narrowed to the single title match")
        assert_eq(tf.invoke("/external/pin_create_filter", "zzz"), True, "filter 'zzz'")
        assert_eq(candidates(tf), [], "no title matches -> empty")
        assert_eq(tf.invoke("/external/pin_create_filter", ""), True, "clear the filter")
        assert_eq(candidates(tf), ["Multiply", "Add", "Output", "Lerp"], "all four again")

        # ── (D) roving HIGHLIGHT (over RPC), wrapping ─────────────────
        assert_eq(menu(tf)["highlight"], 0, "highlight starts at the top")
        assert_eq(tf.invoke("/external/pin_create_highlight", "2"), True, "rove +2")
        assert_eq(menu(tf)["highlight"], 2, "highlight moved to index 2")
        assert_eq(tf.invoke("/external/pin_create_highlight", "-3"), True, "rove -3 wraps")
        assert_eq(menu(tf)["highlight"], 3, "(2 - 3) mod 4 = 3 (wrapped)")

        # ── (E) GUI COMMIT by CLICKING a menu card ───────────────────
        # The menu is open (all four candidates). Click the Multiply card.
        before = edge_count(tf)
        steps = undo_count(tf)
        tf.click(path=f"{G}#create_2")  # PALETTE[2] = Multiply
        wait_until(lambda: menu(tf) is None, timeout=4.0, desc="the click committed + closed the menu")
        assert_eq(node_count(tf), 5, "a Multiply node was created")
        assert_eq(edge_count(tf), before + 1, "and auto-wired (one new edge)")
        assert "0:0->4:0" in conns(tf), "wired Texture.out -> the new node's first input"
        assert_eq(tf.query("/external/selected"), 4, "the new node is selected")
        assert_eq(tf.query(f"{UNDO}/undo_label"), "Add Multiply + wire", "one labelled step")
        assert_eq(undo_count(tf), steps + 1, "the create+wire is exactly ONE undo step")
        assert_eq(undo(tf), True, "undo the create")
        assert_eq(node_count(tf), 4, "one undo removed the node")
        assert_eq(edge_count(tf), before, "... and its wire, in the SAME step (atomic)")

        # ── (F) RPC COMMIT by kind NAME ──────────────────────────────
        assert_eq(tf.invoke("/external/open_pin_create", "1.0"), True, "open for Color.out")
        before = edge_count(tf)
        new_id = tf.invoke("/external/commit_pin_create", "Output")
        assert isinstance(new_id, int), "commit-by-name returns the new node id"
        assert_eq(menu(tf), None, "the menu closed on commit")
        assert_eq(edge_count(tf), before + 1, "auto-wired")
        assert f"1:0->{new_id}:0" in conns(tf), "wired Color.out -> the new Output node"
        assert_eq(undo(tf), True, "undo it (leave the graph clean for the next case)")

        # ── (G) KEYBOARD: type-to-filter + Enter, and Escape cancels ──
        assert_eq(tf.invoke("/external/open_pin_create", "0.0"), True, "open for Texture.out")
        tf.key(path=G, name="a")  # 'a' matches only "Add"
        wait_until(lambda: candidates(tf) == ["Add"], timeout=2.0,
                   desc="typing 'a' narrows the menu to Add")
        n_before = node_count(tf)
        tf.key(path=G, name="Enter")  # commit the sole match
        wait_until(lambda: menu(tf) is None, timeout=2.0, desc="Enter commits + closes")
        assert_eq(node_count(tf), n_before + 1, "Enter created the Add node")
        assert_eq(undo(tf), True, "undo the keyboard create")
        # Escape cancels an open menu (modal over the graph shortcuts).
        assert_eq(tf.invoke("/external/open_pin_create", "0.0"), True, "re-open")
        tf.key(path=G, name="Escape")
        wait_until(lambda: menu(tf) is None, timeout=2.0, desc="Escape cancels the menu")

        # ── (H) a NON-candidate kind is REJECTED; menu stays open ─────
        assert_eq(tf.invoke("/external/open_pin_create", "0.0"), True, "open once more")
        rejected = False
        try:
            tf.invoke("/external/commit_pin_create", "Texture")  # sourceless: not a candidate
        except RpcError:
            rejected = True
        assert rejected, "committing a non-candidate kind is rejected"
        assert menu(tf) is not None, "the menu stays open on a rejected commit"
        assert_eq(node_count(tf), 4, "the graph is unchanged")
        assert_eq(tf.invoke("/external/cancel_pin_create", None), True, "tidy up")


if __name__ == "__main__":
    sys.exit(run_demo("R1220 node-graph pin-drop create menu", body))
