#!/usr/bin/env python3
"""R701 §5.38 — single-open WAI-ARIA APG accordion end-to-end.

Drives a real running `hello-accordion-single` over JSON-RPC — the
AI-first path (§2 #2) — to verify the GUI consumer of the R700
`DisclosureGroup` single-expand coordinator. Unlike multi-open
`hello-accordion` (N independent `DisclosureExternal`s), this binary
holds ONE composite `DisclosureGroupExternal` at the `accordion_single`
paint root; the per-header rows are tagged `accordion_single#<i>` and
the R51.42 `'#'`-split routes clicks to the coordinator's
`"<i>:<EventName>"` send. The contract under test:

  * **at most one section open** — `expanded_index` is the single source
    of truth; opening a section collapses whichever was open
    (open-switch-collapse) and re-activating the open section collapses
    to none;
  * each section's body (`accordion_single_body_<i>`) is present in the
    paint scene only while that section is the open one;
  * pointer (real `scene/click` on the composite row tag), keyboard
    (`Space` / `Enter` through the `"<i>:KeyboardActivate"` wire form),
    `scene/invoke send`, and the model-driven `scene/intervene
    expanded_index` (Int / Null restore) all converge on the same
    coordinator;
  * the WAI-ARIA APG arrow-roving model moves *focus* between headers
    (`ArrowDown` / `ArrowUp` wrap, `Home` / `End` jump) through the R664
    focus_request mailbox — observable via `focus/get` — and never
    toggles expansion.

Exit 0 on every assertion satisfied, non-zero with a typed reason on
failure.
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

N = 3
PRIMARY = "accordion_single"
ROW = [f"{PRIMARY}#{i}" for i in range(N)]
BODY = [f"accordion_single_body_{i}" for i in range(N)]
VIEWPORT = (420, 440)


def _q(d, slot: str):
    # The single composite coordinator is the state-scene ROOT external,
    # so its introspect slots address as `/external/<slot>` (R666 §5.34
    # root-external path); the `/<tag>/external/...` form is for nested
    # children (multi-open `hello-accordion`).
    return d.query(f"/external/{slot}")


def _state(d, i: int) -> str:
    return _q(d, f"state.{i}")


def _expanded(d, i: int) -> bool:
    return _q(d, f"expanded.{i}")


def _expanded_index(d):
    return _q(d, "expanded_index")


def _send(d, i: int, ev: str):
    return d.invoke("/external/send", f"{i}:{ev}")


def _body_present(d, i: int) -> bool:
    snap = d.snapshot(source="paint", viewport=VIEWPORT)
    return find_by_tag(snap, BODY[i]) is not None


def _focused(d) -> str:
    return d.request("focus/get").result.get("focused")


def body() -> None:
    with RpcSubprocess("hello-accordion-single") as d:
        # ── (A) initial: N collapsed Idle sections, nothing open ──────
        assert_eq(_q(d, "count"), N, "coordinator wraps N sections")
        assert_eq(_expanded_index(d), None, "initial: no section open")
        for i in range(N):
            assert_eq(_state(d, i), "Idle", f"initial state sec {i}")
            assert_eq(_expanded(d, i), False, f"initial collapsed sec {i}")
            assert_eq(_body_present(d, i), False, f"collapsed sec {i} hides body")
            head = find_by_tag(d.snapshot(source="paint", viewport=VIEWPORT), ROW[i])
            assert head is not None, f"header {i} present in paint scene"

        # ── (B) click section 1 → only section 1 opens ────────────────
        d.click(path=ROW[1])
        assert_eq(_expanded_index(d), 1, "click sec 1 -> expanded_index = 1")
        assert_eq(_expanded(d, 1), True, "sec 1 open")
        assert_eq(_expanded(d, 0), False, "sec 0 closed")
        assert_eq(_expanded(d, 2), False, "sec 2 closed")
        assert_eq(_body_present(d, 1), True, "open sec 1 shows body")
        assert_eq(_body_present(d, 0), False, "closed sec 0 hides body")
        body_node = find_by_tag(d.snapshot(source="paint", viewport=VIEWPORT), BODY[1])
        assert rect_of(body_node)["h"] > 0, "panel body 1 has positive height"

        # ── (C) single-open: click section 0 → 1 COLLAPSES ────────────
        # The whole point vs multi-open: opening one closes the other.
        d.click(path=ROW[0])
        assert_eq(_expanded_index(d), 0, "open-switch: expanded_index 1 -> 0")
        assert_eq(_expanded(d, 0), True, "sec 0 now open")
        assert_eq(_expanded(d, 1), False, "sec 1 COLLAPSED by single-open exclusion")
        assert_eq(_body_present(d, 0), True, "sec 0 body present")
        assert_eq(_body_present(d, 1), False, "sec 1 body gone after switch")

        # ── (D) re-click the open section → collapse to none ──────────
        d.click(path=ROW[0])
        assert_eq(_expanded_index(d), None, "re-click open sec collapses to none")
        assert_eq(_expanded(d, 0), False, "sec 0 closed")
        for i in range(N):
            assert_eq(_body_present(d, i), False, f"no body painted sec {i}")

        # ── (E) invoke send pointer arc drives the statechart ─────────
        assert_eq(_send(d, 2, "PointerEnter"), None, "Enter alone does not open")
        assert_eq(_state(d, 2), "Hover", "sec2 Enter -> Hover")
        _send(d, 2, "PointerDown")
        assert_eq(_state(d, 2), "Pressed", "sec2 Down -> Pressed")
        assert_eq(_send(d, 2, "PointerUp"), 2, "sec2 Up activates -> expanded_index 2")
        assert_eq(_expanded(d, 2), True, "sec2 open via pointer arc")
        _send(d, 2, "PointerLeave")
        # switch to sec 0 via send — single-open enforced through RPC
        for ev in ("PointerEnter", "PointerDown", "PointerUp", "PointerLeave"):
            _send(d, 0, ev)
        assert_eq(_expanded_index(d), 0, "send-driven open-switch -> 0")
        assert_eq(_expanded(d, 2), False, "sec 2 collapsed by RPC-driven switch")

        # ── (F) model-driven intervene expanded_index (restore path) ──
        d.intervene("/external/expanded_index", 2)
        assert_eq(_expanded_index(d), 2, "intervene Int restores open section 2")
        assert_eq(_expanded(d, 0), False, "intervene collapses the others")
        d.intervene("/external/expanded_index", None)
        assert_eq(_expanded_index(d), None, "intervene Null collapses all")

        # ── (G) keyboard funnel: focus a header, Space AND Enter ──────
        d.request("focus/set", {"tag": ROW[0]})
        assert_eq(_focused(d), ROW[0], "focus set on header 0 (composite sub-tag)")
        d.key(path=ROW[0], name="Space")
        assert_eq(_expanded_index(d), 0, "Space opens focused section 0")
        d.key(path=ROW[0], name="Enter")
        assert_eq(_expanded_index(d), None, "Enter toggles focused section 0 closed")

        # ── (H) keyboard open-switch collapses the previous ───────────
        d.request("focus/set", {"tag": ROW[1]})
        d.key(path=ROW[1], name="Space")
        assert_eq(_expanded_index(d), 1, "Space opens focused sec 1")
        d.request("focus/set", {"tag": ROW[2]})
        d.key(path=ROW[2], name="Enter")
        assert_eq(_expanded_index(d), 2, "open sec 2 ...")
        assert_eq(_expanded(d, 1), False, "... collapses sec 1 (single-open via keyboard)")
        # reset
        d.key(path=ROW[2], name="Space")
        assert_eq(_expanded_index(d), None, "reset: all collapsed")

        # ── (I) APG arrow roving moves FOCUS only, never expansion ────
        d.request("focus/set", {"tag": ROW[0]})
        assert_eq(_focused(d), ROW[0], "start on header 0")
        d.key(path=ROW[0], name="ArrowDown")
        assert_eq(_focused(d), ROW[1], "ArrowDown: 0 -> 1")
        d.key(path=ROW[1], name="ArrowDown")
        assert_eq(_focused(d), ROW[2], "ArrowDown: 1 -> 2")
        d.key(path=ROW[2], name="ArrowDown")
        assert_eq(_focused(d), ROW[0], "ArrowDown wraps: 2 -> 0")
        d.key(path=ROW[0], name="ArrowUp")
        assert_eq(_focused(d), ROW[2], "ArrowUp wraps: 0 -> 2")
        d.key(path=ROW[2], name="Home")
        assert_eq(_focused(d), ROW[0], "Home -> first header")
        d.key(path=ROW[0], name="End")
        assert_eq(_focused(d), ROW[2], "End -> last header")
        assert_eq(_expanded_index(d), None, "roving toggled nothing open")

        # ── (J) negatives: bad send / bad slot reject cleanly ─────────
        try:
            _send(d, 9, "PointerUp")
            raised = False
        except RpcError:
            raised = True
        assert raised, "out-of-range section index must be rejected"

        try:
            _send(d, 0, "BogusEvent")
            raised = False
        except RpcError:
            raised = True
        assert raised, "unknown event name must be rejected"

        try:
            _q(d, "no_such_slot")
            raised = False
        except RpcError:
            raised = True
        assert raised, "unknown introspect slot must raise, not silently pass"


if __name__ == "__main__":
    sys.exit(run_demo("R701 single-open accordion widget", body))
