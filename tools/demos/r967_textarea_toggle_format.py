#!/usr/bin/env python3
"""R967 §5.36 — AI-first `toggle-format` RPC verb (mergeCharFormat over RPC).

R769 gave the *human* a B / I toolbar that toggles bold / italic over a
selection while keeping its colour (`TextEditState::merge_style_run`). But the
RPC surface only had `apply-style` (wholesale `setCharFormat`, which CLOBBERS
the colour) — so to toggle one field the AI had to read a run's full style,
mutate it, and write the whole thing back (R769 Phase 3b). R967 closes that
[[wire-form-read-write-symmetry]] gap: a single AI-first `toggle-format` verb,
routed through the SAME `TextEditState::toggle_format` SSOT the toolbar now
shares, so the human + AI channels flip identically.

  invoke("toggle-format", "bold"|"italic"|"underline"|"strikethrough")

R1540 — `decoration.underline` is a FORM token (`"none"` / `"single"` / …), not
a bool. `toggle-format underline` still means Qt's `setFontUnderline(bool)`:
on selects `"single"`, off clears whatever form was there. The bool VERB is
unchanged; only what the wire reports is finer.

It toggles ONE field over the live selection (or arms a pending mark at a
collapsed caret), preserving the covered run's OTHER fields, and returns the
new on-state. underline / strikethrough have no toolbar button — the RPC
surface is a superset of the human toolbar (the AI gets all four).

This drives hello-textarea over JSON-RPC, reading the canonical
`/external/style_runs` introspection (a divergence between paint + this read
would be a bug — the lockstep contract). Collapsed-caret pending-mark behaviour
+ the toolbar's shared-SSOT path are unit-tested in the binding; the toolbar's
end-to-end click path stays covered by r769.

Run from the workspace root:
    cargo build -p hello-textarea --release
    python3 tools/demos/r967_textarea_toggle_format.py

>= 30 assertions.
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
    run_demo,
    wait_query,
    wait_snap,
)

EXAMPLE = "hello-textarea"
TA_TAG = "main_textarea"
VIEWPORT = (480, 320)
SEED_TEXT = "first line\nsecond line\nthird line"
RED = (0xD0, 0x28, 0x28)  # the seed colour of "first" [0,5]


def runs(ta: RpcSubprocess) -> list:
    return ta.query("/external/style_runs")


def run_at(ta: RpcSubprocess, byte: int) -> dict | None:
    for r in runs(ta):
        if r["start"] <= byte < r["end"]:
            return r
    return None


def select(ta: RpcSubprocess, start: int, end: int) -> None:
    ta.intervene("/external/selection", {"start": start, "end": end})


def toggle(ta: RpcSubprocess, field: str):
    return ta.invoke("/external/toggle-format", field)


def color(style: dict) -> tuple[int, int, int]:
    c = style["fg_color"]
    return (c["r"], c["g"], c["b"])


def body() -> None:
    with RpcSubprocess(EXAMPLE, request_timeout=12.0) as ta:
        wait_snap(ta, lambda s: find_by_tag(s, TA_TAG) is not None, viewport=VIEWPORT, desc="textarea painted")
        ta.request("focus/set", {"tag": TA_TAG})
        wait_query(ta, "/external/state", "Focused", desc="field focused")

        # ── seed: "first" [0,5] is red, normal weight / style, no decoration
        seed = run_at(ta, 0)
        assert seed is not None, "the seed 'first' run is present"
        assert_eq((seed["start"], seed["end"]), (0, 5), "seed run spans [0,5]")
        assert_eq(seed["style"]["font_weight"], 400, "seed is normal weight")
        assert_eq(seed["style"]["font_style"], "Normal", "seed is upright")
        assert_eq(color(seed["style"]), RED, "seed is red")
        assert_eq(seed["style"]["decoration"]["underline"], "none", "seed not underlined")

        # ── (A) ATOMIC toggle-format bold: ONE RPC call, colour preserved
        # (apply-style would have clobbered the colour — the whole point).
        select(ta, 0, 5)
        assert_eq(toggle(ta, "bold"), True, "toggle-format returns the new on-state (bold)")
        r = run_at(ta, 0)
        assert_eq(r["style"]["font_weight"], 700, "now bold, via one toggle-format call")
        assert_eq(color(r["style"]), RED, "bold KEPT the colour (mergeCharFormat, not wholesale)")
        assert_eq((r["start"], r["end"]), (0, 5), "the run span is unchanged")
        assert_eq(ta.query("/external/text"), SEED_TEXT, "a format toggle never edits the text")

        # ── (B) reversible round-trip: a second toggle returns off + un-bolds
        select(ta, 0, 5)
        assert_eq(toggle(ta, "bold"), False, "the second toggle reports off")
        assert_eq(run_at(ta, 0)["style"]["font_weight"], 400, "un-bolded")
        assert_eq(color(run_at(ta, 0)["style"]), RED, "still red after the round-trip")

        # ── (C) orthogonal fields: italic leaves weight + colour untouched
        select(ta, 0, 5)
        assert_eq(toggle(ta, "bold"), True, "re-bold for the orthogonality check")
        select(ta, 0, 5)
        assert_eq(toggle(ta, "italic"), True, "italic on")
        r = run_at(ta, 0)
        assert_eq(r["style"]["font_style"], "Italic", "italic set")
        assert_eq(r["style"]["font_weight"], 700, "the italic toggle left the weight bold")
        assert_eq(color(r["style"]), RED, "the italic toggle left the colour")

        # ── (D) RPC SUPERSET: underline + strikethrough (no toolbar button)
        select(ta, 0, 5)
        assert_eq(toggle(ta, "underline"), True, "underline on (an RPC-only field)")
        r = run_at(ta, 0)
        assert_eq(r["style"]["decoration"]["underline"], "single", "underline set")
        assert_eq(r["style"]["font_weight"], 700, "underline left the weight")
        assert_eq(r["style"]["font_style"], "Italic", "underline left the style")
        select(ta, 0, 5)
        assert_eq(toggle(ta, "strikethrough"), True, "strikethrough on")
        r = run_at(ta, 0)
        assert_eq(r["style"]["decoration"]["strikethrough"], True, "strikethrough set")
        assert_eq(
            r["style"]["decoration"]["underline"],
            "single",
            "underline still set (orthogonal)",
        )
        # reverse underline only — strikethrough stays
        select(ta, 0, 5)
        assert_eq(toggle(ta, "underline"), False, "underline off")
        r = run_at(ta, 0)
        assert_eq(r["style"]["decoration"]["underline"], "none", "underline cleared")
        assert_eq(r["style"]["decoration"]["strikethrough"], True, "strikethrough kept through the underline toggle")
        assert_eq(color(r["style"]), RED, "colour survived every toggle")

        # ── (E) an unknown field token is rejected (not a silent no-op)
        rejected = False
        try:
            toggle(ta, "rainbow")
        except RpcError:
            rejected = True
        assert rejected, "an unknown field token is rejected"
        # the document is byte-unchanged through every format toggle
        assert_eq(ta.query("/external/text"), SEED_TEXT, "no toggle edited the text")


if __name__ == "__main__":
    sys.exit(run_demo("R967 §5.36 — AI-first toggle-format RPC verb", body))
