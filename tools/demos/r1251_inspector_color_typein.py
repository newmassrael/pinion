#!/usr/bin/env python3
"""R1251 §5.38 §5.40 — inspector Colour cell hex TYPE-IN.

R1249 gave the inspector's Int/Float Details cells an absolute type-in editor.
R1251 extends the SAME inline field to **Colour** cells: a double-click (or
`invoke begin_edit <i>`) opens the field seeded with the colour's `#RRGGBB`
hex; typing a new hex + Enter (or `invoke commit_edit "#RRGGBB"`) writes the
colour, Escape cancels, a malformed hex keeps the prior colour.

No new editor: the shared `EDIT_TF` field rides the `CellKind::Color` keystroke
gate (hex digits + `#` only — a letter like `z`/`g` is rejected at the keystroke
edge) and `CellKind::parse` (`Color::from_hex`). The commit writes the parsed
`CellValue` directly across the selection — NOT via `to_introspect`, because a
Colour's `to_introspect` is rich JSON while the write path wants the hex `Text`.

Player's `Tint` (Color) is common row 6 in the boot single-selection
(Visible/Layer/Locked/Health/Speed/Team/Tint). All observable over the §5.12 RPC
plane (§2 #2), no pixels: `value.6` reads the colour JSON (its `hex`), `editing`
the open row, `edit_text` the live buffer.

  (A) boot: Tint is a common Colour row; the editor is closed.
  (B) begin_edit opens + seeds with the hex; introspect reads it back.
  (C) commit a new hex writes the colour (round-trips through value.6.hex).
  (D) a malformed hex keeps the prior colour (no data loss).
  (E) a double-click on the colour cell opens the editor.
  (F) keyboard: the hex keystroke gate rejects non-hex letters, accepts hex.
  (G) rejects: begin_edit on a Bool row is a benign no-op.

Run from the workspace root:
    cargo build -p hello-inspector --release
    python3 tools/demos/r1251_inspector_color_typein.py
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    run_demo,
    wait_until,
)

INSPECTOR = "inspector"
TINT = 6  # Player's Tint (Color) is common row 6 in the boot single-selection.


def q(tf: RpcSubprocess, key: str) -> Any:
    return tf.query(f"/external/{key}")


def hex_of(tf: RpcSubprocess) -> str:
    """Tint's current hex off the `value.6` Colour JSON."""
    return q(tf, f"value.{TINT}")["hex"]


def wait_editing(tf: RpcSubprocess, expected: Any, desc: str) -> None:
    wait_until(lambda: True if q(tf, "editing") == expected else None, desc=desc)


def body() -> None:
    with RpcSubprocess("hello-inspector", boot_grace=1.5) as tf:
        # ── (A) boot: Tint is a common Colour row, editor closed ─────
        wait_until(lambda: True if q(tf, "object_count") == 3 else None,
                   desc="inspector ready")
        assert_eq(q(tf, "selected"), 0, "Player is the boot selection")
        assert_eq(q(tf, "selection_count"), 1, "one object selected")
        assert_eq(q(tf, "row_count"), 7, "Player exposes 7 properties")
        assert_eq(q(tf, f"kind.{TINT}"), "color", "row 6 (Tint) is a Colour")
        assert_eq(q(tf, f"name.{TINT}"), "Tint", "row 6 is named Tint")
        assert_eq(q(tf, "editing"), None, "the editor is closed at boot")
        boot_hex = hex_of(tf)
        assert boot_hex.startswith("#"), f"Tint reads a hex, got {boot_hex!r}"

        # ── (B) begin_edit opens + seeds with the hex ────────────────
        assert_eq(tf.invoke("/external/begin_edit", TINT), True,
                  "begin_edit on a Colour row opens the editor")
        wait_editing(tf, TINT, desc="editing -> Tint (row 6)")
        assert_eq(q(tf, "edit_text"), boot_hex, "seeded with the current hex")
        tf.invoke("/external/cancel_edit", None)
        wait_editing(tf, None, desc="cancel closes the editor")
        assert_eq(hex_of(tf), boot_hex, "cancel left the colour untouched")

        # ── (C) commit a new hex writes the colour ───────────────────
        for new_hex in ("#ff0000", "#123456", "#00ff88"):
            assert_eq(tf.invoke("/external/begin_edit", TINT), True, "re-open Tint")
            assert_eq(tf.invoke("/external/commit_edit", new_hex), True,
                      f"commit {new_hex} returns true")
            wait_editing(tf, None, desc=f"commit {new_hex} closes the editor")
            assert_eq(hex_of(tf), new_hex, f"Tint is now {new_hex}")

        # ── (D) a malformed hex keeps the prior colour ───────────────
        kept = hex_of(tf)  # #00ff88 from (C)
        assert_eq(tf.invoke("/external/begin_edit", TINT), True, "re-open")
        assert_eq(tf.invoke("/external/commit_edit", "not-a-hex"), False,
                  "a malformed hex commit returns false")
        assert_eq(hex_of(tf), kept, "the prior colour is kept (no data loss)")
        wait_editing(tf, None, desc="editor closed after a failed commit")

        # ── (E) a double-click opens the editor ──────────────────────
        tf.double_click(path=f"{INSPECTOR}#typein{TINT}")
        wait_editing(tf, TINT, desc="a double-click opens the hex editor")
        assert_eq(q(tf, "edit_text"), kept, "the double-click seeds the hex")

        # ── (F) keyboard: the hex keystroke gate ─────────────────────
        # The editor is open (from E), focused. A non-hex letter is rejected at
        # the CellKind::Color keystroke edge; hex digits + '#' land.
        before = q(tf, "edit_text")
        tf.key(path=INSPECTOR, name="z")
        assert_eq(q(tf, "edit_text"), before, "a non-hex 'z' is rejected by the gate")
        tf.key(path=INSPECTOR, name="g")
        assert_eq(q(tf, "edit_text"), before, "a non-hex 'g' is rejected by the gate")
        tf.key(path=INSPECTOR, name="a")
        wait_until(lambda: True if q(tf, "edit_text") == before + "a" else None,
                   desc="a hex digit 'a' lands in the field")
        tf.key(path=INSPECTOR, name="Escape")
        wait_editing(tf, None, desc="Escape discards the keyboard edit")
        assert_eq(hex_of(tf), kept, "Escape left the colour unchanged")

        # A full keyboard commit: open, clear, type a hex, Enter.
        assert_eq(tf.invoke("/external/begin_edit", TINT), True, "open for keyboard")
        for _ in range(len(kept)):
            tf.key(path=INSPECTOR, name="Backspace")
        wait_until(lambda: True if q(tf, "edit_text") == "" else None,
                   desc="Backspace clears the seeded hex")
        for ch in "#ffffff":
            tf.key(path=INSPECTOR, name=ch)
        wait_until(lambda: True if q(tf, "edit_text") == "#ffffff" else None,
                   desc="typed hex lands in the buffer")
        tf.key(path=INSPECTOR, name="Enter")
        wait_editing(tf, None, desc="Enter commits + closes the editor")
        assert_eq(hex_of(tf), "#ffffff", "Enter committed the typed hex")

        # ── (G) rejects: begin_edit on a Bool is a benign no-op ──────
        assert_eq(tf.invoke("/external/begin_edit", 0), False,
                  "begin_edit on a Bool (Visible) is a no-op false")
        assert_eq(q(tf, "editing"), None, "the reject did not open the editor")


if __name__ == "__main__":
    sys.exit(run_demo("R1251 inspector Colour hex type-in", body))
