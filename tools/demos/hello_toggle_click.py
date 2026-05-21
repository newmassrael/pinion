#!/usr/bin/env python3
"""hello-toggle click dogfood (§5.49 R59, R51.196 / R51.198).

Closes R51.193's invoke-based dogfood with a real-event variant:
instead of sending the activate cycle via `scene/invoke "/external/send"`,
this demo synthesises a `scene/click` at the toggle track's centre
and observes the same `value: false → true` transition.

The wire path the click exercises:

  scene/click {at: (cx, cy)}
    → DeferredInput::Click enqueue
    → dispatch returns
    → ShellCore drain: cursor_moved(cx, cy) → mouse_pressed → mouse_released
      → InputRouter walks the paint tree, hits the "main_toggle" tag
      → routes PointerEvent::Down + Up into the ToggleExternal
      → SCXML transitions Off → Hover → Pressed → Idle
      → reducer flips value to true

R51.198 §5.49 — `(cx, cy)` is no longer hardcoded. The demo asks for
a paint snapshot, walks the tree for the `main_toggle` tag (the
track `Container`), and clicks the rect's centre. A future layout
tweak that shifts the track moves the click target with it instead
of regressing the dogfood.
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
    node_center,
    run_demo,
)


WIN_W = 360
WIN_H = 220


def body() -> None:
    with RpcSubprocess("hello-toggle") as toggle:
        initial = toggle.query("/external/value")
        assert_eq(initial, False, "initial /external/value")

        snap = toggle.snapshot(source="paint", viewport=(WIN_W, WIN_H))
        track = find_by_tag(snap, "main_toggle")
        if track is None:
            raise AssertionError("main_toggle tag not found in paint snapshot")
        cx, cy = node_center(track)

        toggle.click(at=(cx, cy))
        # The deferred-input drain runs on the dispatcher's return
        # path, so a brief sleep lets the next paint cycle settle
        # before the value query lands.
        time.sleep(0.1)

        after = toggle.query("/external/value")
        assert_eq(after, True, "post-click /external/value")


if __name__ == "__main__":
    sys.exit(run_demo("hello-toggle click activate", body))
