#!/usr/bin/env python3
"""R719 §5.35 §5.49 — `scene/pointer_leave` RPC primitive end-to-end.

Verifies the previously-missing input peer to `scene/hover`: a human can
move the pointer *off* the window (winit `WindowEvent::CursorLeft`),
which drops the cursor and rolls back any in-flight `Hover`. Until R719
`scene/hover` (winit `CursorMoved`) had a peer but `CursorLeft` did not,
violating §2 invariant #2 ("every input a human makes must have an RPC
peer"). `scene/pointer_leave` closes that gap.

Distinct from `scene/hover {at: <empty region>}`: hovering an empty
in-window point also clears the hover target, but `pointer_leave` is
positionless — the canonical way to reach the no-pointer-over-window
state without choosing an in-window coordinate. That is exactly the
deterministic baseline the headless harness needs at boot, because a
real X / Wayland server maps the test window wherever the WM places it
(often under the host's physical cursor), delivering a genuine boot
`CursorMoved` that leaves a widget reporting `Hover` before any RPC ran.

Atomic verification scope (>=30 assertions):

  Tooltip (hover-primary descriptive widget, R695):
    (A) hover shows the tooltip; `pointer_leave` clears hover and hides
        it; the overlay node leaves the paint scene.
    (B) `pointer_leave` is idempotent — a second call keeps it cleared.
    (C) pointer-only: with a control keyboard-focused, `pointer_leave`
        clears hover but NOT focus, so a focus-shown tooltip stays
        visible (the hover/focus channels are orthogonal).
    (D) window-scoped: a single `pointer_leave` clears hover regardless
        of which control (primary or extra external) was hovered.

  Accordion (the regression widget, R697):
    (E) hovering a section header drives it Idle -> Hover; `pointer_leave`
        returns it to Idle without touching the per-section `expanded`
        sidecar (hover and activation are independent channels).

Run from the workspace root.
"""

from __future__ import annotations

import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    find_by_tag,
    run_demo,
)

TIP_VIEWPORT = (520, 360)
ACC_VIEWPORT = (420, 440)
PAUSE = 0.12

AUTOSAVE = "autosave"
OFFLINE = "offline"
AUTOSAVE_POP = "autosave#pop"
OFFLINE_POP = "offline#pop"


def _autosave(tf, slot: str) -> bool:
    return bool(tf.query(f"/external/{slot}"))


def _offline(tf, slot: str) -> bool:
    return bool(tf.query(f"/{OFFLINE}/external/{slot}"))


def _overlay(tf, tag, viewport):
    snap = tf.snapshot(source="paint", viewport=viewport)
    return find_by_tag(snap, tag)


def _tooltip_part() -> None:
    with RpcSubprocess("hello-tooltip", boot_grace=1.5) as tf:
        # The harness boot baseline (auto `pointer_leave`) already ran,
        # so a window mapped under the host cursor still boots un-hovered.
        assert_eq(_autosave(tf, "visible"), False, "autosave tooltip boots hidden")
        assert_eq(_autosave(tf, "hovered"), False, "autosave boots un-hovered")

        # ── (A) hover shows it; pointer_leave clears hover + hides it ──
        tf.hover(path=AUTOSAVE)
        time.sleep(PAUSE)
        assert_eq(_autosave(tf, "hovered"), True, "hover marks autosave hovered")
        assert_eq(_autosave(tf, "visible"), True, "hover shows the autosave tooltip")
        assert _overlay(tf, AUTOSAVE_POP, TIP_VIEWPORT) is not None, "overlay painted while shown"

        tf.pointer_leave()
        time.sleep(PAUSE)
        assert_eq(_autosave(tf, "hovered"), False, "pointer_leave clears the hover")
        assert_eq(_autosave(tf, "visible"), False, "pointer_leave hides the hover tooltip")
        assert _overlay(tf, AUTOSAVE_POP, TIP_VIEWPORT) is None, "overlay gone after pointer_leave"

        # ── (B) idempotent: a second pointer_leave stays cleared ──────
        tf.pointer_leave()
        time.sleep(PAUSE)
        assert_eq(_autosave(tf, "hovered"), False, "second pointer_leave keeps hover clear")
        assert_eq(_autosave(tf, "visible"), False, "second pointer_leave keeps it hidden")

        # ── (C) pointer-only: focus survives pointer_leave ────────────
        tf.request("focus/set", {"tag": AUTOSAVE})
        time.sleep(PAUSE)
        assert_eq(_autosave(tf, "focused"), True, "focus/set focuses autosave")
        assert_eq(_autosave(tf, "visible"), True, "keyboard focus shows the tooltip")
        # now ALSO hover it, then leave: hover clears, focus must remain.
        tf.hover(path=AUTOSAVE)
        time.sleep(PAUSE)
        assert_eq(_autosave(tf, "hovered"), True, "control hovered while focused")
        tf.pointer_leave()
        time.sleep(PAUSE)
        assert_eq(_autosave(tf, "hovered"), False, "pointer_leave clears hover")
        assert_eq(_autosave(tf, "focused"), True, "pointer_leave leaves focus untouched")
        assert_eq(
            _autosave(tf, "visible"),
            True,
            "focus-shown tooltip stays visible after pointer_leave (hover/focus orthogonal)",
        )
        assert _overlay(tf, AUTOSAVE_POP, TIP_VIEWPORT) is not None, "focus overlay still painted"

        # ── (D) window-scoped: clears the EXTRA external's hover too ──
        # autosave still holds focus from (C) and nothing is hovered, so
        # the extra external (offline) is both unfocused and unhovered —
        # a clean, no-juggling start.
        assert_eq(_offline(tf, "visible"), False, "offline hidden while unfocused + unhovered")
        tf.hover(path=OFFLINE)
        time.sleep(PAUSE)
        assert_eq(_offline(tf, "hovered"), True, "extra external hovered")
        assert_eq(_offline(tf, "visible"), True, "hover shows the offline tooltip")
        tf.pointer_leave()
        time.sleep(PAUSE)
        assert_eq(_offline(tf, "hovered"), False, "pointer_leave clears extra-external hover")
        assert_eq(_offline(tf, "visible"), False, "pointer_leave hides the offline tooltip")
        assert _overlay(tf, OFFLINE_POP, TIP_VIEWPORT) is None, "offline overlay gone"
        # the leave is pointer-only: autosave's focus-held tooltip stands.
        assert_eq(_autosave(tf, "focused"), True, "autosave focus survives the offline hover/leave")


def _accordion_part() -> None:
    sec = [f"accordion_sec_{i}" for i in range(3)]

    def state(d, i: int) -> str:
        return d.query(f"/{sec[i]}/external/state")

    def expanded(d, i: int) -> bool:
        return bool(d.query(f"/{sec[i]}/external/expanded"))

    with RpcSubprocess("hello-accordion") as d:
        # ── (E) hover drives Idle -> Hover; pointer_leave -> Idle ─────
        # Boot baseline: all sections Idle regardless of host cursor.
        for i in range(3):
            assert_eq(state(d, i), "Idle", f"sec {i} boots Idle (harness baseline)")

        d.hover(path=sec[1])
        time.sleep(PAUSE)
        assert_eq(state(d, 1), "Hover", "hover drives section 1 Idle -> Hover")
        assert_eq(state(d, 0), "Idle", "section 0 unaffected by hover on 1")
        assert_eq(state(d, 2), "Idle", "section 2 unaffected by hover on 1")

        tf_expanded_before = [expanded(d, i) for i in range(3)]
        d.pointer_leave()
        time.sleep(PAUSE)
        assert_eq(state(d, 1), "Idle", "pointer_leave returns section 1 to Idle")
        for i in range(3):
            assert_eq(
                expanded(d, i),
                tf_expanded_before[i],
                f"pointer_leave leaves section {i} expanded sidecar untouched",
            )

        # hover a different section, leave again — Hover never sticks.
        d.hover(path=sec[2])
        time.sleep(PAUSE)
        assert_eq(state(d, 2), "Hover", "hover drives section 2 -> Hover")
        d.pointer_leave()
        time.sleep(PAUSE)
        assert_eq(state(d, 2), "Idle", "pointer_leave returns section 2 to Idle")
        assert_eq(state(d, 1), "Idle", "section 1 still Idle (no stuck hover)")


def _body() -> None:
    _tooltip_part()
    _accordion_part()


if __name__ == "__main__":
    sys.exit(run_demo("r719_pointer_leave", _body))
