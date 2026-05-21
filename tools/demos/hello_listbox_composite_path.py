#!/usr/bin/env python3
"""hello-listbox composite-tag key dispatch (§5.49 R59, R55.G.17).

Closes the R55.G.12 carry — `scene/key {path: "main_list"}` previously
failed because the composite paint root carried no tag matching the
`WidgetCore::tag()` of the listbox (`"main_list"`). R55.G.17 wraps the
`Scroll` in a transparent `Container` tagged `"main_list"`, so the
composite is now paint-addressable for AI-side keyboard injection.

Sequence:
  1. spawn hello-listbox; focused_index starts unset (`None`).
  2. `focus/set` the listbox so `V::apply_key` receives the key.
  3. `scene/key {path: "main_list", key: "ArrowDown"}` — the
     dispatcher walks the paint scene for the composite tag, finds
     the wrapper rect, lands the cursor at its centre, then
     `handle_named_key` routes the key through the focused
     composite's `V::apply_key`.
  4. The listbox's `move_focus(+1)` arm runs: `None → 0` per the
     W3C ARIA Listbox first-Arrow boundary. Assert
     `focused_index == 0`.
  5. A second `ArrowDown` advances `0 → 1`. Assert `focused_index == 1`.

Together (4) + (5) prove both the boundary (first-press) and the
steady-state (subsequent press) arms reach the composite through the
new composite-tag path — closing the carry's "paint-mode 와 align"
gap with no framework changes (R55.G.17 is an app-layer fix; the
wrapper convention is the application contract for composites that
want to be addressable via their `WidgetCore::tag()`).
"""

from __future__ import annotations

import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import RpcSubprocess, assert_eq, run_demo


def body() -> None:
    with RpcSubprocess("hello-listbox") as listbox:
        initial = listbox.query("/external/focused_index")
        assert_eq(initial, None, "initial focused_index unset")

        listbox.request("focus/set", {"tag": "main_list"})
        time.sleep(0.05)

        # First ArrowDown via composite path — None → 0 boundary.
        listbox.key(path="main_list", name="ArrowDown")
        time.sleep(0.1)
        assert_eq(
            listbox.query("/external/focused_index"),
            0,
            "first ArrowDown via composite path lands focused_index=0",
        )

        # Second ArrowDown via composite path — 0 → 1 step.
        listbox.key(path="main_list", name="ArrowDown")
        time.sleep(0.1)
        assert_eq(
            listbox.query("/external/focused_index"),
            1,
            "second ArrowDown via composite path advances focused_index=1",
        )


if __name__ == "__main__":
    sys.exit(run_demo("hello-listbox composite path key", body))
