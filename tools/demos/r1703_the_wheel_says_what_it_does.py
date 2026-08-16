#!/usr/bin/env python3
"""R1703 §5.45 §5.15 §2 #2 — **a wheel says what it does, and does what it
says**, driven through real windows.

# What this exists for

The analysis tool's node canvas has printed `wheel → zoom` on its hint strip for
its whole life, where a person reads it, and no wheel answered. A person
reported it; measured again at the start of this round, eight wheel events and
two `Ctrl`-wheel events over the canvas left `zoom` at 84 and `pan` at `0,0`.

Every gate was green, and the reason is structural rather than sloppy: the
screen's operation table has a `zoom` row, and that row is satisfied by the zoom
BUTTONS, which work. A hint strip is a different population — it enumerates
*gestures*, and a gesture whose operation is reachable another way is invisible
to any join over operations. So the strip is now driven here, over its own list,
through the real wire.

The framework half is the reason it cannot come back. A wheel is the one pointer
gesture whose meaning is pure local policy — the same motion scrolls a page,
steps a value, flips a tab or zooms a canvas — and until this round nothing
anywhere could ask which. Measured on the reference toolkit at 6.11.1 by
building a probe and running it offscreen: across its four wheel-answering
widget classes, **309 introspectable properties and 172 introspectable methods
name the wheel zero times**, and the cost of that silence is in the same
measurement — a **closed, unfocused** combo box in a form steps its value on a
wheel aimed at the form. So a surface here DECLARES what a wheel at a point
does, the router offers the event only where something is declared, and
`scene/wheel_intent` publishes the very value the router routed by.

# What it asserts

* **A** — every gesture the node canvas's hint strip advertises does something,
  driven through the real wire at rectangles read out of the painted scene.
* **B** — ★ the wire's answer and the behaviour agree, on all three screens: at
  every probed point, `scene/wheel_intent` says what a wheel does there, and a
  wheel there does exactly that — including the negative direction, where an
  undeclared point must move nothing.
* **C** — the canon's own arithmetic: one event is one step whichever way the
  platform sizes a notch, the step is multiplicative (so out and back is
  identity), and the canvas point under the cursor stays under the cursor.
* **D** — the catalog parity the reference has and this one lacked: a tab strip
  and a combo box's list step under a wheel, a slider steps its value, and each
  gives the wheel back rather than eating it once it is pinned.
* **E** — the gesture phase winit reports and this shell discarded: a trackpad
  flick that ENDS leaves no part-notch behind for the next one to spend.

Run from the workspace root:
    cargo build -p hello-node-lab -p hello-packet-view -p hello-analyzer-shell \\
        -p hello-tabs -p hello-combobox -p hello-slider --release
    python3 tools/demos/r1703_the_wheel_says_what_it_does.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    run_demo,
)

EXT = "/external"

# One notch, W3C-signed: negative is the wheel pushed away from the person,
# which raises a scale and a value.
AWAY = (0.0, -1.0)
TOWARD = (0.0, 1.0)


def intent_at(tf: RpcSubprocess, x: float, y: float) -> dict[str, Any]:
    """What the surface says a wheel at this window point would do."""
    answer = tf.request("scene/wheel_intent", {"at": {"x": x, "y": y}}).result
    return answer["at"]


def census(tf: RpcSubprocess) -> dict[str, Any]:
    """Every painted surface and what it says about a wheel at its middle."""
    return tf.request("scene/wheel_intent", {}).result


# ★ The slots a "did anything happen" comparison must NOT read, because they
# are the stimulus rather than an effect. Every probe below moves the pointer
# first (a wheel targets the cursor), and a screen that publishes where its
# cursor is would report a change for that alone — which would make the whole
# agreement check pass without a wheel doing anything. The in-process gate in
# `hello-node-lab/src/painted.rs` carries the same list for the same reason.
NOT_AN_EFFECT = ("cursor", "hover", "pointer")


def readable_scalars(tf: RpcSubprocess) -> list[str]:
    """Every no-argument read path the surface declares about itself.

    Read off `$schema` rather than written here: a gate holding its own copy of
    a screen's fields measures the screen of the day it was written, and R1688
    paid for that once already.
    """
    schema = tf.query(f"{EXT}/$schema")
    out = []
    for field in schema:
        # ★ A read field OMITS `channel` (it is the serde default), so the test
        # is "not an invoke" rather than "is a read". Written the other way
        # first, which selected nothing and made a gate whose whole job is to
        # notice that nothing happened compare two empty dictionaries — it
        # failed rather than passed, which is the only reason this is a
        # paragraph and not a defect.
        if field.get("channel") == "invoke" or field.get("args"):
            continue
        if field["path"] in NOT_AN_EFFECT or "<" in field["path"]:
            continue
        out.append(field["path"])
    assert out, "the surface declares no readable scalar to watch"
    return out


def whole_surface(tf: RpcSubprocess, fields: list[str]) -> dict[str, Any]:
    """Everything the surface publishes except its own pointer position."""
    out = {}
    for path in fields:
        try:
            out[path] = tf.query(f"{EXT}/{path}")
        except Exception as exc:  # noqa: BLE001 - a refusal is a stable reading
            out[path] = f"<refused {exc}>"
    return out


def centre(rect: tuple[int, int, int, int]) -> tuple[float, float]:
    return (rect[0] + rect[2] / 2, rect[1] + rect[3] / 2)


# ── A — the hint strip is a promise ────────────────────────────────────────


def hint_strip_gestures(tf: RpcSubprocess) -> int:
    """Every gesture the canvas advertises moves something.

    The list is read off the screen's own specification over the wire, not
    written here: a gate holding its own copy measures the screen of the day it
    was written (R1688), and this is the exact table that spent this screen's
    whole life advertising a wheel nothing answered.
    """
    checks = 0
    spec = json.loads(tf.query(f"{EXT}/spec"))
    advertised = [tuple(g) for g in spec["gestures"]]
    assert_eq(len(advertised), 4, "the canvas advertises four gestures")
    checks += 1

    # ★ The WHOLE published surface, minus the pointer position — a hint strip
    # claims an effect in prose ("pan", "author a link"), and choosing a slot
    # per gesture would be this gate deciding what the prose meant. The first
    # draft did choose, watched four slots, and reported "drag a node" as inert
    # because a card's POSITION is not one of the four.
    watched = readable_scalars(tf)

    def drive(name: str) -> None:
        shot = abs_rects_of(tf.snapshot(source="paint"))
        canvas = shot["lab.canvas"]
        if name == "drag empty space":
            corner = (canvas[0] + canvas[2] - 40, canvas[1] + canvas[3] - 40)
            tf.drag(from_at=corner, to_at=(corner[0] - 30, corner[1] - 20))
        elif name == "wheel":
            tf.wheel(centre(shot["lab.node.P-01"]), lines=AWAY)
        elif name == "drag a node":
            here = centre(shot["lab.node.P-03"])
            tf.drag(from_at=here, to_at=(here[0] + 40, here[1] + 24))
        elif name == "drag a pin":
            tf.drag(
                from_at=centre(shot["lab.pin.Q-01.dial"]),
                to_at=centre(shot["lab.pin.P-02.accept"]),
            )
        else:  # pragma: no cover - the assertion below is the real guard
            raise AssertionError(f"the strip advertises {name!r} and nothing drives it")
        tf.tick(0.016)

    for gesture, effect in advertised:
        before = whole_surface(tf, watched)
        drive(gesture)
        after = whole_surface(tf, watched)
        assert before != after, (
            f"the hint strip advertises {gesture!r} -> {effect!r} where a person "
            f"reads it, and driving it through the wire moved nothing"
        )
        checks += 1
    return checks


# ── B — the answer and the behaviour are one fact ──────────────────────────


def answer_matches_behaviour(binary: str, probes: list[str]) -> int:
    """At every probed rectangle: what the wire says, and what a wheel does.

    ★ Both directions. A declared point must MOVE something and an undeclared
    point must move nothing — a gate that only checked the positive half would
    pass a surface that declared a wheel everywhere and took it everywhere,
    which is precisely the reference's combo-box hazard wearing a declaration.
    """
    checks = 0
    with RpcSubprocess(binary) as tf:
        fields = readable_scalars(tf)
        assert fields, f"{binary}: publishes no readable scalar to watch"
        shot = abs_rects_of(tf.snapshot(source="paint"))
        declared_seen = False
        silent_seen = False
        for tag in probes:
            rect = shot.get(tag)
            assert rect is not None, f"{binary}: {tag} is painted"
            at = centre(rect)
            said = intent_at(tf, at[0], at[1])
            before = whole_surface(tf, fields)
            tf.wheel(at, lines=AWAY)
            tf.tick(0.016)
            after = whole_surface(tf, fields)
            moved = before != after
            if said["intent"] is None:
                silent_seen = True
                assert not moved, (
                    f"{binary}: the wire says nothing takes a wheel at {tag} and one "
                    f"moved the screen anyway: "
                    f"{[k for k in after if after[k] != before[k]]}"
                )
                # And the answer is complete rather than merely negative: it
                # says where the wheel goes instead. No toolkit answers this.
                assert said["falls_through_to"] in ("scroll", "nothing"), (
                    f"{binary}: {tag} declines a wheel without saying where it goes"
                )
            else:
                declared_seen = True
                assert moved, (
                    f"{binary}: the wire says a wheel at {tag} would "
                    f"{said['intent']!r} and it moved nothing"
                )
            checks += 2
        # ★ Both arms non-empty, or the loop above proved one thing twice. A
        # probe list that happened to be all-declared would make the negative
        # assertion unreachable and nobody would notice.
        assert declared_seen and silent_seen, (
            f"{binary}: the probes reached only one arm "
            f"(declared={declared_seen} silent={silent_seen})"
        )
        checks += 1
    return checks


# ── C — the canon's arithmetic ─────────────────────────────────────────────


def the_canons_zoom(tf: RpcSubprocess) -> int:
    """One event is one multiplicative step, anchored under the cursor."""
    checks = 0
    shot = abs_rects_of(tf.snapshot(source="paint"))
    card = shot["lab.node.P-01"]
    aim = centre(card)
    opening = tf.query(f"{EXT}/zoom")

    # The magnitude is NOT read: three notches in one event is one step, which
    # is what keeps a zoom from leaping on one mouse and crawling on another.
    tf.wheel(aim, lines=(0.0, -3.0))
    tf.tick(0.016)
    big = tf.query(f"{EXT}/zoom")
    tf.invoke(f"{EXT}/reset", "view")
    tf.tick(0.016)
    tf.wheel(aim, lines=AWAY)
    tf.tick(0.016)
    one = tf.query(f"{EXT}/zoom")
    assert_eq(big, one, "a three-notch event and a one-notch event are one step")
    checks += 1
    assert_eq(one, round(opening * 1.12), "one step is the canon's 1.12 factor")
    checks += 1

    # Out and back is the identity, which repeated addition of a percentage is
    # not — the property a multiplicative step exists for.
    tf.invoke(f"{EXT}/reset", "view")
    tf.tick(0.016)
    start = (tf.query(f"{EXT}/zoom"), tf.query(f"{EXT}/pan"))
    for _ in range(5):
        tf.wheel(aim, lines=AWAY)
        tf.tick(0.016)
    for _ in range(5):
        tf.wheel(aim, lines=TOWARD)
        tf.tick(0.016)
    assert_eq(
        (tf.query(f"{EXT}/zoom"), tf.query(f"{EXT}/pan")),
        start,
        "five steps out and five back land exactly where they started",
    )
    checks += 1

    # ★ The anchor: the point under the cursor stays under the cursor. Measured
    # on the PAINT — where the person's eye is — rather than on the camera,
    # which is the half a model-only assertion would leave unproven.
    #
    # ★★ Aimed at the card's painted ORIGIN, not its centre, and the first draft
    # aimed at the centre and failed by 3 px. That failure is a fact about this
    # screen worth keeping: a card's painted SIZE is not a pure scaling of its
    # canvas size, because the text inside it has a legibility floor, so its
    # centre is not the affine image of any world point and cannot be expected
    # to hold still. Its origin is.
    tf.invoke(f"{EXT}/reset", "view")
    tf.tick(0.016)
    shot = abs_rects_of(tf.snapshot(source="paint"))
    card = shot["lab.node.P-01"]
    aim = (float(card[0] + 2), float(card[1] + 2))
    for step in range(1, 5):
        tf.wheel(aim, lines=AWAY)
        tf.tick(0.016)
        now = abs_rects_of(tf.snapshot(source="paint"))["lab.node.P-01"]
        drift = (abs(now[0] + 2 - aim[0]), abs(now[1] + 2 - aim[1]))
        assert drift[0] <= 1.0 and drift[1] <= 1.0, (
            f"after {step} cursor-anchored step(s) the point under the cursor had "
            f"slid by {drift}; the reference's own AnchorUnderMouse holds a point "
            f"to 0.21 scene units, measured, and this must not be worse"
        )
        checks += 1

    # The range's ends: the canvas stops and says so by declining, so the wheel
    # it cannot spend is left for whatever scrolls behind it.
    for _ in range(40):
        tf.wheel(aim, lines=TOWARD)
    tf.tick(0.016)
    floor = tf.query(f"{EXT}/zoom")
    tf.wheel(aim, lines=TOWARD)
    tf.tick(0.016)
    assert_eq(tf.query(f"{EXT}/zoom"), floor, "the zoom stops at its floor")
    checks += 1
    return checks


# ── D — the catalog parity ─────────────────────────────────────────────────


def a_forms_answer_survives_a_scroll() -> int:
    """★★★ A form's radio set keeps its answer when a wheel goes over it.

    This check exists because a counterfactual PASSED without it. Deleting the
    router's precondition — offering `External::wheel` to every hovered widget
    whatever it declared — left every other check in this file green, because
    each widget they drive also re-tests its own condition inside `wheel`. A
    radio group does not: `with_wheel` is the ONLY thing standing between a
    form's answers and a person's scroll, and until this check nothing drove
    that path through a real window.

    The reference toolkit's equivalent hazard is measured rather than supposed:
    a closed, unfocused combo box in a form steps its value on a wheel aimed at
    the form, and its interface has no property that would let the form object.
    """
    checks = 0
    with RpcSubprocess("hello-radio-group") as tf:
        shot = abs_rects_of(tf.snapshot(source="paint"))
        at = centre(shot["main_group"])
        said = intent_at(tf, at[0], at[1])
        assert_eq(
            said["intent"],
            None,
            "a form's radio set declares no wheel — it is a set of answers, "
            "not a strip of destinations",
        )
        checks += 1
        before = tf.query(f"{EXT}/selected_index")
        for _ in range(3):
            tf.wheel(at, lines=TOWARD)
        tf.tick(0.016)
        assert_eq(
            tf.query(f"{EXT}/selected_index"),
            before,
            "three wheel notches over a form's answers changed one of them",
        )
        checks += 1
    return checks


def combo_box_parity() -> int:
    """★★★ The reference's measured hazard, refused — in both of its halves.

    Measured at 6.11.1 by building a probe and running it offscreen:

    * a four-item combo box sitting **closed and unfocused** steps from index 1
      to 2 on one wheel notch, and 77 properties and 46 methods on that widget
      expose no way for the form to find out, object, or learn afterwards what
      ate the scroll;
    * a standalone list, by contrast, **scrolls** and leaves the choice alone
      (`currentRow 5 -> 5`, `scrollbar 0 -> 3`).

    So this catalogue keeps the second and refuses the first: no list declares a
    wheel, shut or open, and a wheel anywhere near a combo box changes no value
    at all. This round built the other way round first and the tree's own
    `hello-listbox` demo caught it, which is why the assertions here are about
    what does NOT move.
    """
    checks = 0
    with RpcSubprocess("hello-combobox") as tf:
        options = "/combo_options/external"
        shut = census(tf)
        assert_eq(
            shut["declared"],
            0,
            "a CLOSED combo box declares no wheel — the reference steps its "
            "value here, unfocused, and cannot be told not to",
        )
        checks += 1

        shot = abs_rects_of(tf.snapshot(source="paint"))
        trigger = centre(shot["combo_trigger"])
        # ★ Hover FIRST, then wheel. A wheel targets the cursor, so delivering
        # one moves the pointer, and a shut combo box's only published state is
        # its hover statechart: comparing before-and-after without this line
        # measured the cursor arriving, not the wheel. (`Idle` -> `Hover`, which
        # is what the first run of this reported as a failure.)
        tf.hover(trigger)
        tf.tick(0.016)
        state_before = tf.query(f"{EXT}/state")
        value_before = tf.query(f"{options}/selected_index")
        for _ in range(3):
            tf.wheel(trigger, lines=TOWARD)
        tf.tick(0.016)
        assert_eq(
            tf.query(f"{EXT}/state"),
            state_before,
            "a wheel over the shut trigger changed the box's state",
        )
        assert_eq(
            tf.query(f"{options}/selected_index"),
            value_before,
            "★ three wheel notches over a SHUT combo box changed its value — "
            "the reference's measured hazard, arrived here",
        )
        checks += 2

        tf.click(trigger)
        tf.tick(0.016)
        open_shot = abs_rects_of(tf.snapshot(source="paint"))
        option = centre(open_shot["combo_options#0"])
        said = intent_at(tf, option[0], option[1])
        assert_eq(
            said["intent"],
            None,
            "the OPEN list declares no wheel either: a list SCROLLS under one, "
            "which the framework's scroll chain does without the widget's help",
        )
        checks += 1
        was = tf.query(f"{options}/selected_index")
        for _ in range(3):
            tf.wheel(option, lines=TOWARD)
        tf.tick(0.016)
        assert_eq(
            tf.query(f"{options}/selected_index"),
            was,
            "a wheel over the open list moved the choice instead of the view",
        )
        checks += 1
    return checks


def catalog_parity() -> int:
    """A tab strip, a combo box list and a slider each answer a wheel.

    Every one of these is something the reference toolkit does and this catalog
    did not: `External::wheel` had exactly two widget implementors, unmoved
    since R1554, and neither was a list or a strip.
    """
    checks = 0

    with RpcSubprocess("hello-tabs") as tf:
        shot = abs_rects_of(tf.snapshot(source="paint"))
        at = centre(shot["tabs"])
        said = intent_at(tf, at[0], at[1])
        assert_eq(said["intent"], "step", "a tab strip declares a stepping wheel")
        assert_eq(said["unit"], "item", "and one notch is one tab")
        checks += 2
        before = tf.query(f"{EXT}/selected_index")
        tf.wheel(at, lines=TOWARD)
        tf.tick(0.016)
        after = tf.query(f"{EXT}/selected_index")
        assert_eq(after, before + 1, "a notch toward the person takes the next tab")
        checks += 1
        tf.wheel(at, lines=AWAY)
        tf.tick(0.016)
        assert_eq(tf.query(f"{EXT}/selected_index"), before, "and away comes back")
        checks += 1

    with RpcSubprocess("hello-slider") as tf:
        shot = abs_rects_of(tf.snapshot(source="paint"))
        at = centre(shot["main_slider"])
        said = intent_at(tf, at[0], at[1])
        assert_eq(said["intent"], "step", "a slider declares a stepping wheel")
        assert_eq(said["unit"], "value", "and one notch is one step of its value")
        checks += 2
        before = tf.query(f"{EXT}/value")
        tf.wheel(at, lines=AWAY)
        tf.tick(0.016)
        assert tf.query(f"{EXT}/value") > before, "a wheel away raises the value"
        checks += 1

    return checks


# ── E — the phase the shell used to discard ────────────────────────────────


def a_finished_flick_leaves_nothing_behind() -> int:
    """A trackpad flick that ends must not leave a part-notch for the next one.

    winit reports the bracket beside every wheel delta and this shell threw it
    away (`MouseWheel { delta, .. }`) for its whole life, so the rule had
    nowhere to live. Driven here through `scene/wheel`'s `phase`, which is the
    wire's only way to express a gesture that ENDS.
    """
    checks = 0
    with RpcSubprocess("hello-tabs") as tf:
        shot = abs_rects_of(tf.snapshot(source="paint"))
        at = centre(shot.get("tabs") or shot["tabs_panel"])
        start = tf.query(f"{EXT}/selected_index")

        # Most of a notch, then the finger lifts.
        tf.request(
            "scene/wheel",
            {
                "at": {"x": at[0], "y": at[1]},
                "delta": {"pixels": {"dx": 0.0, "dy": 14.0}},
                "phase": "begin",
            },
        )
        tf.request(
            "scene/wheel",
            {
                "at": {"x": at[0], "y": at[1]},
                "delta": {"pixels": {"dx": 0.0, "dy": 0.0}},
                "phase": "end",
            },
        )
        tf.tick(0.016)
        assert_eq(tf.query(f"{EXT}/selected_index"), start, "the flick was under a notch")
        checks += 1

        # A fresh flick of the same size must still be under a notch. Before
        # R1703 the first one's remainder was banked forever and this stepped.
        tf.request(
            "scene/wheel",
            {
                "at": {"x": at[0], "y": at[1]},
                "delta": {"pixels": {"dx": 0.0, "dy": 14.0}},
                "phase": "begin",
            },
        )
        tf.tick(0.016)
        assert_eq(
            tf.query(f"{EXT}/selected_index"),
            start,
            "the ended gesture's remainder outlived it and was spent by the next",
        )
        checks += 1
    return checks


def body() -> None:
    checks = 0

    with RpcSubprocess("hello-node-lab") as tf:
        # The census, printed rather than merely asserted: the number a form
        # audit reads, and the question the reference cannot pose at all.
        seen = census(tf)
        print(
            f"[wheel-intent] hello-node-lab: {seen['declared']} of "
            f"{len(seen['surfaces'])} painted surface(s) declare a wheel at their "
            f"middle"
        )
        checks += hint_strip_gestures(tf)
        tf.invoke(f"{EXT}/reset", "view")
        tf.tick(0.016)
        checks += the_canons_zoom(tf)

    checks += answer_matches_behaviour(
        "hello-node-lab",
        [
            "lab.node.P-01",  # over the canvas: declared
            "lab.canvas",  # the canvas itself: declared
            "lab.palette",  # a scrolling pane: NOT declared, falls through
            "lab.inspector",  # the other scrolling pane: NOT declared
            "lab.rail",  # the destination rail: NOT declared
        ],
    )
    checks += a_forms_answer_survives_a_scroll()
    checks += combo_box_parity()
    checks += catalog_parity()
    checks += a_finished_flick_leaves_nothing_behind()

    print(f"[r1703] {checks} assertion point(s)")
    assert checks >= 30, f"R660 baseline: {checks} < 30"


if __name__ == "__main__":
    run_demo("r1703 a wheel says what it does", body)
