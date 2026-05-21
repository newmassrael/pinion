#!/usr/bin/env python3
"""hello-listbox keyboard scroll dogfood (§5.49 R59, R51.197 / R51.202).

Mirror of `hello_listbox_scroll.py` but driven through `scene/key`
instead of `scene/wheel`. Exercises the §5.45 R55.C.3 keyboard scroll
arc the substrate added in R51.187: when `V::apply_key` returns
unhandled, the shell falls through to `InputRouter::scroll_key`,
which walks the deepest `Scene::Scroll` under the cursor and applies
`scroll_by` for `ArrowUp/Down/Left/Right` and `PageUp/Down`, or
`scroll_to(_, 0)` / `scroll_to(_, max_y)` for `Home` / `End`.

Sequence:
  1. spawn hello-listbox
  2. snapshot (paint) → find `main_list_scroll`, assert offset = (0, 0)
  3. scene/key `{path: "main_list_scroll", key: "PageDown"}`
  4. snapshot → assert offset_y > 0

`PageDown` is one full viewport step which always lands a visible
delta regardless of the line-height multiplier, keeping the
assertion robust against future tweaks. The horizontal axis stays
untouched.

R51.202 §5.49 — key target is now the `main_list_scroll` tag, not
a coordinate. Pre-R51.202 the demo hand-rolled the viewport centre.
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


def body() -> None:
    with RpcSubprocess("hello-listbox") as listbox:
        before = listbox.snapshot(source="paint", viewport=(WIN_W, WIN_H))
        scroll = find_by_tag(before, "main_list_scroll")
        if scroll is None:
            raise AssertionError("main_list_scroll tag not found in paint snapshot")
        assert_eq(scroll.get("offset_x"), 0, "initial offset_x")
        assert_eq(scroll.get("offset_y"), 0, "initial offset_y")

        listbox.key(path="main_list_scroll", name="PageDown")
        time.sleep(0.1)

        after = listbox.snapshot(source="paint", viewport=(WIN_W, WIN_H))
        after_scroll = find_by_tag(after, "main_list_scroll")
        if after_scroll is None:
            raise AssertionError("main_list_scroll tag vanished after key")
        after_offset_y = after_scroll.get("offset_y")
        if not isinstance(after_offset_y, (int, float)) or after_offset_y <= 0:
            raise AssertionError(
                f"post-PageDown offset_y: expected > 0, got {after_offset_y!r}"
            )
        assert_eq(after_scroll.get("offset_x"), 0, "post-key offset_x")


if __name__ == "__main__":
    sys.exit(run_demo("hello-listbox keyboard scroll", body))
