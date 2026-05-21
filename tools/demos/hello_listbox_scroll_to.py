#!/usr/bin/env python3
"""hello-listbox scroll_to dogfood (§5.49 R59, R55.F §5.45).

Programmatic-scroll counterpart to the wheel / keyboard scroll
demos: `scene/scroll {path, to}` writes directly to the attached
`ScrollState::scroll_to`, bypassing the wheel/key InputRouter
activation arc. Useful for "jump to row N" without simulating
multiple PageDown injections.

Sequence:
  1. spawn hello-listbox
  2. snapshot paint → assert initial Scroll.offset = (0, 0)
  3. scene/scroll `{path: "main_list_scroll", to: {x: 0, y: 100}}`
  4. snapshot paint → assert offset_y == 100
  5. scene/scroll `{path: "main_list_scroll", by: {dx: 0, dy: -50}}`
  6. snapshot paint → assert offset_y == 50

The clamp boundary is tested by sending `to: {y: 9999}`; the
ScrollState saturates against `max_y`. Hello-listbox's content is
12 rows × 34 (incl gap) − 6 gap = 402 intrinsic height, viewport
164, so max_y = 238.
"""

from __future__ import annotations

import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (
    RpcSubprocess,
    assert_eq,
    find_by_tag,
    run_demo,
)


WIN_W = 360
WIN_H = 320
SCROLL_TAG = "main_list_scroll"


def offset_y(listbox: RpcSubprocess) -> int:
    snap = listbox.snapshot(source="paint", viewport=(WIN_W, WIN_H))
    scroll = find_by_tag(snap, SCROLL_TAG)
    if scroll is None:
        raise AssertionError(f"{SCROLL_TAG} not found in paint snapshot")
    value = scroll.get("offset_y")
    if not isinstance(value, int):
        raise AssertionError(f"offset_y not an int: {value!r}")
    return value


def body() -> None:
    with RpcSubprocess("hello-listbox") as listbox:
        assert_eq(offset_y(listbox), 0, "initial offset_y")

        listbox.scroll(SCROLL_TAG, to=(0, 100))
        time.sleep(0.1)
        assert_eq(offset_y(listbox), 100, "post-scroll_to offset_y")

        listbox.scroll(SCROLL_TAG, by=(0, -50))
        time.sleep(0.1)
        assert_eq(offset_y(listbox), 50, "post-scroll_by offset_y")

        # Clamp boundary — request way past max, expect saturate to max.
        listbox.scroll(SCROLL_TAG, to=(0, 9999))
        time.sleep(0.1)
        clamped = offset_y(listbox)
        if clamped <= 50 or clamped > 500:
            raise AssertionError(
                f"clamp-to-max offset_y: expected 50 < n <= max_y, got {clamped}"
            )


if __name__ == "__main__":
    sys.exit(run_demo("hello-listbox scroll_to", body))
