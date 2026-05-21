#!/usr/bin/env python3
"""hello-toggle click dogfood (§5.49 R59, R51.196).

Closes R51.193's invoke-based dogfood with a real-event variant:
instead of sending the activate cycle via `scene/invoke "/external/send"`,
this demo synthesises a `scene/click` at the toggle track's centre
and observes the same `value: false → true` transition.

The wire path the click exercises:

  scene/click {at: (180, 113)}
    → DeferredInput::Click enqueue
    → dispatch returns
    → ShellCore drain: cursor_moved(180, 113) → mouse_pressed → mouse_released
      → InputRouter walks the paint tree, hits the "main_toggle" tag
      → routes PointerEvent::Down + Up into the ToggleExternal
      → SCXML transitions Off → Hover → Pressed → Idle
      → reducer flips value to true

The coordinate is the visual centre of the 64×32 pill in hello-toggle's
360×220 window — a `WIN_W / 2 = 180` horizontal centre plus a
vertical position derived from the flex-column layout (label 18px +
16 gap → track top ≈ 97px, height 32 → centre y ≈ 113). The follow-up
R51.197 carry is to expose leaf-primitive `rect`s in `scene/snapshot`
so this coordinate can be computed from the dump instead of guessed.
"""

from __future__ import annotations

import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import RpcSubprocess, assert_eq, run_demo


TOGGLE_CX = 180
TOGGLE_CY = 113


def body() -> None:
    with RpcSubprocess("hello-toggle") as toggle:
        initial = toggle.query("/external/value")
        assert_eq(initial, False, "initial /external/value")

        toggle.click(at=(TOGGLE_CX, TOGGLE_CY))
        # The deferred-input drain runs on the dispatcher's return
        # path, so a brief sleep lets the next paint cycle settle
        # before the value query lands.
        time.sleep(0.1)

        after = toggle.query("/external/value")
        assert_eq(after, True, "post-click /external/value")


if __name__ == "__main__":
    sys.exit(run_demo("hello-toggle click activate", body))
