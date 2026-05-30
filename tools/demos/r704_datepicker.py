#!/usr/bin/env python3
"""R704 hello-datepicker demo — inline month-calendar date picker.

Drives the live `hello-datepicker` window over JSON-RPC 2.0 — the
AI-first path (§2 #2) — to verify the date picker's selection, month
navigation (incl. year rollover both directions), keyboard roving (APG
date grid), and PageUp/PageDown month nav entirely through RPC. No
display, no screenshot: every assertion is a typed `scene/query` /
`scene/click` / `scene/key` round-trip plus `scene/snapshot` (paint
source) for structural shape.

The picker holds ONE `DatePickerExternal` at the composite root
"datepicker" (the state-scene ROOT external), so its introspect slots
address as `/external/<slot>` (R666 §5.34 root-external path). Day cells
are composite tags "datepicker#<day>"; the R51.42 `'#'`-split routes a
click on day d to the coordinator's `"<d>:<EventName>"` send.

a11y note: pinion has no `access/node` RPC method (the AccessKit tree is
emitted to the platform AT, not the §5.12 JSON-RPC surface). The grid /
gridcell / columnheader role contributions are pinned by the Rust unit
tests in `examples/hello-datepicker/src/main.rs` (access_node tests).
This demo asserts the a11y-relevant scene shape indirectly: the grid
root, weekday-header, and day-cell paint tags are all present in the
paint snapshot, which is the substrate the `access_node` walker stamps.

Run from the workspace root:
    cargo build -p hello-datepicker --release
    python3 tools/demos/r704_datepicker.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    assert_eq,
    find_by_tag,
    isolated_storage_dir,
    run_demo,
)

DP = "datepicker"
VIEWPORT = (360, 420)


def _q(d, slot: str):
    return d.query(f"/external/{slot}")


def _present(d, tag: str) -> bool:
    snap = d.snapshot(source="paint", viewport=VIEWPORT)
    return find_by_tag(snap, tag) is not None


def _focused(d):
    return d.request("focus/get").result.get("focused")


def _focus_set(d, tag: str):
    return d.request("focus/set", {"tag": tag}).result.get("focused")


def body() -> None:
    # Isolate any persistence side effects (mirror the r666 harness
    # convention; the picker itself does not persist).
    with isolated_storage_dir("r704-datepicker"):
        with RpcSubprocess("hello-datepicker") as d:
            # ── 1. initial state: May 2026, no selection ────────────
            assert_eq(_q(d, "year"), 2026, "initial year == 2026")
            assert_eq(_q(d, "month"), 5, "initial month == 5")
            assert_eq(_q(d, "days"), 31, "May 2026 has 31 days")
            assert_eq(_q(d, "selected"), False, "no initial selection")
            assert_eq(_q(d, "selected_day"), -1, "selected_day == -1 when none")
            # 2026-05-01 is a Friday → column index 5. Day cells 1..=31
            # present in the paint scene; day 32 absent (the a11y grid
            # walker stamps gridcell nodes onto exactly these tags).
            assert _present(d, f"{DP}#1"), "day cell 1 present in paint"
            assert _present(d, f"{DP}#31"), "day cell 31 present in paint"
            assert not _present(d, f"{DP}#32"), "day cell 32 absent (May has 31)"
            # Weekday-header columnheader tags (Su..Sa) + nav buttons.
            for col in range(7):
                assert _present(d, f"{DP}_wh{col}"), f"weekday header col {col} present"
            assert _present(d, f"{DP}#prev"), "prev nav button present"
            assert _present(d, f"{DP}#next"), "next nav button present"

            # ── 2. click day 15 → selected becomes 2026-05-15 ───────
            d.click(path=f"{DP}#15")
            assert_eq(_q(d, "selected"), True, "selected flag true after click")
            assert_eq(_q(d, "selected_year"), 2026, "selected_year == 2026")
            assert_eq(_q(d, "selected_month"), 5, "selected_month == 5")
            assert_eq(_q(d, "selected_day"), 15, "selected_day == 15")
            assert_eq(_q(d, "selected.15"), True, "selected.15 true")
            assert_eq(_q(d, "selected.14"), False, "selected.14 false")

            # ── 3. prev → April 2026 (30 days); next ×2 → June ──────
            d.click(path=f"{DP}#prev")
            assert_eq(_q(d, "month"), 4, "prev -> month 4")
            assert_eq(_q(d, "days"), 30, "April 2026 has 30 days")
            assert_eq(_q(d, "year"), 2026, "prev keeps year 2026")
            assert_eq(_q(d, "selected_month"), 5, "selection unchanged by nav")
            d.click(path=f"{DP}#next")
            d.click(path=f"{DP}#next")
            assert_eq(_q(d, "month"), 6, "next x2 -> month 6")
            assert_eq(_q(d, "days"), 30, "June 2026 has 30 days")

            # ── 4. year rollover both directions ────────────────────
            # From June, prev 6x -> December 2025.
            for _ in range(6):
                d.click(path=f"{DP}#prev")
            assert_eq(_q(d, "month"), 12, "prev 6x -> December")
            assert_eq(_q(d, "year"), 2025, "year rolled back to 2025")
            assert_eq(_q(d, "days"), 31, "December 2025 has 31 days")
            # Forward rollover: next -> January 2026.
            d.click(path=f"{DP}#next")
            assert_eq(_q(d, "month"), 1, "next -> January")
            assert_eq(_q(d, "year"), 2026, "year rolled forward to 2026")

            # ── 5. keyboard roving + activation (active descendant) ─
            # The grid is a single Tab stop; the focused day is an
            # internal roving active descendant (the `focused_day` slot),
            # the WAI-ARIA date-grid model. Shell focus stays on the grid
            # root; arrow keys move `focused_day` within the month.
            # Navigate to a clean May 2026 (Jan -> May = +4).
            for _ in range(4):
                d.click(path=f"{DP}#next")
            assert_eq(_q(d, "month"), 5, "back to May 2026")
            assert_eq(_focus_set(d, DP), DP, "focus set on grid root")
            # The day-15 click in step 2 synced the active descendant
            # (WAI-ARIA "activation moves focus"); it survives the month
            # rolls (every intermediate month has >= 28 days, so 15 stays
            # valid + clamped).
            assert_eq(_q(d, "focused_day"), 15, "active descendant carried from click")
            d.key(path=DP, name="Home")
            assert_eq(_q(d, "focused_day"), 1, "Home -> day 1")
            d.key(path=DP, name="ArrowRight")
            assert_eq(_q(d, "focused_day"), 2, "ArrowRight -> day 2")
            d.key(path=DP, name="ArrowDown")
            assert_eq(_q(d, "focused_day"), 9, "ArrowDown -> +7 -> day 9")
            d.key(path=DP, name="Home")
            assert_eq(_q(d, "focused_day"), 1, "Home -> day 1")
            d.key(path=DP, name="End")
            assert_eq(_q(d, "focused_day"), 31, "End -> day 31")
            # ArrowRight on the last day clamps within the month
            # (deferred axis: month-crossing arrow nav).
            d.key(path=DP, name="ArrowRight")
            assert_eq(_q(d, "focused_day"), 31, "ArrowRight clamps at day 31")
            # Shell focus never left the grid root through all the roving.
            assert_eq(_focused(d), DP, "shell focus stays on the grid root")
            # Enter activates the active-descendant day.
            d.key(path=DP, name="Enter")
            assert_eq(_q(d, "selected_day"), 31, "Enter selects active day 31")

            # ── 6. PageDown / PageUp month navigation via keyboard ──
            d.key(path=DP, name="PageDown")
            assert_eq(_q(d, "month"), 6, "PageDown -> month 6")
            # June has 30 days; the active descendant (31) clamps to 30.
            assert_eq(_q(d, "focused_day"), 30, "active descendant clamps to 30")
            d.key(path=DP, name="PageUp")
            assert_eq(_q(d, "month"), 5, "PageUp -> back to month 5")

            # ── 7. scene/invoke send wire form (the click funnel) ───
            # Selecting day 7 through the introspect send wire returns
            # the new selected day, mirroring the composite click path.
            for ev in ("PointerEnter", "PointerDown", "PointerUp"):
                out = d.invoke("/external/send", f"7:{ev}")
            assert_eq(out, 7, "send 7:PointerUp activates -> selected day 7")
            assert_eq(_q(d, "selected_day"), 7, "introspect send selected day 7")
            # Month-nav sentinels via send.
            assert_eq(d.invoke("/external/send", "NextMonth"), None,
                      "send NextMonth returns Null")
            assert_eq(_q(d, "month"), 6, "send NextMonth -> month 6")
            d.invoke("/external/send", "PrevMonth")
            assert_eq(_q(d, "month"), 5, "send PrevMonth -> back to month 5")

            # ── 8. scene/simulate hypothetical with rollback ────────
            # R646 §5.34 — steps are `{path, value}` slot writes applied
            # against a snapshot, captured after the final step, then
            # rolled back. The response result IS the final-projection
            # snapshot node directly (a `{type, rect, tag, ...}` node — the
            # same shape `scene/snapshot` returns; there is no wrapper
            # object), mirroring the r646 slider simulate demo. The
            # picker's one writable slot is `focused_day` (the roving
            # active descendant): preview moving the cursor to day 10 then
            # 20; the live cursor must be untouched afterwards.
            before_focus = _q(d, "focused_day")
            steps = [
                {"path": "/external/focused_day", "value": 10},
                {"path": "/external/focused_day", "value": 20},
            ]
            resp = d.request("scene/simulate", {"steps": steps})
            assert resp is not None, "simulate returned a response"
            final = resp.result
            assert isinstance(final, dict) and final.get("type"), \
                "simulate returns the final-projection snapshot node"
            assert_eq(_q(d, "focused_day"), before_focus,
                      "live active descendant untouched after simulate (rollback)")

            # ── 9. negatives: bad send / unknown slot reject cleanly ─
            raised = False
            try:
                d.invoke("/external/send", "99:PointerUp")
            except RpcError:
                raised = True
            assert raised, "out-of-range day index must be rejected"
            raised = False
            try:
                _q(d, "no_such_slot")
            except RpcError:
                raised = True
            assert raised, "unknown introspect slot must raise, not silently pass"


if __name__ == "__main__":
    sys.exit(run_demo("R704 inline DatePicker (month calendar grid)", body))
