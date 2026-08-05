#!/usr/bin/env python3
"""R1570 §5.16 §5.12 — a declared `#[widget]` forward reaches a real body.

`#[widget(keybinding)]` generates `fn keybinding(key) { <TheView>::keybinding(key) }`
inside the very trait impl that defines `keybinding`. When the view declares an
inherent function of that name, that one wins and the generated line is a
bridge. When it does not, the call resolves back to the trait method it is
standing inside and the forward calls itself — unconditionally, forever. Release
mode makes the self-call a tail call, so it is a bare jump: 100% CPU, no stack
growth, no syscall, no return. Nothing in the toolchain objects; measured at
R1570, even `#[deny(unconditional_recursion)]` on the generated body compiles
silently.

Four shipped bindings were in exactly that state — `hello-richtext-background`,
`-blocks`, `-list` and `-cells`, each a copy of `hello-richtext`'s attribute
without its three method bodies, so `keybinding`, `event_name` AND `apply_key`
were each an infinite self-call. It stayed invisible because nothing called
them: their demos never typed a character. R1569 gave `WidgetCore::keybinding` a
caller on the RPC read path, and all four stopped answering any request at all.

R1570 makes the declaration enforceable: each forward brings
`pinion_core::widget_forward`'s guard for that one name into scope, so a missing
inherent function is a second applicable candidate and rustc rejects it (E0034)
at the `#[widget(...)]` attribute. That half is a compile-time gate and cannot
be asserted from here. What CAN be asserted from here — and is, because a gate
whose subject is never exercised is a gate nobody has watched work — is that the
declaration now MEANS something at run time.

The probe is `scene/accelerators` (R1569): it answers by *calling*
`WidgetCore::keybinding` over printable ASCII. A row for a character therefore
proves the forward resolved to the binding's own function and returned — which
is precisely what the defect could not do.

What this asserts, over the wire, of all FIVE members of the family:

  * the read ANSWERS AT ALL. This is the CI symptom itself: before R1570 four
    of these five never replied to any request, because assembling any
    window-scoped read calls `keybinding`. No wall-clock threshold is used —
    a hang is a hang and the harness's own request budget catches it;
  * `d` and `e` are published as `keybinding` accelerators, with the exact row
    shape R1569 declared;
  * `d` REACHES THE STATECHART (the switch goes `Disabled`), which traverses
    two forwards in one keystroke: `keybinding` to name the event, then
    `event_name` to spell it for the SCXML external;
  * `e` brings it back, so the first result is a transition rather than a
    terminal state anything could have fallen into;
  * a key the binding does NOT map reaches `apply_key` — the third forward,
    which the shell consults exactly when `keybinding` declines — and the
    binding is STILL ANSWERING afterwards. That liveness is the whole
    assertion and it is not indirect: before R1570 this one keystroke put all
    four of these processes into the same never-returning loop;
  * and it changed nothing, which separates "the forward answered" from "the
    forward answers everything" — a stub claiming every key would satisfy
    every assertion above.

Not asserted, and why: `apply_key`'s own ARIA activation (Space / Enter on the
focused switch) cannot be driven from here, because `main_toggle` is not a focus
stop in any of the five — `focus/set` refuses it as `tag_not_focusable`, and
`apply_aria_activate` gates on the focused tag. That is a real gap for a
`role = Switch` and it is a pre-existing one, on the axis
[[debt-a11y-focus-stop-absent-from-tree]] already tracks; this round does not
widen its scope to five bindings' focus declarations. The forward itself is
reached and proven regardless, by the path above.

`hello-richtext` is in the list as the NEGATIVE CONTROL: it is the binding the
other four were copied from, it always had the three bodies, and it is asserted
by the same code. If these assertions could pass without the fix, they would
pass for it and for the others alike — and before R1570 they did not.

ZERO-FLAKE: bounded `wait_until` polling (never a fixed sleep). >=30 assertions.

Run from the workspace root:
    cargo build --release -p hello-richtext -p hello-richtext-background \
        -p hello-richtext-blocks -p hello-richtext-list -p hello-richtext-cells
    python3 tools/demos/r1570_declared_forward_reaches_its_body.py
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_until,
)

TOGGLE = "main_toggle"

# The binding whose three method bodies the other four dropped, first in the
# list so a failure reads as "the control broke" before "the fix broke".
CONTROL = "hello-richtext"

# The four that declared `apply_key` / `event_name` / `keybinding` and defined
# none of them. rustc's own census at R1570 found these and only these.
REPAIRED = [
    "hello-richtext-background",
    "hello-richtext-blocks",
    "hello-richtext-list",
    "hello-richtext-cells",
]

FAMILY = [CONTROL, *REPAIRED]

# R1569's published row shape. Restated here rather than imported so that a
# change to it is a decision made twice.
ROW_KEYS = {"accel", "layer", "target", "label", "shadowed", "shadowed_by"}

# The map every member of the family declares.
MAPPED = ["d", "e"]


def accelerators(tf: RpcSubprocess) -> dict[str, Any]:
    return tf.request("scene/accelerators", {}).result


def keybinding_rows(tf: RpcSubprocess) -> list[dict[str, Any]]:
    return [r for r in accelerators(tf)["accelerators"] if r["layer"] == "keybinding"]


def toggle_state(tf: RpcSubprocess) -> tuple[str, bool]:
    """The switch's `(scxml state, value bit)`, read the way its view does."""
    node = find_by_tag(tf.snapshot(), TOGGLE)
    assert node is not None, "the toggle External is in the state scene"
    intro = node["introspect"]
    return intro["state"], intro["value"]


def press(tf: RpcSubprocess, name: str) -> None:
    tf.key(path=TOGGLE, name=name)


def exercise(example: str) -> int:
    """Every assertion this demo makes, made identically of one binding.

    One body for all five is deliberate: the control and the four repaired
    bindings are held to the same statements by the same code, so a fix that
    only half-worked cannot look like a passing control.
    """
    checks = 0
    with RpcSubprocess(example, boot_grace=1.5) as tf:
        # ── (A) it answers at all ──────────────────────────────────────────
        # THE regression, stated first: assembling any window-scoped read calls
        # `WidgetCore::keybinding`, so before R1570 this request never returned
        # for four of these five and the process sat at 100% CPU forever. The
        # assertion is the reply's existence — `wait_until` bounds it without
        # any threshold that could read the host's speed.
        wait_until(
            lambda: len(keybinding_rows(tf)) == len(MAPPED),
            timeout=5.0,
            interval=0.03,
            desc=f"{example}: the accelerator read answers, and calling "
            f"`keybinding` is what produced its rows",
        )
        checks += 1

        pub = accelerators(tf)
        assert_eq(
            pub["probed"],
            "U+0020..=U+007E",
            f"{example}: A the read states the domain it probed",
        )
        checks += 1
        rows = [r for r in pub["accelerators"] if r["layer"] == "keybinding"]
        assert_eq(
            sorted(r["accel"] for r in rows),
            MAPPED,
            f"{example}: A the binding's own character map is what came back",
        )
        checks += 1
        assert_eq(
            set(rows[0].keys()),
            ROW_KEYS,
            f"{example}: A one row's exact key set",
        )
        checks += 1
        assert_eq(
            [r["target"] for r in rows],
            ["", ""],
            f"{example}: A a keybinding maps to an event, so it names no node",
        )
        checks += 1

        # ── (B) `d` reaches the statechart ─────────────────────────────────
        # Two forwards in one keystroke: `keybinding` turns the character into
        # `ToggleEvent::Disable`, then `event_name` spells it for the SCXML
        # external. A state change is the only observation that requires BOTH
        # to have resolved to real code.
        assert_eq(toggle_state(tf)[0], "Idle", f"{example}: B the switch starts idle")
        checks += 1
        press(tf, "d")
        wait_until(
            lambda: toggle_state(tf)[0] == "Disabled",
            timeout=4.0,
            interval=0.03,
            desc=f"{example}: `d` reached the statechart through two forwards",
        )
        checks += 1

        # ── (C) and `e` brings it back ─────────────────────────────────────
        # Without this, (B) could be satisfied by any path that lands in an
        # absorbing state; a round trip cannot.
        press(tf, "e")
        wait_until(
            lambda: toggle_state(tf)[0] == "Idle",
            timeout=4.0,
            interval=0.03,
            desc=f"{example}: `e` is the inverse transition, so `d` was one too",
        )
        checks += 1

        # ── (D) an unmapped key reaches `apply_key`, which RETURNS ─────────
        # The shell consults `V::apply_key` exactly when `keybinding` declines,
        # so `z` is the keystroke that enters the third forward. The assertion
        # is liveness on the far side of it: the reads below only happen if the
        # call came back. Before R1570 this single keystroke was terminal for
        # all four repaired bindings.
        settled = toggle_state(tf)
        press(tf, "z")
        assert_eq(
            toggle_state(tf),
            settled,
            f"{example}: D `apply_key` ran and returned, and changed nothing "
            f"for a key the binding does not map",
        )
        checks += 1
        assert_eq(
            sorted(r["accel"] for r in keybinding_rows(tf)),
            MAPPED,
            f"{example}: D the unmapped key is absent from the published map",
        )
        checks += 1

        # ── (E) and the mapped keys still work after it ────────────────────
        # `apply_key` shares the scene with the keybinding path, so this rules
        # out the reading where (D) survived by leaving the widget inert.
        press(tf, "d")
        wait_until(
            lambda: toggle_state(tf)[0] == "Disabled",
            timeout=4.0,
            interval=0.03,
            desc=f"{example}: the mapped map still works after the unmapped key",
        )
        checks += 1
    return checks


def body() -> None:
    total = 0
    for example in FAMILY:
        role = "control" if example == CONTROL else "repaired"
        print(f"[demo] --- {example} ({role}) ---")
        total += exercise(example)
    print(f"[demo] {total} assertions across {len(FAMILY)} bindings")
    assert total >= 30, f"the R660 baseline is 30 assertions; made {total}"


if __name__ == "__main__":
    run_demo("R1570 §5.16 a declared forward reaches a real body", body)
