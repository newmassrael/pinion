#!/usr/bin/env python3
"""R641 §5.16 — hello-button #[widget] retrofit RPC parity dogfood.

R641 moved the WidgetCore + WidgetA11y + WidgetView mechanical wiring
out of `hello-button/src/main.rs` and into the
[`#[pinion_derive::widget]`](pinion_derive::widget) attribute. The
substrate change is invisible at the §5.15 introspect surface — the
emitted trait impls forward to the same inherent fns that the
hand-written impls used to call, so AI clients observe bit-identical
behaviour.

This demo dogfoods that invariant directly:

  1. spawn hello-button
  2. baseline `scene/query "/external/state"` → `"Idle"`
  3. full pointer activation cycle via `scene/invoke "/external/send"`:
        PointerEnter → Hover
        PointerDown  → Pressed
        PointerUp    → Hover (Material 3 click — `Button::detect`
                              emits a `"click"` intent on this edge)
        PointerLeave → Idle
  4. lifecycle: Disable → Disabled, Enable → Idle

Same shape `design_button_m3_r640.py` runs on design-button-m3
binding — both wire the same `ButtonExternal` substrate, so the wire
contract for state names + events is identical.

The `#[widget(..., keybinding)]` opt-in flag is verified at compile
time (the macro emits the trait impl method; absence would be a type
error). RPC-side keybinding dispatch is NOT covered here because
`scene/key` always routes through `handle_named_key` — character-key
mappings consulted via `WidgetCore::keybinding` only fire when the
shell observes a winit `Key::Character` event from a real keyboard
press, not from synthesised RPC injection. Wiring a `scene/key`
character-vs-named arm is its own substrate gap (R660+ candidate).
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import RpcSubprocess, assert_eq, run_demo


def body() -> None:
    with RpcSubprocess("hello-button") as btn:
        initial = btn.query("/external/state")
        assert_eq(initial, "Idle", "initial /external/state")

        # ── Idle → Hover → Pressed → Hover (activate) → Idle ──────
        btn.invoke("/external/send", "PointerEnter")
        assert_eq(
            btn.query("/external/state"),
            "Hover",
            "/external/state after PointerEnter",
        )

        btn.invoke("/external/send", "PointerDown")
        assert_eq(
            btn.query("/external/state"),
            "Pressed",
            "/external/state after PointerDown",
        )

        btn.invoke("/external/send", "PointerUp")
        assert_eq(
            btn.query("/external/state"),
            "Hover",
            "/external/state after PointerUp (click resolves to Hover)",
        )

        btn.invoke("/external/send", "PointerLeave")
        assert_eq(
            btn.query("/external/state"),
            "Idle",
            "/external/state after PointerLeave",
        )

        # ── Disable / Enable lifecycle via direct invoke ─────────
        btn.invoke("/external/send", "Disable")
        assert_eq(
            btn.query("/external/state"),
            "Disabled",
            "/external/state after Disable invoke",
        )

        btn.invoke("/external/send", "Enable")
        assert_eq(
            btn.query("/external/state"),
            "Idle",
            "/external/state after Enable invoke",
        )


if __name__ == "__main__":
    sys.exit(run_demo("hello-button R641 #[widget] retrofit parity", body))
