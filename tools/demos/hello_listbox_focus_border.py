#!/usr/bin/env python3
"""hello-listbox focused-row border verification (§5.49 R59, R55.G.11).

Second-consumer of the R55.G.8 style snapshot wire (after
`hello_toggle_style.py`). Proves the focused-row visual cue — the
2-px (0x40, 0x80, 0xe0) blue border on the WAI-ARIA active-descendant
hint per `examples/hello-listbox/src/main.rs:272` — round-trips
through the JSON-RPC `scene/snapshot` payload.

Asserts:
  1. Row 0 carries the focus border by default
     (`border = {color:{r:0x40,g:0x80,b:0xe0,a:0xff}, width:2}`).
  2. Row 5 (non-focused) has `border == null`.
  3. After `scene/rewind` moves `focused_index = 3`, row 0's border
     disappears and row 3 gains it — proves the wire reflects
     reactive focus mutations frame by frame.

Exit 0 when every assertion holds.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import RpcSubprocess, assert_eq, find_by_tag, run_demo


FOCUS_BORDER_RGB = (0x40, 0x80, 0xE0)
FOCUS_BORDER_WIDTH = 2


def row_border(snap, row_idx):
    """Return the `border` JSON for `main_list#{row_idx}` (or None)."""
    row = find_by_tag(snap, f"main_list#{row_idx}")
    if row is None:
        raise AssertionError(f"main_list#{row_idx} not found in paint snapshot")
    return row.get("style", {}).get("border")


def body() -> None:
    with RpcSubprocess("hello-listbox") as lb:
        snap = lb.snapshot(source="paint")

        # 1) Row 0 is focused by default — carries the 2-px blue border.
        b0 = row_border(snap, 0)
        if b0 is None:
            raise AssertionError("row 0 expected to carry focus border")
        assert_eq(b0.get("width"), FOCUS_BORDER_WIDTH, "row 0 border width")
        color = b0.get("color") or {}
        assert_eq(color.get("r"), FOCUS_BORDER_RGB[0], "row 0 border.r")
        assert_eq(color.get("g"), FOCUS_BORDER_RGB[1], "row 0 border.g")
        assert_eq(color.get("b"), FOCUS_BORDER_RGB[2], "row 0 border.b")
        assert_eq(color.get("a"), 0xFF, "row 0 border.a")

        # 2) Row 5 (idle, not focused) reports null border.
        assert_eq(row_border(snap, 5), None, "row 5 idle, no border")

        # 3) Move focus to row 3 via scene/rewind on focused_index.
        #    `scene/key` is the user-facing arc but routes by tag — at
        #    paint time the External wrapper (which carries the focus
        #    keybinding) lives behind the paint scene and isn't a tag
        #    target. `scene/rewind` is the introspection-channel
        #    equivalent: set the reactive `focused_index` directly and
        #    let the next paint reflect the move.
        lb.request("scene/rewind", {
            "path": "/external/focused_index",
            "value": 3,
        })
        snap_after = lb.snapshot(source="paint")
        assert_eq(row_border(snap_after, 0), None, "row 0 no longer focused")
        b3 = row_border(snap_after, 3)
        if b3 is None:
            raise AssertionError("row 3 expected to gain focus border")
        assert_eq(b3.get("width"), FOCUS_BORDER_WIDTH, "row 3 border width")


if __name__ == "__main__":
    sys.exit(run_demo("hello-listbox focus border", body))
