#!/usr/bin/env python3
"""hello-textfield typing dogfood (§5.49 R59, R56.1.b.1).

First visible-consumer end-to-end demo for the R56 TextField axis.
Drives the live window over JSON-RPC 2.0 through the full focus +
keystroke + caret-position pipeline:

  focus/set {tag: "main_textfield"}
    → ShellSubstrate::notify_focus_change (R56.1.h)
    → TextFieldExternal::on_focus_change(true)
    → TextField.send(Focus) → SCXML `Idle -> Focused`
    → CaretBlink::set_enabled(true) (R56.1.h sync_blink)
    → query "/external/state" -> "Focused"

  scene/invoke "/external/key" Text("h")
    → TextFieldExternal::invoke("key", Text("h")) (R56.1.d)
    → apply_key dispatches into TextEditState::insert("h") (R56.1.d)
    → query "/external/text" -> "h", "/external/caret" -> 1

The demo exercises every R56 substrate primitive end-to-end:
- §5.38 SCXML statechart (R56.1.a) — Focus / Blur drive
- §5.22 TextEditState (R56.1.b) — text + caret content sidecar
- §5.28 CaretBlink (R56.1.c) — enable gate sync via R56.1.h
- §5.22 apply_key + invoke "key" RPC (R56.1.d) — keystroke surface
- §5.39 focus lifecycle wire (R56.1.h) — shell ↔ External plumbing
- §5.36 caret_rect_for_byte_offset (R56.1.b.2) — paint-side caret
  geometry (verified visually; this demo does not poke the paint
  scene because the substrate fix at substrate.rs `collect_access_
  emit_inputs` is the path that proves it)

R56.1.b.1 substrate cascade:
- AriaRole::TextInput added (pinion-a11y)
- core_shell::CoreShell::new wraps V::create_external in
  root_owner.run so the External's `attach_state` / `attach_blink`
  hooks resolve via the same Owner the view fn reaches later
- substrate.rs collect_access_emit_inputs wraps V::access_node in
  root_owner.run for the same reason
- Owner::cache key shape becomes (TypeId, &'static str) so
  `use_text_edit_state(tag)` + `use_caret_blink(tag)` compose without
  type collision on the shared widget tag
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import RpcSubprocess, assert_eq, run_demo, wait_query


TF_TAG = "main_textfield"


def focus_set(tf: RpcSubprocess, tag: str | None) -> None:
    """`focus/set` wrapper — the rpc_verify harness exposes
    `request(...)` directly; the typed `focus.*` family is binding-
    facing and not part of the convenience surface yet."""
    tf.request("focus/set", {"tag": tag})


def body() -> None:
    with RpcSubprocess("hello-textfield") as tf:
        # Initial state — un-focused, no text, caret at byte 0.
        assert_eq(tf.query("/external/state"), "Idle", "initial /external/state")
        assert_eq(tf.query("/external/text"), "", "initial /external/text")
        assert_eq(tf.query("/external/caret"), 0, "initial /external/caret")

        # Focus the field via the focus mgr — exercises the R56.1.h
        # shell ↔ External::on_focus_change wire.
        focus_set(tf, TF_TAG)
        wait_query(tf, "/external/state", "Focused", desc="post-focus /external/state")

        # Type "hi" — two single-printable invocations.
        assert_eq(
            tf.invoke("/external/key", "h"), True, "invoke('key', 'h') recognized",
        )
        wait_query(tf, "/external/text", "h", desc="after 'h' /external/text")
        assert_eq(tf.query("/external/caret"), 1, "after 'h' /external/caret")

        assert_eq(
            tf.invoke("/external/key", "i"), True, "invoke('key', 'i') recognized",
        )
        wait_query(tf, "/external/text", "hi", desc="after 'i' /external/text")
        assert_eq(tf.query("/external/caret"), 2, "after 'i' /external/caret")

        # Backspace — delete char left of caret.
        assert_eq(
            tf.invoke("/external/key", "Backspace"),
            True,
            "invoke('key', 'Backspace') recognized",
        )
        wait_query(tf, "/external/text", "h", desc="after Backspace /external/text")
        assert_eq(tf.query("/external/caret"), 1, "after Backspace /external/caret")

        # Home — caret to start.
        assert_eq(
            tf.invoke("/external/key", "Home"),
            True,
            "invoke('key', 'Home') recognized",
        )
        wait_query(tf, "/external/caret", 0, desc="after Home /external/caret")
        assert_eq(tf.query("/external/text"), "h", "after Home /external/text unchanged")

        # End — caret to end.
        assert_eq(
            tf.invoke("/external/key", "End"),
            True,
            "invoke('key', 'End') recognized",
        )
        wait_query(tf, "/external/caret", 1, desc="after End /external/caret")

        # Insert at end — caret advances past the existing 'h'.
        assert_eq(
            tf.invoke("/external/key", "a"),
            True,
            "invoke('key', 'a') recognized at end",
        )
        wait_query(tf, "/external/text", "ha", desc="after 'a' /external/text")
        assert_eq(tf.query("/external/caret"), 2, "after 'a' /external/caret")

        # ArrowLeft — caret moves one char left without text change.
        assert_eq(
            tf.invoke("/external/key", "ArrowLeft"),
            True,
            "invoke('key', 'ArrowLeft') recognized",
        )
        wait_query(tf, "/external/caret", 1, desc="after ArrowLeft /external/caret")
        assert_eq(tf.query("/external/text"), "ha", "text unchanged after ArrowLeft")

        # Delete — remove char at caret.
        assert_eq(
            tf.invoke("/external/key", "Delete"),
            True,
            "invoke('key', 'Delete') recognized",
        )
        wait_query(tf, "/external/text", "h", desc="after Delete /external/text")
        assert_eq(tf.query("/external/caret"), 1, "after Delete /external/caret")

        # Unrecognized key — F1 rejected per apply_key contract; the
        # invoke surface returns Bool(false) instead of raising.
        assert_eq(
            tf.invoke("/external/key", "F1"),
            False,
            "invoke('key', 'F1') rejected (unrecognized)",
        )
        wait_query(tf, "/external/text", "h", desc="F1 leaves text unchanged")
        assert_eq(tf.query("/external/caret"), 1, "F1 leaves caret unchanged")

        # Blur via focus/set null — SCXML transitions Focused → Idle.
        focus_set(tf, None)
        wait_query(tf, "/external/state", "Idle", desc="post-blur /external/state")
        # Text content survives blur — TextEditState lives across the
        # focus mgr's mutations because the substrate keeps the same
        # Rc<TextEditState> for the binding's lifetime.
        assert_eq(tf.query("/external/text"), "h", "text survives blur")


if __name__ == "__main__":
    sys.exit(run_demo("hello-textfield typing", body))
