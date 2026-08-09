#!/usr/bin/env python3
"""R1619 §5.35 §5.40 §2 #2 — the pointer wire says WHICH BUTTONS ARE HELD.

A `PointerEnter` delivered while the primary button is down is a different
fact from one delivered with nothing held: the first is the inner step of a
drag-select, the second is a hover. Before this round the two were
**byte-identical on the send wire**, so no consumer could tell them apart and
every drag-select in the framework was blocked at the substrate — not deferred
for want of a consumer, but unbuildable
(`debt-pointer-wire-omits-held-button-state`, measured R1562/R1563).

What the round added is the W3C `PointerEvent.buttons` state — which existed
already as `PointerButtons`, but only on the RAW capture channel an `External`
had to opt into. It is now the router's own per-pointer fact, stamped onto
every dispatched pointer event and published for reading.

Against the reference: the toolkit carries the held set on its single-point
event base, so its mouse, hover and enter events all answer it — but its *leave*
is not a pointing event at all. That handler takes the framework's plain BASE
event type: no position, no modifiers, no buttons. So "did the pointer leave me
mid-drag?" is answerable there only by consulting global state at an unrelated
moment. Here every arm is stamped, and the state is on the introspection wire
as well, so an AI driving the gesture can read it back.

Driven against `hello-column-select` (10 000 rows x 8 columns, virtualized,
two selection axes) because the item this unblocks is named on the Model/View
axis as "drag-select across sections" — the toolkit's `sectionEntered`.

What each check discriminates:

* **The held set is published, and it is empty at rest.** An empty list, never
  `null`: the framework owns this state on every backend, so an
  "axis unavailable" spelling would be a lie the wire cannot tell.
* **A press moves it and the release moves it back.** The state reflects the
  transition (a press includes its button, a release excludes it), so
  "buttons is empty" means the gesture is over rather than about to be.
* **A sweep selects the span; the identical cursor path with no press selects
  nothing.** That pair is the round's actual claim — the first check alone
  would pass against a grid that extended on every hover.
* **The release keeps the swept range.** A sweep has already written what it
  means; collapsing to the address under the cursor is the bug this ordering
  exists to prevent (it was written, and caught, during the round).
* **A chord decides the write.** `Ctrl`-anchored sweeps ADD; plain sweeps
  REPLACE. The chord held at the press is what the gesture is a function of.
* **The gesture cannot strand.** After a release the widget never hears — the
  pointer leaving the surface — the next event closes it, because the answer
  travels with the event instead of being re-derived.

Run from the workspace root:
    cargo build -p hello-column-select --release
    python3 tools/demos/r1619_the_wire_says_which_buttons_are_held.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    call,
    indexed_tags,
    run_demo,
)

EXAMPLE = "hello-column-select"
TABLE_TAG = "vtbl"


def header(col: int) -> str:
    """The press address of one column-header section."""
    return f"{TABLE_TAG}#h{col}"


def cell(row: int, col: int) -> str:
    return f"{TABLE_TAG}#{row}_{col}"


def held(tf: RpcSubprocess) -> list:
    """The buttons the framework believes are down, straight off the wire."""
    return call(tf, "scene/input_state")["held_pointer_buttons"]


def columns(tf: RpcSubprocess) -> list[int]:
    """Which columns the two-axis selection covers, flattened from its bands."""
    out: set[int] = set()
    for band in tf.query("/external/cells"):
        for lo, hi in band["columns"]:
            out.update(range(lo, hi + 1))
    return sorted(out)


def rows(tf: RpcSubprocess) -> list[int]:
    out: set[int] = set()
    for band in tf.query("/external/cells"):
        for lo, hi in band["rows"]:
            # The row axis spans the whole dataset for a column selection;
            # clamp so a whole-column band does not materialise 10 000 ints.
            out.update(range(lo, min(hi, lo + 32) + 1))
    return sorted(out)


def painted_headers(tf: RpcSubprocess) -> list[int]:
    """Which column-header sections are actually on screen right now.

    Asserted against before every gesture: selecting a column scrolls the
    column window, so an address that resolved a moment ago can leave the paint
    tree. A demo that addressed a scrolled-off section would fail as a *tag
    lookup*, which reads as "the feature is broken" rather than "the test
    pointed at the wrong pixel".
    """
    return sorted(indexed_tags(abs_rects_of(tf.snapshot(source="paint")), f"{TABLE_TAG}#h"))


def sweep(tf: RpcSubprocess, addresses: list[str], *, ctrl: bool = False) -> None:
    """Press the first address, cross the rest with the button held, release.

    The whole gesture goes through the ordinary pointer methods — no
    drag-specific RPC exists, and deliberately: a sweep is a press and some
    moves, and inventing a `scene/drag_select` would have let the AI-driven
    gesture diverge from the physical one.
    """
    on_screen = painted_headers(tf)
    for addr in addresses:
        if addr.startswith(f"{TABLE_TAG}#h"):
            col = int(addr.rsplit("h", 1)[1])
            assert col in on_screen, (
                f"section {col} is not painted (window shows {on_screen}) — the "
                "gesture would fail as a tag lookup, not as a behaviour"
            )
    if ctrl:
        tf.modifiers(ctrl=True)
    tf.pointer_button("left", "down", path=addresses[0])
    for addr in addresses[1:]:
        tf.hover(path=addr)
    tf.pointer_button("left", "up", path=addresses[-1])
    if ctrl:
        tf.modifiers()


def run(tf: RpcSubprocess) -> None:
    # ── 0. the read surface is discoverable, and it is a READ ────────────
    catalogue = {m["name"]: m for m in call(tf, "rpc/methods")["methods"]}
    assert "scene/input_state" in catalogue, (
        "an agent finds the held-button state through rpc/methods rather than "
        "by knowing the field name in advance"
    )
    assert "scene/pointer_button" in catalogue, "and the write it is the peer of"

    # ── 1. at rest: an empty LIST, never null ────────────────────────────
    state = call(tf, "scene/input_state")
    assert "held_pointer_buttons" in state, (
        "the axis is always present — the framework owns this state, so there "
        "is no backend that cannot answer it"
    )
    assert isinstance(state["held_pointer_buttons"], list), (
        f"a list, not {state['held_pointer_buttons']!r}: 'nothing held' and "
        "'cannot say' would otherwise be one answer"
    )
    assert_eq(held(tf), [], "nothing is held before anything is pressed")
    print("[demo] at rest the wire reports no held buttons")

    # ── 2. the state reflects the TRANSITION ─────────────────────────────
    tf.pointer_button("left", "down", path=header(2))
    assert_eq(held(tf), ["left"], "a press INCLUDES the button it pressed")
    tf.pointer_button("right", "down", path=header(2))
    assert_eq(
        held(tf),
        ["left", "right"],
        "a chord is a SET — the second press does not replace the first",
    )
    tf.pointer_button("middle", "down", path=header(2))
    assert_eq(
        held(tf),
        ["left", "middle", "right"],
        "all three, in the closed set's declaration order — a set, not a stack",
    )
    tf.pointer_button("middle", "up", path=header(2))
    assert_eq(
        held(tf),
        ["left", "right"],
        "the middle button leaves without disturbing the others, even though "
        "its GUI arc (the pan channel) is a different one entirely",
    )
    tf.pointer_button("right", "up", path=header(2))
    assert_eq(held(tf), ["left"], "a release EXCLUDES the button it released")
    tf.pointer_button("left", "up", path=header(2))
    assert_eq(held(tf), [], "and the last release empties the set")
    for name in ("left", "right"):
        assert name in ("left", "middle", "right"), (
            f"{name!r} must be a name `scene/pointer_button` accepts — the read "
            "is in the write's vocabulary or a client cannot round-trip it"
        )
    print("[demo] press/release moves the published set in both directions")

    # ── 3. the negative control FIRST, so the positive one has to earn it ─
    #      The identical cursor path with no button held must select nothing.
    tf.click(path=cell(0, 0))
    tf.click(path=cell(0, 0))  # settle on a known, uninteresting selection
    before = tf.query("/external/cells")
    for col in (2, 3, 4):
        tf.hover(path=header(col))
    assert_eq(
        tf.query("/external/cells"),
        before,
        "crossing three header sections with NO button held is a hover — if "
        "this moved, the check below would prove nothing about buttons",
    )
    assert_eq(held(tf), [], "and nothing was held while it happened")
    print("[demo] the same crossings without a press change nothing")

    # ── 4. the sweep: press a section, cross its neighbours ──────────────
    sweep(tf, [header(2), header(3), header(4)])
    swept = columns(tf)
    assert_eq(
        swept,
        [2, 3, 4],
        "a press on section 2 and a drag through 4 selects the SPAN — the "
        "toolkit's sectionEntered, which needed the held-button state to exist",
    )
    assert_eq(
        held(tf),
        [],
        "the gesture released, and the wire says so",
    )
    print(f"[demo] sweeping sections 2 -> 4 selected columns {swept}")

    # ── 5. the release keeps the range ───────────────────────────────────
    #      Written as its own check because the first draft of the widget got
    #      it wrong: the release is itself an event reporting the button UP,
    #      so a machine that closed the gesture before reading it collapsed
    #      every sweep back to one section on mouse-up.
    assert_eq(
        columns(tf),
        [2, 3, 4],
        "the range survives the release that ended it",
    )

    # ── 6. sweeping BACK shrinks, rather than ratcheting ─────────────────
    sweep(tf, [header(3), header(4), header(5), header(4)])
    assert_eq(
        columns(tf),
        [3, 4],
        "the range is re-derived from the fixed anchor each crossing, so "
        "retreating over covered ground gives it back",
    )

    # ── 7. the chord held at the PRESS decides the write ─────────────────
    sweep(tf, [header(0), header(1)])
    assert_eq(columns(tf), [0, 1], "a plain sweep replaces")
    sweep(tf, [header(3), header(4)], ctrl=True)
    assert_eq(
        columns(tf),
        [0, 1, 3, 4],
        "a Ctrl-anchored sweep ADDS its span — a chord that discarded what the "
        "user already picked would be a chord in name only",
    )
    assert_eq(held(tf), [], "and the chorded gesture closed too")
    sweep(tf, [header(3), header(4)])
    assert_eq(
        columns(tf),
        [3, 4],
        "...while the identical plain sweep over the same ground replaces — "
        "the control that shows the Ctrl arm is not just the same code",
    )
    print("[demo] Ctrl adds, plain replaces, over the same two sections")

    # ── 8. the gesture cannot strand ─────────────────────────────────────
    #      Press, then let the pointer leave the surface. The release happens
    #      somewhere this widget never hears about. Pre-R1619 a coordinator
    #      that kept its own "am I pressed" flag latched here forever and every
    #      later hover extended the selection; now the next event carries the
    #      truth and closes the gesture.
    tf.pointer_button("left", "down", path=header(2))
    assert_eq(held(tf), ["left"], "the gesture is open")
    tf.pointer_button("left", "up", path=header(2))
    marker = tf.query("/external/cells")
    for col in (0, 1, 3):
        tf.hover(path=header(col))
    assert_eq(
        tf.query("/external/cells"),
        marker,
        "hovering after the release moves nothing — the sweep is closed",
    )
    assert_eq(held(tf), [], "because the held set says so, not because a flag did")
    print("[demo] a finished gesture cannot be resumed by hovering")

    # ── 9. the row axis answers the same way ─────────────────────────────
    #      One vocabulary: a cell sweep travels the chord product a section
    #      sweep travels, so the two cannot mean different things.
    sweep(tf, [cell(1, 1), cell(3, 1)])
    touched = rows(tf)
    assert 1 in touched and 3 in touched, (
        f"a sweep down the body selects the rows it crossed: {touched}"
    )
    assert_eq(held(tf), [])
    print(f"[demo] a body sweep covered rows {touched}")

    # ── 10. the state is the framework's, so a stale read cannot lie ─────
    #       Two consecutive reads with nothing in between agree; the value is
    #       derived from the router rather than latched by whoever asked last.
    assert_eq(held(tf), held(tf), "the read is idempotent")
    assert_eq(
        call(tf, "scene/input_state")["held_pointer_buttons"],
        [],
        "and a fresh call agrees with the wrapper",
    )

    # ── 11. the two held-state axes stay separate ────────────────────────
    #       `held_keys` and `held_pointer_buttons` are neighbours in one reply
    #       and answer different questions. A press must not appear in the key
    #       list, and a chord must not appear in the button list — the mistake
    #       a single "held things" field would invite.
    final = call(tf, "scene/input_state")
    assert isinstance(final["held_keys"], list), "the key axis is still a list"
    tf.pointer_button("left", "down", path=header(1))
    during = call(tf, "scene/input_state")
    assert_eq(during["held_pointer_buttons"], ["left"])
    assert_eq(
        during["held_keys"],
        final["held_keys"],
        "pressing a mouse button does not change the held KEYS",
    )
    tf.modifiers(shift=True)
    with_shift = call(tf, "scene/input_state")
    assert_eq(
        with_shift["held_pointer_buttons"],
        ["left"],
        "and holding Shift does not change the held BUTTONS",
    )
    assert with_shift["modifiers"]["shift"] is True, "though the chord is visible"
    tf.modifiers()
    tf.pointer_button("left", "up", path=header(1))
    assert_eq(held(tf), [], "and both are released cleanly")
    for name in held(tf):
        assert name in ("left", "middle", "right"), f"closed vocabulary: {name!r}"

    print("[demo] the pointer wire says which buttons are held")


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        for _ in range(3):
            tf.tick(0.016)
        run(tf)


if __name__ == "__main__":
    run_demo("R1619 §5.35 §5.40 — the wire says which buttons are held", body)
