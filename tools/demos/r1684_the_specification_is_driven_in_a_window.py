#!/usr/bin/env python3
"""R1684 §5.20 §5.21 §5.49 §2 #7 — the analyser screen's WHOLE specification,
caused in a real window.

The screen publishes its own operation list (`spec.operations`, R1677): thirty
rows, each saying what an agent invokes to cause it, whether a person has a way
in, and which introspection slot must change once it has happened. Until this
round that table was driven only in process — `painted.rs` builds a scene by
calling the view function directly, which is fast, deterministic, and cannot
mount the field's external, cannot deliver a keystroke, and never runs the
shell's input router. So every claim about a person's way in was a claim about
a harness rather than about the application.

This is the same table, driven through a real shell:

  * a fresh process per operation, so nothing is caused on a screen an earlier
    operation left behind;
  * the ACTION column through the wire, exactly as an agent would;
  * the GESTURE column through `scene/click`, `scene/drag` and `scene/key`,
    which go through winit's own event arc into the shell's router — including
    the keystrokes, which is the half no in-process gate can reach
    ([[debt-the-in-process-sweep-cannot-mount-a-screens-extra-externals]]);
  * every aim read out of the PAINTED scene, never computed here;
  * the witness read back through the screen's own introspection surface, so
    "it changed" means an agent can see that it changed.

The recipes below are cross-checked against the declaration in both directions:
a declared gesture with no recipe and a recipe for an undeclared operation are
both failures, which is what keeps this file from quietly covering less than
the table says.

Run from the workspace root:
    cargo build -p hello-node-lab --release
    python3 tools/demos/r1684_the_specification_is_driven_in_a_window.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    press_painted_tag,
    run_demo,
)

EXAMPLE = "hello-node-lab"
EXT = "/external"
VIEWPORT = (1440, 900)

# The seat that applies whatever the one text field is holding. Named once: it
# is the same button for a name, a configuration path and a value.
APPLY = "lab.inspector.rename"


# ── reading the screen ──────────────────────────────────────────────────────


def q(tf, path: str) -> str:
    return tf.query(f"{EXT}/{path}")


def catalogue_key(tf) -> str:
    """The chip key the operation table drives "add a field" with.

    Read off the screen's own published table rather than written down here, so
    a catalogue key that is made more precise moves this with it.
    """
    spec = json.loads(q(tf, "spec"))
    for op in spec["operations"]:
        if op["name"] == "add a field from the catalogue":
            return op["verb"][1]
    raise AssertionError("the operation table declares how a field is added")


def rects(tf) -> dict:
    return abs_rects_of(tf.snapshot(source="paint", viewport=VIEWPORT))


def rect_of(tf, tag: str):
    painted = rects(tf)
    assert tag in painted, f"{tag} is painted, so a person can aim at it"
    return painted[tag]


def centre(box) -> tuple[int, int]:
    return (box[0] + box[2] // 2, box[1] + box[3] // 2)


def witness(tf, slot: str) -> str:
    """One witness reading, through the surface an agent would use.

    A refusal is a reading too: "no node is selected" is a state the screen
    legitimately passes through, and an operation that moves it out of that
    state has changed the answer.
    """
    try:
        return f"ok:{q(tf, slot)}"
    except RpcError as why:
        return f"refused:{why}"


# ── causing things ──────────────────────────────────────────────────────────


def press(tf, tag: str) -> None:
    # ★★★★★ R1795 — through the shared press, which opens the toolbar's overflow
    # control first when it is holding this seat. R1791 gave the row the ability
    # to give a group up, so `lab.toolbar.config` is on the row at one width and
    # one press away at another; a demo that aimed at its rectangle stopped
    # finding one. This is the third demo to learn it and the second to learn it
    # from CI rather than from the round that caused it — see
    # `debt-nothing-enumerates-which-demo-asserts-which-wire-fact`.
    press_painted_tag(tf, tag, VIEWPORT)


def press_wire(tf, frm: str, to: str) -> None:
    """Press the middle of the wire between two cards.

    A wire is a path, and a path's painted rectangle is its bounding box — most
    of the canvas, with its centre nowhere near the stroke. So the aim comes
    from the two things that do have rectangles: the pins it runs between.
    """
    painted = rects(tf)
    a = centre(painted[f"lab.pin.{frm}.dial"])
    b = centre(painted[f"lab.pin.{to}.accept"])
    tf.click(at=((a[0] + b[0]) // 2, (a[1] + b[1]) // 2))


def drag_by(tf, tag: str, by: tuple[int, int]) -> None:
    at = centre(rect_of(tf, tag))
    tf.drag(from_at=at, to_at=(at[0] + by[0], at[1] + by[1]))


def drag_onto(tf, frm: str, to: str) -> None:
    painted = rects(tf)
    tf.drag(from_at=centre(painted[frm]), to_at=centre(painted[to]))


def type_keys(tf, text: str) -> None:
    """One key event per character, aimed at the field itself."""
    for ch in text:
        tf.key(path="lab.edit", name=ch)


def type_into(tf, tag: str, text: str) -> None:
    """Open the field by pressing something, type, and apply — the whole path a
    person takes to put a value anywhere on this screen."""
    press(tf, tag)
    type_keys(tf, text)
    press(tf, APPLY)


# ── the gesture recipes ─────────────────────────────────────────────────────
#
# One per operation the specification says a person can cause. Each is a
# sequence — put the cursor here, press, travel there, release, type — which is
# why it lives as code rather than as a row of the published table.

GESTURES = {
    # a node's life
    "add a node": lambda tf: press(tf, "lab.palette.role.Responder"),
    "delete a node": lambda tf: (
        press(tf, "lab.node.P-03"),
        press(tf, "lab.inspector.delete"),
    ),
    "rename a node": lambda tf: (
        press(tf, "lab.node.P-03"),
        type_into(tf, APPLY, "edge-01"),
    ),
    "move a node": lambda tf: drag_by(tf, "lab.node.P-03", (40, 24)),
    "collapse a node": lambda tf: (
        press(tf, "lab.node.P-03"),
        press(tf, "lab.inspector.collapse"),
    ),
    "disable a node": lambda tf: (
        press(tf, "lab.node.P-03"),
        press(tf, "lab.inspector.disable"),
    ),
    # a frame's life
    "re-parent a node between frames": lambda tf: drag_onto(
        tf, "lab.node.P-03", "lab.frame.host-b"
    ),
    "move a frame and its members": lambda tf: drag_by(
        tf, "lab.frame.host-b.caption", (30, 0)
    ),
    # the form
    # ★★ R1690 — the chip is named by the operation table's own argument rather
    # than written here. This line held a copy of the key, the option surface
    # made that key precise, and the copy went stale: the same shape as R1688's
    # hand-held seat list, one level out in the demo.
    "add a field from the catalogue": lambda tf: press(
        tf, "lab.form.add." + catalogue_key(tf)
    ),
    "add a field by typing its key": lambda tf: type_into(
        tf, "lab.inspector.addkey", "transport.unicast.lowlatency"
    ),
    "edit a field": lambda tf: press(tf, "lab.form.item.listen.endpoints.add"),
    # ★★ R1686 — the seat at the trailing edge of a row's key line. This row
    # read `gesture: false` from R1677 until now: the wire could take a row out
    # and the screen offered no way to, which the table was written to make
    # impossible to forget.
    "remove a field": lambda tf: press(
        tf, "lab.form.remove.admin.permissions.write"
    ),
    # ★★★ R1716 — the same edge of a row nobody wrote: the seat takes the value
    # OVER. `mode` is worked out from the role on every card, so it is the row
    # this is always available on.
    "take a derived field over": lambda tf: press(tf, "lab.form.author.mode"),
    # ★★★ R1684 — the launch gate, closed by a person. The stepper clamps at the
    # field's ceiling, correctly, so the only way past it is to type — which is
    # why this row read `gesture: false` until the form's rows learned to be
    # typed into.
    "validate": lambda tf: type_into(
        tf, "lab.form.control.transport.link.tx.batch_size", "70000"
    ),
    # a link's life
    "author a link": lambda tf: drag_onto(
        tf, "lab.pin.S-01.dial", "lab.pin.P-02.accept"
    ),
    "delete a link": lambda tf: (
        press_wire(tf, "Q-01", "R-01"),
        press(tf, "lab.link.act"),
    ),
    "rewire a link": lambda tf: drag_onto(
        tf, "lab.pin.R-01.accept", "lab.pin.P-03.accept"
    ),
    "select a link endpoint": lambda tf: press(tf, "lab.link.endpoint.1"),
    "adopt an observed link": lambda tf: (
        press_wire(tf, "P-01", "P-02"),
        press(tf, "lab.link.act"),
    ),
    # the view
    "pan": lambda tf: drag_from_canvas(tf, (-30, 20)),
    "zoom": lambda tf: press(tf, "lab.toolbar.zoom.in"),
    "toggle discovery": lambda tf: press(tf, "lab.palette.discovery"),
    # putting things back
    "reset the node set": lambda tf: press(tf, "lab.reset.nodes"),
    "reset the layout": lambda tf: press(tf, "lab.reset.layout"),
    "reset the fields": lambda tf: press(tf, "lab.reset.fields"),
    "reset the links": lambda tf: press(tf, "lab.reset.links"),
    "reset the view": lambda tf: press(tf, "lab.reset.view"),
    # ★★ R1687 — what leaves the screen, from the two seats the reference puts
    # side by side. They were the last pair absent on BOTH channels.
    "export the configuration": lambda tf: press(tf, "lab.toolbar.config"),
    "produce the launch script": lambda tf: press(tf, "lab.toolbar.script"),
    # ★★★ R1688 — the last two rows of the table, and with them the absence
    # count reaches zero. The fit is the zoom pill's trailing seat, where the
    # reference puts it; the jump is the LAUNCH CHIP, which had been on screen
    # saying the verdict and answering no press at all.
    "fit the graph to the view": lambda tf: press(tf, "lab.toolbar.fit"),
    "go to the first problem": lambda tf: press(tf, "lab.toolbar.gate"),
}


def drag_from_canvas(tf, by: tuple[int, int]) -> None:
    box = rect_of(tf, "lab.canvas")
    at = (box[0] + box[2] // 2, box[1] + box[3] // 2)
    tf.drag(from_at=at, to_at=(at[0] + by[0], at[1] + by[1]))


# ── driving one operation ───────────────────────────────────────────────────


def reach_precondition(tf, op: dict, table: dict) -> None:
    """Bring the screen to the state an operation needs, by CAUSING the earlier
    operation the specification names — preferring its gesture.

    A setup that wrote the state directly would let a reset be proven against a
    state no session can produce, which is why `needs` names an operation
    rather than describing a condition.
    """
    needed = op.get("needs")
    if not needed:
        return
    earlier = table.get(needed)
    assert earlier, f"{op['name']!r} needs {needed!r}, which the table does not hold"
    if needed in GESTURES:
        GESTURES[needed](tf)
        return
    verb = earlier.get("verb")
    assert verb, f"{op['name']!r} needs {needed!r}, which has no way in at all"
    tf.invoke(f"{EXT}/{verb[0]}", verb[1])


def screen(first: bool):
    return RpcSubprocess(EXAMPLE, boot_grace=1.5, ensure_build=first)


def body() -> None:
    inert: list[str] = []
    caused = 0

    with screen(True) as tf:
        spec = json.loads(q(tf, "spec"))
        table = {op["name"]: op for op in spec["operations"]}
        assert_eq(
            len(table), len(spec["operations"]), "the operations are named uniquely"
        )

        declared = {name for name, op in table.items() if op["gesture"]}
        assert_eq(
            sorted(declared),
            sorted(GESTURES),
            "★ the specification and this file name the same operations — a "
            "declared gesture with nothing behind it is what put a wheel on "
            "the hint strip that no wheel answers",
        )
        absent = sorted(name for name, op in table.items() if op["absent"])
        print(
            f"    driving {len(table)} operations "
            f"({sum(1 for op in table.values() if op['verb'])} by action, "
            f"{len(declared)} by gesture); "
            f"{len(absent)} this screen cannot do at all: {', '.join(absent)}"
        )

    for name, op in table.items():
        if op["verb"]:
            verb, arg = op["verb"]
            with screen(False) as tf:
                reach_precondition(tf, op, table)
                before = witness(tf, op["witness"])
                try:
                    tf.invoke(f"{EXT}/{verb}", arg)
                except RpcError as why:
                    inert.append(f"{name!r}: the wire refused `{verb} {arg}` ({why})")
                    continue
                after = witness(tf, op["witness"])
                caused += 1
                if before == after:
                    inert.append(
                        f"{name!r}: `{verb} {arg}` was accepted and "
                        f"`{op['witness']}` did not move"
                    )

        if name in GESTURES:
            with screen(False) as tf:
                reach_precondition(tf, op, table)
                before = witness(tf, op["witness"])
                GESTURES[name](tf)
                after = witness(tf, op["witness"])
                caused += 1
                if before == after:
                    inert.append(
                        f"{name!r}: the gesture ran in a real window and "
                        f"`{op['witness']}` did not move — this is the column "
                        f"an in-process test cannot see"
                    )

    assert not inert, (
        f"{len(inert)} of {caused} declared way(s) of causing an operation "
        "caused nothing in a real window:\n  " + "\n  ".join(inert)
    )
    print(f"    {caused} declared ways of causing an operation, all of them caused it")

    # ── and the field's own path, end to end in the window ──────────────
    #
    # The operation table says a witness MOVED; these say what it moved to,
    # over the path this round built. Kept in one process because they are one
    # session: open a row, type, apply, see the value.
    with screen(False) as tf:
        row = "transport.link.tx.batch_size"
        held = {f["key"]: f for f in json.loads(q(tf, "form"))}
        assert row in held, "the opening card has the row this drives"
        was = held[row]["value"]
        assert_eq(json.loads(q(tf, "editing"))["target"], None, "the field opens shut")

        press(tf, f"lab.form.control.{row}")
        editing = json.loads(q(tf, "editing"))
        assert_eq(
            editing["target"],
            f"value:{row}",
            "★★ pressing the middle of a form row opens the one field ON that "
            "row — the press that used to resolve to a name and then be dropped",
        )
        assert_eq(editing["text"], was, "seeded with the value that is there")

        type_keys(tf, "70000")
        assert_eq(
            json.loads(q(tf, "editing"))["text"],
            "70000",
            "★★ the keystrokes reached the buffer through the shell's own key "
            "path, which is the half no in-process gate can drive",
        )
        assert_eq(
            {f["key"]: f["value"] for f in json.loads(q(tf, "form"))}[row],
            was,
            "★ and typing alone has changed nothing — the row still holds what "
            "it held",
        )

        press(tf, APPLY)
        assert_eq(
            {f["key"]: f["value"] for f in json.loads(q(tf, "form"))}[row],
            "70000",
            "★ applied, and the value is the one that was typed",
        )
        assert_eq(json.loads(q(tf, "editing"))["target"], None, "the field shut behind it")
        verdict = json.loads(q(tf, "verdict"))
        assert verdict["blocking"] >= 1, (
            "★★★ the launch gate is CLOSED by a value a person typed. The "
            f"stepper cannot reach it — it clamps at the field's ceiling: {verdict}"
        )
        assert any(row in line["sentence"] for line in json.loads(q(tf, "gate"))), (
            "and the gate names the row the defect is about, not just a count"
        )

        # The same field, a different row, a different shape: a list ELEMENT.
        press(tf, "lab.form.item.listen.endpoints.0")
        editing = json.loads(q(tf, "editing"))
        assert_eq(
            editing["target"],
            "value:listen.endpoints[0]",
            "★★ a list's element is a target of its own — the add affordance "
            "puts a placeholder in the list, so a screen that could not edit "
            "one could only ever grow invented addresses",
        )
        elements = [
            e.strip()
            for e in {f["key"]: f["value"] for f in json.loads(q(tf, "form"))}[
                "listen.endpoints"
            ].split(",")
        ]
        assert_eq(editing["text"], elements[0], "seeded with that element alone")
        type_keys(tf, "tcp/0.0.0.0:7999")
        press(tf, APPLY)
        after = [
            e.strip()
            for e in {f["key"]: f["value"] for f in json.loads(q(tf, "form"))}[
                "listen.endpoints"
            ].split(",")
        ]
        assert_eq(after[0], "tcp/0.0.0.0:7999", "★ the element that was pressed changed")
        assert_eq(after[1:], elements[1:], "★ and its neighbours did not")

        # ★★★ Who owns a press INSIDE the open box. The field is painted over
        # the row, so the two hit targets are the same rectangle: if the
        # screen's own hit test answered it, the press would re-open the editor
        # and throw away what has been typed. Measured here rather than
        # reasoned about, because in process there is no field external to
        # compete with and the question cannot be asked at all.
        press(tf, "lab.form.control.id")
        assert_eq(
            json.loads(q(tf, "editing"))["target"], "value:id", "opened on the row"
        )
        type_keys(tf, "a9")
        assert_eq(json.loads(q(tf, "editing"))["text"], "a9", "holding what was typed")
        press(tf, "lab.form.control.id")
        assert_eq(
            json.loads(q(tf, "editing"))["text"],
            "a9",
            "★★ a second press inside the open box keeps the text — the field "
            "owns its own rectangle, and the screen's hit test stands aside",
        )

        # ★★★ And that press PLACES THE CARET. Every press on this screen is
        # routed to its one root external, so the field's own external never
        # sees a pointer — the screen forwards to the framework's hit test on
        # purpose. Proven by typing after a press at the LEFT edge of the box:
        # the characters land at the front, which can only happen if the caret
        # moved there.
        box = rect_of(tf, "lab.edit")
        tf.click(at=(box[0] + 3, box[1] + box[3] // 2))
        type_keys(tf, "z")
        assert_eq(
            json.loads(q(tf, "editing"))["text"],
            "za9",
            "★★★ a click at the front of the box put the caret at the front — "
            "without the caret hooks the box can be typed into and never "
            "clicked into",
        )
        tf.key(path="lab.edit", name="Escape")

        # A row whose control is not a plain text box: the affordances are the
        # shortcut and the row underneath them can still be typed, which is the
        # only way a person can put a value the shortcut cannot reach on the row
        # and see it reported.
        #
        # ★ R1842 — the whole-number row, where this was the set-of-words row
        # until the option surface stopped being written from memory: the
        # target declares no set-valued key here, so this screen has no such
        # row any more. The claim is unchanged — the middle of a control with
        # its own affordances is not dead space — and the stepper pair sits at
        # the trailing edge, so the middle is exactly what is being pressed.
        ROW_WITH_PARTS = "transport.link.tx.batch_size"
        # ⚠ R1850 — read the row HERE rather than from `held`. R1842 repointed
        # this section at the whole-number row without noticing that an EARLIER
        # section of this same demo types `70000` into it and applies, so the
        # stale snapshot said `65535` while the row held `70000` and the
        # "changing nothing" assertion below failed on a change this demo had
        # made itself. The claim is about what Escape does to the value that is
        # there NOW; anything else is comparing two moments.
        before_escape = {
            f["key"]: f["value"] for f in json.loads(q(tf, "form"))
        }[ROW_WITH_PARTS]
        press(tf, f"lab.form.control.{ROW_WITH_PARTS}")
        assert_eq(
            json.loads(q(tf, "editing"))["target"],
            f"value:{ROW_WITH_PARTS}",
            "★ the middle of a row with its own affordances opens the field "
            "too — before this it was a bordered box with dead space inside it",
        )
        tf.key(path="lab.edit", name="Escape")
        assert_eq(
            json.loads(q(tf, "editing"))["target"], None, "and Escape shuts it"
        )
        assert_eq(
            {f["key"]: f["value"] for f in json.loads(q(tf, "form"))}[
                ROW_WITH_PARTS
            ],
            before_escape,
            "changing nothing",
        )

        # The wire reaches the same rows by the same names it reads back.
        tf.invoke(f"{EXT}/edit", "value:listen.endpoints[0]")
        assert_eq(
            json.loads(q(tf, "editing"))["target"],
            "value:listen.endpoints[0]",
            "★ an agent opens the row an agent read, by the same spelling",
        )
        for bad, why in (
            ("value:not.a.row", "is not a row"),
            ("value:listen.endpoints[99]", "no element"),
            ("value:listen.endpoints[x]", "not an element number"),
        ):
            try:
                tf.invoke(f"{EXT}/edit", bad)
            except RpcError as refusal:
                assert why in str(refusal), f"{bad}: {refusal}"
            else:
                raise AssertionError(f"{bad} was accepted and had to be refused")


if __name__ == "__main__":
    sys.exit(
        run_demo("R1684 §5.21 — the specification, driven in a window", body)
    )
