#!/usr/bin/env python3
"""R1578 §5.38 §5.52 §2 #7 — a fragment of a graph is a value.

Copy, paste and duplicate look like three editor gestures and are one
question: can a piece of a node graph be lifted out and stand on its own? A
selection is not a list of nodes — it is nodes, the links BETWEEN them, the
links that CROSS its edge, and, when one of those nodes is a group instance,
the whole transitive closure of definitions it depends on. Lift only the nodes
and you have copied a group instance that points at nothing.

`pinion-node-graph` answers it with `Fragment`, which carries a whole
`Document`, and `hello-node-groups` supplies only where that value is KEPT.

What this script checks, and why each check discriminates:

* **The clipboard is data.** How many nodes, how many definitions came with
  them, exactly which wires were severed, and how many bytes it serializes
  to — all readable WITHOUT pasting. The DCC's clipboard is a `.blend` file
  written to the temp directory (`copybuffer_nodes.blend`, measured at
  `8cf50599`), so a paste is the only thing that can be done with it.
* **PAST the DCC (1): the cut is NAMED.** `node_copy_local` copies a link only
  when both of its ends are selected and records the others in no form at all,
  so a user who copies the middle of a chain is told nothing about the wires
  that went. Here `clipboard_severed` names every one, producer first.
* **PAST the DCC (2): a paste can RESTORE the inputs.** the DCC has that as
  `keep_inputs`, a boolean on `NODE_OT_duplicate` — and only there:
  `NODE_OT_clipboard_paste` declares one property, `offset`.
* **The asymmetry is DERIVED, not a missing boolean.** the DCC has
  `keep_inputs` and no `keep_outputs` and says nowhere why: an output may feed
  any number of consumers, so an inbound crossing costs the original nothing,
  while an input takes at most one link, so an outbound one would STEAL the
  original's connection. The sink keeps reading the original.
* **PAST the DCC (3): a definition is matched by CONTENT.** `BKE_main_merge`
  keys candidates on the datablock name and, for two local IDs,
  `are_ids_from_different_mains_matching` returns true on the name alone. Here
  pasting twice leaves ONE definition and three instances, and a definition
  edited since the copy is added rather than silently reused.
* **PAST the DCC (4): a refused paste changes NOTHING.** `node_copy_local`
  reports the node it cannot place, skips it and its links, and finishes — so a
  paste can land a partial graph plus a message in a report list. Here the
  recursion is refused whole, and the chain is named.
* **A cut is not a collapse.** The selection R1577's own bypass test refuses to
  GROUP is copied without complaint, because severing a crossing cannot create
  a cycle. R1577 fused the two questions; this round separates them.
* **A copy computes what the original computes**, group instance and all.

Run from the workspace root:
    cargo build -p hello-node-groups --release
    python3 tools/demos/r1578_a_fragment_is_a_value.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    access_node_by_tag,
    assert_eq,
    find_by_tag,
    run_demo,
    text_of_tag,
)

VIEWPORT = (900, 560)
#: The view's scene tag, which every painted node's tag is prefixed with.
VIEW = "nodegroups"
#: A widget's primary External is addressed by the framework path, not the tag.
EXT = "/external"

#: `hello-node-groups`'s seed, mirrored rather than imported — a demo that read
#: the fixture out of the code under test could not catch it changing.
BASE, BLEND, LEVEL, MIX, FADE, OUT = 0, 1, 2, 3, 4, 5

#: mix(200,60,60 @ 75% with 40,90,220 @ 25%) — what the seeded Mix answers.
SEEDED = "160,67,100"
#: A Mix on its port defaults: black under white at 50%.
UNFED = "127,127,127"


def q(tf: RpcSubprocess, path: str):
    return tf.query(f"{EXT}/{path}")


def inv(tf: RpcSubprocess, path: str, args):
    return tf.invoke(f"{EXT}/{path}", args)


def fields(reply: str) -> dict[str, str]:
    """`"nodes:1|links:0|added:2"` -> a dict. The insertion report's own form."""
    out: dict[str, str] = {}
    for piece in str(reply).split("|"):
        key, _, value = piece.partition(":")
        out[key] = value
    return out


def refused(tf: RpcSubprocess, path: str, args) -> str:
    """Run a verb that must be refused, and answer the recorded sentence.

    Read back through the READ channel rather than off the error frame, because
    the point is that the application can SHOW it.
    """
    try:
        inv(tf, path, args)
    except AssertionError:
        pass
    except Exception:  # noqa: BLE001 - any refusal shape is fine here
        pass
    sentence = q(tf, "last_refusal")
    assert sentence, f"{path}({args!r}) was expected to be refused and record why"
    return str(sentence)


def body() -> None:
    with RpcSubprocess("hello-node-groups", boot_grace=1.5) as tf:
        for _ in range(3):
            tf.tick(0.016)

        # ── (A) nothing is on the clipboard yet ─────────────────────────────
        assert_eq(q(tf, "clipboard"), "empty", "A: the clipboard starts empty")
        assert_eq(q(tf, "clipboard_bytes"), 0, "A: and weighs nothing")
        assert_eq(q(tf, "last_insert"), "", "A: nothing has been pasted")
        assert_eq(q(tf, "nodes"), 6, "A: the seeded material")
        assert_eq(inv(tf, "node_value", str(MIX)), SEEDED, "A: which evaluates")

        # ── (B) the cut is NAMED ────────────────────────────────────────────
        assert_eq(inv(tf, "select", str(MIX)), "1", "B: select the mix alone")
        assert_eq(
            inv(tf, "copy", ""),
            "1n/0d in:3 out:1",
            "B: one node, no definitions, three values in and one out",
        )
        assert_eq(
            q(tf, "clipboard_severed"),
            f"in:{BASE}.0>{MIX}.0;{BLEND}.0>{MIX}.1;{LEVEL}.0>{MIX}.2"
            f"|out:{MIX}.0>{FADE}.0",
            "B: every severed wire is named, producer first. Blender's "
            "node_copy_local drops these and records them nowhere",
        )
        assert q(tf, "clipboard_bytes") > 0, (
            "B: the fragment SERIALIZES — that is what makes it a clipboard "
            "entry rather than a handle into this process"
        )
        assert_eq(q(tf, "nodes"), 6, "B: copying moved nothing")
        assert_eq(q(tf, "valid"), "ok", "B: and the document is untouched")

        # ── (C) pasting without the crossings leaves the copy unfed ─────────
        first = fields(inv(tf, "paste", "600,320"))
        assert_eq(first["nodes"], "1", "C: one node landed")
        assert_eq(first["reattached"], "0", "C: no wire came back")
        assert_eq(first["unattached"], "", "C: and none was attempted")
        assert_eq(q(tf, "nodes"), 7, "C: the tree grew by one")
        copy_a = int(str(q(tf, "selection")))
        assert_eq(copy_a, 6, "C: the copy took the next free id")
        assert_eq(
            inv(tf, "node_value", str(copy_a)),
            UNFED,
            "C: so it sits on its own port defaults",
        )
        assert_eq(q(tf, "valid"), "ok", "C: invariants hold")

        # ── (D) pasting WITH them re-feeds the copy, and costs the original
        #        nothing ──────────────────────────────────────────────────────
        second = fields(inv(tf, "paste", "600,440,keep"))
        assert_eq(second["reattached"], "3", "D: all three inputs came back")
        assert_eq(second["unattached"], "", "D: none was refused")
        copy_b = int(str(q(tf, "selection")))
        assert_eq(
            inv(tf, "node_value", str(copy_b)),
            SEEDED,
            "D: the copy computes what the original computes",
        )
        assert_eq(
            inv(tf, "node_value", str(MIX)),
            SEEDED,
            "D: and the original still does — an output feeds any number of "
            "consumers, which is WHY the inbound half is restorable",
        )
        assert_eq(
            inv(tf, "node_value", str(OUT)),
            "",
            "D: the sink has no outputs of its own",
        )
        outbound_kept = str(q(tf, "clipboard_severed")).split("|out:")[1]
        assert_eq(
            outbound_kept,
            f"{MIX}.0>{FADE}.0",
            "D: the outbound crossing is PUBLISHED and never restored — an "
            "input takes one link, so restoring it would steal the "
            "original's. Blender has keep_inputs, no keep_outputs, and no "
            "statement of why",
        )
        assert_eq(q(tf, "valid"), "ok", "D: still consistent")

        # ── (E) a cut is not a collapse ─────────────────────────────────────
        assert_eq(inv(tf, "select", f"{BASE},{OUT}"), "2", "E: the two ends")
        bypass = refused(tf, "group", "Bad")
        assert "cycle" in bypass, f"E: grouping them is refused: {bypass!r}"
        assert_eq(
            inv(tf, "copy", ""),
            "2n/0d in:1 out:1",
            "E: and COPYING them is not, because severing a crossing cannot "
            "create a cycle. R1577 asked both questions with one derivation",
        )
        assert_eq(q(tf, "last_refusal"), "", "E: the copy was not refused")

        # ── (F) a copied group instance brings its definition ───────────────
        assert_eq(inv(tf, "select", str(MIX)), "1", "F: back to the mix")
        made = str(inv(tf, "group", "Blend"))
        # R1589 appended the containments the collapse could not carry, so the
        # pair is the first field rather than the whole reply.
        definition, instance = (int(p) for p in made.split("|")[0].split(":"))
        assert_eq(definition, 1, "F: the first definition")
        assert_eq(inv(tf, "select", str(instance)), "1", "F: select the instance")
        assert_eq(
            inv(tf, "copy", ""),
            "1n/1d in:3 out:1",
            "F: the DEFINITION travelled with it — a fragment holding only the "
            "node would be an instance pointing at nothing",
        )

        # ── (G) pasting twice leaves ONE definition ─────────────────────────
        assert_eq(inv(tf, "instances", str(definition)), 1, "G: one instance")
        third = fields(inv(tf, "paste", "620,150"))
        assert_eq(third["added"], "", "G: no definition was added")
        assert_eq(third["reused"], str(definition), "G: the existing one is used")
        fourth = fields(inv(tf, "paste", "620,260"))
        assert_eq(fourth["reused"], str(definition), "G: and again")
        assert_eq(q(tf, "trees"), 2, "G: still one definition in the document")
        assert_eq(
            inv(tf, "instances", str(definition)),
            3,
            "G: three instances of ONE definition — which is what a group IS. "
            "Blender matches a pasted definition by NAME",
        )
        assert_eq(q(tf, "valid"), "ok", "G: consistent")

        # ── (H) forking gives the copy a definition of its own ──────────────
        fifth = fields(inv(tf, "paste", "620,370,fork"))
        forked = int(fifth["added"])
        assert forked != definition, f"H: a fresh definition {forked}"
        assert_eq(fifth["reused"], "", "H: and nothing was reused")
        assert_eq(q(tf, "trees"), 3, "H: the document has two definitions now")
        assert_eq(
            inv(tf, "interface", str(forked)),
            inv(tf, "interface", str(definition)),
            "H: with the same derived interface, being a copy of it",
        )
        assert_eq(
            inv(tf, "instances", str(definition)),
            3,
            "H: the original definition kept exactly its instances",
        )
        assert_eq(inv(tf, "tree_name", str(forked)), "Blend", "H: and its name")

        # ── (I) pasting a group inside itself is refused, whole ─────────────
        assert_eq(inv(tf, "enter", str(instance)), str(definition), "I: go in")
        assert_eq(q(tf, "depth"), 1, "I: one level down")
        inside_before = q(tf, "nodes")
        trees_before = q(tf, "trees")
        recursion = refused(tf, "paste", "0,0")
        assert "nest a group inside itself" in recursion, (
            f"I: refused, and it says what it is: {recursion!r}"
        )
        assert str(definition) in recursion, (
            f"I: naming the definition in the chain: {recursion!r}"
        )
        assert_eq(q(tf, "nodes"), inside_before, "I: not one node landed")
        assert_eq(q(tf, "trees"), trees_before, "I: and no definition was added")
        assert_eq(
            q(tf, "valid"),
            "ok",
            "I: a refused paste leaves the document EXACTLY as it was. "
            "Blender's skips the offending node and finishes the rest",
        )
        assert_eq(inv(tf, "exit", ""), "0", "I: back out")

        # ── (J) duplicate is a cut and a paste ──────────────────────────────
        assert_eq(inv(tf, "select", str(instance)), "1", "J: the fed instance")
        dup = fields(inv(tf, "duplicate", "0,300,keep"))
        assert_eq(dup["nodes"], "1", "J: one copy")
        assert_eq(dup["reused"], str(definition), "J: sharing the definition")
        assert_eq(dup["reattached"], "3", "J: fed like the original")
        twin = int(str(q(tf, "selection")))
        assert_eq(
            inv(tf, "node_value", str(twin)),
            SEEDED,
            "J: and it computes what the original computes, through a GROUP",
        )
        assert_eq(inv(tf, "node_value", str(instance)), SEEDED, "J: as does it")
        assert_eq(q(tf, "valid"), "ok", "J: consistent throughout")

        # ── (K) a duplicate of nothing is refused by the CUT half ───────────
        assert_eq(inv(tf, "select", ""), "0", "K: select nothing")
        empty = refused(tf, "duplicate", "0,100")
        assert "nothing is selected" in empty, f"K: and it says so: {empty!r}"

        # ── (L) the clipboard outlives the document ─────────────────────────
        held = str(q(tf, "clipboard"))
        assert_eq(inv(tf, "reset", ""), "reset", "L: throw the document away")
        assert_eq(q(tf, "nodes"), 6, "L: back to the seed")
        assert_eq(q(tf, "trees"), 1, "L: with no definitions")
        assert_eq(q(tf, "last_insert"), "", "L: the insertion report is gone")
        assert_eq(
            q(tf, "clipboard"),
            held,
            "L: and the clipboard SURVIVES — a fragment is a value, not a view "
            "into the document it came from",
        )
        revived = fields(inv(tf, "paste", "620,380"))
        assert_eq(
            revived["added"],
            "1",
            "L: pasting it into the fresh document brings the definition back",
        )
        assert_eq(q(tf, "valid"), "ok", "L: and the result is consistent")

        # ── (M) it is on screen, and it reaches assistive technology ────────
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert snap, "M: the window paints"
        assert find_by_tag(snap, f"{VIEW}.status") is not None, (
            "M: the status line is painted"
        )
        status = text_of_tag(tf, f"{VIEW}.status", viewport=VIEWPORT)
        assert "clipboard" in status, f"M: and it says what is held: {status!r}"
        assert "1n/1d" in status, (
            f"M: including what the held fragment carries: {status!r}"
        )
        acc = tf.request("scene/access", {}).result or {}
        surface = access_node_by_tag(acc, VIEW)
        assert surface is not None, "M: the graph is an AT node"


if __name__ == "__main__":
    run_demo("r1578_a_fragment_is_a_value", body)
