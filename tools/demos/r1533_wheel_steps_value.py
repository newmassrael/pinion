#!/usr/bin/env python3
"""R1533 §5.45 §5.38 §2 #7 — a value widget answers the wheel.

`External::wheel` has been the router's stage-1 offer since R877: on every
wheel event the framework asks the widget under the cursor first, and only
then falls through to the enclosing `Scene::Scroll`. A census of the whole
repo found **one** implementor — `hello-node-editor`'s canvas, which zooms.
Every widget in the catalog declined, so a wheel over a slider, a spin box
or a number field did what a wheel over blank wallpaper does: it scrolled
whatever was behind it.

the toolkit implements `wheelEvent` on precisely these widgets
(`wheelEvent`, `wheelEvent`), which is
why every volume slider, zoom slider and DCC parameter field on a desktop
answers a wheel without being clicked first. R1533 gives the two stepped
value widgets that hook, under one rule: **one notch is one step**.

## What is shared, and why it is shared

Both widgets bank sub-notch motion in a `WheelStepper` (`pinion-core`,
beside `WheelDelta` and `LINE_HEIGHT_PX`). A continuous consumer — the node
canvas' zoom exponent — can divide a pixel delta and be done; a *stepped*
one cannot, because a trackpad reporting a fraction of a notch per event
would round to zero forever and the widget would never move. The router
already keeps exactly this carry for its integer scroll offsets, and having
it on only one of two paths was a shipped bug once (R881.1). So it has one
home, and both widgets read it.

That type also states the three-way consume verdict both widgets follow:

  * banked (under one notch) — consume, so the page behind cannot jitter
    between the notches of a slow trackpad;
  * stepped — consume;
  * saturated (pinned at a bound) — **decline**, so the wheel this widget
    cannot spend reaches the scroll container behind it.

The third is a deliberate divergence from the toolkit's spin box, which always
accepts and therefore eats the page scroll of any form containing a pinned
field.

## Verification scope (>= 30 assertions, sections A-H)

  (A) Premise — the two widgets are the shapes this demo assumes.
  (B) Direction — a forward wheel RAISES the value. Half of a sign error is
      invisible in a test that only checks "it moved".
  (C) Whole notches in one event all step.
  (D) Sub-notch motion BANKS: three quarter-notches move nothing, the
      fourth steps once. This is the accumulator, over the wire.
  (E) Saturation declines, and the value stays exactly at the bound.
  (F) A wheel is not a press — the interaction statechart is untouched, and
      the AT-visible value tracks the wheel (a screen reader reports what a
      sighted user sees).
  (G) The spin button, in domain units rather than normalised ones.
  (H) NEGATIVE CONTROL — `hello-range-slider` implements no wheel hook and
      must be unmoved. This is what a change that gave every widget the
      behaviour (a shared base class, a router-level default) fails.
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

VALUE = "/external/value"

#: `pinion_core::event::LINE_HEIGHT_PX` — one notch, in the pixel units the
#: router hands `External::wheel`. Mirrored rather than imported: a demo that
#: read the constant from the code under test could not catch the code
#: changing it.
NOTCH_PX = 16.0
#: `pinion_core::widgets::slider::CONTINUOUS_WHEEL_STEP`.
STEP = 0.05
#: `hello-slider-discrete`'s snap increment.
DISCRETE_STEP = 0.2

TOL = 1e-5


def close(got: float, want: float, what: str) -> None:
    assert abs(got - want) < TOL, f"{what}: expected {want}, got {got}"


def value(tf: RpcSubprocess) -> float:
    raw = tf.query(VALUE)
    assert isinstance(raw, (int, float)), f"non-numeric value {raw!r}"
    return float(raw)


def wheel(tf: RpcSubprocess, tag: str, *, notches: float) -> None:
    """One wheel event of `notches` notches UP (forward, W3C `dy < 0`)."""
    tf.wheel(path=tag, pixels=(0.0, -notches * NOTCH_PX))
    tf.tick(0.016)


def walk(node, out: list) -> None:
    if isinstance(node, dict):
        out.append(node)
        for v in node.values():
            walk(v, out)
    elif isinstance(node, list):
        for v in node:
            walk(v, out)


def access_node(tf: RpcSubprocess, tag: str) -> dict:
    flat: list = []
    walk(tf.request("scene/access").result, flat)
    for n in flat:
        if n.get("tag") == tag:
            return n
    raise AssertionError(f"no AT node tagged {tag!r}")


def continuous_slider() -> None:
    with RpcSubprocess("hello-slider", boot_grace=1.5) as tf:
        for _ in range(3):
            tf.tick(0.016)

        # ── (A) premise ──────────────────────────────────────────────
        assert_eq(
            tf.query("/external/step"),
            0.0,
            "premise: hello-slider is CONTINUOUS, so the notch step is the "
            "framework's constant and not the widget's own",
        )
        close(value(tf), 0.0, "premise: boots at zero")

        # ── (B) direction ────────────────────────────────────────────
        wheel(tf, "main_slider", notches=1)
        close(value(tf), STEP, "one notch forward RAISES by one step")
        wheel(tf, "main_slider", notches=-1)
        close(value(tf), 0.0, "and one notch back lowers by one")
        wheel(tf, "main_slider", notches=1)
        close(value(tf), STEP, "back up, to leave room below")

        # ── (C) several notches in one event ─────────────────────────
        wheel(tf, "main_slider", notches=3)
        close(value(tf), STEP * 4, "three notches in one event = three steps")

        # ── (D) sub-notch motion banks ───────────────────────────────
        base = value(tf)
        for i in range(1, 4):
            wheel(tf, "main_slider", notches=0.25)
            close(
                value(tf),
                base,
                f"a quarter notch ({i} of 4) moves nothing yet — and is not "
                f"discarded either, which is the next assertion",
            )
        wheel(tf, "main_slider", notches=0.25)
        close(
            value(tf),
            base + STEP,
            "the fourth quarter completes the notch: a trackpad that reports "
            "fractions moves the slider, which is the whole reason the carry "
            "exists",
        )

        # ── (E) saturation ───────────────────────────────────────────
        tf.intervene(VALUE, 1.0)
        close(value(tf), 1.0, "premise: pinned at the maximum")
        wheel(tf, "main_slider", notches=1)
        close(value(tf), 1.0, "a wheel that would raise a pinned slider is declined")
        wheel(tf, "main_slider", notches=4)
        close(value(tf), 1.0, "and stays declined however hard it is pushed")
        wheel(tf, "main_slider", notches=-1)
        close(value(tf), 1.0 - STEP, "the direction it CAN move still answers")

        tf.intervene(VALUE, 0.0)
        wheel(tf, "main_slider", notches=-1)
        close(value(tf), 0.0, "the same at the minimum")

        # ── (F) a wheel is not a press ───────────────────────────────
        tf.intervene(VALUE, 0.5)
        wheel(tf, "main_slider", notches=1)
        state = tf.query("/external/state")
        assert_eq(
            state,
            "Hover",
            "the pointer is over the track so the statechart is Hover — a "
            "wheel that drove the SCXML would read Dragging here, paint a "
            "phantom drag, and fire a second commit on the next PointerUp",
        )
        at = access_node(tf, "main_slider")
        assert_eq(at.get("role"), "slider", "still a slider to the AT")
        at_value = at["value"]["float"]
        close(
            float(at_value["value"]),
            0.5 + STEP,
            "the AT-visible value tracks the wheel, so a screen reader reads "
            "the number a sighted user sees",
        )
        close(float(at_value["min"]), 0.0, "with its range intact")
        close(float(at_value["max"]), 1.0, "with its range intact")


def discrete_slider() -> None:
    """The step is the WIDGET's, not the wheel's — a discrete slider walks
    its own stops instead of landing 5% away from one."""
    with RpcSubprocess("hello-slider-discrete", boot_grace=1.5) as tf:
        for _ in range(3):
            tf.tick(0.016)
        close(
            float(tf.query("/external/step")),
            DISCRETE_STEP,
            "premise: this slider declares a snap increment",
        )
        start = value(tf)
        wheel(tf, "disc_slider", notches=1)
        close(
            value(tf),
            start + DISCRETE_STEP,
            "one notch = one STOP, not one framework step",
        )
        wheel(tf, "disc_slider", notches=1)
        close(value(tf), start + 2 * DISCRETE_STEP, "and the next stop")
        # Four quarter-notches must land on a stop, not a quarter past one.
        landed = value(tf)
        for _ in range(4):
            wheel(tf, "disc_slider", notches=0.25)
        close(
            value(tf),
            landed + DISCRETE_STEP,
            "banked fractions still resolve to exactly one stop — a discrete "
            "slider cannot be left between its ticks by a trackpad",
        )


def spin_button() -> None:
    with RpcSubprocess("hello-spinbutton", boot_grace=1.5) as tf:
        for _ in range(3):
            tf.tick(0.016)

        # ── (G) domain units ─────────────────────────────────────────
        step = float(tf.query("/external/step"))
        vmin = float(tf.query("/external/min"))
        vmax = float(tf.query("/external/max"))
        assert step > 0.0, f"premise: a positive step, got {step}"
        assert vmax > vmin, f"premise: a real range, got [{vmin}, {vmax}]"
        start = value(tf)
        assert vmin < start < vmax, (
            f"premise: it boots off both bounds so both directions are "
            f"testable, got {start} in [{vmin}, {vmax}]"
        )

        wheel(tf, "spin", notches=1)
        close(value(tf), start + step, "one notch = one step, in domain units")
        wheel(tf, "spin", notches=-1)
        close(value(tf), start, "and back")
        wheel(tf, "spin", notches=2)
        close(value(tf), start + 2 * step, "two notches = two steps")

        base = value(tf)
        wheel(tf, "spin", notches=0.5)
        close(value(tf), base, "half a notch banks here too")
        wheel(tf, "spin", notches=0.5)
        close(value(tf), base + step, "two halves make the step")

        tf.intervene(VALUE, vmax)
        wheel(tf, "spin", notches=1)
        close(value(tf), vmax, "a pinned field declines the wheel it cannot use")
        tf.intervene(VALUE, vmin)
        wheel(tf, "spin", notches=-1)
        close(value(tf), vmin, "at the minimum too")
        wheel(tf, "spin", notches=1)
        close(value(tf), vmin + step, "and it is not stuck — the way out works")

        # The steppers are Buttons with their own statecharts. A wheel over
        # the field is a press on neither.
        assert_eq(
            tf.query("/external/dec_state"),
            "Idle",
            "the decrement arrow was not pressed by a wheel",
        )
        assert_eq(
            tf.query("/external/inc_state"),
            "Idle",
            "nor the increment arrow — a phantom pressed arrow is what a "
            "wheel routed through the stepper channel would paint",
        )


def range_slider_is_untouched() -> None:
    """(H) NEGATIVE CONTROL.

    `RangeSliderExternal` shares the slider's SCXML and its normalised value
    domain, and deliberately gets no wheel hook this round (the toolkit has no
    two-thumb slider, so there is no `wheelEvent` to mirror and no answer to
    "which thumb does a notch move"). It is the control that fails if the
    wheel were wired somewhere shared instead of per widget.
    """
    with RpcSubprocess("hello-range-slider", boot_grace=1.5) as tf:
        for _ in range(3):
            tf.tick(0.016)
        low = float(tf.query("/external/low"))
        high = float(tf.query("/external/high"))
        assert low < high, f"premise: two thumbs, got {low} / {high}"
        for notches in (1, -1, 3, 0.25):
            tf.wheel(path="range", pixels=(0.0, -notches * NOTCH_PX))
            tf.tick(0.016)
        close(float(tf.query("/external/low")), low, "the low thumb did not move")
        close(float(tf.query("/external/high")), high, "nor the high thumb")
        assert_eq(
            tf.query("/external/state"),
            "Hover",
            "and the wheel left its statechart alone as well",
        )


def body() -> None:
    continuous_slider()
    discrete_slider()
    spin_button()
    range_slider_is_untouched()


if __name__ == "__main__":
    run_demo("R1533 §5.45 — a value widget answers the wheel", body)
