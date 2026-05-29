#!/usr/bin/env python3
"""R695 §5.16 §5.35 §5.40 §5.50 — Tooltip widget end-to-end.

Verifies the catalog's first descriptive-class widget (WAI-ARIA
`tooltip`, WCAG 2.2 SC 1.4.13 "Content on Hover or Focus") entirely over
JSON-RPC — the AI-first path (§2 invariant #2). Two trigger buttons
(`save` = primary external, `delete` = extra external) each own a
`TooltipExternal`; a tooltip shows while its trigger is hovered or
keyboard-focused and hides once both clear.

The round also lands the `scene/hover` RPC primitive (the pointer-
position-only peer to `scene/click`) — the tooltip's primary trigger,
and the previously-missing input peer to click / drag / wheel / key.

Atomic verification scope (>=30 assertions):

  Hover trigger + anchored position (save, opens below):
    (A) boots hidden; hover shows it; the overlay paints flush below
        the trigger at the anchored (left, top); hover-away hides it.
  Hoverable (WCAG 1.4.13):
    (B) hovering the tooltip body keeps it shown (shared-tag contract).
  Focus trigger + dismiss (WCAG 1.4.13):
    (C) focus shows it; the `dismiss` invoke hides it while focus stays;
        the latch clears on blur so re-focus re-shows it.
  Flip + clamp (delete, low + right):
    (D) the overlay flips *above* its trigger and clamps to the right
        viewport edge.

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
    rect_of,
    run_demo,
)

VIEWPORT = (520, 360)
PAUSE = 0.12

SAVE = "save"
DELETE = "delete"
SAVE_POP = "save#pop"
DELETE_POP = "delete#pop"

# Fixed trigger geometry (mirrors the binding consts).
SAVE_RECT = (40, 64, 170, 44)
DELETE_RECT = (372, 296, 120, 44)
# Expected anchored tooltip rects (see the binding's anchor_position
# unit tests): save opens flush below; delete flips above + clamps left.
SAVE_TIP_RECT = (40, 108, 210, 28)
DELETE_TIP_RECT = (300, 268, 220, 28)

EMPTY = (260, 180)  # a window region over no widget (hover-away target)


def _overlay(tf, tag):
    """The paint-scene overlay node for `tag` (None when not shown)."""
    snap = tf.snapshot(source="paint", viewport=VIEWPORT)
    return find_by_tag(snap, tag)


def _rect_tuple(node) -> tuple[int, int, int, int]:
    r = rect_of(node)
    return (int(r["x"]), int(r["y"]), int(r["w"]), int(r["h"]))


def _save_slot(tf, slot: str) -> bool:
    return bool(tf.query(f"/external/{slot}"))


def _delete_slot(tf, slot: str) -> bool:
    return bool(tf.query(f"/{DELETE}/external/{slot}"))


def _body() -> None:
    with RpcSubprocess("hello-tooltip", boot_grace=1.5) as tf:
        # ── (A) hover trigger + anchored position (save, below) ───────
        assert_eq(_save_slot(tf, "visible"), False, "save tooltip boots hidden")
        assert _overlay(tf, SAVE_POP) is None, "no save overlay painted while hidden"

        tf.hover(path=SAVE)
        time.sleep(PAUSE)
        assert_eq(_save_slot(tf, "visible"), True, "hover shows the save tooltip")
        assert_eq(_save_slot(tf, "hovered"), True, "save trigger is hovered")
        assert_eq(_save_slot(tf, "focused"), False, "hover does not focus")

        node = _overlay(tf, SAVE_POP)
        assert node is not None, "save overlay painted while shown"
        assert_eq(_rect_tuple(node), SAVE_TIP_RECT, "save tooltip anchored flush below")
        # Flush-below contract: overlay top == trigger bottom (gap 0).
        assert_eq(
            SAVE_TIP_RECT[1],
            SAVE_RECT[1] + SAVE_RECT[3],
            "save overlay top touches the trigger bottom (hoverable contiguity)",
        )

        # ── (B) hoverable: hovering the tooltip body keeps it shown ───
        tf.hover(path=SAVE_POP)
        time.sleep(PAUSE)
        assert_eq(
            _save_slot(tf, "visible"),
            True,
            "hovering the tooltip body keeps it shown (WCAG 1.4.13 hoverable)",
        )
        assert_eq(_save_slot(tf, "hovered"), True, "body hover still reads as hovered")

        # ── hover away hides it ───────────────────────────────────────
        tf.hover(at=EMPTY)
        time.sleep(PAUSE)
        assert_eq(_save_slot(tf, "visible"), False, "hover-away hides the tooltip")
        assert _overlay(tf, SAVE_POP) is None, "save overlay gone after hover-away"

        # ── (C) focus trigger + dismiss + latch reset ─────────────────
        tf.request("focus/set", {"tag": SAVE})
        time.sleep(PAUSE)
        assert_eq(
            tf.request("focus/get").result.get("focused"),
            SAVE,
            "focus/set lands on the save trigger",
        )
        assert_eq(_save_slot(tf, "focused"), True, "save trigger reports focus")
        assert_eq(_save_slot(tf, "visible"), True, "keyboard focus shows the tooltip")
        assert _overlay(tf, SAVE_POP) is not None, "focus-shown overlay painted"

        # dismiss while focus stays (WCAG dismissible) — RPC action
        # channel, the same funnel the shell's Escape key uses.
        tf.invoke("/external/dismiss", None)
        time.sleep(PAUSE)
        assert_eq(_save_slot(tf, "dismissed"), True, "dismiss latch set")
        assert_eq(_save_slot(tf, "visible"), False, "dismiss hides while still focused")
        assert _overlay(tf, SAVE_POP) is None, "dismissed overlay not painted"
        assert_eq(_save_slot(tf, "focused"), True, "focus unchanged by dismiss")

        # blur clears the latch so a later focus re-shows it.
        tf.request("focus/set", {"tag": DELETE})
        time.sleep(PAUSE)
        assert_eq(_save_slot(tf, "focused"), False, "focus moved off save")
        assert_eq(_save_slot(tf, "dismissed"), False, "blur clears the dismiss latch")
        tf.request("focus/set", {"tag": SAVE})
        time.sleep(PAUSE)
        assert_eq(_save_slot(tf, "visible"), True, "re-focus after dismiss re-shows")
        # park focus away from save for the delete checks.
        tf.request("focus/set", {"tag": DELETE})
        time.sleep(PAUSE)

        # ── (D) flip + clamp (delete, low-right) ──────────────────────
        # delete already has focus from the park above -> tooltip shown.
        assert_eq(_delete_slot(tf, "visible"), True, "delete tooltip shown on focus")
        node = _overlay(tf, DELETE_POP)
        assert node is not None, "delete overlay painted"
        assert_eq(_rect_tuple(node), DELETE_TIP_RECT, "delete tooltip flips + clamps")
        # Flip contract: below would overflow, so the overlay opens ABOVE.
        assert (
            DELETE_TIP_RECT[1] + DELETE_TIP_RECT[3] <= DELETE_RECT[1]
        ), "delete overlay flipped above its trigger"
        # Clamp contract: the overlay's right edge sits on the viewport edge.
        assert_eq(
            DELETE_TIP_RECT[0] + DELETE_TIP_RECT[2],
            VIEWPORT[0],
            "delete overlay clamped to the right viewport edge",
        )

        # delete hover trigger also works (extra external, scene/hover).
        tf.request("focus/set", {"tag": SAVE})  # drop delete focus
        time.sleep(PAUSE)
        assert_eq(_delete_slot(tf, "visible"), False, "delete hidden after blur")
        tf.hover(path=DELETE)
        time.sleep(PAUSE)
        assert_eq(_delete_slot(tf, "visible"), True, "hover shows the delete tooltip")
        assert _overlay(tf, DELETE_POP) is not None, "hover-shown delete overlay painted"
        tf.hover(at=EMPTY)
        time.sleep(PAUSE)
        assert_eq(_delete_slot(tf, "visible"), False, "hover-away hides delete")

        # ── (E) Escape key dismiss (the real-keyboard funnel) ─────────
        # `scene/key "Escape"` routes through the shell's apply_key (the
        # same funnel a winit Escape now takes), which the binding maps
        # to the tooltip's WCAG dismiss — without moving hover. Park
        # focus off save first so the dismiss latch's falling edge is
        # the hover-leave (not gated by a lingering focus episode).
        tf.request("focus/set", {"tag": DELETE})
        time.sleep(PAUSE)
        assert_eq(_save_slot(tf, "focused"), False, "save unfocused for the Escape test")
        tf.hover(path=SAVE)
        time.sleep(PAUSE)
        assert_eq(_save_slot(tf, "visible"), True, "hover re-shows save for the Escape test")
        tf.key(path=SAVE, name="Escape")
        time.sleep(PAUSE)
        assert_eq(_save_slot(tf, "dismissed"), True, "Escape sets the dismiss latch")
        assert_eq(_save_slot(tf, "visible"), False, "Escape dismisses while still hovered")
        tf.hover(at=EMPTY)
        time.sleep(PAUSE)
        assert_eq(_save_slot(tf, "dismissed"), False, "hover-away clears the latch after Escape")


if __name__ == "__main__":
    sys.exit(run_demo("r695_tooltip", _body))
