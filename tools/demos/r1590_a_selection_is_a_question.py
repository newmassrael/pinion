#!/usr/bin/env python3
"""R1590 §5.38 §5.52 §2 #3 §2 #7 — a selection grows by asking the graph a
question.

The selection itself is deliberately NOT in the model: R1586 put it outside the
document because two people looking at one graph have two selections and one
document. What belongs to the graph is the other half — *given these nodes,
which others are the ones you mean?* Blender spells that as six operators; four
of them are questions about the graph and are now `Document::grow`, and the
other two are questions about the CANVAS (see the last bullet).

What this script checks, and why each check discriminates:

* **Every one of them is a pure query.** Blender's set the `SELECT` bit on
  `bNode` and carry `OPTYPE_UNDO`, because the selection lives in its document —
  so "what would this select?" cannot be asked there without selecting it, and
  every answer costs an undo step. Here the document is untouched, which is what
  makes the question previewable (§2 #3); asserted by reading the whole graph
  back after every growth.
* **PAST BLENDER (1): the reach is a parameter, not a keystroke count.**
  `NODE_OT_select_linked_to` walks `directly_linked_sockets()` — one hop — and
  so does `..._from`. The question a person has is *what depends on this*, which
  is the closure, and Blender answers it by having you press the key until the
  picture stops changing with nothing telling you when that has happened. Here
  it is one call, and `added` empty is the signal Blender's mutating form cannot
  give.
* **PAST BLENDER (2): two instances of DIFFERENT definitions are not one kind.**
  Every group node in Blender is `type_legacy == NODE_GROUP`, so grouping by
  type sweeps in instances of unrelated definitions. An instance's signature IS
  its definition's interface, so here they are alike in nothing the model sees.
* **PAST BLENDER (3): an affix that is not there offers no criterion.**
  `node_select_grouped_name` substitutes the WHOLE NAME for a missing suffix,
  conflating "no suffix" with "the suffix is the entire name".
* **PAST BLENDER (4): the affix is read off the name that is PAINTED.** Blender
  groups on `bNode::name`, the datablock id (`Mix.001`), which is not what its
  own node header draws — the header shows the label or the type's ui name.
* **PAST BLENDER (5): the run is published.** `same_kind` answers "3 of 7" plus
  the whole ordered run, so `NODE_OT_select_same_type_step` is one line over it.
  That operator reports by moving the active node and says only whether it
  moved.
* **The two relations cannot collide.** A frame has no ports, so no link reaches
  one; containment only ever relates a frame to its members. Both directions are
  driven and each is shown not to answer with the other's nodes.
* **A muted link is still a wire.** R1586: mutedness is about the value, and
  every structural derivation goes on seeing the wire. Driven on a wire that has
  just been shown to stop a value.
* **Not here, and where it belongs**: `select_circle` / `select_lasso` test a
  region against `node->runtime->draw_bounds`. R1589 recorded that a node's
  extent is the application's; these two are that same fact and belong to the
  layer that knows what was painted where, not to the node model.

Run from the workspace root:
    cargo build -p hello-node-groups --release
    python3 tools/demos/r1590_a_selection_is_a_question.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    run_demo,
)

#: A widget's primary External is addressed by the framework path, not the tag.
EXT = "/external"

#: `hello-node-groups`'s seed, mirrored rather than imported — a demo that read
#: the fixture out of the code under test could not catch it changing.
BASE, BLEND, LEVEL, MIX, FADE, OUT = 0, 1, 2, 3, 4, 5

#: mix(200,60,60 @ 75% with 40,90,220 @ 25%) — what the seeded Mix answers.
SEEDED = "160,67,100"


def q(tf: RpcSubprocess, path: str):
    return tf.query(f"{EXT}/{path}")


def inv(tf: RpcSubprocess, path: str, args):
    return tf.invoke(f"{EXT}/{path}", args)


def fields(reply: str, sep: str = " ") -> dict[str, str]:
    out: dict[str, str] = {}
    for piece in str(reply).split(sep):
        if not piece:
            continue
        key, _, value = piece.partition(":")
        out[key] = value
    return out


def select(tf: RpcSubprocess, *nodes: int) -> None:
    inv(tf, "select", ",".join(str(n) for n in nodes))


def grow(tf: RpcSubprocess, word: str) -> dict[str, str]:
    return fields(str(inv(tf, "grow", word)), "|")


def selection(tf: RpcSubprocess) -> list[int]:
    raw = str(q(tf, "selection"))
    return [int(n) for n in raw.split(",")] if raw else []


def shape(tf: RpcSubprocess) -> tuple:
    """Everything about the graph a growth must not change."""
    return (
        q(tf, "nodes"),
        q(tf, "links"),
        q(tf, "trees"),
        q(tf, "frames"),
        q(tf, "bypassed"),
        q(tf, "muted_links"),
        q(tf, "valid"),
        inv(tf, "node_value", str(MIX)),
    )


def refused(tf: RpcSubprocess, path: str, args) -> str:
    try:
        inv(tf, path, args)
    except Exception:  # noqa: BLE001 - any refusal shape is fine; the reason is read back
        pass
    else:
        raise AssertionError(f"{path}({args!r}) was expected to be refused")
    return str(q(tf, "last_refusal"))


def body() -> None:
    with RpcSubprocess("hello-node-groups", boot_grace=1.5) as tf:
        for _ in range(3):
            tf.tick(0.016)

        # ── (A) one hop, which is all Blender has ───────────────────────────
        before = shape(tf)
        select(tf, BASE)
        step = grow(tf, "downstream:direct")
        assert_eq(step["added"], str(MIX), "A: one hop reaches the Mix")
        assert_eq(selection(tf), [BASE, MIX], "A: and the selection is both")
        assert_eq(
            shape(tf),
            before,
            "A: the graph is untouched. This is the consistency check; the "
            "GUARANTEE is that `Document::grow` takes `&self` (the crate holds "
            "a compile-time witness for it). Blender's select ops take the tree "
            "mutably, set the SELECT bit on bNode and carry OPTYPE_UNDO, so the "
            "question cannot be asked there without answering it (§2 #3)",
        )

        # Keep pressing, which is Blender's only way to reach the far end.
        assert_eq(grow(tf, "downstream:direct")["added"], str(FADE), "A: two")
        assert_eq(grow(tf, "downstream:direct")["added"], str(OUT), "A: three")
        assert_eq(grow(tf, "downstream:direct")["added"], "", "A: and now nothing")
        walked = selection(tf)

        # ── (B) the reach as a parameter ────────────────────────────────────
        select(tf, BASE)
        once = grow(tf, "downstream:transitive")
        assert_eq(
            once["added"],
            f"{MIX},{FADE},{OUT}",
            "B: PAST BLENDER — the whole closure in ONE call, where the reach "
            "is a keystroke count",
        )
        assert_eq(selection(tf), walked, "B: the same answer three keypresses gave")
        assert_eq(
            grow(tf, "downstream:transitive")["added"],
            "",
            "B: and asking again adds nothing — the signal that the walk is "
            "finished, which a mutating operator cannot give",
        )

        # ── (C) upstream is the other direction of the same relation ────────
        select(tf, OUT)
        up = grow(tf, "upstream:transitive")
        assert_eq(up["added"], f"{BASE},{BLEND},{LEVEL},{MIX},{FADE}", "C: every source")
        select(tf, OUT)
        assert_eq(grow(tf, "upstream:direct")["added"], str(FADE), "C: or one hop")

        # ── (D) a muted wire is still a way to a node ───────────────────────
        # Link 3 is Mix -> Fade. Muting it makes Fade's input fall back to its
        # own default, which is a different colour: the value has stopped.
        assert_eq(inv(tf, "node_value", str(FADE)), SEEDED, "D: before")
        inv(tf, "mute_link", "3")
        assert_eq(inv(tf, "node_value", str(FADE)), "0,0,0", "D: the value has stopped")
        select(tf, MIX)
        assert_eq(
            grow(tf, "downstream:direct")["added"],
            str(FADE),
            "D: R1586 — mutedness is about the VALUE; every structural "
            "derivation goes on seeing the wire",
        )
        inv(tf, "mute_link", "3")
        assert_eq(inv(tf, "node_value", str(FADE)), SEEDED, "D: put back")

        # ── (E) same kind is what a node DOES ───────────────────────────────
        select(tf, BASE)
        kin = grow(tf, "same_kind")
        assert_eq(
            kin["added"],
            str(BLEND),
            "E: the other Swatch — two settings of one kind, never the setting",
        )
        assert_eq(
            inv(tf, "same_kind", str(BASE)),
            f"at:1 of:2 run:{BASE},{BLEND}",
            "E: PAST BLENDER — the RUN is published in evaluation order, with "
            "the subject's place in it. NODE_OT_select_same_type_step answers "
            "by moving the active node and reports only whether it moved",
        )

        # ── (F) two instances of different definitions are not one kind ─────
        select(tf, MIX)
        made = str(inv(tf, "group", "Stage")).split("|")[0].split(":")
        definition, instance = int(made[0]), int(made[1])
        select(tf, FADE)
        other = str(inv(tf, "group", "Other")).split("|")[0].split(":")
        twin = int(other[1])
        select(tf, instance)
        assert_eq(
            grow(tf, "same_kind")["added"],
            "",
            "F: PAST BLENDER — every group node in Blender is "
            "type_legacy == NODE_GROUP, so grouping by type sweeps in instances "
            "of unrelated definitions",
        )
        assert twin not in selection(tf), "F: read back off the selection"
        placed = int(inv(tf, "instantiate", str(definition)))
        select(tf, instance)
        assert_eq(
            grow(tf, "same_kind")["added"],
            str(placed),
            "F: the SAME definition, though, is the same kind",
        )

        # ── (G) affixes, read off the painted name ──────────────────────────
        # `hello-node-groups` has no rename verb, so the affixes are exercised
        # against the names the cards actually show — the body's own.
        select(tf, LEVEL)
        assert_eq(
            grow(tf, "prefix")["added"],
            "",
            "G: `Level` has no delimiter, so it has no prefix and offers no "
            "criterion. PAST BLENDER — node_select_grouped_name substitutes the "
            "WHOLE NAME for a missing suffix, conflating 'no suffix' with 'the "
            "suffix is the entire name'",
        )
        assert_eq(grow(tf, "suffix")["added"], "", "G: and the same on the other end")

        # ── (H) the two relations cannot collide ────────────────────────────
        select(tf, BASE, BLEND)
        fence = int(str(inv(tf, "frame", "sources")).split(":")[0])
        select(tf, fence)
        inside = grow(tf, "contents:transitive")
        assert_eq(inside["added"], f"{BASE},{BLEND}", "H: containment answers")
        select(tf, fence)
        assert_eq(
            grow(tf, "downstream:transitive")["added"],
            "",
            "H: and the link relation answers nothing for a frame — it has no "
            "ports, so no link ever reaches or leaves one",
        )
        select(tf, BASE)
        assert_eq(
            grow(tf, "containers:direct")["added"],
            str(fence),
            "H: the other direction of containment",
        )
        # A frame's signature has to be asserted where the tree HAS one, because
        # the root's interface is empty and a frame wrongly wearing its tree's
        # ports would answer identically there — zero discrimination, which is
        # how D-CF-11 passed against the first draft of this phase (and how
        # R1589's own CF-9 passed against the crate test it has since fixed).
        inv(tf, "enter", str(instance))
        assert_eq(q(tf, "depth"), 1, "H: inside a definition")
        assert (
            "|" in str(inv(tf, "interface", str(definition)))
        ), "H: which HAS interface ports, so the check can fail"
        select(tf, MIX)
        deep_fence = int(str(inv(tf, "frame", "inside")).split(":")[0])
        assert_eq(
            inv(tf, "node_ports", str(deep_fence)),
            "in:|out:",
            "H: a frame has no ports OF ITS OWN, in a tree that has ports",
        )
        inv(tf, "exit", "")
        assert_eq(q(tf, "depth"), 0, "H: back out")

        select(tf, BASE)
        by_link = grow(tf, "downstream:transitive")
        reached = [int(n) for n in by_link["added"].split(",") if n]
        assert reached, f"H: the walk has to reach SOMETHING to discriminate: {by_link}"
        assert fence not in reached, (
            f"H: and a link walk never answers with a frame: {by_link}"
        )

        # ── (I) refusals ────────────────────────────────────────────────────
        reason = refused(tf, "grow", "sideways")
        assert "not a way to grow" in reason, f"I: {reason!r}"
        reason = refused(tf, "grow", "downstream:everywhere")
        assert "not a reach" in reason, f"I: {reason!r}"
        assert_eq(q(tf, "valid"), "ok", "I: and nothing was touched by asking")

        # ── (J) the whole thing left the graph exactly as it found it ───────
        select(tf)
        assert_eq(selection(tf), [], "J: an empty selection is a legal question")
        assert_eq(grow(tf, "same_kind")["added"], "", "J: with an empty answer")
        # `mix` and `fade` were each collapsed in (F), so the last computing
        # node before the Output is that second instance — and it still answers
        # what the pipeline answered before any of this.
        assert_eq(
            inv(tf, "node_value", str(twin)),
            SEEDED,
            "J: one value throughout — nothing in this round could change one",
        )
        assert_eq(q(tf, "valid"), "ok", "J: and one valid document")


if __name__ == "__main__":
    run_demo("r1590_a_selection_is_a_question", body)
