#!/usr/bin/env python3
"""R763 §5.36 §5.22 — pointer text selection (drag + Shift-click).

R56.1.f built the selection MODEL (`TextEditState::set_selection` /
`selection_range`), the PAINT (the accent selection band), the
INTROSPECTION (`/external/selection` read + intervene), and the KEYBOARD
wiring (Shift+Arrow / Ctrl+A via the Json `invoke("key", ...)` shape —
see `hello_textfield_select.py`). R762 added the click-to-position caret
hit-test (`byte_for_field_point`). The one remaining gap was
POINTER-DRIVEN selection: nothing turned a real mouse drag (or a
Shift-click) into a selection. R763 wires it at the shell's single press
→ move → release convergence point:

- `position_caret_for_point(.., extend)` — a press hit-tests the cursor
  to a byte; a plain press collapses the caret there (and arms the drag
  anchor), a Shift-press (`extend`) extends the selection from the
  retained anchor. Returns the pinned anchor the shell stores.
- `select_drag_to_point(.., anchor, x, y)` — every `cursor_moved` while
  the button is held extends the selection from that anchor to the byte
  under the cursor.
- `scene/modifiers` — the winit `ModifiersChanged` RPC peer, so a
  Shift-click is drivable headless (sets the shell's absolute modifier
  cache a subsequent `scene/click` press reads). Closes the R742.2
  RPC-modifier-channel gap.

Because the native winit press/move/release and the `scene/drag` /
`scene/click` deferred-input drains converge on the SAME
`mouse_pressed_for_window` / `cursor_moved_for_window` /
`mouse_released_for_window` (no native-only branch), this RPC demo's
coverage equals native-mouse coverage. Everything is observed as DATA
via `query("/external/selection")` / `caret` / `text` (scene-as-data;
the press / drag mutate the live TextEditState synchronously).

Run from the workspace root:
    cargo build -p hello-textfield --release
    python3 tools/demos/r763_textfield_drag_select.py
"""

from __future__ import annotations

import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import RpcSubprocess, assert_eq, find_by_tag, run_demo

TF_TAG = "main_textfield"
VIEWPORT = (420, 200)
TEXT = "hello world"  # 11 ASCII bytes; caret end = 11
SETTLE = 0.06  # let the deferred-input drain apply the gesture


def field_rect(tf: RpcSubprocess):
    """The text field's window-coord rect (x, y, w, h) from the paint scene."""
    snap = tf.snapshot(source="paint", viewport=VIEWPORT)
    node = find_by_tag(snap, TF_TAG)
    assert node is not None, "text field present in paint scene"
    r = node["rect"]
    return (int(r["x"]), int(r["y"]), int(r["w"]), int(r["h"]))


def selection(tf: RpcSubprocess):
    """Current selection range dict {"start","end"} or None (collapsed)."""
    return tf.query("/external/selection")


def caret(tf: RpcSubprocess) -> int:
    return tf.query("/external/caret")


def text(tf: RpcSubprocess) -> str:
    return tf.query("/external/text")


def byte_at_x(tf: RpcSubprocess, x: float, y: float) -> int:
    """Probe: plain-click at (x, y) and read the resolved caret byte.

    Collapses any selection (a plain press), so callers use it to learn
    the byte a coordinate maps to BEFORE setting up a selection test.
    """
    tf.click(at=(x, y))
    time.sleep(SETTLE)
    return caret(tf)


def body() -> None:
    # Generous per-request timeout: the demo issues many gesture
    # round-trips; the selection logic itself is fast.
    with RpcSubprocess("hello-textfield", request_timeout=12.0) as tf:
        # ── boot baseline ────────────────────────────────────────────
        assert_eq(tf.query("/external/state"), "Idle", "initial state Idle")
        assert_eq(selection(tf), None, "initial selection null (collapsed)")

        # Focus + type the fixture text.
        tf.request("focus/set", {"tag": TF_TAG})
        time.sleep(0.05)
        assert_eq(tf.query("/external/state"), "Focused", "focused after focus/set")
        for ch in TEXT:
            key = "Space" if ch == " " else ch
            assert_eq(tf.invoke("/external/key", key), True, f"type {key!r}")
        time.sleep(0.05)
        assert_eq(text(tf), TEXT, "typed text")
        assert_eq(caret(tf), len(TEXT), "caret at end after typing")
        assert_eq(selection(tf), None, "no selection after plain typing")

        (fx, fy, fw, fh) = field_rect(tf)
        y_mid = fy + fh / 2.0
        x_left = fx + 2
        x_right = fx + fw - 2

        # Probe the edge byte mapping (each probe collapses any selection).
        assert_eq(byte_at_x(tf, x_left, y_mid), 0, "left edge maps to byte 0")
        assert_eq(byte_at_x(tf, x_right, y_mid), len(TEXT), "right edge maps to text end")

        # The field is far wider than the short fixture text, so a fixed
        # fraction of the field width overshoots past the text end. Sweep
        # to find a coordinate that maps to a strict-interior byte near
        # the text middle (used for the partial-selection tests).
        x_mid = None
        b_mid = None
        sweep_steps = 24
        for i in range(sweep_steps + 1):
            x = x_left + (x_right - x_left) * (i / sweep_steps)
            b = byte_at_x(tf, x, y_mid)
            if 0 < b < len(TEXT):
                x_mid, b_mid = x, b
                if b >= len(TEXT) // 2:
                    break
        assert x_mid is not None, "found an interior click coordinate"
        assert 0 < b_mid < len(TEXT), f"interior probe lands mid-text (got {b_mid})"

        # ── drag-to-select: left → right selects the whole text ───────
        tf.drag(from_at=(x_left, y_mid), to_at=(x_right, y_mid), steps=8)
        time.sleep(SETTLE)
        assert_eq(selection(tf), {"start": 0, "end": len(TEXT)}, "drag L→R selects all")
        assert_eq(caret(tf), len(TEXT), "drag focus end at right (caret 11)")
        cur = text(tf)
        assert_eq(cur[0:len(TEXT)], TEXT, "selection_text spans whole buffer")

        # ── plain click collapses the selection ──────────────────────
        tf.click(at=(x_left, y_mid))
        time.sleep(SETTLE)
        assert_eq(selection(tf), None, "plain click collapses selection")
        assert_eq(caret(tf), 0, "collapsed caret at click byte 0")

        # ── partial drag: left → interior x selects a prefix ──────────
        tf.click(at=(x_left, y_mid))  # caret 0 anchor
        time.sleep(SETTLE)
        tf.drag(from_at=(x_left, y_mid), to_at=(x_mid, y_mid), steps=6)
        time.sleep(SETTLE)
        sel = selection(tf)
        assert sel is not None, "partial drag produces a selection"
        assert_eq(sel["start"], 0, "partial drag start at byte 0")
        assert_eq(sel["end"], b_mid, "partial drag end matches probed interior byte")
        assert 0 < sel["end"] < len(TEXT), "partial selection is a strict interior prefix"
        assert_eq(text(tf)[0:sel["end"]], TEXT[0:b_mid], "partial selection_text = prefix")

        # ── reversed drag: right → left normalizes start <= end ───────
        tf.drag(from_at=(x_right, y_mid), to_at=(x_left, y_mid), steps=8)
        time.sleep(SETTLE)
        assert_eq(selection(tf), {"start": 0, "end": len(TEXT)}, "reversed drag selects all")
        assert_eq(caret(tf), 0, "reversed drag focus end at left (caret 0)")

        # ── Shift-click extends from the current caret ────────────────
        tf.click(at=(x_left, y_mid))  # caret 0, no selection
        time.sleep(SETTLE)
        assert_eq(selection(tf), None, "pre-shift-click selection cleared")
        tf.modifiers(shift=True)
        tf.click(at=(x_right, y_mid))  # extend anchor(0) → focus(11)
        tf.modifiers()  # release Shift
        time.sleep(SETTLE)
        assert_eq(selection(tf), {"start": 0, "end": len(TEXT)}, "Shift-click extends to end")
        assert_eq(caret(tf), len(TEXT), "Shift-click focus at right")

        # ── Shift-click again shrinks (anchor pinned, focus moves) ────
        tf.modifiers(shift=True)
        tf.click(at=(x_mid, y_mid))  # anchor stays 0, focus → b_mid
        tf.modifiers()
        time.sleep(SETTLE)
        sel = selection(tf)
        assert sel is not None, "second Shift-click keeps a selection"
        assert_eq(sel["start"], 0, "Shift-click anchor pinned at 0")
        assert_eq(sel["end"], b_mid, "Shift-click focus moved to interior byte")

        # ── Shift-click with no prior selection latches the caret ─────
        tf.click(at=(x_mid, y_mid))  # caret = b_mid, collapsed
        time.sleep(SETTLE)
        assert_eq(selection(tf), None, "collapsed before latch test")
        assert_eq(caret(tf), b_mid, "caret at interior byte before latch")
        tf.modifiers(shift=True)
        tf.click(at=(x_left, y_mid))  # anchor latches b_mid, focus → 0
        tf.modifiers()
        time.sleep(SETTLE)
        assert_eq(selection(tf), {"start": 0, "end": b_mid}, "Shift-click latches caret as anchor")
        assert_eq(caret(tf), 0, "latch focus end at left")

        # ── modifiers persist until released: a click between a
        # shift-press and its release must NOT be re-read as shifted
        # once released. (release, then plain click collapses.)
        tf.click(at=(x_right, y_mid))
        time.sleep(SETTLE)
        assert_eq(selection(tf), None, "plain click after Shift released collapses")
        assert_eq(caret(tf), len(TEXT), "plain click caret at right")

        # ── type-to-replace after a drag selection ────────────────────
        tf.drag(from_at=(x_left, y_mid), to_at=(x_right, y_mid), steps=8)
        time.sleep(SETTLE)
        assert_eq(selection(tf), {"start": 0, "end": len(TEXT)}, "select-all before replace")
        assert_eq(tf.invoke("/external/key", "X"), True, "type X over selection")
        time.sleep(SETTLE)
        assert_eq(text(tf), "X", "type-to-replace drained the selection")
        assert_eq(caret(tf), 1, "caret after replacement char")
        assert_eq(selection(tf), None, "type-to-replace collapses selection")


if __name__ == "__main__":
    sys.exit(run_demo("R763 TextField pointer selection (drag + Shift-click)", body))
