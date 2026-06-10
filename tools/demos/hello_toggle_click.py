#!/usr/bin/env python3
"""hello-toggle click dogfood (§5.49 R59, R51.196 / R51.201).

Closes R51.193's invoke-based dogfood with a real-event variant:
instead of sending the activate cycle via `scene/invoke "/external/send"`,
this demo synthesises a `scene/click` at the toggle track and observes
the same `value: false → true` transition.

The wire path the click exercises:

  scene/click {path: "main_toggle"}
    → dispatcher walks paint scene → finds tag → rect centre
    → DeferredInput::Click enqueue
    → dispatch returns
    → ShellCore drain: cursor_moved → mouse_pressed → mouse_released
      → InputRouter walks the paint tree, hits the "main_toggle" tag
      → routes PointerEvent::Down + Up into the ToggleExternal
      → SCXML transitions Off → Hover → Pressed → Idle
      → reducer flips value to true

R51.201 §5.49 — the click target is now a tag, not a coordinate.
Pre-R51.201 the demo did `snapshot → find_by_tag → node_center →
click(at=…)`; the path-based shape collapses that into a single
RPC call. The earlier R51.198 hardcoded-(180,113) workaround stays
retired by the same fix.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import RpcSubprocess, assert_eq, run_demo, wait_query


def body() -> None:
    with RpcSubprocess("hello-toggle") as toggle:
        initial = toggle.query("/external/value")
        assert_eq(initial, False, "initial /external/value")

        toggle.click(path="main_toggle")
        wait_query(toggle, "/external/value", True, desc="post-click /external/value")


if __name__ == "__main__":
    sys.exit(run_demo("hello-toggle click activate", body))
