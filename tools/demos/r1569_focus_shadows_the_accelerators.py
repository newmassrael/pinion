#!/usr/bin/env python3
"""R1569 §5.39 §5.20 §5.12 — the focused widget shadows the accelerator layers.

A window's ACCELERATORS fire from anywhere in it regardless of focus — that is
what an accelerator is. pinion has two such layers: the §5.20 mnemonic map
(R1543, Alt+char derived from painted labels) and the binding's
`WidgetCore::keybinding` character map. Both ran ahead of the focused widget,
and before this round there was no way for a widget to say otherwise.

the toolkit has the hatch: `ShortcutOverride` is offered to the focus widget
before shortcut processing, and line edit accepts it for any unmodified
printable key — "if it would be text, it is mine". pinion had NOTHING, and the
consequence was shipped, not theoretical: in `hello-textfield`, which binds
`d` -> Disable, typing `d` into the focused field DISABLED THE FIELD and the
character never arrived. Four bindings in the tree carried it.

R1569 makes the hatch a QUESTION the router asks (`External::shadows_accelerator`)
rather than an event a widget must remember to accept, and this demo drives its
extreme consumer: `KeySequenceEdit` (the toolkit key-sequence editor), the widget whose
entire job is to record a chord that already means something.

The binding is built to be hostile to its own editor: a `&File` / `&Save`
menubar makes Alt+F and Alt+S mnemonics, and the `keybinding` map claims the
bare characters `r` / `c` / `d` / `e`.

What this asserts, over the wire:

  * IDLE, both accelerator layers work — `r` starts a recording and Alt+F
    activates the File title — because an idle editor claims nothing. This is
    the negative control without which every later assertion could pass by the
    accelerators simply being broken;
  * RECORDING, the same two keystrokes are RECORDED instead, and
    `scene/accelerators` names this widget as the one taking them. The toolkit's
    override is anonymous — `accept()` leaves no record of who accepted;
  * `scene/accelerators` EXISTS at all: the toolkit keeps its shortcut state in the
    private shortcut map, so a toolkit application can enumerate its own
    accelerators, let alone say which are currently overridden;
  * a bare modifier is a PUBLISHED PREFIX, where the toolkit's `keyPressEvent` returns
    early on modifier keys and the fact is lost;
  * the sequence FILLS and commits itself, and the intent it emits is the one
    an explicit commit emits — one transition, not two paths;
  * CANCEL abandons the run and keeps the previously accepted one, an exit
    key-sequence editor does not have (its release timer commits whatever
    arrived);
  * `scene/accelerators {"chord": ...}` answers what a chord would COLLIDE with
    before it is recorded — the question that makes a keymap editor usable, and
    the one the toolkit answers only later, at dispatch, via
    `isAmbiguous()`;
  * a malformed chord spelling is a NAMED refusal, where
    `fromString` substitutes `Key_unknown` and reports
    nothing;
  * a DISABLED editor claims nothing — a widget that will not act on the key
    must not stop the accelerator that would have (the toolkit gates on `isReadOnly()`
    for the same reason).

ZERO-FLAKE: bounded `wait_until` polling (never a fixed sleep). >=30 assertions.

Run from the workspace root:
    cargo build -p hello-key-sequence --release
    python3 tools/demos/r1569_focus_shadows_the_accelerators.py
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    access_node_by_tag,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_until,
)

VIEWPORT = (520, 300)
KS = "key_sequence"

# The exact published shape of one accelerator row and of a chord verdict. A
# response an agent is told to rely on must not gain or lose a key unnoticed
# (the R1538 `FrameTimingsMirror` lesson).
ROW_KEYS = {"accel", "layer", "target", "label", "shadowed", "shadowed_by"}
VERDICT_KEYS = {"accel", "claimed_by", "shadowed", "shadowed_by"}


def accelerators(tf, chord: Optional[str] = None) -> dict[str, Any]:
    params = {"chord": chord} if chord is not None else {}
    return tf.request("scene/accelerators", params).result


def row_for(tf, accel: str) -> Optional[dict[str, Any]]:
    for row in accelerators(tf)["accelerators"]:
        if row["accel"] == accel:
            return row
    return None


def ks(tf, path: str) -> Any:
    """Read one of the editor's published slots."""
    node = find_by_tag(tf.snapshot(), KS)
    assert node is not None, f"the editor External is in the state scene"
    return node["introspect"][path]


def press(tf, char: str, *, alt: bool = False) -> None:
    """Type `char`, with or without Alt held.

    The modifier is set out of band exactly as winit delivers it
    (`ModifiersChanged` is a separate event from the key), the discipline every
    modifier-bearing shortcut in this repo follows.
    """
    tf.modifiers(alt=alt)
    tf.key(path=KS, name=char)
    tf.modifiers()


def body() -> None:
    with RpcSubprocess("hello-key-sequence", boot_grace=1.5) as tf:
        # ── (A) the window really is hostile: both layers are live ─────────
        # Asserted FIRST because every "the editor took it instead" claim below
        # is vacuous if the accelerator was never there to be taken.
        wait_until(
            lambda: len(accelerators(tf)["accelerators"]) >= 6,
            timeout=4.0,
            interval=0.03,
            desc="the menubar's mnemonics and the binding's keybindings publish",
        )
        pub = accelerators(tf)
        assert_eq(set(pub["accelerators"][0].keys()), ROW_KEYS, "a row's exact key set")
        assert_eq(pub["probed"], "U+0020..=U+007E", "the probe states its domain")
        by_layer: dict[str, list[str]] = {}
        for row in pub["accelerators"]:
            by_layer.setdefault(row["layer"], []).append(row["accel"])
        assert_eq(sorted(by_layer["mnemonic"]), ["Alt+F", "Alt+S"], "two mnemonics")
        assert_eq(sorted(by_layer["keybinding"]), ["c", "d", "e", "r"], "four keybindings")
        assert_eq(
            row_for(tf, "Alt+F")["target"],
            "menu#file",
            "a mnemonic names the painted tag it activates",
        )
        assert_eq(row_for(tf, "Alt+F")["label"], "File", "the `&` is not in the label")
        assert_eq(
            row_for(tf, "r")["target"],
            "",
            "a keybinding maps to a typed event, so it names no node — and that "
            "absence IS the defect R1543 recorded about hand-rolled char maps",
        )

        # ── (B) idle, nothing is shadowed ──────────────────────────────────
        assert_eq(pub["focused"], None, "nothing has focus yet")
        assert_eq(pub["shadowing"], None, "so nothing can be shadowing")
        assert_eq(
            [r["shadowed"] for r in pub["accelerators"]],
            [False] * len(pub["accelerators"]),
            "every accelerator is live",
        )

        # ── (C) idle, the accelerators WORK — the negative control ─────────
        assert_eq(ks(tf, "state"), "Idle", "the editor starts idle")
        press(tf, "r")  # keybinding("r") => Record
        wait_until(
            lambda: ks(tf, "state") == "Recording",
            timeout=4.0,
            interval=0.03,
            desc="a bare `r` reached the binding's keybinding map",
        )
        assert_eq(
            ks(tf, "in_flight"),
            "",
            "and it was NOT recorded as a chord — an idle editor claims nothing",
        )

        # ── (D) recording, the editor is named as the one shadowing ────────
        tf.click(path=KS)
        wait_until(
            lambda: accelerators(tf)["focused"] == KS,
            timeout=4.0,
            interval=0.03,
            desc="the editor takes focus",
        )
        rec = accelerators(tf)
        assert_eq(rec["shadowing"], KS, "the shadow is ATTRIBUTED, not anonymous")
        assert_eq(
            [r["shadowed"] for r in rec["accelerators"]],
            [True] * len(rec["accelerators"]),
            "a recording editor claims EVERY chord — Alt+F included",
        )
        assert_eq(
            row_for(tf, "Alt+F")["shadowed_by"],
            KS,
            "including the mnemonic layer, which no widget declares",
        )

        # ── (E) the same two keystrokes are now RECORDED ───────────────────
        press(tf, "r")
        wait_until(
            lambda: ks(tf, "in_flight") == "r",
            timeout=4.0,
            interval=0.03,
            desc="`r` is a chord now, not the Record accelerator",
        )
        assert_eq(ks(tf, "state"), "Recording", "and it did not re-enter recording")

        # ── (F) a bare modifier is a published prefix, not a drop ──────────
        tf.modifiers(ctrl=True)
        tf.key(path=KS, name="Control")
        wait_until(
            lambda: ks(tf, "pending") == "Ctrl+",
            timeout=4.0,
            interval=0.03,
            desc="the held prefix is published — Qt discards this fact entirely",
        )
        assert_eq(ks(tf, "in_flight"), "r", "a prefix is not a chord")
        painted = find_by_tag(tf.snapshot(source="paint", viewport=VIEWPORT), "key_sequence#value")
        assert painted is not None, "the field paints its value"
        assert "Ctrl+" in str(painted), f"and the prefix is on screen: {painted}"

        # ── (G) filling the sequence commits it (max_len = 2 here) ─────────
        tf.key(path=KS, name="s")
        tf.modifiers()
        wait_until(
            lambda: ks(tf, "state") == "Idle",
            timeout=4.0,
            interval=0.03,
            desc="the second chord fills the sequence and commits",
        )
        assert_eq(ks(tf, "sequence"), "r, Ctrl+s", "both chords, in order")
        # NOT asserted here: that the fill-commit raised its §5.20 intent.
        # `scene/intents` reads empty at this cadence for a SHIPPED control too
        # (`hello-disclosure` after a click), so the shell has already drained
        # the queue by the time an RPC read arrives — an assertion on it would
        # pass whether or not the intent was ever emitted. The contract is held
        # instead by `key_sequence.rs`'s
        # `an_explicit_commit_and_a_fill_commit_emit_the_same_intent`, which
        # drains the emitter directly and does discriminate.
        assert_eq(ks(tf, "in_flight"), "", "the run is spent")
        assert_eq(ks(tf, "pending"), "", "and the prefix cleared with it")

        # ── (H) committed, the accelerators come BACK while focus stays ────
        # the toolkit cannot do this: key-sequence editor grabs until focus
        # leaves.
        back = accelerators(tf)
        assert_eq(back["focused"], KS, "the editor still has focus")
        assert_eq(back["shadowing"], None, "and has stopped claiming anything")
        press(tf, "f", alt=True)
        assert_eq(
            ks(tf, "sequence"),
            "r, Ctrl+s",
            "Alt+F is a mnemonic again — it did not overwrite the binding",
        )

        # ── (I) cancel abandons the run and KEEPS the accepted one ─────────
        press(tf, "r")
        wait_until(
            lambda: ks(tf, "state") == "Recording",
            timeout=4.0,
            interval=0.03,
            desc="recording again",
        )
        press(tf, "z")
        assert_eq(ks(tf, "in_flight"), "z", "a chord is in flight")
        tf.key(path=KS, name="Escape")
        wait_until(
            lambda: ks(tf, "state") == "Idle",
            timeout=4.0,
            interval=0.03,
            desc="Escape cancels — the exit Qt's release timer does not have",
        )
        assert_eq(
            ks(tf, "sequence"),
            "r, Ctrl+s",
            "the abandoned run did not overwrite the accepted one",
        )
        assert_eq(ks(tf, "in_flight"), "", "and left nothing behind")

        # ── (J) a chord can be ASKED about before it is pressed ────────────
        asked = accelerators(tf, chord="Alt+F")
        assert_eq(set(asked["chord"].keys()), VERDICT_KEYS, "a verdict's key set")
        assert_eq(asked["chord"]["accel"], "Alt+F", "re-spelled canonically")
        assert_eq(asked["chord"]["claimed_by"], "mnemonic", "the File title claims it")
        assert_eq(asked["chord"]["shadowed"], False, "nothing is recording")
        free = accelerators(tf, chord="Ctrl+Shift+P")
        assert_eq(free["chord"]["claimed_by"], None, "an unclaimed chord is free")
        # The `keybinding` map is MODIFIER-BLIND — the shell consults it with the
        # character alone — so a command chord over a mapped character collides.
        # Reporting otherwise would describe a dispatch order that does not exist.
        blind = accelerators(tf, chord="Ctrl+r")
        assert_eq(
            blind["chord"]["claimed_by"],
            "keybinding",
            "Ctrl+r collides because the keybinding layer never sees the modifier",
        )
        assert_eq(
            accelerators(tf)["accelerators"][0]["accel"],
            pub["accelerators"][0]["accel"],
            "asking about a chord changed nothing",
        )

        # ── (K) a malformed spelling is a NAMED refusal ────────────────────
        try:
            accelerators(tf, chord="Ctrl+Frobnicate+P")
            raise AssertionError("a malformed chord must be refused, not guessed")
        except AssertionError:
            raise
        except Exception as exc:  # the harness raises RpcError
            assert "Frobnicate" in str(exc), f"the refusal names the part: {exc}"

        # ── (L) the AT is told the accepted spelling, from the same source ─
        access = tf.request("scene/access").result
        node = access_node_by_tag(access, KS)
        assert node is not None, "the editor has an access node"
        assert_eq(
            node["value"],
            {"text": "r, Ctrl+s"},
            "the announced value IS the accepted spelling the wire publishes",
        )

        # ── (M) a disabled editor claims nothing ───────────────────────────
        # the toolkit gates `ShortcutOverride` on `isReadOnly()` for the same reason: a widget that will
        # not act on the key must not stop the accelerator that would have.
        press(tf, "r")
        wait_until(
            lambda: ks(tf, "state") == "Recording",
            timeout=4.0,
            interval=0.03,
            desc="recording once more",
        )
        assert_eq(accelerators(tf)["shadowing"], KS, "claiming again")
        tf.invoke("/external/send", "Disable")
        wait_until(
            lambda: ks(tf, "state") == "Disabled",
            timeout=4.0,
            interval=0.03,
            desc="the editor is disabled mid-recording",
        )
        assert_eq(
            accelerators(tf)["shadowing"],
            None,
            "and the window's accelerators came back with it",
        )
        press(tf, "e")  # keybinding("e") => Enable
        wait_until(
            lambda: ks(tf, "state") == "Idle",
            timeout=4.0,
            interval=0.03,
            desc="a bare `e` reaches the accelerator layer again",
        )


    # ── (N) the SHIPPED defect, in the binding that shipped it ─────────────
    # The round's load-bearing assertion, and the one that has nothing to do
    # with the new widget: `hello-textfield` binds `d` -> Disable / `e` -> Enable through
    # `keybinding`, and before R1569 typing `d` into the FOCUSED field disabled the
    # field — the character never arrived. The toolkit does not have that
    # defect (line edit accepts `ShortcutOverride` for any unmodified printable key), so this
    # is the tree reaching the toolkit's floor, not passing it.
    with RpcSubprocess("hello-textfield", boot_grace=1.5) as tf:
        field = "main_textfield"

        def read() -> tuple[str, str]:
            i = find_by_tag(tf.snapshot(), field)["introspect"]
            return i["text"], i["state"]

        # Negative control FIRST: unfocused, the accelerator must still fire.
        # Without it, "the letter arrived" would also pass if the keybinding
        # layer had simply been broken.
        assert_eq(read(), ("", "Idle"), "the field starts empty and idle")
        tf.key(path=field, name="d")
        wait_until(
            lambda: read()[1] == "Disabled",
            timeout=4.0,
            interval=0.03,
            desc="unfocused, a bare `d` is still the window's accelerator",
        )
        tf.key(path=field, name="e")
        wait_until(
            lambda: read()[1] == "Idle",
            timeout=4.0,
            interval=0.03,
            desc="and `e` still re-enables it",
        )
        assert_eq(read()[0], "", "neither keystroke was ever text")

        # Focused, the same characters are TEXT.
        tf.click(path=field)
        wait_until(
            lambda: read()[1] == "Focused",
            timeout=4.0,
            interval=0.03,
            desc="the field takes focus",
        )
        for ch in "abcde":
            tf.key(path=field, name=ch)
        wait_until(
            lambda: read()[0] == "abcde",
            timeout=4.0,
            interval=0.03,
            desc="`d` and `e` reach the field as characters, not as commands",
        )
        assert_eq(read()[1], "Focused", "and the field was never disabled")

        # Alt+char stays an ACCELERATOR even while typing — the toolkit's
        # behaviour, and the reason the declaration is per CHORD rather than a
        # bool on the widget. A field that claimed Alt+d would swallow every
        # mnemonic in the window.
        tf.modifiers(alt=True)
        tf.key(path=field, name="d")
        tf.modifiers()
        wait_until(
            lambda: read()[1] == "Disabled",
            timeout=4.0,
            interval=0.03,
            desc="Alt+d is not text, so the accelerator layer keeps it",
        )
        assert_eq(read()[0], "abcde", "and no `d` was inserted")

        # A disabled field claims nothing, so `e` gets through to re-enable it
        # — the recovery path that would be unreachable if a disabled widget
        # could still shadow.
        tf.key(path=field, name="e")
        wait_until(
            lambda: read()[1] == "Idle",
            timeout=4.0,
            interval=0.03,
            desc="a disabled field claims no chord (Qt gates on isReadOnly)",
        )


if __name__ == "__main__":
    sys.exit(run_demo("R1569 §5.39 the focus shadows the accelerators", body))
