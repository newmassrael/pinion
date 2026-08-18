#!/usr/bin/env python3
"""R1651 §5.21 §5.51 §2 #7 — the node graph lab, against the reference screen
it was written from, in both directions.

The standing instruction for this axis is to reproduce the reference tool's
screen A exactly. A round can claim that; what makes the claim checkable is
that the screen is a **value** the application publishes (`spec`) and this
script compares the painted scene against it — an element the screen is missing
and an element the screen invented are both failures.

What is new in the framework, and what this therefore checks hardest: the node
inspector is now a widget with a crate home (`pinion_widget_paint::config_form`
over `pinion_core::widgets::config_form`), which is the analysis-tool census's
must-have row that had scored `gap` since R1646 for want of anywhere to live.
So every claim about it is a claim about framework code:

* a row per configuration path, and the **key is the path** — not a label;
* a per-row applies badge, and exactly one row on this node is live;
* a defect shown **on the row it is about**, with the launch verdict derived
  from the rows rather than set beside them;
* a **deployable document** derived from the same rows, and read back.

Run from the workspace root:
    cargo build -p hello-node-lab --release
    python3 tools/demos/r1651_the_node_lab_matches_the_reference.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_declared_channels_are_true,
    assert_eq,
    assert_router_press_moves,
    call,
    find_by_tag,
    run_demo,
    walk_nodes,
)

EXT = "/external"
WIN = (1440, 900)


def q(tf: RpcSubprocess, path: str):
    return tf.query(f"{EXT}/{path}")


def inv(tf: RpcSubprocess, path: str, args):
    return tf.invoke(f"{EXT}/{path}", args)


def refused(tf: RpcSubprocess, path: str, args) -> str:
    try:
        inv(tf, path, args)
    except Exception as why:  # noqa: BLE001 - any refusal shape is fine here
        return str(why)
    raise AssertionError(f"{path}({args!r}) was expected to be refused")


def paint(tf: RpcSubprocess):
    return tf.snapshot(source="paint", viewport=WIN)


def tags(snap) -> dict:
    """Every painted tag, with the rectangle it was painted in."""
    found = {}
    for _path, node in walk_nodes(snap):
        tag = node.get("tag")
        if tag:
            found[tag] = node.get("rect")
    return found


def centre(rect: dict) -> str:
    return f"{rect['x'] + rect['w'] // 2},{rect['y'] + rect['h'] // 2}"


def window_of(tf: RpcSubprocess, tag: str) -> dict:
    """The rectangle `tag` occupies ON SCREEN, which is where a pointer goes."""
    answer = call(tf, "scene/bbox", {"tag": tag, "from": "paint"})
    window = answer.get("window")
    assert window is not None, f"{tag} is painted somewhere a pointer can reach"
    return window


def at(tf: RpcSubprocess, tag: str) -> str:
    """The point at the centre of the rectangle `tag` was PAINTED in.

    Asking the scene rather than recomputing the screen's layout here: a demo
    carrying a second copy of the geometry could not notice a drift between the
    painter and the hit test, which is the property section (H) holds.

    ★ R1653 — `scene/bbox`'s `window`, not the snapshot node's own `rect`. The
    two differ for everything inside a scroll, and the canvas is one: a rect
    read straight off the tree is stated in the scrolling surface's own
    coordinates, so a press aimed at it lands outside the window. That is
    exactly what happened the moment the canvas became a viewport — the demo
    asked for (2207, 2081) in a 1440x900 window and the action refused it.
    """
    return centre(window_of(tf, tag))


def click(tf: RpcSubprocess, where: str) -> str:
    inv(tf, "point", where)
    inv(tf, "send", "PointerDown")
    return inv(tf, "send", "PointerUp")


def press_after_scrolling_to(tf: RpcSubprocess, tag: str) -> str:
    """What a press answers once the pane holding `tag` has been scrolled to it.

    ★ R1690 — the offset comes from the screen's own `scene/scroll_reach`, so
    this drives a capability the screen publishes rather than a number this file
    guessed. The pane is put back afterwards, because a sweep that left the
    panes wherever it stopped would make every later step depend on the order
    the tags happened to sort in.
    """
    out = call(tf, "scene/scroll_reach")
    entry = next(
        (o for o in out["out_of_sight"] if o.get("tag") == tag),
        None,
    )
    assert entry is not None, (
        f"{tag} has no window rectangle and the screen does not report it as "
        f"off screen either — it is painted where nothing can reach it"
    )
    assert entry["reach"] == "scrollable", (
        f"{tag} is painted where no scrolling brings it into view: {entry}"
    )
    # ★ R1714 — the answer is the whole recipe: every viewport that has to move,
    # outermost first. A window that pans over its own layout is one of them,
    # and performing half a recipe leaves the mark exactly where it was.
    recipe = entry["moves"]
    assert recipe, f"{tag} is reachable and names nothing to move: {entry}"
    was = [
        (m["viewport"], offset_of(tf, m["viewport"], entry))
        for m in recipe
    ]
    for m in recipe:
        tf.scroll(m["viewport"], to=(m["to_x"], m["to_y"]))
    tf.tick(0.05)
    try:
        return inv(tf, "point", at(tf, tag))
    finally:
        for pane, back in was:
            tf.scroll(pane, to=back)
        tf.tick(0.05)


def offset_of(tf: RpcSubprocess, viewport: str, entry: dict) -> tuple[int, int]:
    """Where `viewport` is right now, so the caller can put it back.

    The row's own viewport publishes its offset in the very answer the recipe
    came from; anything further out is asked for, once.
    """
    if entry["viewport"]["name"] == viewport:
        return (entry["viewport"]["at_x"], entry["viewport"]["at_y"])
    reach = tf.request("scene/scroll_reach")
    assert reach is not None and isinstance(reach.result, dict)
    for row in reach.result["out_of_sight"]:
        if row["viewport"]["name"] == viewport:
            return (row["viewport"]["at_x"], row["viewport"]["at_y"])
    return (0, 0)


def body() -> None:
    with RpcSubprocess("hello-node-lab", boot_grace=1.5) as tf:
        counted = assert_declared_channels_are_true(tf)
        print(f"[A] {counted['read']} declared read(s) answer, "
              f"{counted['invoke']} declared action(s) dispatch")

        # ── (B) The specification is on the wire, and it is what was built ──
        spec = json.loads(q(tf, "spec"))
        assert_eq(q(tf, "graph"), spec["graph"], "the graph is the one declared")
        assert_eq(q(tf, "zoom"), spec["zoom"], "and it opens at the declared zoom")
        assert_eq(
            q(tf, "selected"),
            spec["selected_node"],
            "on the node the reference opens on",
        )
        assert_eq(
            q(tf, "discovery"),
            False,
            "★ auto-discovery is OFF by default — a graph whose links are all "
            "authored is the one whose behaviour is a function of the canvas",
        )
        painted = tags(paint(tf))
        print(f"[B] the specification declares {len(spec['nodes'])} node(s), "
              f"{len(spec['links'])} link(s), {len(spec['roles'])} role(s); "
              f"{len(painted)} tag(s) painted")

        # ── (C) FORWARD: every declared element is on the screen ────────────
        missing = []
        for pane in spec["panes"]:
            if pane["tag"] not in painted:
                missing.append(pane["tag"])
        for seat in spec["rail"]:
            tag = f"lab.rail.{seat['name']}"
            if tag not in painted:
                missing.append(tag)
        for role in spec["roles"]:
            for tag in (
                f"lab.palette.role.{role['name']}",
                f"lab.palette.swatch.{role['name']}",
            ):
                if tag not in painted:
                    missing.append(tag)
        for kind in spec["pin_legend"]:
            tag = f"lab.palette.pin.{kind['kind']}"
            if tag not in painted:
                missing.append(tag)
        for word in spec["protocols"]:
            tag = f"lab.palette.protocol.{word}"
            if tag not in painted:
                missing.append(tag)
        for frame in spec["frames"]:
            for tag in (f"lab.frame.{frame['name']}", f"lab.frame.{frame['name']}.name"):
                if tag not in painted:
                    missing.append(tag)
        # ★ R1678 — the reset the specification says is UNCONDITIONAL has to be
        # on the opening screen. Read off the `gated` column rather than named
        # here, so a scope that changed its mind about being conditional moves
        # this check with it.
        for reset in spec["resets"]:
            tag = f"lab.reset.{reset['scope']}"
            if not reset["gated"] and tag not in painted:
                missing.append(tag)
        for node in spec["nodes"]:
            for tag in (
                f"lab.node.{node['id']}",
                f"lab.node.{node['id']}.id",
                f"lab.node.{node['id']}.badge",
                f"lab.pin.{node['id']}.dial",
            ):
                if tag not in painted:
                    missing.append(tag)
        for field in spec["fields"]:
            for tag in (
                f"lab.form.control.{field['key']}",
                f"lab.form.applies.{field['key']}",
                # ★★ R1686 — the seat that takes the row away. DEMANDED per
                # row, not permitted per row: the reference draws it on every
                # row it does not derive and every row here is authored, so a
                # form that grew one row without a seat is a configuration a
                # person can only add to.
                f"lab.form.remove.{field['key']}",
            ):
                if tag not in painted:
                    missing.append(tag)
        for key in spec["addable"]:
            tag = f"lab.form.add.{key}"
            if tag not in painted:
                missing.append(tag)
        for tag in (
            "lab.toolbar.title",
            "lab.toolbar.meta",
            "lab.toolbar.gate",
            "lab.toolbar.zoom",
            "lab.toolbar.run",
            "lab.gate",
            "lab.gate.verdict",
            "lab.hint",
            "lab.hint.text",
            "lab.link.label",
            "lab.inspector.id",
            "lab.inspector.degree",
        ):
            if tag not in painted:
                missing.append(tag)
        assert not missing, f"the specification declares {len(missing)} element(s) the screen does not paint: {missing}"
        print(f"[C] FORWARD — every declared element is painted "
              f"({len(spec['nodes']) * 4 + len(spec['fields']) * 2 + len(spec['addable'])}+ checked)")

        # ── (D) BACKWARD: the screen invented nothing ───────────────────────
        # Every painted tag has to be accounted for by the specification or by
        # one of the named structural families below. A screen free to add
        # chrome nobody declared is a screen that has stopped being a
        # reproduction, and a forward-only check cannot see that.
        declared = set()
        for pane in spec["panes"]:
            declared.add(pane["tag"])
            # ★ R1664 — a pane that SCROLLS paints a body node, and the
            # specification is where that is stated (R1662 added the column).
            # This loop read `tag` and not `body`, so R1662's scroll panes were
            # painted tags nothing declared and the backward check went red on
            # CI while every local test passed: the tag set is only checked by
            # this demo, and the demo sweep does not gate a push.
            if pane.get("body"):
                declared.add(pane["body"])
        for seat in spec["rail"]:
            declared.add(f"lab.rail.{seat['name']}")
        for role in spec["roles"]:
            declared.add(f"lab.palette.role.{role['name']}")
            declared.add(f"lab.palette.swatch.{role['name']}")
        for kind in spec["pin_legend"]:
            declared.add(f"lab.palette.pin.{kind['kind']}")
        for word in spec["protocols"]:
            declared.add(f"lab.palette.protocol.{word}")
        for frame in spec["frames"]:
            declared.add(f"lab.frame.{frame['name']}")
            declared.add(f"lab.frame.{frame['name']}.name")
        for node in spec["nodes"]:
            for suffix in ("", ".id", ".badge"):
                declared.add(f"lab.node.{node['id']}{suffix}")
            declared.add(f"lab.pin.{node['id']}.dial")
            declared.add(f"lab.pin.{node['id']}.accept")
        for field in spec["fields"]:
            declared.add(f"lab.form.control.{field['key']}")
            declared.add(f"lab.form.applies.{field['key']}")
            declared.add(f"lab.form.defect.{field['key']}")
            declared.add(f"lab.form.remove.{field['key']}")
            # Every affordance a shape can put inside its control. Declared per
            # family rather than per instance because how many a row has is a
            # function of its VALUE (a list grows), and the count pin below is
            # what keeps that from being a hole.
            for word in ("read", "write"):
                declared.add(f"lab.form.option.{field['key']}.{word}")
            for part in ("up", "down"):
                declared.add(f"lab.form.step.{field['key']}.{part}")
            declared.add(f"lab.form.toggle.{field['key']}")
            declared.add(f"lab.form.item.{field['key']}.add")
            for n in range(8):
                declared.add(f"lab.form.item.{field['key']}.{n}")
        for key in spec["addable"]:
            declared.add(f"lab.form.add.{key}")
        # ★ R1678 — every reset affordance is a permitted tag; only the
        # UNGATED one is demanded above, because the other four are painted
        # exactly when their scope has something to put back and the screen
        # this demo drives has just opened.
        for reset in spec["resets"]:
            declared.add(f"lab.reset.{reset['scope']}")
        # The families the specification names as a WHOLE rather than per item,
        # because the reference describes those regions as a block ("title,
        # meta, chip, zoom, config, Run") and a table written at element
        # resolution would be a copy of the screen rather than a specification
        # of it.
        #
        # ★ R1652 — each family carries a MEMBER COUNT, which is what stops
        # "accepted wholesale" from being a hole. R1651.1 registered this as a
        # debt in exactly those words: a family prefix let anything under it
        # through, so "all N painted tags are declared" was true of far fewer
        # than N elements. A count cannot say WHICH element arrived, but it
        # fails the moment one does, and that is the R1650 shape — pin the
        # member set, do not search for an absence.
        # `lab.link` is a wire per link plus the selected one's two label
        # nodes, so its pin is DERIVED from the specification rather than
        # written down — a family whose size is a function of the graph must
        # not be pinned to a constant, or adding a link fails the wrong check.
        FAMILIES = {
            "node_lab": 1,
            "lab.appbar": 3,
            # ★★ R1687 — 11, not 10: the toolbar gained the second of the two
            # seats the reference puts side by side, `lab.toolbar.script`. Its
            # sibling `lab.toolbar.config` was already here, answering the
            # selected card's key count; this round made the pair what the
            # reference has — one derivation rendered as a document and as a
            # script. The pin is what noticed, which is the check working.
            # ★★ R1688 — 12, not 11: the zoom pill gained `lab.toolbar.fit`, the
            # reference's own trailing seat. It gained one member and LOST none,
            # because the seat this round removed (`home`, the separate view
            # reset) kept its tag — the read-out is that control now, so
            # `lab.reset.view` is the box and `lab.toolbar.zoom` its caption.
            # ★★ R1689 — 15, not 12: the file pill, which the reference groups
            # between the launch-script button and the run button —
            # `lab.toolbar.{save,open,clear}`. Three at once and none lost, so
            # the pin moving by exactly three is what says the group landed
            # whole rather than a seat arriving and another quietly going.
            "lab.toolbar": 15,
            "lab.gate": 7,
            "lab.hint": 2,
            # ★ R1681 — the picked link now carries its own affordances: the
            # endpoint caption (a panel and its run) and the act seat (ditto).
            # Still derived, still not a constant.
            "lab.link": len(spec["links"]) + 4,
            # And what a source reported is its own family, because it is its
            # own layer.
            "lab.observed": len(spec["observed"]),
            # ★ R1664 — 8, not 7: R1662 gave the inspector a scrolling body,
            # which is a member. The pin is what NOTICED (this is the check
            # working), and the number moves with a reason rather than by
            # somebody widening it to make a red go away.
            # ★★ R1682 — 11, not 8: the node's-life row is three seats, one per
            # thing a person can do to the selected card itself (collapse,
            # switch off, delete). Each is demanded back by `must_answer` in the
            # screen's own press census, so the number growing here is matched
            # by three more affordances that have to answer a press.
            # ★★ R1683 — 14, not 11: the one text field's row is a box (the
            # placeholder, which is also its seat when shut) and two seats —
            # "rename" and "+ key", the field's two targets. Three more
            # affordances, and all three are demanded back by `must_answer` in
            # the screen's own press census.
            # ★★ R1690 — 16, not 14: the reach meter is a pill and the run
            # inside it. Two more marks and NEITHER is demanded back by
            # `must_answer`, which is the difference worth writing down — it is
            # a read-out, not an affordance, so the press census legitimately
            # passes over it and this pin is the only thing that would notice if
            # it disappeared.
            # ★★ R1706 — 18, not 16: the selection-count chip is a pill and the
            # run inside it, in the reference's own place between the degree
            # pill and the node's-life row. Same kind as the reach meter — a
            # read-out rather than an affordance — so again the press census
            # passes over it and this pin is what would miss it.
            "lab.inspector": 18,
            "lab.palette.discovery": 3,
        }
        undeclared = [
            tag
            for tag in painted
            if tag not in declared
            and not any(tag == f or tag.startswith(f + ".") for f in FAMILIES)
        ]
        assert not undeclared, (
            f"the screen paints {len(undeclared)} element(s) the specification does "
            f"not declare: {undeclared}"
        )
        drifted = []
        for family, pinned in FAMILIES.items():
            held = sorted(
                t for t in painted if t == family or t.startswith(family + ".")
            )
            if len(held) != pinned:
                drifted.append((family, pinned, len(held), held))
        assert not drifted, (
            "a family gained or lost members without the specification saying so "
            f"(pin, actual, members): {drifted}"
        )
        counted = sum(FAMILIES.values())
        print(f"[D] BACKWARD — {len(painted) - counted} painted tag(s) declared "
              f"element by element, {counted} more pinned by member count across "
              f"{len(FAMILIES)} named families; nothing unaccounted for")

        # ── (E) The inspector IS the settings editor ────────────────────────
        form = json.loads(q(tf, "form"))
        assert_eq(
            [f["key"] for f in form],
            [f["key"] for f in spec["fields"]],
            "the same rows the reference shows, in the same order",
        )
        for want, held in zip(spec["fields"], form):
            assert_eq(held["ty"], want["ty"], f"{want['key']} type")
            assert_eq(held["applies"], want["applies"], f"{want['key']} applies")
            assert_eq(held["value"], want["value"], f"{want['key']} value")
        hot = [f["key"] for f in form if f["applies"] == "hot"]
        assert_eq(
            len(hot), 1,
            "★ exactly one row reaches a running node, which is why the badge "
            f"exists at all: {hot}",
        )
        print(f"[E] the inspector holds {len(form)} row(s) keyed by configuration "
              f"path; {hot[0]} is the one that applies live")

        # ── (F) The document is derived from those rows, and reads back ─────
        document = json.loads(q(tf, "document"))
        assert "refused" not in document, document
        assert_eq(
            document["transport"]["link"]["tx"]["batch_size"],
            65535,
            "★ the dotted key IS the path and the declared type IS the type — a "
            "form and a settings file mapped by hand are mapped twice",
        )
        assert_eq(
            document["control"]["permissions"],
            ["read", "write"],
            "and a set of flags is an array",
        )
        assert isinstance(document["listen"]["endpoints"], list)
        print(f"[F] the form derives a deployable document: "
              f"{len(json.dumps(document))} bytes, nested from its own paths")

        # ── (G) The gate is derived, and warns without blocking ─────────────
        verdict = json.loads(q(tf, "verdict"))
        gate = json.loads(q(tf, "gate"))
        assert verdict["may_launch"], verdict
        assert_eq(verdict["blocking"], 0)
        assert verdict["warning"] >= 2, gate
        assert any("listening" in line["sentence"] for line in gate), gate
        assert any("discovery" in line["sentence"] for line in gate), gate
        assert all(not line["blocks"] for line in gate), gate
        assert "warning" in verdict["sentence"], (
            "★ the gate SAYS the warnings stand even while it opens: "
            "'nothing is stopping you' and 'nothing is wrong' are different "
            f"statements: {verdict['sentence']}"
        )
        print(f"[G] the gate opens with {verdict['warning']} warning(s) and says so: "
              f"{verdict['sentence']!r}")

        # A value that would fail at start-up closes it, and the refusal names
        # what would be accepted rather than saying 'invalid'.
        inv(tf, "set_field", "transport.link.tx.batch_size=70000")
        verdict = json.loads(q(tf, "verdict"))
        assert not verdict["may_launch"], verdict
        assert_eq(verdict["blocking"], 1)
        why = refused(tf, "run", True)
        assert "gate is closed" in why, why
        painted = tags(paint(tf))
        assert "lab.form.defect.transport.link.tx.batch_size" in painted, (
            "★ and the defect is painted ON the row it is about, not only in a "
            "list at the bottom"
        )
        assert not any(
            t.startswith("lab.form.defect.") and "batch_size" not in t for t in painted
        ), "and on no other row"
        document = json.loads(q(tf, "document"))
        assert "refused" in document, (
            "★ and no document is emitted — a file the tool called fine that "
            f"fails at start-up is the failure this prevents: {document}"
        )
        assert "0..=65535" in document["refused"], document
        print(f"[G] out of range closes it and names the range: {document['refused']!r}")

        inv(tf, "set_field", "transport.link.tx.batch_size=65535")
        assert json.loads(q(tf, "verdict"))["may_launch"], "and repairing it reopens"

        # ── (H) Paint and gesture read ONE geometry, both directions ────────
        # ★ R1651.1 — the probe list is DERIVED FROM THE PAINTED SCENE, not
        # written out here. R1651 hand-listed it, and a hand-listed population
        # makes "40 controls pass" read as coverage when it is a sample: an
        # audit immediately found three controls the list did not name, and all
        # three were broken. Every painted tag whose name the hit test can also
        # produce must answer for ITSELF at the centre of the rectangle it was
        # painted in, so a new control cannot join the screen unprobed.
        def expected(tag: str) -> str | None:
            """What a press at this tag's centre must answer, from the tag alone.

            A control that carries affordances INSIDE it (a list's element rows,
            a stepper) legitimately answers with the affordance under the
            cursor rather than with itself — that is the affordance working.
            What must never happen is an answer naming a different ROW, which
            is what `same_row` below checks and what all three R1651.1 defects
            violated.
            """
            for prefix, verb in (
                ("lab.rail.", "rail"),
                ("lab.palette.role.", "role"),
                ("lab.form.add.", "add"),
            ):
                if tag.startswith(prefix):
                    return f"{verb}:{tag[len(prefix):]}"
            if tag.startswith("lab.form.control."):
                return f"field:{tag[len('lab.form.control.'):]}"
            for family in ("option", "step", "toggle", "item"):
                if tag.startswith(f"lab.form.{family}."):
                    return tag[len("lab.form."):]
            if tag.startswith("lab.pin."):
                node, _, side = tag[len("lab.pin."):].rpartition(".")
                return f"pin:{node}:{side}"
            if tag.startswith("lab.node.") and tag.count(".") == 2:
                return f"node:{tag[len('lab.node.'):]}"
            return {
                "lab.toolbar.zoom.in": "zoom:in",
                "lab.toolbar.zoom.out": "zoom:out",
                "lab.toolbar.config": "config",
                "lab.toolbar.script": "script",  # R1687
                "lab.toolbar.run": "run",
                "lab.palette.discovery": "discovery",
            }.get(tag)

        def sweep(when: str) -> int:
            painted_now = tags(paint(tf))
            probes = [
                (tag, want)
                for tag in sorted(painted_now)
                if (want := expected(tag)) is not None
            ]
            # A floor, so a regression that stops PAINTING a control shows up
            # here rather than as a smaller number nobody reads.
            assert len(probes) >= 55, f"{when}: only {len(probes)} control(s)"
            bad = []
            # ★★★ R1690 — a control below the fold of a scrolling pane is
            # aimed at AFTER scrolling to it, not skipped and not failed.
            #
            # This sweep read `scene/bbox`'s window rectangle and treated its
            # absence as "painted where a pointer cannot reach", which was true
            # only while every control happened to fit at the design size. It
            # stopped being true the moment the inspector's head grew by a row:
            # the third row of add-chips went under the fold and this reported
            # a screen defect for a pane doing exactly what a scrolling pane is
            # for. The framework already draws the distinction — `scene/
            # scroll_reach` publishes the offset that brings a mark into view —
            # so what was missing was this side asking.
            below = []
            for tag, want in probes:
                seat = call(tf, "scene/bbox", {"tag": tag, "from": "paint"}).get("window")
                if seat is None:
                    below.append((tag, want))
                    continue
                answered = inv(tf, "point", centre(seat))
                if answered != want and not same_row(want, answered):
                    # ★★ R1681.3 — the picked link's own chrome legitimately
                    # covers what it is drawn over. It is an affordance the
                    # person summoned by picking that link, the reference draws
                    # it in exactly that place, and a screen that moved it clear
                    # of every card instead put it 240px from the wire it
                    # annotates (measured on the running app). Recognised by the
                    # ANSWER, which is the screen's own vocabulary, rather than
                    # by a rectangle this side would have to re-derive.
                    #
                    # ★ Never for the chrome's own seats: `lab.link.*` is
                    # excluded, so "the delete seat is painted where nothing
                    # presses it" still fails here.
                    if answered.startswith(
                        ("link:act", "link:endpoint:")
                    ) and not tag.startswith("lab.link."):
                        continue
                    bad.append((tag, want, answered))
            # And the ones under the fold, at the offset the screen itself says
            # brings them into view. Not a lenient path: a control that the
            # screen publishes as reachable and that does not answer after
            # scrolling there fails exactly like one on screen would.
            for tag, want in below:
                answered = press_after_scrolling_to(tf, tag)
                if answered != want and not same_row(want, answered):
                    bad.append((f"{tag} (after scrolling to it)", want, answered))
            assert not bad, (
                f"{when}: {len(bad)} of {len(probes)} painted control(s) are drawn "
                f"where a press does not reach them: {bad}"
            )
            # No silent cap: how many needed a scroll is printed, because a
            # number that quietly grew would mean the panes are filling up.
            if below:
                print(
                    f"[H] {len(below)} of {len(probes)} control(s) were below a "
                    f"fold and answered after scrolling to them"
                )
            return len(probes)
        def same_row(want: str, got: str) -> bool:  # noqa: F811
            """Both answers are about the same form row."""
            if not want.startswith("field:"):
                return False
            key = want[len("field:"):]
            return got.startswith(("option.", "step.", "toggle.", "item.")) and got.split(
                ".", 1
            )[1].startswith(key)

        opening = sweep("opening")

        # ★ R1652.1 — and AGAIN on a screen somebody has used. The opening
        # screen is the one state a specification describes and the one state
        # nobody works in: a list has one element there and cannot overflow its
        # own row, which is how R1652 shipped a list of six painting straight
        # over the next field with this sweep passing. Growing the list is the
        # cheapest thing that changes a control's SIZE, and size is what a
        # single-state sweep cannot see.
        for _ in range(6):
            click(tf, at(tf, "lab.form.item.listen.endpoints.add"))
        used = sweep("after growing a list to seven elements")
        assert used > opening, (
            f"the used screen must paint MORE than the opening one, or growing "
            f"the list did nothing: {opening} -> {used}"
        )
        print(f"[H] {opening} painted control(s) on the opening screen and {used} "
              f"after growing a list, DERIVED from the scene rather than listed, "
              f"each answering a press at their painted centre")

        # ── (I) The gestures the hint strip advertises ──────────────────────
        # Every gesture the screen tells a person about has to work, or the
        # hint is a lie. There are four, and this drives all four.
        assert_eq(len(spec["gestures"]), 4)

        # 1. drag empty space = pan
        before = q(tf, "pan")
        inv(tf, "point", "700,700")
        inv(tf, "send", "PointerDown")
        inv(tf, "point", "760,730")
        inv(tf, "send", "PointerUp")
        after = q(tf, "pan")
        assert after != before, f"a drag on empty canvas pans: {before} -> {after}"

        # 2. wheel = zoom
        was = q(tf, "zoom")
        inv(tf, "send", "WheelUp")
        assert q(tf, "zoom") > was, "the wheel zooms in"
        inv(tf, "send", "WheelDown")
        assert_eq(q(tf, "zoom"), was, "and back")

        # 3. drag a node = place it
        #    ★ R1653 — where it is PAINTED, not the rect the card carries: the
        #    canvas is a viewport over a world surface, so a card's own
        #    rectangle is stated in that surface's coordinates and a press aimed
        #    at it lands outside the window.
        held = window_of(tf, "lab.node.T-01")
        inv(tf, "point", centre(held))
        inv(tf, "send", "PointerDown")
        inv(tf, "point", f"{held['x'] + held['w'] // 2 + 40},{held['y'] + held['h'] // 2 + 30}")
        inv(tf, "send", "PointerUp")
        moved = window_of(tf, "lab.node.T-01")
        assert moved != held, f"a node drag places it: {held} -> {moved}"

        # 4. drag a pin = author a link
        links_before = len(json.loads(q(tf, "links")))
        inv(tf, "point", at(tf, "lab.pin.T-01.dial"))
        inv(tf, "send", "PointerDown")
        inv(tf, "point", at(tf, "lab.pin.P-03.accept"))
        inv(tf, "send", "PointerUp")
        links_after = json.loads(q(tf, "links"))
        assert_eq(
            len(links_after), links_before + 1,
            "a drag from a dial pin to an accept pin authors a link",
        )
        assert any(l["from"] == "T-01" and l["to"] == "P-03" for l in links_after), links_after
        print(f"[I] all {len(spec['gestures'])} advertised gesture(s) answer: pan, "
              f"zoom, place, and author")

        # ── (J) A link the model cannot hold is refused BY NAME ─────────────
        # The crate refuses it, not this screen: `Document::connect` is the one
        # authority, so the canvas, the wire and the gate cannot disagree.
        why = refused(tf, "connect", "R-01,T-01")
        assert "T-01" in why, why
        assert "listen" in why or "dial" in why, (
            f"and the refusal says WHY rather than 'invalid': {why}"
        )
        # And a listening node takes as many dials as reach it: the reference's
        # router shows four inbound links on one pin, which a dataflow input
        # could not hold.
        inbound = sum(1 for l in json.loads(q(tf, "links")) if l["to"] == "R-01")
        assert inbound >= 3, f"the router is dialled by {inbound} node(s)"
        print(f"[J] an impossible link is refused by name; the router's one pin "
              f"holds {inbound} inbound link(s)")

        # ── (K) Adding a key moves it out of the offered set ────────────────
        offered = spec["addable"][0]
        click(tf, at(tf, f"lab.form.add.{offered}"))
        form = json.loads(q(tf, "form"))
        assert offered in [f["key"] for f in form], f"{offered} is now a row"
        painted = tags(paint(tf))
        assert f"lab.form.add.{offered}" not in painted, (
            "★ and it is no longer offered — two rows for one path is a "
            "configuration with no single value"
        )
        assert f"lab.form.applies.{offered}" in painted, "with its own applies badge"
        print(f"[K] adding {offered!r} made it a row and retired its chip")

        # ── (L) A declared read cannot be written ───────────────────────────
        for path in ("running", "zoom", "selected"):
            try:
                tf.intervene(f"{EXT}/{path}", 1)
            except Exception as why:  # noqa: BLE001
                assert "read" in str(why).lower() or "only" in str(why).lower(), (
                    f"{path} refuses a write as read-only: {why}"
                )
            else:
                raise AssertionError(f"a write to {path} was expected to be refused")
        print("[L] the three derived reads refuse a write as read-only")

        # ── (M) Selecting another node re-derives the whole inspector ───────
        click(tf, at(tf, "lab.node.P-01"))
        assert_eq(q(tf, "selected"), "P-01")
        form = json.loads(q(tf, "form"))
        assert any(f["key"] == "discovery.multicast" for f in form), (
            "the peer the gate warned about holds the key it warned about"
        )
        painted = tags(paint(tf))
        assert "lab.form.control.discovery.multicast" in painted
        assert find_by_tag(paint(tf), "lab.inspector.id") is not None
        print("[M] selecting another node re-derives its rows, badges and degree")

        # ── (N) Running settles the form ────────────────────────────────────
        click(tf, at(tf, "lab.node.R-01"))
        inv(tf, "set_field", "connect.endpoints=tcp/10.0.0.99:7449")
        assert any(f["edited"] for f in json.loads(q(tf, "form"))), "the row is edited"
        assert_eq(inv(tf, "run", True), True, "and the gate is open, so it runs")
        assert not any(f["edited"] for f in json.loads(q(tf, "form"))), (
            "★ a launch settles every row: what is running now IS what the "
            "screen shows, so nothing is pending a restart"
        )
        inv(tf, "run", False)
        print("[N] a launch settles the form, so nothing is left pending a restart")

        # ── (O) ★★★★★ a real press, through the §5.35 router ────────────────
        #
        # Every `click(...)` above is `invoke("point")` + `invoke("send")` — the
        # lab's own oracle, handed the answer the router has to WORK OUT from a
        # bare coordinate. That gap is not theoretical on this screen's family:
        # R1649.1 measured a sibling shell dead to a mouse with 118 assertions
        # green, and R1663 shipped a second sibling with the same defect in both
        # of its joins. This section is the one that would have failed.
        assert_router_press_moves(
            tf, "lab.node.P-02", lambda: q(tf, "selected"), "O: a node card"
        )
        assert_router_press_moves(
            tf, "lab.toolbar.zoom.in", lambda: q(tf, "zoom"), "O: a toolbar stepper"
        )
        # ★ The negative control: same verb, a decorative point, nothing moves.
        before = (q(tf, "selected"), q(tf, "zoom"))
        tf.request("scene/click", {"button": "left", "at": {"x": 3, "y": 3}})
        tf.tick(16)
        assert_eq(
            (q(tf, "selected"), q(tf, "zoom")),
            before,
            "O: ★ a press in the app bar's corner moves nothing",
        )
        print("[O] a real router press reaches the canvas and the toolbar")

        # ── (P) ★★★★★ R1681 — the rest of a link's life ─────────────────────
        #
        # The screen could make a link and could do nothing else to one. Every
        # check below drives the POINTER as well as the wire, because this
        # screen's whole defect history is "the wire does it and the pointer
        # does not".
        def links_now() -> list:
            return json.loads(q(tf, "links"))

        def link_between(a: str, b: str):
            return next((l for l in links_now() if l["from"] == a and l["to"] == b), None)

        # P1. An endpoint belongs to the LINK, and is published.
        held = link_between("P-01", "R-01")
        assert held is not None, links_now()
        assert held["endpoint"], (
            f"★ the link says which of the target's addresses it dialled: {held}"
        )
        opening_endpoint = held["endpoint"]

        # P2. Growing the target's listen list makes a CHOICE appear on a wire
        #     that was already drawn — nothing is re-authored. The seats are one
        #     per address, always, so this holds whatever earlier sections left
        #     the list at.
        click(tf, at(tf, "lab.node.R-01"))
        inv(tf, "select_link", f"{held['id']}")

        def seats() -> list:
            return sorted(t for t in tags(paint(tf)) if t.startswith("lab.link.endpoint.")
                          and not t.endswith(".text"))

        def addresses() -> list:
            row = next(f for f in json.loads(q(tf, "form")) if f["key"] == "listen.endpoints")
            return [p.strip() for p in row["value"].split(",") if p.strip()]

        before_seats, before_addresses = len(seats()), len(addresses())
        assert_eq(before_seats, before_addresses if before_addresses > 1 else 0,
                  "★ one seat per address, and none at all when there is one "
                  "address — a choice between one thing is not a choice")
        click(tf, at(tf, "lab.form.item.listen.endpoints.add"))
        assert_eq(len(addresses()), before_addresses + 1, "the list grew by one")
        assert_eq(len(seats()), before_addresses + 1,
                  "★ and the wire that was ALREADY DRAWN grew a seat, with "
                  "nothing re-authored")

        # P3. Pressing the other seat MOVES the link's end — and the link keeps
        #     its identity, which is the whole reason the crate has one verb for
        #     this rather than a disconnect and a connect.
        click(tf, at(tf, "lab.link.endpoint.1"))
        moved = link_between("P-01", "R-01")
        assert moved is not None and moved["id"] == held["id"], (
            f"★ the link is the SAME link: {held} -> {moved}"
        )
        assert moved["endpoint"] != opening_endpoint, (
            f"★ and it dials the other address now: {moved}"
        )
        assert_eq(q(tf, "selected_link"), str(held["id"]),
                  "so the selection survived the move, having nothing to repair")

        # P4. Re-aiming at another node, by dragging the accept pin it lands on.
        #     Pressing an accept pin PICKS UP what arrived there.
        before = len(links_now())
        inv(tf, "point", at(tf, "lab.pin.R-01.accept"))
        inv(tf, "send", "PointerDown")
        inv(tf, "point", at(tf, "lab.pin.P-03.accept"))
        inv(tf, "send", "PointerUp")
        after = links_now()
        assert_eq(len(after), before, "a re-aimed link is moved, not added")
        again = next((l for l in after if l["id"] == held["id"]), None)
        assert again is not None and again["to"] == "P-03", (
            f"★ the same link now lands on another node: {again}"
        )

        # P5. Dropping a picked-up link on empty canvas lets it go, which is the
        #     rule every node editor has.
        before = len(links_now())
        inv(tf, "point", at(tf, "lab.pin.P-03.accept"))
        inv(tf, "send", "PointerDown")
        inv(tf, "point", "300,860")
        inv(tf, "send", "PointerUp")
        assert_eq(len(links_now()), before - 1,
                  "★ released over nothing, a picked-up link is disconnected")

        # P6. Delete, by the seat the picked link carries.
        target = link_between("S-01", "R-01")
        assert target is not None, links_now()
        inv(tf, "select_link", f"{target['id']}")
        assert "lab.link.act" in tags(paint(tf)), "the picked link carries one act"
        click(tf, at(tf, "lab.link.act"))
        assert link_between("S-01", "R-01") is None, (
            f"★ the act seat deleted it: {links_now()}"
        )
        assert_eq(q(tf, "selected_link"), "", "and nothing is picked afterwards")

        # P7. ★★ The other layer. A reported link is NOT in the graph, its act
        #     seat says the opposite word, and adopting runs the authoring rules.
        reported = json.loads(q(tf, "observed"))
        assert reported, "the screen opens with something reported"
        seen = reported[0]
        assert_eq(seen["layer"], "drift",
                  "★ nothing drawn accounts for it — that is what makes it drift")
        assert link_between(seen["from"], seen["to"]) is None, (
            "and it is not among the drawn links at all"
        )
        inv(tf, "select_link", f"{seen['from']}>{seen['to']}")
        assert_eq(q(tf, "selected_link"), f"{seen['from']}>{seen['to']}",
                  "a reported link is named by the pair it runs between")
        before = len(links_now())
        click(tf, at(tf, "lab.link.act"))
        assert_eq(len(links_now()), before + 1, "★ adopting DRAWS it")
        drawn = link_between(seen["from"], seen["to"])
        assert drawn is not None, links_now()
        assert_eq(json.loads(q(tf, "observed"))[0]["layer"], "matched",
                  "★ and the two layers now agree about it, which is derived "
                  "rather than a flag anybody set")
        print("[P] a link can be re-aimed, re-addressed, let go, deleted and "
              "adopted — and it keeps its identity through every one")


if __name__ == "__main__":
    run_demo("R1651 the node lab matches the reference", body)
