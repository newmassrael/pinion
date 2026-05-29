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

AUTOSAVE = "autosave"
OFFLINE = "offline"
AUTOSAVE_POP = "autosave#pop"
OFFLINE_POP = "offline#pop"

# Fixed control geometry (mirrors the binding consts).
AUTOSAVE_RECT = (40, 64, 170, 44)
OFFLINE_RECT = (372, 296, 120, 44)
# Expected anchored tooltip rects (see the binding's anchor_position
# unit tests): autosave opens flush below; offline flips above + clamps.
AUTOSAVE_TIP_RECT = (40, 108, 210, 28)
OFFLINE_TIP_RECT = (300, 268, 220, 28)

EMPTY = (260, 180)  # a window region over no control (hover-away target)


def _overlay(tf, tag):
    """The paint-scene overlay node for `tag` (None when not shown)."""
    snap = tf.snapshot(source="paint", viewport=VIEWPORT)
    return find_by_tag(snap, tag)


def _rect_tuple(node) -> tuple[int, int, int, int]:
    r = rect_of(node)
    return (int(r["x"]), int(r["y"]), int(r["w"]), int(r["h"]))


def _autosave_slot(tf, slot: str) -> bool:
    return bool(tf.query(f"/external/{slot}"))


def _offline_slot(tf, slot: str) -> bool:
    return bool(tf.query(f"/{OFFLINE}/external/{slot}"))


def _body() -> None:
    with RpcSubprocess("hello-tooltip", boot_grace=1.5) as tf:
        # ── (A) hover trigger + anchored position (autosave, below) ───
        assert_eq(_autosave_slot(tf, "visible"), False, "autosave tooltip boots hidden")
        assert _overlay(tf, AUTOSAVE_POP) is None, "no autosave overlay while hidden"

        tf.hover(path=AUTOSAVE)
        time.sleep(PAUSE)
        assert_eq(_autosave_slot(tf, "visible"), True, "hover shows the autosave tooltip")
        assert_eq(_autosave_slot(tf, "hovered"), True, "autosave control is hovered")
        assert_eq(_autosave_slot(tf, "focused"), False, "hover does not focus")

        node = _overlay(tf, AUTOSAVE_POP)
        assert node is not None, "autosave overlay painted while shown"
        assert_eq(_rect_tuple(node), AUTOSAVE_TIP_RECT, "autosave tooltip flush below")
        # Flush-below contract: overlay top == control bottom (gap 0).
        assert_eq(
            AUTOSAVE_TIP_RECT[1],
            AUTOSAVE_RECT[1] + AUTOSAVE_RECT[3],
            "overlay top touches the control bottom (hoverable contiguity)",
        )

        # ── (B) hoverable: hovering the tooltip body keeps it shown ───
        tf.hover(path=AUTOSAVE_POP)
        time.sleep(PAUSE)
        assert_eq(
            _autosave_slot(tf, "visible"),
            True,
            "hovering the tooltip body keeps it shown (WCAG 1.4.13 hoverable)",
        )
        assert_eq(_autosave_slot(tf, "hovered"), True, "body hover still reads as hovered")

        # ── hover away hides it ───────────────────────────────────────
        tf.hover(at=EMPTY)
        time.sleep(PAUSE)
        assert_eq(_autosave_slot(tf, "visible"), False, "hover-away hides the tooltip")
        assert _overlay(tf, AUTOSAVE_POP) is None, "autosave overlay gone after hover-away"

        # ── (C) focus trigger + dismiss + latch reset ─────────────────
        tf.request("focus/set", {"tag": AUTOSAVE})
        time.sleep(PAUSE)
        assert_eq(
            tf.request("focus/get").result.get("focused"),
            AUTOSAVE,
            "focus/set lands on the autosave control",
        )
        assert_eq(_autosave_slot(tf, "focused"), True, "autosave control reports focus")
        assert_eq(_autosave_slot(tf, "visible"), True, "keyboard focus shows the tooltip")
        assert _overlay(tf, AUTOSAVE_POP) is not None, "focus-shown overlay painted"

        # dismiss while focus stays (WCAG dismissible) — RPC action
        # channel, the same funnel the shell's Escape key uses.
        tf.invoke("/external/dismiss", None)
        time.sleep(PAUSE)
        assert_eq(_autosave_slot(tf, "dismissed"), True, "dismiss latch set")
        assert_eq(_autosave_slot(tf, "visible"), False, "dismiss hides while still focused")
        assert _overlay(tf, AUTOSAVE_POP) is None, "dismissed overlay not painted"
        assert_eq(_autosave_slot(tf, "focused"), True, "focus unchanged by dismiss")

        # blur clears the latch so a later focus re-shows it.
        tf.request("focus/set", {"tag": OFFLINE})
        time.sleep(PAUSE)
        assert_eq(_autosave_slot(tf, "focused"), False, "focus moved off autosave")
        assert_eq(_autosave_slot(tf, "dismissed"), False, "blur clears the dismiss latch")
        tf.request("focus/set", {"tag": AUTOSAVE})
        time.sleep(PAUSE)
        assert_eq(_autosave_slot(tf, "visible"), True, "re-focus after dismiss re-shows")
        # park focus away from autosave for the offline checks.
        tf.request("focus/set", {"tag": OFFLINE})
        time.sleep(PAUSE)

        # ── (D) flip + clamp (offline, low-right) ─────────────────────
        # offline already has focus from the park above -> tooltip shown.
        assert_eq(_offline_slot(tf, "visible"), True, "offline tooltip shown on focus")
        node = _overlay(tf, OFFLINE_POP)
        assert node is not None, "offline overlay painted"
        assert_eq(_rect_tuple(node), OFFLINE_TIP_RECT, "offline tooltip flips + clamps")
        # Flip contract: below would overflow, so the overlay opens ABOVE.
        assert (
            OFFLINE_TIP_RECT[1] + OFFLINE_TIP_RECT[3] <= OFFLINE_RECT[1]
        ), "offline overlay flipped above its control"
        # Clamp contract: the overlay's right edge sits on the viewport edge.
        assert_eq(
            OFFLINE_TIP_RECT[0] + OFFLINE_TIP_RECT[2],
            VIEWPORT[0],
            "offline overlay clamped to the right viewport edge",
        )

        # offline hover trigger also works (extra external, scene/hover).
        tf.request("focus/set", {"tag": AUTOSAVE})  # drop offline focus
        time.sleep(PAUSE)
        assert_eq(_offline_slot(tf, "visible"), False, "offline hidden after blur")
        tf.hover(path=OFFLINE)
        time.sleep(PAUSE)
        assert_eq(_offline_slot(tf, "visible"), True, "hover shows the offline tooltip")
        assert _overlay(tf, OFFLINE_POP) is not None, "hover-shown offline overlay painted"
        tf.hover(at=EMPTY)
        time.sleep(PAUSE)
        assert_eq(_offline_slot(tf, "visible"), False, "hover-away hides offline")

        # ── (E) Escape key dismiss (the real-keyboard funnel) ─────────
        # `scene/key "Escape"` routes through the shell's apply_key (the
        # same funnel a winit Escape now takes), which the binding maps
        # to the tooltip's WCAG dismiss — without moving hover. Park
        # focus off autosave first so the dismiss latch's falling edge is
        # the hover-leave (not gated by a lingering focus episode).
        tf.request("focus/set", {"tag": OFFLINE})
        time.sleep(PAUSE)
        assert_eq(_autosave_slot(tf, "focused"), False, "autosave unfocused for the Escape test")
        tf.hover(path=AUTOSAVE)
        time.sleep(PAUSE)
        assert_eq(_autosave_slot(tf, "visible"), True, "hover re-shows autosave for Escape test")
        tf.key(path=AUTOSAVE, name="Escape")
        time.sleep(PAUSE)
        assert_eq(_autosave_slot(tf, "dismissed"), True, "Escape sets the dismiss latch")
        assert_eq(_autosave_slot(tf, "visible"), False, "Escape dismisses while still hovered")
        tf.hover(at=EMPTY)
        time.sleep(PAUSE)
        assert_eq(_autosave_slot(tf, "dismissed"), False, "hover-away clears latch after Escape")


if __name__ == "__main__":
    sys.exit(run_demo("r695_tooltip", _body))
