#!/usr/bin/env python3
"""hello-toggle activation dogfood (§5.49 R59, R51.193).

Mirrors the canonical `scene_invoke_full_cycle_on_toggle_external_emits_toggle_intent`
integration test (see `crates/pinion-rpc/src/dispatch.rs`), but drives a
real running `hello-toggle` instance — not a unit-test scene — through
the same stdin / stdout JSON-RPC framing that the AI consumes:

  1. spawn hello-toggle
  2. observe initial value via `scene/query "/external/value"` → false
  3. synthesise the full activate cycle (PointerEnter, PointerDown,
     PointerUp) via three `scene/invoke "/external/send"` calls
  4. observe value again → true

The reference unit test (`scene_invoke_full_cycle_on_toggle_external_emits_toggle_intent`)
asserts an additional `scene/intents` drain row, but a *running* shell
already drains the intent batch every frame as part of `walk_scene_and_drain`
(R51.169) — by the time the demo's `scene/intents` call lands the
batch is empty. The `value: false → true` transition is itself proof
that the reducer fired on a real intent path; the intent row in the
unit test is observable only because the unit scene has no shell.

Exit 0 on every assertion satisfied, non-zero with a typed reason on
failure (so CI / `tools/loop.sh` can short-circuit).
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import RpcSubprocess, assert_eq, run_demo


def body() -> None:
    with RpcSubprocess("hello-toggle") as toggle:
        initial = toggle.query("/external/value")
        assert_eq(initial, False, "initial /external/value")

        for ev in ("PointerEnter", "PointerDown", "PointerUp"):
            toggle.invoke("/external/send", ev)

        after = toggle.query("/external/value")
        assert_eq(after, True, "post-activate /external/value")


if __name__ == "__main__":
    sys.exit(run_demo("hello-toggle activate cycle", body))
