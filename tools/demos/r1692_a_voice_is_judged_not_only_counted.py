#!/usr/bin/env python3
"""R1692 §5.40 §5.12 — a node is not a voice: what a region SAYS is judged,
and the one promise a silence makes about what is below it is checked.

Drives `hello-node-lab` over JSON-RPC, in a real window, with a real pointer.

R1691 made every addressable region classified. It could not ask the next
question: an accessibility node with `role = Button, name = ""` counted as
`announced`, and so did one named `×`, and so did one named `lab.toolbar.meta`.
The census counted nodes and called them voices.

The floor was measured again for this, by building a probe against the
reference toolkit at 6.11.1 and running it. A window of six regions answers
**9 nodes, 8 of which fail one of three mechanical rules** — five with an empty
name, of which exactly ONE is a defect and four are ornament, a layout box and
the window; one named after its own identifier; and one whose name is `×`,
which the framework derived from the button's visible text by construction. So
the rules alone are noise there: 8 flags to find 1 defect, because nothing
separates a region that owes a reader a voice from one that does not. Here the
R1691 silences do that separating, so a flag is a defect.

  (A) the census publishes what a reader HEARS — a name and a fault per row —
      and the partition it sums to is derived from the arms it publishes.
  (B) every announced region says something usable, checked twice: this demo
      re-derives the three rules from the wire and the two agree in BOTH
      directions.
  (C) a borrowed name is the words that were borrowed: where a caption declares
      itself another node's NAME, that node says what the caption paints —
      WAI-ARIA's label-in-name, compared against the painted runs. R1691 could
      only check that the redirect ARRIVES somewhere that speaks.
  (D) `layout` promises the children speak; the promise is kept, and the
      promise is checked GEOMETRICALLY — a region that announces is inside the
      box that vouched for it.
  (E) the other direction has two anchors now, not one: a node is virtual
      because something points at it, or because what it is made of paints.
  (F) it holds through a real press: a card added from the palette brings
      regions that announce names clearing every rule.
  (G) the name column tracks the screen — selecting another card changes what
      the inspector's regions say, and the census reports the new words.

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
    run_demo,
    voice_defects,
    voice_partition_sum,
    voice_rows,
)

EXAMPLE = "hello-node-lab"
EXT = "/external"

_DESIGN: tuple[int, int] | None = None


def viewport(tf) -> tuple[int, int]:
    global _DESIGN
    if _DESIGN is None:
        design = json.loads(q(tf, "spec"))["design"]
        _DESIGN = (design[0], design[1])
    return _DESIGN


def q(tf, path):
    return tf.query(f"{EXT}/{path}")


def rects(tf):
    return abs_rects_of(tf.snapshot(source="paint", viewport=viewport(tf)))


def press(tf, tag):
    box = rects(tf)[tag]
    tf.click(at=(box[0] + box[2] // 2, box[1] + box[3] // 2))


def judge(tag: str, name: str | None) -> str | None:
    """The three rules, re-derived here so (B) is a comparison and not a
    tautology. Deliberately written from the SPECIFICATION of each rule rather
    than transcribed from the Rust: nothing said, the address said, nothing
    pronounceable said."""
    said = (name or "").strip()
    if not said:
        return "absent"
    if said == tag and any((not c.isalnum()) and c != " " for c in tag):
        return "address"
    if not any(c.isalnum() for c in said):
        return "wordless"
    return None


def words(text: str) -> list[str]:
    """The pronounceable words of a string, in order.

    Punctuation is dropped on purpose: a screen renders a separator as `·` or
    `—` and a screen reader is better served by `;`, so comparing raw strings
    would call a deliberate substitution a defect. What must survive is the
    WORDS — that is what a speech-input user says out loud, and what WAI-ARIA's
    label-in-name is about.
    """
    kept = ("".join(c for c in w if c.isalnum()).lower() for w in text.split())
    # A separator that stands alone (`·`, `—`, `=`) leaves nothing behind, and a
    # token of nothing is not a word — keeping it would make the comparison a
    # statement about punctuation after all.
    return [w for w in kept if w]


def contiguous(needle: list[str], haystack: list[str]) -> bool:
    """Whether `needle` appears as a run inside `haystack`."""
    if not needle:
        return True
    return any(
        haystack[i : i + len(needle)] == needle
        for i in range(len(haystack) - len(needle) + 1)
    )


def inside(outer: tuple[int, int, int, int], inner: tuple[int, int, int, int]) -> bool:
    return (
        inner[0] >= outer[0]
        and inner[1] >= outer[1]
        and inner[0] + inner[2] <= outer[0] + outer[2]
        and inner[1] + inner[3] <= outer[1] + outer[3]
    )


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) the census publishes what a reader hears ────────────
        census = tf.voice()
        counts = census["counts"]
        for arm in ("mumbled", "hollow"):
            assert arm in counts, (
                f"the partition carries {arm} — the two questions R1691 could "
                f"not ask: {sorted(counts)}"
            )
        assert_eq(
            voice_partition_sum(census),
            census["total"],
            "★ the partition still covers the painted population exactly, and "
            "the sum is over the arms the binary PUBLISHES — a demo carrying "
            "its own list of four would have gone on summing four",
        )
        rows = voice_rows(census)
        announced = [r for r in rows.values() if r["voice"] == "announced"]
        assert len(announced) >= 100, (
            f"only {len(announced)} regions announce — the judgment below would "
            f"be passing on an empty population"
        )
        for row in announced:
            assert "name" in row and "fault" in row, (
                f"{row['tag']} is announced and the census does not say what it "
                f"says: {sorted(row)}"
            )
            assert row["fault"] is None, f"{row['tag']} announced with a fault"
        print(
            f"[A] {census['total']} regions, {len(announced)} announced, each "
            f"carrying the words a reader receives"
        )

        # ── (B) every announced region says something usable ────────
        #      Both directions. A rule that only fires where the census fires
        #      would agree with it by construction; a rule that fires where the
        #      census does not is the finding.
        disagree = [
            (r["tag"], r["voice"], r.get("fault"), r.get("name"), judge(r["tag"], r.get("name")))
            for r in rows.values()
            if r.get("name") is not None
            and (judge(r["tag"], r.get("name")) is not None) != (r["voice"] == "mumbled")
        ]
        assert_eq(
            disagree,
            [],
            "★★★★★ the census's judgment and an independent re-derivation of "
            "the same three rules agree on every row, in both directions",
        )
        assert_eq(counts["mumbled"], 0, "no region announces something unusable")
        empties = [r["tag"] for r in announced if not (r.get("name") or "").strip()]
        assert_eq(
            empties,
            [],
            "★★★★★ ...and none of them is the floor's default. A window of six "
            "regions there answers four empty names, and the forgotten control "
            "among them is the same answer as the ornament",
        )
        print(f"[B] {len(rows)} rows judged twice, no disagreement")

        # ── (C) a borrowed name is the words that were borrowed ─────
        #      A `name_of` silence says "my text is that node's NAME". R1691
        #      checked the redirect arrives somewhere that speaks; it could not
        #      check that what arrives is what was painted here. That is
        #      WAI-ARIA's label-in-name, and it is the only place on this screen
        #      where a name and some ink are declared to be the same words — so
        #      it is the only place a comparison is a comparison rather than a
        #      guess about how the screen chose to name things.
        boxes = rects(tf)
        runs = tf.request("scene/text_painted").result.get("runs", [])
        assert len(runs) >= 40, f"only {len(runs)} painted run(s)"
        access = tf.request("scene/access").result
        # ★ From the TREE and not from the census: a caption can lend its words
        # to a node with no rectangle of its own — the form's description
        # regions are exactly that — and such a node is deliberately not a
        # census row.
        heard = {n["tag"]: n.get("name") or "" for n in access["nodes"]}
        borrowed = [r for r in rows.values() if r.get("reason") == "name_of"]
        assert len(borrowed) >= 3, f"this screen lends its captions out: {borrowed}"
        checked = 0
        # ★★ R1716 — a caption inside a pane that SCROLLS may be one gesture
        # away rather than on screen, and that is not the failure this loop is
        # about: the inspector grew three rows this round and pushed its closing
        # note 44 pixels below the fold, where `scene/scroll_reach` reports it
        # as reachable with `lost` and `clipped` both zero. What still fails is
        # a caption the screen does not paint AT ALL — which is why absence is
        # asked of the reach walk rather than assumed from the rectangle.
        away = {
            entry["path"].rsplit("/", 1)[-1]
            for entry in tf.request("scene/scroll_reach").result["out_of_sight"]
        }
        for row in borrowed:
            box = boxes.get(row["tag"])
            if box is None:
                assert row["tag"] in away, (
                    f"{row['tag']} neither paints nor is one gesture away — a "
                    f"caption that is nowhere lends its words to nothing"
                )
                continue
            inked = [
                run["content"]
                for run in runs
                if run.get("content")
                and inside(box, (run["x"], run["y"], run["w"], run["h"]))
            ]
            if not inked:
                continue
            target = heard.get(row["detail"])
            assert target is not None, (
                f"{row['tag']} lends its words to {row['detail']}, which the "
                f"tree has no node for"
            )
            said = words(target)
            for text in inked:
                seen = words(text.split("\n")[0])
                assert contiguous(seen, said), (
                    f"★★★ {row['tag']} paints {text!r} and says it is the name "
                    f"of {row['detail']}, which announces {target!r} — the "
                    f"redirect arrives somewhere that speaks and says something "
                    f"else, so a person reading the label aloud reaches nothing"
                )
            checked += 1
        assert checked >= 3, f"only {checked} borrowed name(s) had ink to compare"
        # ★★★★★ R2002 — and the two derivations judge EACH OTHER, which is what
        # (B) above does for the three name rules. The framework judges this one
        # now (`Voice::Misquoted`), so the loop just above is an independent
        # oracle rather than the only reader — and an oracle nobody compares
        # against is a second gate that can rot on its own (R1884). Both
        # directions: a row the census calls misquoted that this loop passed
        # would mean the census is stricter than the rule anybody wrote down.
        misquoted = sorted(r["tag"] for r in rows.values() if r["voice"] == "misquoted")
        assert_eq(
            misquoted,
            [],
            "★★★★★ the census's own label-in-name arm and this independent "
            "re-derivation agree, in both directions",
        )
        assert_eq(counts["misquoted"], 0, "no caption lends words the name does not carry")
        print(
            f"[C] {checked} borrowed name(s) say the words that were painted, "
            f"and the census agrees on all {len(rows)} rows"
        )

        # ── (D) the promise a layout silence makes ──────────────────
        layouts = [r for r in rows.values() if r.get("reason") == "layout"]
        assert len(layouts) >= 2, f"this screen declares layout boxes: {layouts}"
        assert_eq(counts["hollow"], 0, "every box that vouched for its children was right")
        for row in layouts:
            assert_eq(
                row["voice"],
                "silent",
                f"{row['tag']} promises its children speak and the census "
                f"disagrees",
            )
            assert_eq(row["relay"], "children")
        # ★ And the promise is checked against the pixels, not against the
        # census's own walk: a region that announces is INSIDE the box.
        boxes = rects(tf)
        for row in layouts:
            outer = boxes.get(row["tag"])
            assert outer is not None, f"{row['tag']} paints"
            spoken_inside = [
                r["tag"]
                for r in announced
                if r["tag"] in boxes and inside(outer, boxes[r["tag"]])
            ]
            assert spoken_inside, (
                f"★★★★★ {row['tag']} is quiet because 'the children speak' and "
                f"nothing painted inside it announces — the whole region is "
                f"inaudible and every per-node check passes"
            )
        print(f"[D] {len(layouts)} layout box(es), each with a voice inside it")

        # ── (E) the other direction, from both ends ─────────────────
        assert_eq(counts["ghost"], 0, "no name a reader can be sent to and not find")
        painted_tags = set(rows)
        virtual = [n for n in access["nodes"] if n["tag"] not in painted_tags]
        assert virtual, "this screen announces regions it does not paint"
        for node in virtual:
            anchored_by_pointer = any(
                node["tag"] in (o.get("described_by"), o.get("name_from_tag"), o.get("controls"))
                or node["tag"] in (o.get("children") or [])
                or node["tag"] in (o.get("bounds_union_tags") or [])
                for o in access["nodes"]
            )
            anchored_by_parts = any(
                child in painted_tags for child in (node.get("children") or [])
            )
            assert anchored_by_pointer or anchored_by_parts, (
                f"★ {node['tag']} is announced, paints nothing, and neither "
                f"points at anything nor is pointed at — a reader can be sent "
                f"to it and find nothing"
            )
        print(f"[E] {len(virtual)} virtual region(s), every one of them anchored")

        # ── (F) it holds through a real press ───────────────────────
        spec = json.loads(q(tf, "spec"))
        before_names = set(q(tf, "nodes").split(","))
        press(tf, f"lab.palette.role.{spec['roles'][0]['name']}")
        added = set(q(tf, "nodes").split(",")) - before_names
        assert len(added) == 1, f"the press added one card: {sorted(added)}"
        made = added.pop()
        grown = tf.voice()
        assert grown["total"] > census["total"], (
            f"the card grew the population: {census['total']} -> {grown['total']}"
        )
        assert_eq(
            grown["counts"]["mumbled"],
            0,
            "★★★★ every region the new card brought says something usable, in "
            "the same act — a name gate that only held for the screen somebody "
            "looked at once is a gate for that morning",
        )
        assert_eq(grown["counts"]["hollow"], 0, "and no box was left vouching for nothing")
        grown_rows = voice_rows(grown)
        card = grown_rows[f"lab.node.{made}"]
        assert_eq(card["voice"], "announced")
        assert judge(card["tag"], card["name"]) is None, (
            f"the new card announces {card['name']!r}"
        )
        assert made in (card["name"] or ""), (
            f"★ and it announces WHICH card it is: {card['name']!r} does not "
            f"carry {made!r}"
        )
        tf.invoke(f"{EXT}/delete_node", made)
        tf.invoke(f"{EXT}/select", spec["selected_node"])
        print(f"[F] a pointer-added card announced itself as {card['name']!r}")

        # ── (G) the name column tracks the screen ───────────────────
        #      Not a boot snapshot: what the inspector's regions SAY follows the
        #      selection, and the census reports the new words.
        others = [n["id"] for n in spec["nodes"] if n["id"] != spec["selected_node"]]
        assert others, "the opening graph has more than one card"
        head_tag = "lab.inspector.id"
        first = voice_rows(tf.voice()).get(head_tag)
        assert first is not None, f"{head_tag} is a region of this screen"
        assert_eq(first["voice"], "announced")
        tf.invoke(f"{EXT}/select", others[0])
        second = voice_rows(tf.voice())[head_tag]
        assert_eq(second["voice"], "announced")
        assert second["name"] != first["name"], (
            f"★★★ the inspector announces the same words for two different "
            f"cards: {first['name']!r}"
        )
        assert judge(head_tag, second["name"]) is None
        assert_eq(
            voice_defects(tf.voice()),
            [],
            "and the census is still clean after the selection moved",
        )
        tf.invoke(f"{EXT}/select", spec["selected_node"])
        print(
            f"[G] the inspector says {first['name']!r} for one card and "
            f"{second['name']!r} for another"
        )


if __name__ == "__main__":
    run_demo("R1692 §5.40 — a voice is judged, not only counted", body)
