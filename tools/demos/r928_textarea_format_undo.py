#!/usr/bin/env python3
"""R928 §5.52 §5.36 — formatting is an undoable edit in the rich-text textarea.

R798 wired the `UndoStack` into the textarea so typing and delete-with-run-
reversal undo. But the formatting mutators (`apply_style_run` /
`clear_style_runs` / `merge_style_run`) wrote the run list *directly*,
bypassing the stack — a Bold or a recolour was the one editable mutation
`Ctrl+Z` never reversed (`record_edit` even early-returns when the text is
unchanged, so a format produced no journal entry). R928 routes all three
through a granular `StyleRunCommand`, so formatting is a first-class,
reversible edit — through the GUI toolbar and the `scene/invoke` funnel
alike, since both call the same now-journalled mutators.

An AI agent drives and reads the whole round-trip over the §5.12 RPC plane
(§2 #2), the same keys a human presses:

  (A) apply a colour over a selection (invoke funnel) → one Ctrl+Z reverts
      it to the seed exactly; Ctrl+Shift+Z / Ctrl+Y redo — the headline.
  (B) clear formatting (invoke funnel) → Ctrl+Z restores the cleared run.
  (C) toggle Bold via the toolbar CLICK (the routed `merge_style_run` path)
      → Ctrl+Z un-bolds, colour preserved.
  (D) typing then formatting are TWO undo steps — one Ctrl+Z drops the
      format and leaves the text, a second drops the text.
  (E) undo past the seeded document is a no-op (the undo floor).

Run from the workspace root:
    cargo build -p hello-textarea --release
    python3 tools/demos/r928_textarea_format_undo.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_query,
    wait_until,
)

TA_TAG = "main_textarea"
VIEWPORT = (480, 320)
SEED = "first line\nsecond line\nthird line"
SEED_SPANS = [(0, 5), (11, 17), (23, 28)]

RED = (0xD0, 0x28, 0x28)
GREEN = (0x1F, 0x8A, 0x34)
BLUE = (0x26, 0x4C, 0xD8)
BOLD = 700
NORMAL = 400
FMT_BOLD_TAG = "fmt_toolbar#0"  # the Bold control (merge_style_run path)


def text(ta: RpcSubprocess) -> str:
    return ta.query("/external/text")


def hex_of(rgb: tuple[int, int, int]) -> str:
    return f"#{rgb[0]:02x}{rgb[1]:02x}{rgb[2]:02x}"


def _walk(node, out):
    out.append(node)
    for c in node.get("children", []) or []:
        _walk(c, out)
    if isinstance(node.get("content"), dict):
        _walk(node["content"], out)
    return out


def field_runs(ta: RpcSubprocess) -> list[dict]:
    snap = ta.snapshot(source="paint", viewport=VIEWPORT)
    field = find_by_tag(snap, TA_TAG)
    assert field is not None, "textarea present in paint scene"
    texts = [n for n in _walk(field, []) if n.get("type") == "Text" and n.get("runs")]
    assert texts, "a painted Text node carries styled runs"
    return texts[0]["runs"]


def run_spans(ta: RpcSubprocess) -> list[tuple[int, int]]:
    return [(r["start"], r["end"]) for r in field_runs(ta)]


def run_at(ta: RpcSubprocess, byte: int) -> dict | None:
    for r in field_runs(ta):
        if r["start"] <= byte < r["end"]:
            return r
    return None


def color_at(ta: RpcSubprocess, byte: int) -> tuple[int, int, int] | None:
    r = run_at(ta, byte)
    if r is None:
        return None
    c = r["style"]["fg_color"]
    return (c["r"], c["g"], c["b"])


def weight_at(ta: RpcSubprocess, byte: int) -> int | None:
    r = run_at(ta, byte)
    return None if r is None else r["style"]["font_weight"]


def apply_color(ta: RpcSubprocess, start: int, end: int, rgb: tuple[int, int, int]) -> bool:
    return ta.invoke(
        "/external/apply-style",
        {"start": start, "end": end, "fg": hex_of(rgb), "size": 16},
    )


def clear_fmt(ta: RpcSubprocess, start: int, end: int) -> bool:
    return ta.invoke("/external/clear-style", {"start": start, "end": end})


def select(ta: RpcSubprocess, start: int, end: int) -> None:
    ta.intervene("/external/selection", {"start": start, "end": end})
    wait_query(
        ta, "/external/selection", {"start": start, "end": end},
        desc="selection set via intervene",
    )


def chord(ta: RpcSubprocess, key: str, **mods: bool) -> bool:
    return ta.invoke("/external/key", {"key": key, **mods})


def undo(ta: RpcSubprocess) -> bool:
    return chord(ta, "z", ctrl=True)


def redo_shift(ta: RpcSubprocess) -> bool:
    return chord(ta, "z", ctrl=True, shift=True)


def body() -> None:
    with RpcSubprocess("hello-textarea", request_timeout=12.0) as ta:
        # ── boot: seed runs intact, field focused ─────────────────────
        assert_eq(text(ta), SEED, "seeded multi-line text")
        assert_eq(run_spans(ta), SEED_SPANS, "three seed runs")
        assert_eq(color_at(ta, 0), RED, "'first' is red")
        assert_eq(color_at(ta, 11), GREEN, "'second' is green")
        assert_eq(color_at(ta, 23), BLUE, "'third' is blue")
        bold_rect = abs_rects_of(ta.snapshot(source="paint", viewport=VIEWPORT)).get(FMT_BOLD_TAG)
        assert bold_rect is not None, "the Bold toolbar control has an absolute rect"
        ta.request("focus/set", {"tag": TA_TAG})
        wait_query(ta, "/external/state", "Focused", desc="field focused")

        # ── (A) apply a colour → ONE Ctrl+Z reverts it (the headline) ──
        assert apply_color(ta, 1, 3, BLUE) is True, "apply-style invoke handled"
        assert_eq(run_spans(ta)[:3], [(0, 1), (1, 3), (3, 5)], "blue split 'first' into red|blue|red")
        assert_eq(color_at(ta, 1), BLUE, "the split middle is blue")
        assert_eq(text(ta), SEED, "formatting never edits the text")

        assert undo(ta) is True, "Ctrl+Z recognized"
        wait_until(lambda: run_spans(ta) == SEED_SPANS, desc="one undo reverted the format to the seed")
        assert_eq(color_at(ta, 1), RED, "byte 1 is red again (the blue run is gone)")
        assert_eq(text(ta), SEED, "undo of a format leaves the text untouched")

        assert chord(ta, "y", ctrl=True) is True, "Ctrl+Y recognized (alternate redo)"
        wait_until(lambda: color_at(ta, 1) == BLUE, desc="redo re-applies the colour")
        assert redo_shift(ta) is True, "Ctrl+Shift+Z recognized (no-op at the top)"
        assert_eq(color_at(ta, 1), BLUE, "nothing to redo past the top — colour stays")
        assert undo(ta) is True, "Ctrl+Z back toward the seed"
        wait_until(lambda: run_spans(ta) == SEED_SPANS, desc="back at the seed for phase B")

        # ── (B) clear formatting → Ctrl+Z restores the cleared run ────
        assert clear_fmt(ta, 0, 5) is True, "clear-style invoke handled"
        wait_until(lambda: run_at(ta, 0) is None, desc="the red 'first' run was cleared")
        assert_eq(color_at(ta, 11), GREEN, "untouched 'second' still green after clear")
        assert undo(ta) is True, "Ctrl+Z recognized for the clear"
        wait_until(lambda: color_at(ta, 0) == RED, desc="undo restored the cleared red run")
        assert_eq(run_spans(ta), SEED_SPANS, "the seed run set is whole again")

        # ── (C) Bold via the toolbar click (merge_style_run path) ─────
        select(ta, 11, 17)  # the green "second"
        assert_eq(weight_at(ta, 11), NORMAL, "'second' is not bold to start")
        bx, by, bw, bh = bold_rect
        ta.click(at=(bx + bw / 2, by + bh / 2))
        wait_until(lambda: weight_at(ta, 11) == BOLD, desc="the Bold control bolded the selection")
        assert_eq(color_at(ta, 11), GREEN, "Bold preserved the green colour (mergeCharFormat)")
        assert_eq(ta.query("/external/state"), "Focused", "the toolbar click did not blur the field")
        assert undo(ta) is True, "Ctrl+Z recognized for the Bold"
        wait_until(lambda: weight_at(ta, 11) == NORMAL, desc="undo un-bolded 'second'")
        assert_eq(color_at(ta, 11), GREEN, "still green after the un-bold")

        # ── (D) typing then formatting are TWO undo steps ─────────────
        assert_eq(chord(ta, "End", ctrl=True), True, "Ctrl+End recognized")
        wait_query(ta, "/external/caret", len(SEED), desc="caret at the document end")
        ta.invoke("/external/key", "Z")
        wait_query(ta, "/external/text", SEED + "Z", desc="typed a trailing 'Z'")
        assert apply_color(ta, len(SEED), len(SEED) + 1, RED) is True, "formatted the new char"
        wait_until(lambda: color_at(ta, len(SEED)) == RED, desc="the 'Z' is red")

        assert undo(ta) is True, "first undo (the format)"
        wait_until(lambda: run_at(ta, len(SEED)) is None, desc="the format alone reverted")
        assert_eq(text(ta), SEED + "Z", "the typed 'Z' still stands — a separate undo step")
        assert undo(ta) is True, "second undo (the typing)"
        wait_query(ta, "/external/text", SEED, desc="now the typing reverts")

        # ── (E) undo floor — cannot undo past the seeded document ─────
        # Drain any remaining journal back to the seed, then prove the floor.
        for _ in range(4):
            undo(ta)
        wait_until(lambda: text(ta) == SEED and run_spans(ta) == SEED_SPANS, desc="settled at the seed floor")
        assert undo(ta) is True, "Ctrl+Z is consumed at the floor"
        assert_eq(text(ta), SEED, "no change below the seeded floor (text)")
        assert_eq(run_spans(ta), SEED_SPANS, "no change below the seeded floor (runs)")


if __name__ == "__main__":
    sys.exit(run_demo("R928 §5.52 — textarea formatting is undoable", body))
