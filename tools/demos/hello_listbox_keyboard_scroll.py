#!/usr/bin/env python3
"""hello-listbox keyboard scroll dogfood (§5.49 R59, R51.197).

Mirror of `hello_listbox_scroll.py` but driven through `scene/key`
instead of `scene/wheel`. Exercises the §5.45 R55.C.3 keyboard scroll
arc the substrate added in R51.187: when `V::apply_key` returns
unhandled, the shell falls through to `InputRouter::scroll_key`,
which walks the deepest `Scene::Scroll` under the cursor and applies
`scroll_by` for `ArrowUp/Down/Left/Right` and `PageUp/Down`, or
`scroll_to(_, 0)` / `scroll_to(_, max_y)` for `Home` / `End`.

Sequence:
  1. spawn hello-listbox
  2. snapshot (paint) → assert initial Scroll.offset = (0, 0)
  3. scene/key "PageDown" at the viewport centre
  4. snapshot → assert offset_y > 0

`PageDown` is one full viewport step which always lands a visible
delta regardless of the line-height multiplier, keeping the
assertion robust against future tweaks. The horizontal axis stays
untouched.
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
VIEWPORT_H = 5 * 28 + 4 * 6  # 5 rows + 4 gaps

VIEWPORT_CX = (WIN_W - VIEWPORT_W) // 2 + VIEWPORT_W // 2
VIEWPORT_CY = (WIN_H - VIEWPORT_H) // 2 + VIEWPORT_H // 2


def scroll_node(snap: dict) -> dict:
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

        listbox.key(at=(VIEWPORT_CX, VIEWPORT_CY), name="PageDown")
        time.sleep(0.1)

        after = listbox.snapshot(source="paint", viewport=(WIN_W, WIN_H))
        after_scroll = scroll_node(after)
        after_offset_y = after_scroll.get("offset_y")
        if not isinstance(after_offset_y, (int, float)) or after_offset_y <= 0:
            raise AssertionError(
                f"post-PageDown offset_y: expected > 0, got {after_offset_y!r}"
            )
        assert_eq(after_scroll.get("offset_x"), 0, "post-key offset_x")


if __name__ == "__main__":
    sys.exit(run_demo("hello-listbox keyboard scroll", body))
