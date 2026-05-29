#!/usr/bin/env python3
"""R697 §5.38 §5.50 — multi-open WAI-ARIA APG accordion end-to-end.

Drives a real running `hello-accordion` over JSON-RPC — the AI-first
path (§2 #2) — to verify the accordion as the 2nd consumer of the R696
Disclosure substrate (N=3 `DisclosureExternal`s composed through
`create_extra_externals`):

  * each section header is an independent disclosure addressed by its
    own `accordion_sec_{i}` tag (`/accordion_sec_{i}/external/...`
    multi-External path syntax, R666 §5.34);
  * **multi-open** is the default — more than one panel may be expanded
    at once, and toggling one section never collapses another;
  * each section's body (`accordion_body_{i}`) is present in the paint
    scene only while that section is expanded;
  * pointer (real `scene/click`), keyboard (`Space` AND `Enter`), and
    `scene/intervene` all drive the same per-section `expanded` sidecar;
  * the WAI-ARIA APG arrow-roving model moves *focus* between headers
    (`ArrowDown`/`ArrowUp` wrap, `Home`/`End` jump) through the R664
    focus_request mailbox — observable via `focus/get`;
  * `Space` toggles only the focused section.

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
SEC = [f"accordion_sec_{i}" for i in range(N)]
BODY = [f"accordion_body_{i}" for i in range(N)]
VIEWPORT = (420, 440)


def _state(d, i: int) -> str:
    return d.query(f"/{SEC[i]}/external/state")


def _expanded(d, i: int) -> bool:
    return d.query(f"/{SEC[i]}/external/expanded")


def _body_present(d, i: int) -> bool:
    snap = d.snapshot(source="paint", viewport=VIEWPORT)
    return find_by_tag(snap, BODY[i]) is not None


def _focused(d) -> str:
    return d.request("focus/get").result.get("focused")


def body() -> None:
    with RpcSubprocess("hello-accordion") as d:
        # ── (A) initial: 3 collapsed Idle sections, no bodies painted ─
        for i in range(N):
            assert_eq(_state(d, i), "Idle", f"initial state sec {i}")
            assert_eq(_expanded(d, i), False, f"initial collapsed sec {i}")
            assert_eq(_body_present(d, i), False, f"collapsed sec {i} hides body")
            head = find_by_tag(d.snapshot(source="paint", viewport=VIEWPORT), SEC[i])
            assert head is not None, f"header {i} present in paint scene"

        # ── (B) click section 1 → only section 1 expands (independent) ─
        d.click(path=SEC[1])
        assert_eq(_expanded(d, 1), True, "click sec 1 expands it")
        assert_eq(_expanded(d, 0), False, "sec 0 untouched")
        assert_eq(_expanded(d, 2), False, "sec 2 untouched")
        assert_eq(_body_present(d, 1), True, "expanded sec 1 shows body")
        assert_eq(_body_present(d, 0), False, "collapsed sec 0 hides body")
        body_node = find_by_tag(d.snapshot(source="paint", viewport=VIEWPORT), BODY[1])
        assert rect_of(body_node)["h"] > 0, "panel body 1 has positive height"

        # ── (C) multi-open: expand section 0 too — both stay open ─────
        d.click(path=SEC[0])
        assert_eq(_expanded(d, 0), True, "click sec 0 expands it")
        assert_eq(_expanded(d, 1), True, "sec 1 STAYS expanded (multi-open)")
        assert_eq(_body_present(d, 0), True, "sec 0 body present")
        assert_eq(_body_present(d, 1), True, "sec 1 body still present")
        d.click(path=SEC[2])
        assert_eq(_expanded(d, 2), True, "all three sections open at once")

        # ── (D) collapse section 1 only — others stay open ────────────
        d.click(path=SEC[1])
        assert_eq(_expanded(d, 1), False, "re-click sec 1 collapses it")
        assert_eq(_expanded(d, 0), True, "sec 0 still open")
        assert_eq(_expanded(d, 2), True, "sec 2 still open")
        assert_eq(_body_present(d, 1), False, "collapsed sec 1 body gone")

        # reset all to collapsed via intervene for a clean slate
        for i in range(N):
            d.intervene(f"/{SEC[i]}/external/expanded", False)
            assert_eq(_expanded(d, i), False, f"intervene resets sec {i}")

        # ── (E) per-section invoke send arc (pointer statechart) ──────
        assert_eq(d.invoke(f"/{SEC[2]}/external/send", "PointerEnter"), "Hover", "sec2 Enter->Hover")
        assert_eq(d.invoke(f"/{SEC[2]}/external/send", "PointerDown"), "Pressed", "sec2 Down->Pressed")
        assert_eq(d.invoke(f"/{SEC[2]}/external/send", "PointerUp"), "Hover", "sec2 Up->Hover")
        assert_eq(_expanded(d, 2), True, "sec2 activate expands")
        assert_eq(_expanded(d, 0), False, "sec0 unaffected by sec2 activate")
        d.invoke(f"/{SEC[2]}/external/send", "PointerLeave")
        d.intervene(f"/{SEC[2]}/external/expanded", False)

        # ── (F) intervene write channel per section ───────────────────
        d.intervene(f"/{SEC[0]}/external/expanded", True)
        assert_eq(_expanded(d, 0), True, "intervene writes sec0 expanded=true")
        d.intervene(f"/{SEC[0]}/external/expanded", False)
        assert_eq(_expanded(d, 0), False, "intervene writes sec0 expanded=false")

        # ── (G) keyboard funnel: focus a header, Space AND Enter toggle ─
        d.request("focus/set", {"tag": SEC[0]})
        assert_eq(_focused(d), SEC[0], "focus set on header 0")
        d.key(path=SEC[0], name="Space")
        assert_eq(_expanded(d, 0), True, "Space toggles focused section")
        d.key(path=SEC[0], name="Enter")
        assert_eq(_expanded(d, 0), False, "Enter toggles focused section")

        # ── (H) Space toggles ONLY the focused section ────────────────
        d.request("focus/set", {"tag": SEC[1]})
        d.key(path=SEC[1], name="Space")
        assert_eq(_expanded(d, 1), True, "Space expands focused sec 1")
        assert_eq(_expanded(d, 0), False, "sec 0 unaffected")
        assert_eq(_expanded(d, 2), False, "sec 2 unaffected")
        d.key(path=SEC[1], name="Space")  # collapse again
        assert_eq(_expanded(d, 1), False, "reset focused sec 1")

        # ── (I) APG arrow roving moves FOCUS between headers ──────────
        d.request("focus/set", {"tag": SEC[0]})
        assert_eq(_focused(d), SEC[0], "start on header 0")
        d.key(path=SEC[0], name="ArrowDown")
        assert_eq(_focused(d), SEC[1], "ArrowDown: 0 -> 1")
        d.key(path=SEC[1], name="ArrowDown")
        assert_eq(_focused(d), SEC[2], "ArrowDown: 1 -> 2")
        d.key(path=SEC[2], name="ArrowDown")
        assert_eq(_focused(d), SEC[0], "ArrowDown wraps: 2 -> 0")
        d.key(path=SEC[0], name="ArrowUp")
        assert_eq(_focused(d), SEC[2], "ArrowUp wraps: 0 -> 2")
        d.key(path=SEC[2], name="Home")
        assert_eq(_focused(d), SEC[0], "Home -> first header")
        d.key(path=SEC[0], name="End")
        assert_eq(_focused(d), SEC[2], "End -> last header")

        # arrow roving moved focus only — expansion state untouched
        for i in range(N):
            assert_eq(_expanded(d, i), False, f"roving did not toggle sec {i}")

        # ── (J) negative: bad slot path errors cleanly ────────────────
        raised = False
        try:
            d.query(f"/{SEC[0]}/external/no_such_slot")
        except RpcError:
            raised = True
        assert raised, "unknown introspect slot must raise, not silently pass"


if __name__ == "__main__":
    sys.exit(run_demo("R697 multi-open accordion widget", body))
