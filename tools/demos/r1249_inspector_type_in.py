#!/usr/bin/env python3
"""R1249 §5.38 §5.40 §5.45 — inspector Details ABSOLUTE numeric type-in editor.

R1221 made Int/Float Details cells `[-] value [+]` steppers — a value could be
STEPPED (relative) but never TYPED to an exact value. R1249 closes that last
DCC-widget gap: a double-click (or `invoke begin_edit <i>`) opens the shared
inline `TextField` seeded with the row's value; typing + `Enter` (or `invoke
commit_edit "<v>"`) writes the ABSOLUTE value across the WHOLE selection, `Escape`
(or `invoke cancel_edit`) discards, blur commits.

The editor is hosted as a sibling `External` via the NEW `#[widget(extra_externals
= ...)]` macro attribute (the inspector is its first consumer — 65 hand-rolled
extra-hosting bindings proved the abstraction). Hosting it reshapes the state
scene into a `Container`, so `read_state` / `apply_key` walk by tag.

All observable over the §5.12 RPC plane (§2 #2), no pixels:
  * `editing` reads the open row (Null when closed), `edit_text` the live buffer;
  * `begin_edit` / `commit_edit` / `cancel_edit` drive the whole edit AI-first.

  (A) boot: Layer (row 1) is a common Int; the editor is closed.
  (B) `begin_edit` opens + seeds; `editing` / `edit_text` read it back.
  (C) `cancel_edit` closes without writing.
  (D) `commit_edit` writes the ABSOLUTE value across a MULTI-selection at once.
  (E) a double-click on the numeric cell opens the editor; a single click does not.
  (F) keyboard: type into the focused field + `Enter` commits; `Escape` cancels.
  (G) rejects: begin_edit on a Bool / out-of-range row is a benign no-op.

Run from the workspace root:
    cargo build -p hello-inspector --release
    python3 tools/demos/r1249_inspector_type_in.py
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    assert_eq,
    run_demo,
    wait_until,
)

INSPECTOR = "inspector"
LAYER = 1  # common Int row (Visible/Layer/Locked across all three objects).


def q(tf: RpcSubprocess, key: str) -> Any:
    return tf.query(f"/external/{key}")


def wait_editing(tf: RpcSubprocess, expected: Any, desc: str) -> None:
    wait_until(lambda: True if q(tf, "editing") == expected else None, desc=desc)


def body() -> None:
    with RpcSubprocess("hello-inspector", boot_grace=1.5) as tf:
        # ── (A) boot: Layer is a common Int, editor closed ───────────
        wait_until(lambda: True if q(tf, "object_count") == 3 else None,
                   desc="inspector ready")
        # Select all three -> Visible(0)/Layer(1)/Locked(2) are the common rows.
        tf.invoke("/external/select_all", None)
        wait_until(lambda: True if q(tf, "selection_count") == 3 else None,
                   desc="all three objects selected")
        assert_eq(q(tf, "row_count"), 3, "three common rows across the selection")
        assert_eq(q(tf, "kind.0"), "bool", "row 0 (Visible) is a Bool")
        assert_eq(q(tf, f"kind.{LAYER}"), "int", "row 1 (Layer) is an Int")
        assert_eq(q(tf, f"name.{LAYER}"), "Layer", "row 1 is named Layer")
        assert_eq(q(tf, f"mixed.{LAYER}"), True, "Layer is 1,1,2 -> Multiple Values")
        assert_eq(q(tf, "editing"), None, "the editor is closed at boot")

        # ── (B) begin_edit opens + seeds; introspect reads it ────────
        assert_eq(tf.invoke("/external/begin_edit", LAYER), True,
                  "begin_edit on a numeric row opens the editor")
        wait_editing(tf, LAYER, desc="editing -> Layer (row 1)")
        assert_eq(q(tf, "edit_text"), "1",
                  "seeded with the representative (first object's) value")

        # ── (C) cancel closes without writing ────────────────────────
        assert_eq(tf.invoke("/external/cancel_edit", None), None,
                  "cancel_edit returns null")
        wait_editing(tf, None, desc="cancel closes the editor")
        assert_eq(q(tf, f"value.{LAYER}"), 1,
                  "the representative Layer is untouched (still 1) after a cancel")
        assert_eq(q(tf, f"mixed.{LAYER}"), True, "Layer still 1,1,2 after cancel")

        # ── (D) commit writes the ABSOLUTE value across the selection ─
        assert_eq(tf.invoke("/external/begin_edit", LAYER), True, "re-open Layer")
        assert_eq(tf.invoke("/external/commit_edit", "7"), True,
                  "commit_edit of a valid numeric returns true")
        wait_editing(tf, None, desc="commit closes the editor")
        # Every selected object's Layer is now exactly 7 (absolute, collapsing the
        # prior 1/1/2 divergence) -> the representative is 7 and it is UNIFORM.
        assert_eq(q(tf, f"value.{LAYER}"), 7, "the representative Layer is 7")
        assert_eq(q(tf, f"mixed.{LAYER}"), False,
                  "an absolute write across the selection makes Layer uniform")
        # A malformed numeric keeps the prior value (no data loss).
        assert_eq(tf.invoke("/external/begin_edit", LAYER), True, "re-open")
        assert_eq(tf.invoke("/external/commit_edit", "not-a-number"), False,
                  "a malformed numeric commit returns false")
        assert_eq(q(tf, f"value.{LAYER}"), 7, "the prior value (7) is kept")
        wait_editing(tf, None, desc="the editor still closed after a failed commit")

        # ── (E) a double-click on the numeric cell opens the editor ──
        # (A single click on the cell label is a no-op — the steppers own single
        # clicks; the label/cell double-click opens the field. The single-click
        # no-op is unit-covered; interleaving a single then a double click over
        # RPC is an artificial latency case, so the demo drives a clean gesture.)
        tf.double_click(path=f"{INSPECTOR}#typein{LAYER}")
        wait_editing(tf, LAYER, desc="a double-click opens the type-in editor")
        assert_eq(q(tf, "edit_text"), "7", "the double-click seeds the current value 7")
        tf.invoke("/external/cancel_edit", None)
        wait_editing(tf, None, desc="cleanup: editor closed")

        # ── (F) keyboard: type into the focused field + Enter commits ─
        assert_eq(tf.invoke("/external/begin_edit", LAYER), True, "open for keyboard")
        assert_eq(q(tf, "edit_text"), "7", "seeded with the current value 7")
        # The field is focused (begin_edit requested it). Backspace clears the 7,
        # then type 3, 1 -> "31"; the caret / deletion keys + digit keystrokes all
        # route through the shared edit_field_keymap into the field.
        tf.key(path=INSPECTOR, name="Backspace")
        wait_until(lambda: True if q(tf, "edit_text") == "" else None,
                   desc="Backspace clears the seeded value")
        tf.key(path=INSPECTOR, name="3")
        tf.key(path=INSPECTOR, name="1")
        wait_until(lambda: True if q(tf, "edit_text") == "31" else None,
                   desc="typed digits land in the field buffer")
        tf.key(path=INSPECTOR, name="Enter")
        wait_editing(tf, None, desc="Enter commits + closes the editor")
        assert_eq(q(tf, f"value.{LAYER}"), 31, "Enter committed the typed value 31")
        # Escape cancels a keyboard edit without writing. The row now reads 31, so
        # re-opening seeds "31"; typing "9" appends -> "319", then Escape discards.
        tf.invoke("/external/begin_edit", LAYER)
        tf.key(path=INSPECTOR, name="9")
        wait_until(lambda: True if q(tf, "edit_text") == "319" else None,
                   desc="typed 9 appends to the seeded 31 (buffer grows)")
        tf.key(path=INSPECTOR, name="Escape")
        wait_editing(tf, None, desc="Escape closes the editor")
        assert_eq(q(tf, f"value.{LAYER}"), 31, "Escape discarded the edit (still 31)")

        # ── (G) rejects: non-numeric + out-of-range are benign ───────
        assert_eq(tf.invoke("/external/begin_edit", 0), False,
                  "begin_edit on a Bool (Visible) is a no-op false")
        assert_eq(tf.invoke("/external/begin_edit", 2), False,
                  "begin_edit on a Bool (Locked) is a no-op false")
        assert_eq(tf.invoke("/external/begin_edit", 99), False,
                  "begin_edit on an out-of-range row is a no-op false")
        assert_eq(q(tf, "editing"), None, "no reject opened the editor")
        # commit with no editor open is a no-op false (nothing to write).
        assert_eq(tf.invoke("/external/commit_edit", "5"), False,
                  "commit_edit with no open editor returns false")

        # ── (H) R1252: a selection change mid-edit closes the editor ──
        # `editing_prop` is a positional index into the selection-DERIVED common
        # list; a mid-edit selection change used to retarget it to a DIFFERENT
        # property (wrong-property write). Now any selection change closes the
        # editor, so the stale index can never commit.
        tf.invoke("/external/select", 1)   # Camera: common idx3 = "Field of View"
        wait_until(lambda: True if q(tf, "name.3") == "Field of View" else None,
                   desc="Camera row 3 is Field of View")
        fov_before = q(tf, "value.3")
        assert_eq(tf.invoke("/external/begin_edit", 3), True, "open the FoV editor")
        wait_editing(tf, 3, desc="editing -> Field of View")
        tf.invoke("/external/select", 2)   # Sun Light: common idx3 = "Intensity"
        wait_editing(tf, None, desc="the selection change closed the editor")
        assert_eq(q(tf, "name.3"), "Intensity", "row 3 is now Intensity")
        intensity_before = q(tf, "value.3")
        assert_eq(tf.invoke("/external/commit_edit", "45"), False,
                  "commit with a closed editor writes nothing")
        assert_eq(q(tf, "value.3"), intensity_before,
                  "Intensity was NOT clobbered by the stale index (R1252 fix)")
        tf.invoke("/external/select", 1)
        assert_eq(q(tf, "value.3"), fov_before, "Field of View untouched too")

        # ── (I) R1254: F2 opens the editor from the KEYBOARD (was mouse+RPC) ──
        tf.invoke("/external/select", 0)          # Player
        tf.invoke("/external/focus_property", 1)  # Details cursor on Layer (Int)
        assert_eq(q(tf, "editing"), None, "editor closed before F2")
        tf.key(path=INSPECTOR, name="F2")
        wait_editing(tf, 1, desc="F2 opened the editor on Layer from the keyboard")
        tf.invoke("/external/cancel_edit", None)
        # Enter opens a numeric too (a dead key there before R1254).
        tf.invoke("/external/focus_property", 1)
        tf.key(path=INSPECTOR, name="Enter")
        wait_editing(tf, 1, desc="Enter opened the numeric editor from the keyboard")
        tf.invoke("/external/cancel_edit", None)


if __name__ == "__main__":
    sys.exit(run_demo("R1249 inspector absolute numeric type-in", body))
