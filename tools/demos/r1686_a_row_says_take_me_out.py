#!/usr/bin/env python3
"""R1686 §5.20 §5.21 — a settings row offers to be taken out, and the offered
set stops being a record of what has been typed.

Drives `hello-node-lab` over JSON-RPC. The screen's own operation table has
carried `remove a field` as **verb, no gesture** since R1677 — the wire could
take a row out of the inspector and nothing on the screen could. That column
exists to make exactly this impossible to forget, and this is the round that
answers it.

The affordance is the form PAINTER's, not this screen's: a seat cut out of the
trailing edge of every row's key line, published in the geometry the hit test
already reads, so the property grid gets it in the same act.

Making it reachable by a person is also what forced two model repairs, both
proven by a failing test before they were made:

  * a path typed in by hand and then taken out came back as a **catalogue
    chip**, so the form began offering, to every later reader, a key somebody
    once mistyped;
  * a row taken out kept the value that had been typed into it, so putting the
    key back **resurrected a number nobody could see they still had**.

And one the repair itself surfaced: a row put back went to the END of the form,
which left it permanently reporting a change nobody had made.

  (A) boot — the table declares the gesture, and every shown row has a seat.
  (B) the seat is where a person can hit it: inside its row, clear of the
      header the badges are laid into and clear of the control.
  (C) press it — the row goes, and the chip that puts it back appears.
  (D) put it back — same place, opening value, and the form is not dirty.
  (E) an edit does not survive the round trip.
  (F) a hand-typed path leaves no chip behind.
  (G) taking `listen.endpoints` out re-derives the card, because pins come
      from the form.
  (H) the field open on the row being removed is shut, not applied.
  (I) the seat is a named button to a screen reader.
  (J) the wire and the seat are the same act.

>= 30 assertions.
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
    assert_router_press_moves,
    find_by_tag,
    run_demo,
)

EXAMPLE = "hello-node-lab"
EXT = "/external"
VIEWPORT = (1440, 900)

# The row this demo takes out is ASKED FOR, not named — see `find_restorable`.
#
# ★★★★★ R1850 — it was a constant here, and twice that constant went stale
# under a surface change: R1690 named a set-of-words row, R1842 repointed it at
# `admin.permissions.write`, and neither key is one the screen can offer back
# now. The round trip this file is about needs a row that is BOTH written and
# re-offerable, and which rows those are is a fact about the screen that the
# screen can be asked for. A gate naming a key carries a copy of it.


def q(tf, path):
    return tf.query(f"{EXT}/{path}")


def rects(tf):
    return abs_rects_of(tf.snapshot(source="paint", viewport=VIEWPORT))


def form(tf):
    return json.loads(q(tf, "form"))


def control(tf, key: str) -> str:
    """The address that row's control is painted under — the SCREEN's.

    ★ R2050 — asked rather than spelled: a walk cannot call the framework's own
    composition, and a wrong letter here would aim at a mark that is not there.
    """
    return next(row["control"] for row in form(tf) if row["key"] == key)


def row_value(tf, key: str) -> str:
    held = [field for field in form(tf) if field["key"] == key]
    assert held, f"the form holds {key}; it holds {[f['key'] for f in form(tf)]}"
    return held[0]["value"]


def keys(tf):
    return [field["key"] for field in form(tf)]


def press(tf, tag):
    box = rects(tf)[tag]
    tf.click(at=(box[0] + box[2] // 2, box[1] + box[3] // 2))


def find_restorable(tf) -> tuple[str, str]:
    """The first `(card, row)` where taking the row off is UNDOABLE.

    ★★★★★ R1850 — two conditions, both asked of the screen rather than assumed:

    * **offerable** — `catalogue.restorable` is the screen's own answer to
      "which of these come back if you take them off". Measured at R1850, that
      is not every row: the form object counts its present rows as catalogue
      entries, but this screen rebuilds the form each render from its own list
      of offerable keys, so a row it opened with can leave for good.
    * **written**, not worked out from the canvas. A derived row is *disowned*
      rather than removed, so it never leaves the form and section (C)'s claim
      would be about a different act.
    """
    for card in [c for c in q(tf, "nodes").split(",") if c]:
        tf.invoke(f"{EXT}/select", card)
        rows = {f["key"]: f for f in form(tf)}
        for key in json.loads(q(tf, "catalogue"))["restorable"]:
            if rows[key].get("source") is None:
                return card, key
    raise AssertionError(
        "no card has a written row the screen can offer back — the round trip "
        "this file is about is unreachable, which is a finding and not a "
        "reason to skip"
    )


def find_typeable_restorable(tf) -> str:
    """A row that opens a FIELD when its control is pressed, and comes back.

    ★★★★★ R1850 — section (H) needs both, and measured on this screen no row
    that OPENS with the card is both: the only written, re-offerable rows are
    booleans, whose control is the catalogue's switch (R1837) and toggles
    rather than opening a box. So the row is made: an offered key is added,
    which puts it in the intersection by construction, and the first one whose
    published `ty` is neither a boolean nor a chooser is the one driven.

    ⚠⚠ **Chosen by PRESSING, not by reading the type word.** The first draft
    filtered on `ty != "bool"` and no `options`, and picked `connect.endpoints`
    — an `address[]`, whose control is a column of per-element rows and opens
    no box either. Adding `source is None` did not help, because on a card the
    canvas draws no link out of, that row is written. ⇒ *which shapes get a
    plain text box is a fact about the screen's control catalogue*, and a demo
    enumerating them is a third copy of it. So the question is asked the only
    way it is guaranteed to be answered correctly: press the control and see
    whether a field opened.
    """
    for key in json.loads(q(tf, "catalogue"))["offered"]:
        tf.invoke(f"{EXT}/add_field", key)
        seat = control(tf, key)
        scroll_to(tf, seat)
        press(tf, seat)
        opened = json.loads(q(tf, "editing"))["target"] == f"value:{key}"
        if opened:
            # ⚠ Only when it opened: `lab.edit` is painted only while a field
            # is up, so an unconditional Escape is a refusal from the wire
            # ("tag not found in paint scene") rather than a tidy-up.
            tf.key(path="lab.edit", name="Escape")
            return key
        tf.invoke(f"{EXT}/remove_field", key)
    raise AssertionError(
        "no offered key opens a field when its control is pressed — section "
        "(H) is about a box standing over a row, and there is no row it can "
        "stand over"
    )


def scroll_to(tf, tag: str) -> None:
    """Bring `tag` into the painted viewport, or say it cannot be reached.

    ⚠ R1850 — needed because R1842 grew the option surface from 53 paths to
    111, and the offered chips grew with it: measured on this screen, six are
    offered and TWO are in view. `scene/scroll_reach` reports the rest
    `scrollable`, which is the screen being right — the chip is reachable and
    is not on the first screenful. A press that assumed otherwise was asserting
    the scroll position.
    """
    if tag in rects(tf):
        return
    for _ in range(40):
        tf.scroll("lab.inspector.body", by=(0, 60))
        tf.tick_ms(16)
        if tag in rects(tf):
            return
    raise AssertionError(f"{tag} never came into view after scrolling")


def type_keys(tf, text):
    for ch in text:
        tf.key(path="lab.edit", name=ch)


def dirty(tf) -> bool:
    """Whether the form differs from the state it opened in — the screen's own
    predicate, which is what its reset affordance is gated on."""
    return json.loads(q(tf, "changed"))["fields"]


def resolves(tf, at) -> str:
    """What a press at that pixel would be, asked of the screen rather than
    computed here."""
    return tf.invoke(f"{EXT}/point", f"{at[0]},{at[1]}")


def assert_chips_answer_for_themselves(tf) -> None:
    """★★★★★ Every painted add-chip resolves a press to ITS OWN key.

    Found on the running screen while proving the seat, and the cause was one
    layer under this screen: the chip row's widths come from the text measurer,
    which answers only inside an owner scope — so the paint (inside) and the hit
    test (outside) wrapped the row differently and a chip's rectangle carried a
    neighbour's key. Pressing one chip added the key of the chip beside it.

    A gate over EVERY chip rather than the one that was wrong, which is R1684.2's
    lesson: a gate placed where the last defect was finds only the last defect.
    """
    painted = rects(tf)
    for tag, box in sorted(painted.items()):
        if not tag.startswith("lab.form.add."):
            continue
        key = tag[len("lab.form.add.") :]
        at = (box[0] + box[2] // 2, box[1] + box[3] // 2)
        assert_eq(
            resolves(tf, at),
            f"add:{key}",
            f"★★ the chip painted at {box} is tagged {tag} and a press on its "
            "centre must be that key — the paint and the hit test are two "
            "readings of one layout, and a difference is a press that lands "
            "on the wrong configuration path",
        )


def pin_ring(tf, card: str):
    """The colour a card's accept pin is ringed in, read off the paint."""
    node = find_by_tag(
        tf.snapshot(source="paint", viewport=VIEWPORT), f"lab.pin.{card}.accept"
    )
    assert node is not None, f"{card} has an accept pin"
    return node.get("style", {}).get("border")


def access(tf):
    resp = tf.request("scene/access", {})
    assert resp is not None and resp.result is not None, "scene/access must answer"
    return resp.result["nodes"]


def refused(tf, verb, args) -> str:
    try:
        tf.invoke(f"{EXT}/{verb}", args)
    except RpcError as why:
        return str(why)
    raise AssertionError(f"{verb} {args!r} was accepted and had to be refused")


def overlaps(a, b) -> bool:
    return (
        a[0] < b[0] + b[2]
        and b[0] < a[0] + a[2]
        and a[1] < b[1] + b[3]
        and b[1] < a[1] + a[3]
    )


def inside(a, b) -> bool:
    return (
        a[0] >= b[0]
        and a[1] >= b[1]
        and a[0] + a[2] <= b[0] + b[2]
        and a[1] + a[3] <= b[1] + b[3]
    )


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) the declaration, and a seat on every shown row ──────
        spec = json.loads(q(tf, "spec"))
        op = {row["name"]: row for row in spec["operations"]}["remove a field"]
        assert_eq(op["gesture"], True, "★ 'remove a field' has a way in for a person")
        assert_eq(op["verb"][0], "remove_field", "and still has its verb")
        assert_eq(op["absent"], False, "so the operation is answered on both channels")

        # ★ R1850 — the card and the row, asked for rather than named.
        card, ROW = find_restorable(tf)
        tf.invoke(f"{EXT}/select", card)

        opening = keys(tf)
        assert ROW in opening, f"the opening form holds {ROW}"
        painted = rects(tf)
        # ★★★ R1716 — every shown row still offers exactly ONE seat, and which
        # act it is says who owns the row's value: a row somebody wrote can be
        # taken away, and a row the screen works out can be taken OVER. The
        # claim this file was written for is unchanged — a seat on some rows and
        # not others would be a screen deciding for itself which configuration a
        # person may shrink — and it is now checked over both acts, because a
        # row with neither seat is the failure and a row with the wrong one is a
        # press the form refuses.
        derived = {field["key"] for field in form(tf) if field["source"]}
        for key in opening:
            want = "author" if key in derived else "remove"
            other = "remove" if key in derived else "author"
            assert f"lab.form.{want}.{key}" in painted, (
                f"★★ every shown row offers a seat, and {key} has no {want} seat — "
                "a seat on some rows is a screen that decides for itself which "
                "configuration a person is allowed to shrink"
            )
            assert f"lab.form.{other}.{key}" not in painted, (
                f"★★ and only ONE — {key} offers both acts at the same edge, so "
                "a press there means whichever the painter drew last"
            )
        # The other direction, which is the half a count cannot see: a seat for
        # a row that is not there would be a press with nothing behind it.
        seats = [
            t.split(".", 3)[3]
            for t in painted
            if t.startswith("lab.form.remove.") or t.startswith("lab.form.author.")
        ]
        assert_eq(
            sorted(seats),
            sorted(opening),
            "★ and no seat names a row the form does not hold",
        )

        # ── (B) the seat is hittable, and hits nothing else ─────────
        row_seat = painted[f"lab.form.remove.{ROW}"]
        assert row_seat[2] > 0 and row_seat[3] > 0, "the seat has a size"
        control_box = painted[control(tf, ROW)]
        applies = painted[f"lab.form.applies.{ROW}"]
        assert not overlaps(row_seat, control_box), (
            f"the seat {row_seat} is not on the control {control_box}"
        )
        assert not overlaps(row_seat, applies), (
            "★ the seat is not on the applies badge — the header is laid out by "
            "the flex pass and a seat overlaid on it would be painted under a "
            "badge whose box is 'right'"
        )
        pane = painted["lab.inspector"]
        assert inside(row_seat, pane), f"and it is inside the pane {pane}"

        # Every corner answers the seat, not the row under it. R1684.2: a check
        # that aims at centres cannot see an error smaller than half a control,
        # and this control is fifteen pixels wide.
        for dx, dy in ((0, 0), (row_seat[2] - 1, 0), (0, row_seat[3] - 1),
                       (row_seat[2] - 1, row_seat[3] - 1)):
            at = (row_seat[0] + dx, row_seat[1] + dy)
            assert_eq(
                resolves(tf, at),
                f"remove:{ROW}",
                f"★ the corner {at} of the seat is the seat",
            )

        # ── (C) press it ───────────────────────────────────────────
        before = form(tf)
        assert_router_press_moves(
            tf, f"lab.form.remove.{ROW}", lambda: q(tf, "form"), "the row goes"
        )
        after = keys(tf)
        assert ROW not in after, "★★ the row is gone, caused by a pointer press"
        assert_eq(len(after), len(before) - 1, "exactly one row left")
        assert_eq(
            [k for k in after],
            [f["key"] for f in before if f["key"] != ROW],
            "and the rest kept their order",
        )
        assert f"lab.form.remove.{ROW}" not in rects(tf), "its seat went with it"
        assert ROW in set(json.loads(q(tf, "catalogue"))["offered"]), (
            "★ and the chip that puts it back is offered — the key is one this "
            "kind can have, which is why it opened holding it"
        )
        scroll_to(tf, f"lab.form.add.{ROW}")
        assert f"lab.form.add.{ROW}" in rects(tf), "and it is painted, in reach"
        assert_chips_answer_for_themselves(tf)

        # ── (D) put it back ────────────────────────────────────────
        press(tf, f"lab.form.add.{ROW}")
        assert_eq(
            keys(tf),
            opening,
            "★★ the row came back WHERE IT OPENED, not at the end — the order "
            "is what a reader navigates a form by, and it is half of what "
            "'has this changed' compares",
        )
        assert_eq(
            dirty(tf),
            False,
            "★★ so a form at its opening rows and values reports no change",
        )

        # ── (E) the edit does not survive the round trip ────────────
        held = row_value(tf, ROW)
        # ★ R1850 — the value to write is derived from the one that is there.
        # This said `=write`, a word from the set-valued row the surface used
        # to have; the row it drives now is whatever `find_restorable` picked,
        # so the only value guaranteed to MOVE it is the other one it can hold.
        assert held in ("true", "false"), (
            f"this section flips a boolean row and {ROW} holds {held!r} — the "
            "derivation picked a shape it cannot drive"
        )
        moved = "false" if held == "true" else "true"
        tf.invoke(f"{EXT}/set_field", f"{ROW}={moved}")
        assert row_value(tf, ROW) != held, "the value moved"
        press(tf, f"lab.form.remove.{ROW}")
        scroll_to(tf, f"lab.form.add.{ROW}")
        press(tf, f"lab.form.add.{ROW}")
        assert_eq(
            row_value(tf, ROW),
            held,
            "★★★ a row taken out and put back holds what it OPENED with. A "
            "removed row keeping its edit is a ghost: it is off the screen, "
            "nothing shows the value, and putting the key back resurrects a "
            "number nobody can see they still have",
        )
        assert_eq(dirty(tf), False, "and the form is clean again")

        # ── (F) a hand-typed path leaves no chip ───────────────────
        typed = "transport.unicast.lowlatency"
        press(tf, "lab.inspector.addkey")
        type_keys(tf, typed)
        press(tf, "lab.inspector.rename")
        assert typed in keys(tf), "the typed path is a row"
        assert f"lab.form.remove.{typed}" in rects(tf), "with a seat like any other"
        press(tf, f"lab.form.remove.{typed}")
        assert typed not in keys(tf), "and the seat takes it away"
        assert f"lab.form.add.{typed}" not in rects(tf), (
            "★★★ and it leaves NO chip. The offered set is a fact about the "
            "node's kind — the keys worth reaching for — not a record of what "
            "this one node has been through; a form that started offering a "
            "path because somebody once typed it would be publishing one "
            "node's history as the catalogue"
        )

        # ── (G) the card is re-derived, because pins come from the form ──
        #
        # ★★ MEASURED, and the first draft of this block asserted the opposite.
        # The accept pin does not vanish, and it should not: the reference's own
        # palette legend says the ROLE decides whether a node has one and the
        # listen address decides how it is DRAWN — a pin with no address is
        # shown unfilled, because "this kind of node accepts and this one has
        # nowhere to accept at" is the news, and a pin that disappeared would
        # hide it.
        tf.invoke(f"{EXT}/select", "P-03")
        ring_before = pin_ring(tf, "P-03")
        assert ring_before is not None, "a listening card's accept pin has a ring"
        press(tf, "lab.form.remove.listen.endpoints")
        assert "listen.endpoints" not in keys(tf), "the row went"
        assert "listen.endpoints" not in q(tf, "document"), (
            "★★ and it left the deployable document — taking a row out is a "
            "change to the CONFIGURATION, not to a list of rows on a screen"
        )
        assert "lab.pin.P-03.accept" in rects(tf), (
            "★ the pin stays, because the role is what gives a node one"
        )
        assert_eq(
            pin_ring(tf, "P-03") != ring_before,
            True,
            "★★ and the pin is drawn differently now — the address is what the "
            "ring is derived from, and the row that held it is gone",
        )
        assert any(
            "nothing is listening" in line["sentence"]
            for line in json.loads(q(tf, "gate"))
        ), "★ and the launch gate says so in words"

        # ── (H) the field open on that row is shut, not applied ────
        tf.invoke(f"{EXT}/select", "P-02")
        # ★ R1850 — a row of its own, because this section needs one that OPENS
        # A FIELD and the row the rest of the file drives is a boolean whose
        # control is a switch. See `find_typeable_restorable`.
        typed_row = find_typeable_restorable(tf)
        # This card's own opening value, not one captured on another card — an
        # assertion comparing across cards would be comparing two facts.
        opened_as = row_value(tf, typed_row)
        # The card that made the chip defect visible: it holds one more row
        # than the router, so the offered set wraps onto three lines and the
        # measured/estimated difference changes which line a chip is on.
        assert_chips_answer_for_themselves(tf)
        press(tf, control(tf, typed_row))
        assert_eq(
            json.loads(q(tf, "editing"))["target"],
            f"value:{typed_row}",
            "the field is open on the row",
        )
        type_keys(tf, "x")
        press(tf, f"lab.form.remove.{typed_row}")
        assert_eq(
            json.loads(q(tf, "editing"))["target"],
            None,
            "★★ the field shut with the row it was standing on — a box over a "
            "row that is gone is a box aimed at nothing",
        )
        assert typed_row not in keys(tf), "and the row went"
        scroll_to(tf, f"lab.form.add.{typed_row}")
        press(tf, f"lab.form.add.{typed_row}")
        assert_eq(
            row_value(tf, typed_row),
            opened_as,
            "★ and what was half-typed into it was NOT applied on the way out",
        )

        # ── (I) a screen reader is told what the seat does ─────────
        nodes = {n["tag"]: n for n in access(tf) if n.get("tag")}
        seat = nodes.get(f"lab.form.remove.{ROW}")
        assert seat is not None, "the seat is in the access tree"
        assert_eq(seat["role"], "button", "as a button")
        assert_eq(
            seat["name"],
            f"remove {ROW}",
            "★ named by what it does and which row it does it to — a bare "
            "glyph announces as its own character",
        )

        # ── (J) the wire and the seat are the same act ─────────────
        wire_before = keys(tf)
        tf.invoke(f"{EXT}/remove_field", ROW)
        assert_eq(
            keys(tf),
            [k for k in wire_before if k != ROW],
            "★ the verb takes the same row out, through the same function",
        )
        why = refused(tf, "remove_field", ROW)
        assert "no such field" in why.lower() or "field" in why.lower(), (
            f"a row it does not hold is refused by name: {why}"
        )
        assert_eq(
            keys(tf),
            [k for k in wire_before if k != ROW],
            "and the refusal changed nothing",
        )


if __name__ == "__main__":
    sys.exit(run_demo("R1686 §5.21 — a row says take me out", body))
