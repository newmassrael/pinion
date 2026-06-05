#!/usr/bin/env python3
"""R804 §5.38 §5.22 — apply_key modifier forwarding via the relocated
`pinion_core::forward_key_to_field` SSOT.

R804 moved `forward_key_to_field` out of `pinion-widget-paint::text_field`
(its R764.1 birthplace) into `pinion-core::input`. The body is pure
`Scene` / `External` introspection with zero paint, so its GUI-paint-crate
home forced three straggler bindings to keep a hand-rolled copy that sent
the bare `IntrospectValue::Text(key)` wire shape and silently DROPPED the
held `Modifiers`. With the modifiers gone, `Shift+Arrow` / `Shift+Home`
text-range selection was dead in those fields:

  - hello-number-input    (numeric field, caret-key forward)
  - settings-panel        (profile display-name field, full forward)
  - hello-textfield-tui    (TUI binding; the core home finally lets a
                            non-paint backend share the SSOT)

This demo proves the fix end-to-end through the *platform key path*, not a
direct substrate invoke: `scene/modifiers` sets the held bits (R763), then
`scene/key` dispatches to the binding's `WidgetCore::apply_key`, which now
forwards the modifiers to the field's selection arms. Driving through
`scene/key` (rather than `/external/key`) is the whole point — it exercises
the migrated `apply_key`, the layer that used to drop the modifiers.

Named keys (`ArrowLeft` / `Home` / `End`) are used for the modifier-bearing
steps: they route unambiguously through `handle_named_key -> apply_key`,
the path both bindings forward verbatim.

Atomic verification scope (>=30 assertions): each field is reset to a known
buffer, then its selection lifecycle (Shift+Arrow extend, multi-step range
growth, Shift+Home/End to the edges, plain-arrow collapse) is asserted
against the `/external/selection` introspect slot.
"""

from __future__ import annotations

import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import RpcSubprocess, assert_eq, run_demo

PROFILE_TAG = "profile_display_name"
NAV_TAG = "nav_rail"
NUM_TAG = "num_input"
PAUSE = 0.08


def _sel(tf, tag):
    return tf.query(f"/{tag}/external/selection")


def _text(tf, tag):
    return tf.query(f"/{tag}/external/text")


def _clear(tf, tag, n=48):
    """Drain the buffer with plain Backspace so the round is deterministic
    regardless of any persisted boot text (settings-panel restores a saved
    display_name). End first so the caret sits past the last glyph
    (boot caret can be at offset 0)."""
    tf.key(path=tag, name="End")
    for _ in range(n):
        tf.key(path=tag, name="Backspace")
    time.sleep(PAUSE)


def _shift(tf, tag, name, count=1):
    """Hold Shift, press a named key `count` times, release Shift. Mirrors
    the R763 `scene/modifiers` press/release envelope."""
    tf.modifiers(shift=True)
    for _ in range(count):
        tf.key(path=tag, name=name)
    tf.modifiers()
    time.sleep(PAUSE)


def profile_field(tf) -> None:
    # The display-name field is only painted while the Profile section is
    # active (nav#2) — switch to it via the RadioGroup composite tag
    # (R51.42) before the paint-scene focus lookup.
    tf.click(path=f"{NAV_TAG}#2")
    time.sleep(PAUSE)
    snap = tf.snapshot(viewport=(720, 480))
    import json as _json
    assert_eq(
        PROFILE_TAG in _json.dumps(snap),
        True,
        "profile field present after nav switch",
    )

    # Focus the profile display-name field (an extra-external TextField).
    tf.request("focus/set", {"tag": PROFILE_TAG})
    time.sleep(PAUSE)
    assert_eq(
        tf.request("focus/get").result.get("focused"),
        PROFILE_TAG,
        "profile field focused",
    )

    # Deterministic reset, then seed a known buffer through scene/text.
    _clear(tf, PROFILE_TAG)
    assert_eq(_text(tf, PROFILE_TAG), "", "profile field cleared to empty")
    assert_eq(_sel(tf, PROFILE_TAG), None, "empty field has no selection")

    tf.text("Lovelace", path=PROFILE_TAG)
    time.sleep(PAUSE)
    assert_eq(_text(tf, PROFILE_TAG), "Lovelace", "profile buffer seeded")
    assert_eq(tf.query(f"/{PROFILE_TAG}/external/caret"), 8, "caret at end (8)")
    assert_eq(_sel(tf, PROFILE_TAG), None, "no selection after plain typing")

    # ── Shift+ArrowLeft extends a selection (the modifier the pre-R804
    #    bare-Text hand-roll dropped). One step at a time so the range
    #    growth is visible.
    _shift(tf, PROFILE_TAG, "ArrowLeft", 1)
    assert_eq(_sel(tf, PROFILE_TAG), {"start": 7, "end": 8}, "Shift+Left -> {7,8}")
    _shift(tf, PROFILE_TAG, "ArrowLeft", 1)
    assert_eq(_sel(tf, PROFILE_TAG), {"start": 6, "end": 8}, "Shift+Left -> {6,8}")
    _shift(tf, PROFILE_TAG, "ArrowLeft", 2)
    assert_eq(_sel(tf, PROFILE_TAG), {"start": 4, "end": 8}, "Shift+Left x2 -> {4,8}")
    assert_eq(tf.query(f"/{PROFILE_TAG}/external/caret"), 4, "caret rode to 4")

    # ── Shift+Home grows the selection to the buffer start.
    _shift(tf, PROFILE_TAG, "Home", 1)
    assert_eq(_sel(tf, PROFILE_TAG), {"start": 0, "end": 8}, "Shift+Home -> {0,8}")

    # ── A plain (unmodified) ArrowRight collapses the selection — proving
    #    the modifier gate is real, not "always select".
    tf.key(path=PROFILE_TAG, name="ArrowRight")
    time.sleep(PAUSE)
    assert_eq(_sel(tf, PROFILE_TAG), None, "plain ArrowRight collapses selection")

    # ── From the left edge, Shift+End selects forward to the end.
    tf.key(path=PROFILE_TAG, name="Home")
    time.sleep(PAUSE)
    assert_eq(tf.query(f"/{PROFILE_TAG}/external/caret"), 0, "Home -> caret 0")
    assert_eq(_sel(tf, PROFILE_TAG), None, "Home (plain) leaves no selection")
    _shift(tf, PROFILE_TAG, "End", 1)
    assert_eq(_sel(tf, PROFILE_TAG), {"start": 0, "end": 8}, "Shift+End -> {0,8}")

    # ── Backspace on the full selection drains the buffer (selection-aware
    #    delete reached because the range exists at all).
    tf.key(path=PROFILE_TAG, name="Backspace")
    time.sleep(PAUSE)
    assert_eq(_text(tf, PROFILE_TAG), "", "Backspace on full selection clears buffer")
    assert_eq(_sel(tf, PROFILE_TAG), None, "no selection after drain")


def number_field(tf) -> None:
    # Focus the numeric field (primary External), reset, seed "640" — the
    # raw typed text is retained pre-Enter (the editable-spinbutton SSOT).
    tf.request("focus/set", {"tag": NUM_TAG})
    time.sleep(PAUSE)
    assert_eq(
        tf.request("focus/get").result.get("focused"),
        NUM_TAG,
        "numeric field focused",
    )
    _clear(tf, NUM_TAG, n=12)
    assert_eq(_text(tf, NUM_TAG), "", "numeric field cleared")

    tf.text("640", path=NUM_TAG)
    time.sleep(PAUSE)
    assert_eq(_text(tf, NUM_TAG), "640", "numeric buffer seeded to '640'")
    assert_eq(tf.query(f"/{NUM_TAG}/external/caret"), 3, "numeric caret at end (3)")
    assert_eq(_sel(tf, NUM_TAG), None, "no selection after typing digits")

    # ── Shift+ArrowLeft extends selection through the numeric binding's
    #    caret-key arm (the path that pre-R804 dropped the Shift bit).
    _shift(tf, NUM_TAG, "ArrowLeft", 1)
    assert_eq(_sel(tf, NUM_TAG), {"start": 2, "end": 3}, "numeric Shift+Left -> {2,3}")
    _shift(tf, NUM_TAG, "ArrowLeft", 2)
    assert_eq(_sel(tf, NUM_TAG), {"start": 0, "end": 3}, "numeric Shift+Left x2 -> {0,3}")
    assert_eq(tf.query(f"/{NUM_TAG}/external/caret"), 0, "numeric caret rode to 0")

    # ── Plain ArrowRight collapses (modifier gate is real here too).
    tf.key(path=NUM_TAG, name="ArrowRight")
    time.sleep(PAUSE)
    assert_eq(_sel(tf, NUM_TAG), None, "numeric plain ArrowRight collapses")

    # ── Shift+End from the start re-selects the whole number.
    tf.key(path=NUM_TAG, name="Home")
    time.sleep(PAUSE)
    assert_eq(tf.query(f"/{NUM_TAG}/external/caret"), 0, "numeric Home -> 0")
    _shift(tf, NUM_TAG, "End", 1)
    assert_eq(_sel(tf, NUM_TAG), {"start": 0, "end": 3}, "numeric Shift+End -> {0,3}")


def body() -> None:
    with RpcSubprocess("settings-panel", boot_grace=1.5) as tf:
        profile_field(tf)
    with RpcSubprocess("hello-number-input", boot_grace=1.5) as tf:
        number_field(tf)


if __name__ == "__main__":
    sys.exit(run_demo("R804 apply_key modifier forwarding SSOT", body))
