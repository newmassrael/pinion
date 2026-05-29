#!/usr/bin/env python3
"""R696 §5.38 §5.50 — Disclosure (show/hide) widget end-to-end.

Drives a real running `hello-disclosure` over JSON-RPC — the AI-first
path (§2 #2) — to verify the WAI-ARIA APG disclosure pattern:

  * the header is a `button` whose activation toggles `aria-expanded`
    (the R696 `AccessState::expanded` axis, exposed on the introspect
    `expanded` slot — distinct from a checkbox's `checked` value);
  * the content panel (`section_body`) is present in the paint scene
    only while expanded;
  * pointer (invoke arc + real `scene/click`), keyboard (Space AND
    Enter, the disclosure keyboard model), and `scene/intervene`
    state-writes all drive the same `expanded` sidecar;
  * the disabled gate swallows both pointer and keyboard activation.

Exit 0 on every assertion satisfied, non-zero with a typed reason on
failure (so CI / `tools/loop.sh` can short-circuit).
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (
    RpcError,
    RpcSubprocess,
    assert_eq,
    find_by_tag,
    rect_of,
    run_demo,
)

TAG = "main_disclosure"
BODY = "section_body"
VIEWPORT = (420, 320)


def _state(d) -> str:
    return d.query("/external/state")


def _expanded(d) -> bool:
    return d.query("/external/expanded")


def _body_node(d):
    snap = d.snapshot(source="paint", viewport=VIEWPORT)
    return find_by_tag(snap, BODY)


def body() -> None:
    with RpcSubprocess("hello-disclosure") as d:
        # ── (A) initial: collapsed, Idle, no panel body in paint ──────
        assert_eq(_state(d), "Idle", "initial state")
        assert_eq(_expanded(d), False, "initial expanded")
        assert_eq(_body_node(d), None, "collapsed: panel body absent from paint")
        # The header (tag) is always present, expanded or not.
        head = find_by_tag(d.snapshot(source="paint", viewport=VIEWPORT), TAG)
        assert head is not None, "header tag present in paint scene"

        # ── (B) pointer activate via the invoke send arc ──────────────
        assert_eq(d.invoke("/external/send", "PointerEnter"), "Hover", "Enter -> Hover")
        assert_eq(d.invoke("/external/send", "PointerDown"), "Pressed", "Down -> Pressed")
        assert_eq(d.invoke("/external/send", "PointerUp"), "Hover", "Up -> Hover")
        assert_eq(_expanded(d), True, "activate expands")
        assert_eq(_state(d), "Hover", "post-activate state Hover")
        # Panel body now in the paint scene with a positive-height rect.
        body_node = _body_node(d)
        assert body_node is not None, "expanded: panel body present in paint"
        assert rect_of(body_node)["h"] > 0, "panel body has a positive height"

        # ── (C) collapse via a second activate ────────────────────────
        d.invoke("/external/send", "PointerDown")
        assert_eq(d.invoke("/external/send", "PointerUp"), "Hover", "2nd Up -> Hover")
        assert_eq(_expanded(d), False, "second activate collapses")
        assert_eq(_body_node(d), None, "collapsed again: panel body absent")
        assert_eq(d.invoke("/external/send", "PointerLeave"), "Idle", "Leave -> Idle")

        # ── (D) cancel must not toggle (Down then Leave) ──────────────
        d.invoke("/external/send", "PointerEnter")
        d.invoke("/external/send", "PointerDown")
        d.invoke("/external/send", "PointerLeave")
        assert_eq(_expanded(d), False, "press-then-leave does not toggle")
        assert_eq(_state(d), "Idle", "cancel returns to Idle")

        # ── (E) real scene/click arc (path-resolved) ──────────────────
        d.click(path=TAG)
        assert_eq(_expanded(d), True, "scene/click expands")
        d.click(path=TAG)
        assert_eq(_expanded(d), False, "scene/click collapses")
        d.click(path=TAG)
        assert_eq(_expanded(d), True, "scene/click re-expands")
        # leave the widget so focus-driven keyboard tests start clean
        d.invoke("/external/send", "PointerLeave")

        # ── (F) scene/intervene state-write channel ───────────────────
        d.intervene("/external/expanded", False)
        assert_eq(_expanded(d), False, "intervene writes expanded=false")
        d.intervene("/external/expanded", True)
        assert_eq(_expanded(d), True, "intervene writes expanded=true")
        d.intervene("/external/expanded", False)
        assert_eq(_expanded(d), False, "intervene resets expanded=false")

        # ── (G) keyboard activate via the invoke arc ──────────────────
        d.invoke("/external/send", "KeyboardActivate")
        assert_eq(_expanded(d), True, "KeyboardActivate (invoke) expands")
        d.invoke("/external/send", "KeyboardActivate")
        assert_eq(_expanded(d), False, "KeyboardActivate (invoke) collapses")

        # ── (H) end-to-end keyboard funnel: focus + scene/key ─────────
        # Space AND Enter both toggle a focused disclosure (APG model).
        d.request("focus/set", {"tag": TAG})
        assert_eq(d.request("focus/get").result.get("focused"), TAG, "focus set on header")
        d.key(path=TAG, name="Space")
        assert_eq(_expanded(d), True, "Space toggles a focused disclosure")
        d.key(path=TAG, name="Enter")
        assert_eq(_expanded(d), False, "Enter toggles a focused disclosure")
        d.key(path=TAG, name="Space")
        assert_eq(_expanded(d), True, "Space toggles again")

        # ── (I) disabled gate swallows pointer + keyboard ─────────────
        # Reset to collapsed first via keyboard, then disable.
        d.key(path=TAG, name="Space")
        assert_eq(_expanded(d), False, "reset to collapsed before disable")
        assert_eq(d.invoke("/external/send", "Disable"), "Disabled", "Disable -> Disabled")
        d.click(path=TAG)
        assert_eq(_expanded(d), False, "disabled: click does not toggle")
        d.invoke("/external/send", "KeyboardActivate")
        assert_eq(_expanded(d), False, "disabled: KeyboardActivate does not toggle")
        assert_eq(_state(d), "Disabled", "still disabled")
        assert_eq(d.invoke("/external/send", "Enable"), "Idle", "Enable -> Idle")

        # ── (J) negative: bad slot path errors cleanly ────────────────
        raised = False
        try:
            d.query("/external/no_such_slot")
        except RpcError:
            raised = True
        assert raised, "unknown introspect slot must raise, not silently pass"


if __name__ == "__main__":
    sys.exit(run_demo("R696 disclosure widget", body))
