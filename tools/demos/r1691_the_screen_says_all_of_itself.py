#!/usr/bin/env python3
"""R1691 §5.40 §5.12 — every addressable region of the screen is classified:
it has a voice, or it says why it has none.

Drives `hello-node-lab` over JSON-RPC, in a real window, with a real pointer.

A screen paints regions and publishes an accessibility tree, and the two are
built by different code. Nothing in either said whether they AGREE. Measured on
this screen the day this was written: **166 addressable regions painted, 30 of
them announced** — the accessibility tree held 35 nodes and five of those are
virtual description regions the form points at, which is itself a distinction
nothing had ever drawn. **136 were unclassified.**
The palette, the icon rail, the canvas's frames and wires and pins,
the launch gate, the gesture hint and the inspector's own chrome had no voice at
all — and every check here was green, because a region with no accessibility
node paints perfectly and answers every question about its rectangle.

The floor was measured too, by building a probe against the reference toolkit at
6.11.1 and running it: a window of six children answered 7 accessibility nodes,
**4 of them with an empty name** — a button whose name the author forgot, a
decorative rule, a custom painted region and the window itself. The defect and
the three correct silences are one answer there. Clearing every author-settable
accessibility slot on the rule changed the tree by nothing; hiding it left the
node in place and took the ink instead. So there, silence cannot be declared and
cannot be told from an omission.

  (A) the census answers on the wire, as a partition rather than a number.
  (B) it is total: nothing painted and addressable is unclassified.
  (C) it is not total by writing everything off — the split is the
      specification's, driven from the tables it already publishes.
  (D) a silence says WHERE a reader receives the information instead, and the
      relay is derived from the reason rather than declared beside it.
  (E) a redirect that names a node which does not itself speak is its own arm:
      it reads as handled and is a hole.
  (F) the two directions are different questions — a name with no region is a
      `ghost`, unless something points at it.
  (G) a control announces the kind its SHAPE is: the boolean row was a text box
      to a screen reader for its whole life.
  (H) it holds after an act, not only at boot — the transient message this
      screen reports its work in was inaudible, and only a driven act finds it.
  (I) the icon rail's locked seats say what they are waiting for, which is the
      one bit the floor's accessibility layer carries.
  (J) the census tracks the screen: silencing a region moves the numbers.

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
    access_node_by_tag,
    assert_eq,
    form_part_prefixes,
    form_part_tag,
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


def expand(spec: dict, tag: str, population: str) -> list[str]:
    """The tags a voice-table row stands for, expanded from the SPEC's own
    tables — the same expansion the local gate does, from the same source, so
    this demo carries no list of its own."""
    if population == "one":
        return [tag]
    members = {
        "roles": [r["name"] for r in spec["roles"]],
        # ★★★★★ R1970 — `role_groups`, added to the wire by R1968 and NOT added
        # here, so this demo died `KeyError('role_groups')` for two rounds. The
        # population vocabulary is a Rust enum on one side and this dict on the
        # other, and only a run tells them apart — R1968 verified with the
        # `r1651` demo alone and this one is in the sweep, which does not gate a
        # push. Expanded from the spec's own table, like every row above.
        "role_groups": [g["label"] for g in spec["role_groups"]],
        "rail": [r["name"] for r in spec["rail"]],
        "nodes": [n["id"] for n in spec["nodes"]],
        "links": [str(i) for i in range(len(spec["links"]))],
        "fields": [f["key"] for f in spec["fields"]],
        # ★★★ R1716 — a row's regions depend on the axis it is on, so the
        # specification's own `source` and `aside` columns are the populations.
        # Expanded here rather than listed, exactly as the roles and nodes are.
        "fields.authored": [f["key"] for f in spec["fields"] if not f["source"]],
        "fields.derived": [f["key"] for f in spec["fields"] if f["source"]],
        "fields.aside": [f["key"] for f in spec["fields"] if f["aside"]],
        "fields.badged": [
            f["key"]
            for f in spec["fields"]
            if not f["source"] or f["applies"] == "hot"
        ],
        "protocols": list(spec["protocols"]),
        "pin_legend": [p["kind"] for p in spec["pin_legend"]],
    }[population]
    return [tag.replace("{}", m) for m in members]


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        spec = json.loads(q(tf, "spec"))

        # ── (A) the census answers, as a partition ──────────────────
        census = tf.voice()
        for key in ("total", "counts", "nodes"):
            assert key in census, f"the census carries {key}: {sorted(census)}"
        counts = census["counts"]
        for arm in ("announced", "silent", "unvoiced", "ghost", "dangling"):
            assert arm in counts, f"the partition carries {arm}: {sorted(counts)}"
        assert census["total"] >= 150, (
            f"a screen with {census['total']} addressable regions is not this "
            f"one — the census is measuring something else"
        )
        assert_eq(
            voice_partition_sum(census),
            census["total"],
            "★ the partition covers the painted population exactly — a region "
            "in no arm would be one nobody had to decide about",
        )
        print(
            f"[A] {census['total']} addressable regions: {counts['announced']} "
            f"announced, {counts['silent']} declared quiet"
        )

        # ── (B) total ───────────────────────────────────────────────
        rows = voice_rows(census)
        holes = [r["tag"] for r in voice_defects(census)]
        assert_eq(
            holes,
            [],
            "★★★★★ nothing painted and addressable is unclassified. 136 of "
            "these 166 were, and nothing could say so",
        )
        assert_eq(counts["unvoiced"], 0, "no region a reader is never told about")
        assert_eq(counts["ghost"], 0, "no name a reader can be sent to and not find")
        assert_eq(counts["dangling"], 0, "no redirect that goes nowhere")

        # ── (C) and it is not total by writing everything off ───────
        #      The split is the specification's. Both directions: what owes a
        #      voice speaks, what owes a silence is quiet, and with the class
        #      the specification names.
        spoken = 0
        for want in spec["voices"]:
            for tag in expand(spec, want["tag"], want["population"]):
                row = rows.get(tag)
                assert row is not None, f"the specification says {tag} is on screen"
                assert_eq(row["voice"], "announced", f"{tag} owes a reader a voice")
                assert row["reason"] is None, (
                    f"★ {tag} speaks AND declares a silence — a voice wins in the "
                    f"census, so the declaration is a claim nobody acts on"
                )
                if want["role"]:
                    node = access_node_by_tag(tf.request("scene/access").result, tag)
                    assert node is not None, f"{tag} announces"
                    assert_eq(
                        node["role"],
                        want["role"],
                        f"★★ {tag} announces as the wrong KIND — a reader is "
                        f"told what they can do, and the wrong word is an "
                        f"instruction that fails",
                    )
                spoken += 1
        quiet = 0
        for want in spec["silences"]:
            for tag in expand(spec, want["tag"], want["population"]):
                row = rows.get(tag)
                assert row is not None, f"the specification declares {tag} quiet"
                assert_eq(row["voice"], "silent", f"{tag} is declared quiet")
                assert_eq(
                    row["reason"],
                    want["reason"],
                    f"{tag} is quiet for a reason the specification does not state",
                )
                quiet += 1
        assert spoken >= 60, f"only {spoken} regions are declared to speak"
        assert quiet >= 25, f"only {quiet} regions are declared quiet"
        assert_eq(
            sorted(set(spec["voices"][0].keys())),
            ["population", "role", "tag"],
            "the published row shape is what this reads",
        )
        print(f"[C] the specification pins {spoken} voices and {quiet} silences, and both hold")

        # ── (D) a silence says where a reader gets it instead ───────
        relays = {r["relay"] for r in rows.values() if r["voice"] == "silent"}
        assert relays >= {"nowhere", "peer", "ancestor", "children"}, (
            f"★★ every arm of the relay vocabulary is populated: {sorted(relays)} "
            f"— a partition with one arm in use is a bool with extra words"
        )
        # ★ R2049 — the address the screen publishes, not one spelled here.
        swatch = rows[spec["roles"][0]["swatch"]]
        assert_eq(swatch["reason"], "decorative")
        assert_eq(
            swatch["relay"],
            "nowhere",
            "★ ornament relays nowhere, and the relay is DERIVED from the "
            "reason — a screen and an agent cannot disagree about it",
        )
        body_row = rows["lab.palette.body"]
        assert_eq(body_row["reason"], "layout")
        assert_eq(body_row["relay"], "children")
        caption = rows["lab.hint.text"]
        assert_eq(caption["reason"], "name_of")
        assert_eq(caption["relay"], "peer")
        assert_eq(
            caption["detail"],
            "lab.hint",
            "and the redirect NAMES where it goes, so a reader can be taken there",
        )
        assert_eq(
            rows[caption["detail"]]["voice"],
            "announced",
            "★★★ ...and that node really speaks, which is what makes the "
            "redirect true rather than well-formed",
        )
        print(f"[D] silences relay {sorted(relays)}, and each names its destination")

        # ── (E) every silence is declared where it is painted ───────
        #      This screen states each one at the site that paints it, which is
        #      what makes deleting the paint delete the declaration. A table
        #      beside the screen would be edited by whoever noticed rather than
        #      by whoever moved the region, and a tag that stopped being painted
        #      would keep its entry forever.
        silences = [r for r in rows.values() if r["voice"] == "silent"]
        assert len(silences) >= 25, f"only {len(silences)} declared silences"
        for row in silences:
            assert_eq(
                row["self_declared"],
                True,
                f"{row['tag']} is quiet by somebody else's declaration",
            )
            # ★★★★★ R1795 — with ONE derived exception, and the rule's own
            # sentence is what admits it. This exists to catch a covering region
            # meaning **a whole pane had gone quiet**. A box that holds its own
            # caption is not that: `pinion_widget_paint::caption::captioned`
            # gives the caption its box's tag plus `.caption` and makes it a
            # CHILD, precisely so the box answers for the word it draws — so the
            # caption is inside exactly one silent region, its own box, by
            # construction and on purpose.
            #
            # Narrow and derived rather than a list: the declarer has to be this
            # run's own box, which is its tag with the suffix taken off. A pane
            # silencing a caption three levels down still fails, which is the
            # case the rule is for.
            own_box = row["tag"].removesuffix(".caption")
            covered_by_its_own_box = (
                row["tag"].endswith(".caption") and row["declared_by"] == own_box
            )
            assert row["declared_by"] is None or covered_by_its_own_box, (
                f"{row['tag']} sits inside a silent region ({row['declared_by']}) "
                f"that is not its own box — a covering region here would mean a "
                f"whole pane had gone quiet"
            )
        print(f"[E] all {len(silences)} silences are stated where the region is painted")

        # ── (F) the other direction ─────────────────────────────────
        #      A name with no region is a ghost — unless something points at it.
        #      The form's description regions are exactly that case, and the
        #      census must NOT report them.
        access = tf.request("scene/access").result
        # ★ R2054 — the prefix comes from the screen. This reading is a FILTER,
        # which is where a spelled prefix fails most quietly: a wrong letter
        # selects nothing and the assertion below has no population to be true
        # or false about.
        said = form_part_prefixes(tf, ext=EXT)["said"]
        described = [n for n in access["nodes"] if n["tag"].startswith(said)]
        assert described, "the form publishes description regions"
        for node in described:
            assert node["tag"] not in rows, (
                f"★ {node['tag']} is announced and painted by nothing — it is "
                f"deliberately virtual, and the census leaves it alone because "
                f"the control POINTS at it"
            )
            assert "bounds" not in node, f"{node['tag']} has no rectangle of its own"
        pointing = [
            n["tag"] for n in access["nodes"] if n.get("described_by")
        ]
        assert pointing, "and somebody points at them"
        print(f"[F] {len(described)} virtual region(s), each pointed at by a control")

        # ── (G) a control announces the kind its shape is ───────────
        # ★ R1693 — `address[]` is a `group`, like `perm`, and was a `list`
        # until `scene/conform` asked what the list HELD: the parts this shape
        # paints are editable text boxes and an add button, so a `list` promised
        # `listitem`s nothing builds — and a field with no entries yet announced
        # a collection with nothing in it at all. Both endpoint rows on this
        # screen open empty, which is how the census found it.
        # ★★★★★ R1850 — `bool` was missing here, so a boolean row fell to the
        # `textbox` default and this gate demanded that a switch announce as a
        # box a reader types into. It went unnoticed until R1842 put two
        # permission booleans on every card: the screen's one boolean lived on
        # two peers this section does not walk, so the arm below had never been
        # reached.
        #
        # ⚠ And this table is a SECOND COPY of the mapping `painted.rs` makes
        # (`"bool" => "checkbox"`, added by R1842 in the same round that made
        # the row exist). Two declarations of one rule, and the one that is not
        # exercised is the one that rots — which is what happened. The screen
        # does not publish the mapping, so the copy stays for now and says so.
        want_role = {
            "int": "spinbutton",
            "perm": "group",
            "address[]": "group",
            "bool": "checkbox",
        }
        for field in spec["fields"]:
            # ★ R2050 — the address the screen publishes for that row.
            node = access_node_by_tag(access, field["control"])
            assert node is not None, f"{field['key']} announces"
            # ★★★ R1716 — a row nobody wrote is a READ-OUT whatever its type
            # word says, because that is what it paints: no chips, no stepper,
            # no element boxes. Announcing the shape's control on a row that has
            # none would tell a reader to do something the form refuses.
            assert_eq(
                node["role"],
                "textbox" if field["source"] else want_role.get(field["ty"], "textbox"),
                f"★★★ {field['key']} is typed {field['ty']} and announces as "
                f"the wrong kind",
            )
        # ★★★★ ...and the one shape this screen does not open with. A
        # counterfactual proved the point: flipping the boolean arm to a text
        # box — the exact defect, where a reader is told to type into a
        # toggle — was caught by nothing, because none of the five opening rows
        # is a boolean. So the row is ADDED, from the chip that offers it.
        boolean_key = "timestamping.enabled"
        press(tf, form_part_tag(tf, "add", boolean_key, ext=EXT))
        keys = {row["key"] for row in json.loads(q(tf, "form"))}
        assert boolean_key in keys, f"the chip added the row: {sorted(keys)}"
        access = tf.request("scene/access").result
        boolean_control = next(
            row["control"]
            for row in json.loads(q(tf, "form"))
            if row["key"] == boolean_key
        )
        node = access_node_by_tag(access, boolean_control)
        assert node is not None, "the boolean row announces"
        assert_eq(
            node["role"],
            "checkbox",
            "★★★★★ a boolean row is a CHECKBOX to a reader. It was a text box "
            "for its whole life, which tells somebody to type into a control "
            "that only toggles",
        )
        assert_eq(
            (node.get("state") or {}).get("checked"),
            False,
            "and the bit is announced, which is what a reader's toggle reads",
        )
        # ★★★★★ R1837 — announced ONCE, and this line used to demand the
        # opposite. The form published a `toggle.<key>` square inside the
        # control and announced it as a SECOND checkbox with the control's own
        # name and the control's own bit, at a rectangle a fraction of its size:
        # a reader met one checkbox twice. The control IS the switch now, so
        # there is nothing inside it to announce — the same judgment R1732 made
        # about the collapsed chooser's chevron, which was never carried across
        # to the boolean beside it.
        boolean_boxes = [
            n["tag"]
            for n in (access.get("nodes") or [])
            if n.get("role") == "checkbox" and boolean_key in n.get("tag", "")
        ]
        assert boolean_boxes == [boolean_control], (
            "one control, one checkbox — a reader who hears it twice cannot "
            f"tell two controls from one said twice: {boolean_boxes}"
        )
        assert_eq(
            tf.voice()["counts"]["unvoiced"],
            0,
            "and the row's new regions were classified in the same act",
        )
        tf.invoke(f"{EXT}/remove_field", boolean_key)
        print(f"[G] all {len(spec['fields'])} opening rows and an added boolean announce their kind")

        # ── (H) it holds after an act, and the message is heard ─────
        #      The toast is where several of this screen's operations report
        #      what they did. It exists only after an act, so a census at boot
        #      is blind to it and no round had ever looked.
        tf.invoke(f"{EXT}/select", spec["selected_node"])
        tf.invoke(f"{EXT}/export", "")
        after = tf.voice()
        rows_after = voice_rows(after)
        assert "lab.toast" in rows_after, "the act put a message on screen"
        assert_eq(
            rows_after["lab.toast"]["voice"],
            "announced",
            "★★★★★ ...and a reader is told about it. Several operations report "
            "ONLY here, so a silent toast is an operation a reader cannot "
            "confirm happened",
        )
        toast = access_node_by_tag(tf.request("scene/access").result, "lab.toast")
        # ★★★★★ R1719 — this line used to demand `assertive` for EVERYTHING the
        # screen says, on the argument written beside it: "a reply to something
        # the person just did". That argument is right about the half of what
        # this screen says that a person did NOT get, and R1719 measured what it
        # cost for the other half — a screen reader interrupted to be told
        # `selected R-01`, which the person had just asked for. The urgency
        # comes off the tone now, so the export above (a thing that happened) is
        # polite and a refusal cuts in. Both halves are driven in
        # `r1719_what_a_screen_says_knows_its_kind`.
        assert_eq(
            toast.get("live"),
            "polite",
            "a confirmation is worth saying and not worth cutting anybody off "
            "for — the urgency is the utterance's, not this region's",
        )
        assert_eq(
            after["counts"]["unvoiced"], 0, "the census is still total after an act"
        )
        assert after["total"] > census["total"], (
            f"the screen grew regions: {census['total']} -> {after['total']}"
        )
        print(f"[H] after an act: {after['total']} regions, still 0 unclassified")

        # ── (I) the locked rail seats say what they wait for ────────
        locked = [seat for seat in spec["rail"] if seat["locked"]]
        assert len(locked) >= 2, f"the reference locks seats on this rail: {locked}"
        for seat in locked:
            node = access_node_by_tag(tf.request("scene/access").result, f"lab.rail.{seat['name']}")
            assert node is not None, f"the {seat['name']} seat announces"
            assert_eq(node["role"], "link")
            assert_eq(
                (node.get("unavailable") or {}).get("detail"),
                seat["reserved_for"],
                f"★★★ the {seat['name']} seat says WHAT it is waiting for — the "
                f"floor's accessibility layer carries one inert bit, and "
                f"'booked for a release' and 'never' are the same bit there",
            )
            assert_eq((node.get("unavailable") or {}).get("kind"), "reserved")
        live = [seat for seat in spec["rail"] if not seat["locked"]]
        for seat in live:
            node = access_node_by_tag(tf.request("scene/access").result, f"lab.rail.{seat['name']}")
            assert node is not None and not node.get("unavailable"), (
                f"the {seat['name']} seat is open and announces a reason to wait"
            )
        print(f"[I] {len(locked)} locked seat(s) name their booking, {len(live)} open ones do not")

        # ── (J) the census tracks the screen, through a real press ──
        #      A meter that cannot move is a decoration. A card added from the
        #      palette — with the pointer, the way a person adds one — brings a
        #      card, an identifier, a role chip and its pins, and every one of
        #      them is classified in the same act. Nobody edits a number.
        before = tf.voice()
        before_names = set(q(tf, "nodes").split(","))
        press(tf, spec["roles"][0]["tag"])
        added = set(q(tf, "nodes").split(",")) - before_names
        assert len(added) == 1, f"the press added one card: {sorted(added)}"
        made = added.pop()
        grown = tf.voice()
        assert grown["total"] > before["total"], (
            f"★★ a card added with the POINTER grew the addressable population "
            f"and the census followed: {before['total']} -> {grown['total']}"
        )
        assert_eq(
            grown["counts"]["unvoiced"],
            0,
            "★★★★ ...and the new regions were classified in the same act. A "
            "census that only held for the screen somebody looked at once is a "
            "census of that morning",
        )
        rows_grown = voice_rows(grown)
        assert_eq(rows_grown[f"lab.node.{made}"]["voice"], "announced")
        assert_eq(rows_grown[f"lab.node.{made}.id"]["voice"], "silent")
        assert_eq(rows_grown[f"lab.pin.{made}.dial"]["voice"], "announced")
        tf.invoke(f"{EXT}/delete_node", made)
        # ★ Put the selection back first. A delete moves it to another card, and
        # the inspector then shows THAT card's form — a different number of
        # rows, which is the screen behaving correctly and would make this
        # comparison a statement about which card is selected.
        tf.invoke(f"{EXT}/select", spec["selected_node"])
        assert_eq(
            tf.voice()["total"],
            before["total"],
            "and taking it away restores exactly what it brought",
        )
        print(
            f"[J] the census follows a real press: {before['total']} -> "
            f"{grown['total']} -> {before['total']}"
        )


if __name__ == "__main__":
    run_demo("R1691 §5.40 — the screen says all of itself", body)
