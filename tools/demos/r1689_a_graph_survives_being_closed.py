#!/usr/bin/env python3
"""R1689 §5.15 §5.21 — a graph survives being closed, and an archive that will
not open says which of four things stopped it.

Drives `hello-node-lab` over JSON-RPC, twice, against a real file.

The reference publishes its own list of this screen's operations — thirty
entries, which `spec.operations` mirrors one-for-one and which R1688 finished.
**Saving is not on that list**, and the reference nonetheless has it: a save, a
load, an import from pasted text, a reset-to-default, and a meter of its own
that asks whether every piece of state is either carried or declared volatile.
A census taken over a declared list is complete against that list and blind to
whatever the list leaves out, which is R1688's finding one level up.

What this round built is not the save — a `Document` has derived serde since
R1577, so writing one is a call. It is the READ. Two node canvases in this tree
hand-rolled the same envelope, and the node editor answered a failed load with
`false` for four different reasons: the text was not JSON, the version did not
match, the graph broke an invariant, or nothing was stored. The reference
toolkit's own state restore has exactly that shape, runs its check pass twice —
once privately, to find out whether the blob is usable — and hands the caller
none of it.

`pinion_node_graph::Archive::read` does that pass and returns it:

  (A) the partition — what a save carries and what it deliberately does not,
      published, and covering exactly what the operations move.
  (B) the three seats, where the reference puts them, pressed at their CORNERS,
      and named to a screen reader.
  (C) at boot nothing is stored, and the screen can still say what it WOULD
      write.
  (D) save — a real file, under an isolated directory, byte-identical to what
      the screen said it would write.
  (E) a fresh process opens with the seeded graph (no surprise auto-load), and
      then opens the file: every card, link, form value and host comes back.
  (F) four refusals, four sentences, and the graph untouched by each.
  (G) ★ the graph opens when the SCREEN's own saved state does not — the half a
      front-to-back stream cannot do — and says what it left behind.
  (H) the volatile half: an artifact does not come back, which is what the
      partition claims about it.
  (I) clear — back to the graph the screen opens with, and the file is gone.
  (J) the seats and the verbs are the same act.

>= 30 assertions.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    isolated_storage_dir,
    run_demo,
)

EXAMPLE = "hello-node-lab"
EXT = "/external"
# The one key the whole graph is written under — `persist::STORAGE_KEY`.
STORAGE_KEY = "node_lab.graph"

# The three seats the reference groups into a pill of its own, in its order.
SEATS = ["lab.toolbar.save", "lab.toolbar.open", "lab.toolbar.clear"]

# ★ Read from the screen rather than written here: the toolbar sets this
# screen's minimum width and this round moved it again.
_DESIGN: tuple[int, int] | None = None


def q(tf, path):
    return tf.query(f"{EXT}/{path}")


def viewport(tf) -> tuple[int, int]:
    global _DESIGN
    if _DESIGN is None:
        design = json.loads(q(tf, "spec"))["design"]
        _DESIGN = (design[0], design[1])
    return _DESIGN


def rects(tf):
    return abs_rects_of(tf.snapshot(source="paint", viewport=viewport(tf)))


def press(tf, tag):
    box = rects(tf)[tag]
    tf.click(at=(box[0] + box[2] // 2, box[1] + box[3] // 2))


def resolves(tf, at) -> str:
    return tf.invoke(f"{EXT}/point", f"{at[0]},{at[1]}")


def access(tf):
    resp = tf.request("scene/access", {})
    assert resp is not None and resp.result is not None, "scene/access must answer"
    return {node["tag"]: node for node in resp.result["nodes"] if node.get("tag")}


def refused(tf, verb, arg) -> str:
    """The sentence a refused action answers with."""
    try:
        said = tf.invoke(f"{EXT}/{verb}", arg)
    except Exception as e:  # noqa: BLE001 — the transport's own refusal type
        return str(e)
    raise AssertionError(f"{verb}({arg[:40]!r}…) was ACCEPTED: {said}")


def body() -> None:
    with isolated_storage_dir("r1689_node_lab_graph") as sdir:
        # ────────────────────────────────────────────────── launch 1
        with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
            spec = json.loads(q(tf, "spec"))

            # ── (A) the partition ───────────────────────────────────
            kept = {row["witness"]: row for row in spec["kept"]}
            moved = {op["witness"] for op in spec["operations"]}
            assert_eq(
                sorted(kept),
                sorted(moved),
                "★★ every slot an operation moves is classified, and nothing "
                "else is — a slot nobody classified is a save with a hole in it",
            )
            assert all(row["keeps"] in ("saved", "volatile") for row in kept.values()), kept
            assert all(row["why"] for row in kept.values()), (
                "★ each row says WHERE it lives or why it is left — the half a "
                f"bare partition cannot say: {kept}"
            )
            volatile = [w for w, row in kept.items() if row["keeps"] == "volatile"]
            assert volatile, (
                "★ at least one slot is deliberately NOT carried, or the "
                "partition has only one half and proves nothing"
            )

            # ── (B) the three seats ─────────────────────────────────
            painted = rects(tf)
            for tag in SEATS:
                assert tag in painted, f"{tag} is painted: {sorted(painted)[:14]}…"
            boxes = [painted[tag] for tag in SEATS]
            assert_eq(
                len({b[1] for b in boxes}),
                1,
                f"★ one pill, one row — the reference's own grouping: {boxes}",
            )
            for left, right in zip(boxes, boxes[1:]):
                assert left[0] + left[2] <= right[0], f"and in order: {boxes}"
            script_box, run_box = painted["lab.toolbar.script"], painted["lab.toolbar.run"]
            assert script_box[0] + script_box[2] <= boxes[0][0], (
                f"★ the pill sits after the launch-script button: {script_box} "
                f"then {boxes[0]}"
            )
            assert boxes[-1][0] + boxes[-1][2] <= run_box[0], (
                f"and before the run button, which is where the reference puts "
                f"it: {boxes[-1]} then {run_box}"
            )
            # Pressed at the CORNERS: a check aimed at a centre cannot see an
            # error smaller than half a control (R1684).
            for tag, box in zip(SEATS, boxes):
                want = tag.rsplit(".", 1)[1]
                for dx, dy in ((0, 0), (box[2] - 1, 0), (0, box[3] - 1), (box[2] - 1, box[3] - 1)):
                    at = (box[0] + dx, box[1] + dy)
                    assert_eq(
                        resolves(tf, at),
                        want,
                        f"★★ the corner {at} of {tag} (painted {box}) is that seat",
                    )
            nodes = access(tf)
            for tag in SEATS:
                seat = nodes.get(tag)
                assert seat is not None, f"★★ {tag} announces itself: {sorted(nodes)[:12]}…"
                assert_eq(seat["role"], "button", f"{tag} announces as a button")
                assert seat.get("name"), f"★ and is named by what it does: {seat}"
            assert "nothing saved yet" in nodes["lab.toolbar.open"]["name"], (
                "★★ the open seat says whether there is anything to open — the "
                f"fact a person cannot see from the button: {nodes['lab.toolbar.open']}"
            )

            # ── (C) nothing stored; the screen can still say what it would ──
            assert_eq(q(tf, "stored"), "", "nothing has been saved yet")
            would = q(tf, "archive")
            assert '"revision": 1' in would, f"the archive stamps its format: {would[:120]}"
            assert '"document"' in would and '"companion"' in would, would[:200]
            assert '"Router"' in would, "and carries this tool's own taxonomy"

            # ── (D) save writes a real file ─────────────────────────
            opening_nodes = q(tf, "nodes")
            tf.invoke(f"{EXT}/select", spec["selected_node"])
            tf.invoke(f"{EXT}/rename", f"{spec['selected_node']},keeper-01")
            # ★ R1690 — the key comes off the published operation table, not
            # out of this file: a catalogue key made more precise moved every
            # hand-written copy of it out of date at once.
            added_key = next(
                op["verb"][1]
                for op in spec["operations"]
                if op["name"] == "add a field from the catalogue"
            )
            tf.invoke(f"{EXT}/add_field", added_key)
            edited_nodes = q(tf, "nodes")
            assert edited_nodes != opening_nodes, "the graph was edited before the save"
            said = tf.invoke(f"{EXT}/save_graph", "")
            assert "saved" in said, f"the save says so: {said}"
            assert_eq(q(tf, "toast"), said, "which is what a person reads")

            on_disk = sdir / STORAGE_KEY
            assert on_disk.exists(), f"the file backend wrote {STORAGE_KEY} under {sdir}"
            blob = on_disk.read_text(encoding="utf-8")
            assert_eq(
                blob,
                q(tf, "stored"),
                "★ what is on disk is what the screen answers for `stored`",
            )
            assert "keeper-01" in blob, "and it holds the edit"

            # ── (F) four refusals, four sentences ───────────────────
            whys = {
                "not a saved graph": refused(tf, "open_graph", "definitely not one"),
                "another revision": refused(
                    tf, "open_graph", blob.replace('"revision": 1', '"revision": 77')
                ),
                "a role this build has not": refused(
                    tf, "open_graph", blob.replace('"Router"', '"Wormhole"')
                ),
            }
            assert_eq(
                len(set(whys.values())),
                len(whys),
                f"★★★ three refusals, three sentences — the `bool` this round "
                f"replaced: {whys}",
            )
            assert "77" in whys["another revision"], (
                f"★ the revision refusal names the number it found: {whys}"
            )
            assert_eq(q(tf, "nodes"), edited_nodes, "and every refusal changed nothing")

            # ── (G) the graph opens when the SCREEN's state does not ─
            # ★★★★★ The half a front-to-back stream cannot do. The document and
            # the companion are two parses over one envelope, so a screen whose
            # own saved shape moved on still gets its graph back — and is TOLD.
            envelope = json.loads(blob)
            envelope["companion"] = {"forms": "this is not a list of forms"}
            tf.invoke(f"{EXT}/reset", "nodes")
            said = tf.invoke(f"{EXT}/open_graph", json.dumps(envelope))
            assert "left behind" in said, f"★★★ it says what it could not take: {said}"
            assert "screen state" in said, f"and which half that was: {said}"
            assert_eq(
                q(tf, "nodes"),
                edited_nodes,
                "★★★ and the graph came back whole — the reference toolkit's "
                "restore would have answered `false` and left the window alone",
            )

            # ── (H) the volatile half does NOT come back ────────────
            assert_eq(
                volatile,
                ["produced"],
                f"the one slot this screen declares volatile: {kept}",
            )
            tf.invoke(f"{EXT}/export", "")
            assert json.loads(q(tf, "produced"))["config"] is not None, "an artifact exists"
            tf.invoke(f"{EXT}/save_graph", "")
            assert "produced" not in q(tf, "stored"), (
                "★ and the save does not carry it — the partition says so and "
                "this is the file agreeing"
            )
            tf.invoke(f"{EXT}/open_graph", "")
            assert_eq(
                json.loads(q(tf, "produced"))["config"],
                None,
                "★★ an artifact belongs to the moment it was taken, not to the "
                "graph — and this is the assertion that makes the volatile half "
                "of the partition a property rather than a claim",
            )

        # ────────────────────────────────────────────────── launch 2
        # A fresh process, the same storage directory.
        with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
            assert_eq(
                q(tf, "nodes"),
                opening_nodes,
                "★★ boot is the seeded graph — no surprise auto-load. The "
                "reference restores itself here and this deliberately does not: "
                "a screen that opened with whatever is on the machine makes "
                "every gate a function of that machine",
            )
            assert q(tf, "stored"), "the file from the last launch is still there"

            said = tf.invoke(f"{EXT}/open_graph", "")
            assert "opened" in said, f"the load says so: {said}"
            assert_eq(
                q(tf, "nodes"),
                edited_nodes,
                "★★★ the graph survived the process — the card renamed in the "
                "last launch is here",
            )
            tf.invoke(f"{EXT}/select", "keeper-01")
            keys = {row["key"] for row in json.loads(q(tf, "form"))}
            assert added_key in keys, (
                f"★★ and so did the row added to its settings form: {sorted(keys)}"
            )
            assert q(tf, "verdict"), "the launch gate has an opinion about it"

            # ── (I) clear — back to the opening graph, file gone ────
            press(tf, "lab.toolbar.clear")
            assert_eq(
                q(tf, "nodes"),
                opening_nodes,
                "★ clear puts the whole screen back to what it opens with",
            )
            assert_eq(
                q(tf, "stored"),
                "",
                "★★ and forgets the save — a reset that left a copy on disk is "
                "one that a reload undoes",
            )
            assert not (sdir / STORAGE_KEY).exists(), "the file itself is gone"

            # ── (J) the seats and the verbs are the same act ────────
            tf.invoke(f"{EXT}/select", spec["selected_node"])
            tf.invoke(f"{EXT}/rename", f"{spec['selected_node']},by-hand-01")
            press(tf, "lab.toolbar.save")
            by_seat = q(tf, "stored")
            assert "by-hand-01" in by_seat, f"the seat saved: {by_seat[:120]}"
            tf.invoke(f"{EXT}/save_graph", "")
            assert_eq(
                q(tf, "stored"),
                by_seat,
                "★ the seat and the verb write the same bytes, through the same "
                "function",
            )
            tf.invoke(f"{EXT}/reset", "nodes")
            press(tf, "lab.toolbar.open")
            assert_eq(
                q(tf, "nodes").count("by-hand-01"),
                1,
                "★ and the open seat reads it back",
            )


if __name__ == "__main__":
    sys.exit(run_demo("R1689 §5.15 — a graph survives being closed", body))
