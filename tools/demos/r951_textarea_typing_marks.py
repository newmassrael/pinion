#!/usr/bin/env python3
"""R951 §5.36 §5.22 — active typing marks (collapsed-caret formatting).

The rich-text textarea could format an existing *selection* (R768/R769/R928)
and typing *inside* a styled span inherited its style (the run extends). What
it could not do is the canonical "press Bold with nothing selected, then type
bold text" — ProseMirror `storedMarks` / Slate `editor.marks` / Word's pending
format. R951 adds that: a collapsed-caret format arms an active typing mark so
the *next* inserted text carries it, cleared by any caret navigation, preserved
across edits. The granular undo grew the symmetric `inserted_runs` peer so a
redo restores the typed mark (clip+shift alone cannot re-derive an added run).

An AI agent drives + reads the whole round-trip over the §5.12 RPC plane
(§2 #2), the same affordances a human gets:

  (A) AI round-trip — read `style_at_caret`, flip the weight, write it back via
      `mark`, then type → the typed text carries the mark (the headline).
  (B) the mark continues across keystrokes (one coalesced run) and is dropped
      by a caret navigation (`pending_style` → null).
  (C) undo / redo of marked typing — Ctrl+Z removes text + mark, Ctrl+Shift+Z
      restores BOTH (the `inserted_runs` peer; clip+shift cannot re-derive it).
  (D) an edit (Backspace) PRESERVES the mark (the Word convention).
  (E) the toolbar Bold CLICK at a collapsed caret arms the mark too (the routed
      `format_at_caret_or_selection` path — the GUI peer, not only RPC).
  (F) `pending_style` distinguishes an *armed* mark from a merely *inherited*
      style; `clear-mark` drops the mark; `style_at_caret` is null at the base.

Run from the workspace root:
    cargo build -p hello-textarea --release
    python3 tools/demos/r951_textarea_typing_marks.py
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
END = len(SEED)

RED = (0xD0, 0x28, 0x28)
BOLD = 700
NORMAL = 400
FMT_BOLD_TAG = "fmt_toolbar#0"  # the Bold control (routed format command)


def text(ta: RpcSubprocess) -> str:
    return ta.query("/external/text")


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


def weight_at(ta: RpcSubprocess, byte: int) -> int | None:
    r = run_at(ta, byte)
    return None if r is None else r["style"]["font_weight"]


def color_at(ta: RpcSubprocess, byte: int) -> tuple[int, int, int] | None:
    r = run_at(ta, byte)
    if r is None:
        return None
    c = r["style"]["fg_color"]
    return (c["r"], c["g"], c["b"])


def caret_style(ta: RpcSubprocess) -> dict | None:
    """The next-char style: `null` (field base) or the full TextStyle object."""
    return ta.query("/external/style_at_caret")


def pending(ta: RpcSubprocess) -> dict | None:
    """The *armed* mark only: `null` when the next char merely inherits."""
    return ta.query("/external/pending_style")


def pending_weight(ta: RpcSubprocess) -> int | None:
    p = pending(ta)
    return None if p is None else p["font_weight"]


def chord(ta: RpcSubprocess, key: str, **mods: bool) -> bool:
    return ta.invoke("/external/key", {"key": key, **mods})


def type_char(ta: RpcSubprocess, ch: str) -> bool:
    return ta.invoke("/external/key", ch)


def undo(ta: RpcSubprocess) -> bool:
    return chord(ta, "z", ctrl=True)


def redo_shift(ta: RpcSubprocess) -> bool:
    return chord(ta, "z", ctrl=True, shift=True)


def settle_seed(ta: RpcSubprocess) -> None:
    """Drop any armed mark (a navigation) and drain the journal to the seed
    floor, so each phase starts from a known, byte-identical document."""
    chord(ta, "Home", ctrl=True)  # navigation → clears any pending mark
    for _ in range(6):
        if text(ta) == SEED and run_spans(ta) == SEED_SPANS:
            break
        undo(ta)
    wait_until(
        lambda: text(ta) == SEED and run_spans(ta) == SEED_SPANS,
        desc="settled at the seed floor",
    )
    assert pending(ta) is None, "no mark armed at the seed floor"


def body() -> None:
    with RpcSubprocess("hello-textarea", request_timeout=12.0) as ta:
        # ── boot: seeded styled document, field focusable ──────────────
        assert_eq(text(ta), SEED, "seeded multi-line text")
        assert_eq(run_spans(ta), SEED_SPANS, "three seed runs")
        assert_eq(color_at(ta, 0), RED, "'first' is red")
        ta.request("focus/set", {"tag": TA_TAG})
        wait_query(ta, "/external/state", "Focused", desc="field focused")
        bold_rect = abs_rects_of(ta.snapshot(source="paint", viewport=VIEWPORT)).get(FMT_BOLD_TAG)
        assert bold_rect is not None, "the Bold toolbar control has an absolute rect"

        # ── (A) AI round-trip: read style_at_caret, flip weight, write back ──
        # Caret right after the red "first" run: style_at_caret inherits red.
        chord(ta, "Home")  # collapse to line start (and drop any stray mark)
        ta.intervene("/external/selection", None)  # ensure collapsed
        wait_query(ta, "/external/state", "Focused", desc="still focused")
        ta.invoke("/external/key", "ArrowRight")  # caret 1
        for _ in range(4):
            ta.invoke("/external/key", "ArrowRight")  # caret 5 = right edge of "first"
        wait_query(ta, "/external/caret", 5, desc="caret at the right edge of 'first'")
        base = caret_style(ta)
        assert base is not None, "style_at_caret inherits the red 'first' run from the left"
        assert_eq(base["font_weight"], NORMAL, "inherited weight is normal")
        assert_eq(
            (base["fg_color"]["r"], base["fg_color"]["g"], base["fg_color"]["b"]),
            RED,
            "inherited colour is red",
        )
        assert pending(ta) is None, "inherited style is not an *armed* mark"
        # Mutate the read: bold, keep the colour + size (the canonical round-trip).
        marked = dict(base)
        marked["font_weight"] = BOLD
        assert ta.invoke("/external/mark", {"style": marked}) is True, "mark armed via RPC"
        wait_until(lambda: pending_weight(ta) == BOLD, desc="pending mark is now bold")
        assert_eq(
            pending(ta)["font_size_px"], base["font_size_px"], "the mark kept the inherited size"
        )
        cs = caret_style(ta)
        assert_eq(cs["font_weight"], BOLD, "style_at_caret reflects the armed bold mark")
        # Type: the inserted text carries the mark.
        type_char(ta, "Z")
        wait_query(ta, "/external/text", SEED[:5] + "Z" + SEED[5:], desc="typed a marked 'Z'")
        assert_eq(weight_at(ta, 5), BOLD, "the typed 'Z' is bold (the armed mark)")
        assert_eq(color_at(ta, 5), RED, "the typed 'Z' kept the round-tripped red colour")

        # ── (B) the mark continues, then a navigation drops it ─────────
        type_char(ta, "Q")
        wait_query(ta, "/external/text", SEED[:5] + "ZQ" + SEED[5:], desc="typed a second marked char")
        assert_eq(weight_at(ta, 6), BOLD, "the second char is bold too")
        assert pending(ta) is not None, "the mark stays armed across keystrokes"
        ta.invoke("/external/key", "ArrowRight")  # a navigation
        wait_until(lambda: pending(ta) is None, desc="moving the caret drops the mark")
        settle_seed(ta)

        # ── (C) undo / redo of marked typing (the inserted_runs headline) ──
        chord(ta, "End", ctrl=True)
        wait_query(ta, "/external/caret", END, desc="caret at the document end (unstyled)")
        assert caret_style(ta) is None, "style_at_caret is the field base at an unstyled edge"
        # Arm bold from the base (no left style) by writing a sized bold style.
        assert ta.invoke("/external/mark", {"font_weight": BOLD, "font_size_px": 16}) is True, "mark armed"
        type_char(ta, "X")
        type_char(ta, "Y")
        wait_query(ta, "/external/text", SEED + "XY", desc="typed 'XY' with the mark")
        assert_eq(weight_at(ta, END), BOLD, "'X' is bold")
        assert_eq(weight_at(ta, END + 1), BOLD, "'Y' is bold (one coalesced run)")
        assert_eq(run_at(ta, END)["end"], END + 2, "the two marked chars are one run")
        assert undo(ta) is True, "Ctrl+Z recognized"
        wait_query(ta, "/external/text", SEED, desc="one undo removes the coalesced typing")
        assert run_at(ta, END) is None, "undo removed the bold run with the bytes"
        assert redo_shift(ta) is True, "Ctrl+Shift+Z recognized"
        wait_query(ta, "/external/text", SEED + "XY", desc="redo restores the text")
        assert_eq(weight_at(ta, END), BOLD, "redo restored the typed mark, not just the text")
        assert_eq(weight_at(ta, END + 1), BOLD, "the whole redone run is bold")
        settle_seed(ta)

        # ── (D) an edit (Backspace) PRESERVES the mark (Word convention) ──
        chord(ta, "End", ctrl=True)
        assert ta.invoke("/external/mark", {"font_weight": BOLD, "font_size_px": 16}) is True, "mark armed"
        type_char(ta, "A")
        wait_query(ta, "/external/text", SEED + "A", desc="typed a marked 'A'")
        assert_eq(weight_at(ta, END), BOLD, "'A' is bold")
        chord(ta, "Backspace")
        wait_query(ta, "/external/text", SEED, desc="Backspace deleted the 'A'")
        assert pending_weight(ta) == BOLD, "an edit keeps the mark armed (Word)"
        type_char(ta, "B")
        wait_query(ta, "/external/text", SEED + "B", desc="re-typed after the edit")
        assert_eq(weight_at(ta, END), BOLD, "the preserved mark styled the re-typed char")
        settle_seed(ta)

        # ── (E) the toolbar Bold CLICK arms the mark at a collapsed caret ──
        chord(ta, "End", ctrl=True)
        wait_query(ta, "/external/caret", END, desc="collapsed caret at the end")
        assert pending(ta) is None, "no mark before the click"
        bx, by, bw, bh = bold_rect
        ta.click(at=(bx + bw / 2, by + bh / 2))
        wait_until(lambda: pending_weight(ta) == BOLD, desc="the Bold click armed a typing mark")
        assert_eq(ta.query("/external/state"), "Focused", "the toolbar click did not blur the field")
        type_char(ta, "G")
        wait_query(ta, "/external/text", SEED + "G", desc="typed after the toolbar-armed mark")
        assert_eq(weight_at(ta, END), BOLD, "the toolbar-armed mark styled the typed char")
        assert run_at(ta, END)["style"]["font_size_px"] > 0, "the toolbar mark carried a real size"
        settle_seed(ta)

        # ── (F) pending vs inherited + clear-mark + base ───────────────
        chord(ta, "Home", ctrl=True)
        wait_query(ta, "/external/caret", 0, desc="caret at the document start")
        assert caret_style(ta) is None, "no char to the left -> field base (null)"
        # Inheriting (not armed): caret just inside the red run.
        ta.invoke("/external/key", "ArrowRight")
        wait_query(ta, "/external/caret", 1, desc="caret one into 'first'")
        assert caret_style(ta) is not None, "style_at_caret inherits the red run"
        assert pending(ta) is None, "inheriting is not the same as an armed mark"
        # Arm, then clear-mark drops it.
        assert ta.invoke("/external/mark", {"font_weight": BOLD, "font_size_px": 16}) is True, "mark armed"
        wait_until(lambda: pending_weight(ta) == BOLD, desc="mark armed for the clear test")
        assert ta.invoke("/external/clear-mark", None) is True, "clear-mark handled"
        assert pending(ta) is None, "clear-mark dropped the mark"
        # And a bare field reports unbound, not an error.
        settle_seed(ta)
        assert_eq(text(ta), SEED, "document is byte-identical to the seed at the end")
        assert_eq(run_spans(ta), SEED_SPANS, "the seed runs are whole at the end")


if __name__ == "__main__":
    sys.exit(run_demo("R951 §5.36 — textarea active typing marks", body))
