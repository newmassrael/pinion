#!/usr/bin/env python3
"""hello-listbox scroll dogfood (§5.49 R59, R51.195).

The full closure of R51.192's original META violation: spawn
hello-listbox, wheel-scroll over the viewport, and confirm `offset_y`
moved without asking a human to describe the screen.

Sequence:
  1. spawn hello-listbox
  2. snapshot paint scene  → assert initial Scroll.offset = (0, 0)
  3. scene/wheel at the viewport centre, lines delta dy=+3
     (positive dy = content scrolls down, per the W3C convention
     R51.192 fixed at the winit boundary)
  4. snapshot paint scene  → assert Scroll.offset_y > 0

The exact post-wheel offset depends on `wheel_delta_to_pixels`'s
line-height multiplier; the demo only asserts the inequality so a
runtime tweak to that multiplier does not break the dogfood. R51.196
(scene/click v1) will let a follow-up demo also verify the visible
row tag set shifts after the scroll.
"""

from __future__ import annotations

import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import RpcSubprocess, assert_eq, run_demo


WIN_W = 360
WIN_H = 320
VIEWPORT_W = 220
VIEWPORT_H = 5 * 28 + 4 * 6  # 5 rows + 4 gaps — see main.rs

# Viewport is centred inside the window — these are the absolute
# logical-pixel coordinates the shell exposes through the
# `InputRouter` cursor tracker.
VIEWPORT_CX = (WIN_W - VIEWPORT_W) // 2 + VIEWPORT_W // 2
VIEWPORT_CY = (WIN_H - VIEWPORT_H) // 2 + VIEWPORT_H // 2


def scroll_node(snap: dict) -> dict:
    """Pull the `Scroll` node out of a hello-listbox paint snapshot."""
    children = snap.get("children") or []
    if not children:
        raise AssertionError("outer Container has no children")
    scroll = children[0]
    if scroll.get("type") != "Scroll":
        raise AssertionError(f"expected Scroll child, got {scroll.get('type')!r}")
    return scroll


def body() -> None:
    with RpcSubprocess("hello-listbox") as listbox:
        before = listbox.snapshot(source="paint", viewport=(WIN_W, WIN_H))
        before_scroll = scroll_node(before)
        assert_eq(before_scroll.get("offset_x"), 0, "initial offset_x")
        assert_eq(before_scroll.get("offset_y"), 0, "initial offset_y")

        listbox.wheel(at=(VIEWPORT_CX, VIEWPORT_CY), lines=(0.0, 3.0))
        # The deferred-input drain runs on the dispatcher's return
        # path, so a brief sleep lets winit's next user event tick
        # process the redraw bump before the next snapshot lands.
        time.sleep(0.1)

        after = listbox.snapshot(source="paint", viewport=(WIN_W, WIN_H))
        after_scroll = scroll_node(after)
        after_offset_y = after_scroll.get("offset_y")
        if not isinstance(after_offset_y, (int, float)) or after_offset_y <= 0:
            raise AssertionError(
                f"post-wheel offset_y: expected > 0, got {after_offset_y!r}"
            )
        # Horizontal axis untouched — wheel was vertical-only.
        assert_eq(after_scroll.get("offset_x"), 0, "post-wheel offset_x")


if __name__ == "__main__":
    sys.exit(run_demo("hello-listbox wheel scroll", body))
