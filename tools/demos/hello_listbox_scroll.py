#!/usr/bin/env python3
"""hello-listbox scroll dogfood (§5.49 R59, R51.195 / R51.202).

The full closure of R51.192's original META violation: spawn
hello-listbox, wheel-scroll over the viewport, and confirm `offset_y`
moved without asking a human to describe the screen.

Sequence:
  1. spawn hello-listbox
  2. snapshot paint scene  → assert initial Scroll.offset = (0, 0)
  3. scene/wheel `{path: "main_list_scroll", lines: (0, 3)}`
     (positive dy = content scrolls down, per the W3C convention
     R51.192 fixed at the winit boundary)
  4. snapshot paint scene  → assert Scroll.offset_y > 0

R51.202 §5.49 — wheel target is the `main_list_scroll` tag, not a
coordinate. Pre-R51.202 the demo computed the viewport centre by
hand (`(WIN - VIEWPORT) / 2 + VIEWPORT / 2`); the path-based form
collapses that into a single RPC call.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (
    RpcSubprocess,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_snap,
)


WIN_W = 360
WIN_H = 320


def body() -> None:
    with RpcSubprocess("hello-listbox") as listbox:
        before = listbox.snapshot(source="paint", viewport=(WIN_W, WIN_H))
        scroll = find_by_tag(before, "main_list_scroll")
        if scroll is None:
            raise AssertionError("main_list_scroll tag not found in paint snapshot")
        assert_eq(scroll.get("offset_x"), 0, "initial offset_x")
        assert_eq(scroll.get("offset_y"), 0, "initial offset_y")

        listbox.wheel(path="main_list_scroll", lines=(0.0, 3.0))

        def _scrolled(snap):
            node = find_by_tag(snap, "main_list_scroll")
            offset_y = node.get("offset_y") if node is not None else None
            return isinstance(offset_y, (int, float)) and offset_y > 0

        # Gate on the observed post-wheel offset (R883 zero-flake).
        after = wait_snap(
            listbox,
            _scrolled,
            viewport=(WIN_W, WIN_H),
            desc="post-wheel offset_y > 0",
        )
        after_scroll = find_by_tag(after, "main_list_scroll")
        if after_scroll is None:
            raise AssertionError("main_list_scroll tag vanished after wheel")
        # Horizontal axis untouched — wheel was vertical-only.
        assert_eq(after_scroll.get("offset_x"), 0, "post-wheel offset_x")


if __name__ == "__main__":
    sys.exit(run_demo("hello-listbox wheel scroll", body))
