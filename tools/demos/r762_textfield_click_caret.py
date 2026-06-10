#!/usr/bin/env python3
"""R762 §5.36 §5.38 — click-to-position caret (pointer hit-test).

The R762 substrate `pinion_text::byte_offset_for_point` (pixel -> byte,
the inverse of `caret_rect_for_byte_offset`) lets a click inside a text
field move the caret to the byte under the cursor. The shell wires it at
`mouse_pressed_for_window` (the single convergence point for native winit
clicks AND the `scene/click` deferred-input drain) through the new
`WidgetView::position_caret_for_point` hook; hello-textfield resolves the
byte with `tf_paint::byte_for_field_point` (same splice + style +
LayoutCache the visible caret is built from -> one SSOT).

Verification: type "hello world" (caret at end, byte 11), then sweep the
click x-position across the field left -> right and assert the caret byte
(a) clamps to 0 left of the text, (b) clamps to 11 (text end) past the
text, (c) advances monotonically with x, and (d) repositions on a second
click (not append). All observed as DATA via `query("/external/caret")`
(scene-as-data; the caret slot reads the live TextEditState the press
mutated synchronously).

Run from the workspace root:
    cargo build -p hello-textfield --release
    python3 tools/demos/r762_textfield_click_caret.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import RpcSubprocess, assert_eq, find_by_tag, run_demo, wait_query

TF_TAG = "main_textfield"
VIEWPORT = (420, 200)
TEXT = "hello world"  # 11 ASCII bytes; caret end = 11


def field_rect(tf: RpcSubprocess):
    """The text field's window-coord rect (x, y, w, h) from the paint scene."""
    snap = tf.snapshot(source="paint", viewport=VIEWPORT)
    node = find_by_tag(snap, TF_TAG)
    assert node is not None, "text field present in paint scene"
    r = node["rect"]
    return (int(r["x"]), int(r["y"]), int(r["w"]), int(r["h"]))


def click_caret(tf: RpcSubprocess, x: float, y: float) -> int:
    """Click at window (x, y) and return the resulting caret byte.

    The deferred-input drain commits inside the click dispatch (before
    the RPC response), so the follow-up query reads the post-press
    caret directly (R883 zero-flake)."""
    tf.click(at=(x, y))
    return tf.query("/external/caret")


def body() -> None:
    # Generous per-request timeout: this demo issues many scene/click
    # round-trips, and under the full-sweep load a default 5 s ceiling
    # can clip a single response (the caret logic itself is fast).
    with RpcSubprocess("hello-textfield", request_timeout=12.0) as tf:
        assert_eq(tf.query("/external/state"), "Idle", "initial state Idle")

        # Focus + type the fixture text.
        tf.request("focus/set", {"tag": TF_TAG})
        wait_query(tf, "/external/state", "Focused", desc="focused after focus/set")
        for ch in TEXT:
            key = "Space" if ch == " " else ch
            assert_eq(tf.invoke("/external/key", key), True, f"type {key!r}")
        wait_query(tf, "/external/text", TEXT, desc="typed text")
        assert_eq(tf.query("/external/caret"), len(TEXT), "caret at end after typing")

        (fx, fy, fw, fh) = field_rect(tf)
        y_mid = fy + fh / 2.0

        # ── clamp left: a click in the left padding (before the text
        # origin) resolves to byte 0.
        assert_eq(click_caret(tf, fx + 2, y_mid), 0, "click left padding -> caret 0")

        # ── clamp right: a click past the last glyph resolves to the
        # text end (byte 11).
        assert_eq(
            click_caret(tf, fx + fw - 2, y_mid),
            len(TEXT),
            "click past text -> caret at end",
        )

        # ── reposition (not append): after the right click, clicking
        # left again moves the caret back to 0.
        assert_eq(click_caret(tf, fx + 2, y_mid), 0, "re-click left -> caret back to 0")

        # ── monotonic sweep: clicking at increasing x yields a
        # non-decreasing caret byte spanning [0, 11].
        steps = 8
        carets = []
        for i in range(steps + 1):
            x = fx + 2 + (fw - 4) * (i / steps)
            carets.append(click_caret(tf, x, y_mid))

        for i in range(1, len(carets)):
            assert carets[i] >= carets[i - 1], (
                f"caret must be non-decreasing across x sweep: "
                f"step {i} got {carets[i]} < {carets[i - 1]} (sweep={carets})"
            )
        assert_eq(carets[0], 0, "leftmost sweep click -> caret 0")
        assert_eq(carets[-1], len(TEXT), "rightmost sweep click -> caret at end")
        assert any(0 < c < len(TEXT) for c in carets), (
            f"at least one interior click lands mid-text (sweep={carets})"
        )

        # ── every resolved caret is a valid char boundary in [0, 11].
        for c in carets:
            assert 0 <= c <= len(TEXT), f"caret {c} within [0, {len(TEXT)}]"
            assert TEXT.encode().__len__() >= c, "caret within byte length"

        # ── typing still works after click-positioning: place the caret
        # at 0, insert 'X', and the text gains a leading char.
        assert_eq(click_caret(tf, fx + 2, y_mid), 0, "caret to 0 for insert")
        assert_eq(tf.invoke("/external/key", "X"), True, "type X at caret 0")
        wait_query(tf, "/external/text", "X" + TEXT, desc="X inserted at front")
        assert_eq(tf.query("/external/caret"), 1, "caret after inserted X")


if __name__ == "__main__":
    sys.exit(run_demo("R762 TextField click-to-position caret", body))
