#!/usr/bin/env python3
"""R1427 §5.41 §5.39 — a terminal cursor goes HOLLOW + stops blinking on an
unfocused window.

R1424 gave the cursor an OSC-12 colour, R1425 the DECSCUSR blink MODE, R1426 the
render-time blink PHASE. R1427 gates that render on OS focus: an unfocused
window draws its cursor as a HOLLOW outline box (overriding the shape) and stops
blinking — the universal unfocused-terminal indicator (xterm / VTE / alacritty /
kitty / Windows Terminal / iTerm2). This is GUI-only (the TUI has no OS-window
focus), and it is a PAINT concern: the hollow render + stop-blink are verified by
the headless GPU pixel guard (`r1427_unfocused_text_grid_cursor_is_hollow`) and
the runtime unit test (`r1427_unfocused_window_stops_the_cursor_blink`).

What THIS demo proves over RPC is the §2 #7 contract: focus is NOT folded into
the cursor data — the cursor SNAPSHOT is byte-identical whether or not the window
holds focus. An AI reads whether the cursor is hollow by correlating the window's
snapshot with `scene/input_state`'s `os_focused_window` (already data), never by
watching a pixel flicker or finding a "hollow" flag mutated into the cursor.

`hello-window-focus-multi` opens `main` + `inspector`; each carries the SAME
blinking block cursor grid. Driving OS focus per window with
`scene/window_focus {window, focused}` changes only which window's cursor the
shell paints filled+blinking vs hollow+steady — the reported cursor never moves.

Verification scope (>= 30 assertions):

  (A) boot: both windows carry the blinking block cursor mode.
  (B) focus main: os_focused == main; BOTH cursor snapshots unchanged.
  (C) focus inspector: the roles swap; cursor data still identical.
  (D) blur: both unfocused; the blinking MODE survives (focus != mode).
  (E) headline: the cursor snapshot is byte-identical across every focus state.
  (F) regression: the R1419 per-window dimming labels still track focus.
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (
    RpcSubprocess,
    assert_eq,
    find_by_tag,
    run_demo,
)

EXAMPLE = "hello-window-focus-multi"
CURSOR_TAG = "focus_cursor_grid"
STATUS_TAG = "os_focus_status"
MAIN = "main"
INSPECTOR = "inspector"
WIN = (300, 180)


def _cursor(tf: RpcSubprocess, window: str) -> dict[str, Any]:
    # `source="paint"` re-runs THIS window's view against the live focus mirror.
    snap = tf.snapshot(source="paint", viewport=WIN, window=window)
    grid = find_by_tag(snap, CURSOR_TAG)
    assert grid is not None, f"{window}: paints the tagged cursor grid"
    return grid["cursor"]


def _status(tf: RpcSubprocess, window: str) -> Optional[str]:
    snap = tf.snapshot(source="paint", viewport=WIN, window=window)
    node = find_by_tag(snap, STATUS_TAG)
    assert node is not None, f"{window}: paints the status label"
    return node.get("content")


def _os_focused(tf: RpcSubprocess) -> Optional[str]:
    resp = tf.request("scene/input_state", {})
    assert resp is not None and isinstance(resp.result, dict)
    return resp.result["key_dispatch"]["os_focused_window"]


def _drive(tf: RpcSubprocess, window: str, focused: bool) -> None:
    resp = tf.request("scene/window_focus", {"window": window, "focused": focused})
    assert resp is not None


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # The canonical blinking block cursor both windows carry (R1425 mode):
        # col 5 (the input column of "$ vim"), visible, block, blinking.
        expected = {
            "col": 5,
            "row": 0,
            "shape": "block",
            "visible": True,
            "cursor_color": None,
            "blink": True,
        }

        # ── (A) boot: both windows carry the blinking block cursor ───────────
        boot_main = tf.snapshot(source="paint", viewport=WIN, window=MAIN)
        boot_grid = find_by_tag(boot_main, CURSOR_TAG)
        assert boot_grid is not None, "the cursor grid is present at boot"
        assert_eq((boot_grid["buffer_cols"], boot_grid["buffer_rows"]), (6, 1),
                  "the cursor grid is the 6x1 prompt buffer")
        cur_main = _cursor(tf, MAIN)
        cur_insp = _cursor(tf, INSPECTOR)
        assert_eq(cur_main["blink"], True, "main cursor is a blinking DECSCUSR variant")
        assert_eq(cur_main["visible"], True, "main cursor is shown")
        assert_eq(cur_main["shape"], "block", "main cursor is a block")
        assert_eq(cur_main["cursor_color"], None, "no OSC-12 colour — blink is the axis here")
        assert_eq(cur_main, expected, "main cursor snapshot is the full blinking-block mode")
        assert_eq(cur_insp, expected, "inspector carries the SAME cursor mode")
        assert_eq(cur_main, cur_insp, "both windows report an identical cursor at boot")

        # ── (B) focus main: os focus tracks, cursor DATA does not move ───────
        _drive(tf, MAIN, True)
        assert_eq(_os_focused(tf), MAIN, "OS focus is on main")
        cur_main = _cursor(tf, MAIN)
        cur_insp = _cursor(tf, INSPECTOR)
        # The focused window's cursor renders filled+blinking, the unfocused
        # one's hollow+steady — but that is PAINT: the reported data is identical.
        assert_eq(cur_main, expected, "focused main: cursor snapshot unchanged (mode is data)")
        assert_eq(cur_insp, expected, "unfocused inspector: cursor snapshot unchanged too")
        assert_eq(
            cur_main,
            cur_insp,
            "★ focused and unfocused windows report the SAME cursor — focus is "
            "NOT folded into the cursor (the hollow render is paint-only, §2 #7)",
        )
        assert "blink" in cur_insp and "hollow" not in cur_insp, (
            "no 'hollow' flag is mutated into the cursor — focus stays out of it"
        )

        # ── (C) focus inspector: roles swap, cursor data still identical ─────
        _drive(tf, INSPECTOR, True)
        assert_eq(_os_focused(tf), INSPECTOR, "OS focus moved to inspector")
        assert_eq(_cursor(tf, INSPECTOR), expected, "now-focused inspector: cursor unchanged")
        assert_eq(_cursor(tf, MAIN), expected, "now-unfocused main: cursor unchanged")
        assert_eq(
            _cursor(tf, MAIN),
            _cursor(tf, INSPECTOR),
            "the roles swapped in paint, but the reported cursor is still identical",
        )

        # ── (D) blur: both unfocused; the blinking MODE survives ─────────────
        _drive(tf, INSPECTOR, False)
        assert_eq(_os_focused(tf), None, "whole application blurred")
        blurred_main = _cursor(tf, MAIN)
        blurred_insp = _cursor(tf, INSPECTOR)
        assert_eq(blurred_main["blink"], True, "a blurred window's cursor is STILL a blinking type")
        assert_eq(blurred_main["visible"], True, "and still DECTCEM-visible (blur != hide)")
        assert_eq(blurred_main, expected, "blurred main: the blinking MODE survives the blur")
        assert_eq(blurred_insp, expected, "blurred inspector: same")

        # ── (E) HEADLINE: the cursor snapshot is byte-identical across every
        #        focus state — focus never folds into the cursor (§2 #7) ───────
        states = []
        # Each focus drive names a window (focus-true is deterministic: the drove
        # window holds focus); the cursor snapshot must not move under any of them.
        for win in [MAIN, INSPECTOR, MAIN]:
            _drive(tf, win, True)
            assert_eq(_os_focused(tf), win, f"os focus tracks the {win} drive")
            states.append(_cursor(tf, MAIN))
            states.append(_cursor(tf, INSPECTOR))
        assert all(c == expected for c in states), (
            "the cursor snapshot is identical in EVERY focus configuration"
        )
        assert_eq(len({tuple(sorted(c.items())) for c in states}), 1,
                  "exactly one distinct cursor snapshot across all focus states")

        # ── (F) regression: R1419 per-window dimming still tracks focus ──────
        _drive(tf, MAIN, True)
        assert_eq(_status(tf, MAIN), "main: OS-ACTIVE", "R1419 dimming: focused main is active")
        assert_eq(_status(tf, INSPECTOR), "inspector: blurred",
                  "R1419 dimming: unfocused inspector dims — coexists with the cursor axis")
        # And the cursor data is STILL unchanged while the dimming label flipped.
        assert_eq(_cursor(tf, MAIN), expected, "cursor data unmoved while the label flipped")
        assert_eq(_cursor(tf, INSPECTOR), expected, "inspector cursor data unmoved too")


def main() -> int:
    return run_demo("r1427_cursor_focus", body)


if __name__ == "__main__":
    sys.exit(main())
