#!/usr/bin/env python3
"""R799 §5.36 §5.22 §5.38 — the formatting toolbar as a framework-routed,
non-focusable `ToolbarExternal` (the reflective-toolbar substrate decision).

Pre-R799 hello-textarea hand-rolled its toolbar: tagged `BoxNode`
decorations plus a manual `hit_tag` / `try_toolbar_press` rect-scan in the
caret press hook — an application re-implementing the InputRouter (an
R47-class smell). R799 makes the strip a real widget: a non-focusable
`ToolbarExternal` extra-external whose six controls paint with the composite
tags `fmt_toolbar#<i>`, so the InputRouter dispatches a click to the
External (which emits a `"command"` intent the reducer maps to a format op
on the live selection). Two framework axes compose to give the editor's
canonical behaviour with no new substrate:

  * routing (InputRouter, by composite tag) applies the format, and
  * focus (the `focusable_tags()` enumeration) — the strip is *not* a
    member, so the W3C / the toolkit-`NoFocus` rule the shell encodes means clicking
    a control never steals the field's focus; the selection survives.

The toggle "pressed" state is reflective: the B / I cell paints a tonal
fill read from the *selection's* style (`style_at`), not an owned bit, so
the toolbar mirrors the document.

  (A) six controls route as `fmt_toolbar#<i>` in a row.
  (B) a routed click bolds the selection, keeps focus, preserves selection.
  (C) italic / colour swatch / clear all route the same way.
  (D) reflective pressed — the B cell fill tracks whether the selection
      is bold.
  (E) a click with no selection is a no-op (formatting needs a range).

Run from the workspace root:
    cargo build -p hello-textarea --release
    python3 tools/demos/r799_textarea_format_toolbar.py
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
FMT = "fmt_toolbar"
VIEWPORT = (480, 320)
SEED = "first line\nsecond line\nthird line"
RED = (0xD0, 0x28, 0x28)
GREEN = (0x1F, 0x8A, 0x34)


def ctrl(i: int) -> str:
    return f"{FMT}#{i}"


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
    return texts[0]["runs"] if texts else []


def run_at(ta: RpcSubprocess, byte: int) -> dict | None:
    for r in field_runs(ta):
        if r["start"] <= byte < r["end"]:
            return r
    return None


def ink(run: dict) -> tuple[int, int, int]:
    c = run["style"]["fg_color"]
    return (c["r"], c["g"], c["b"])


def cell_fill(ta: RpcSubprocess, i: int) -> tuple[int, int, int]:
    snap = ta.snapshot(source="paint", viewport=VIEWPORT)
    node = find_by_tag(snap, ctrl(i))
    assert node is not None, f"{ctrl(i)} present in paint"
    f = node["style"]["fill"]
    return (f["r"], f["g"], f["b"])


def select(ta: RpcSubprocess, s: int, e: int) -> None:
    ta.intervene("/external/selection", {"start": s, "end": e})
    wait_query(
        ta, "/external/selection", {"start": s, "end": e},
        desc=f"selection {s}..{e} set",
    )


def click(ta: RpcSubprocess, rect: tuple[int, int, int, int]) -> None:
    x, y, w, h = rect
    ta.click(at=(x + w / 2, y + h / 2))
    # The click's effect is gated at each call site (R883 zero-flake).


def body() -> None:
    with RpcSubprocess("hello-textarea", request_timeout=12.0) as ta:
        # ── (A) six controls route as `fmt_toolbar#<i>` in a row ──────
        assert_eq(text(ta), SEED, "seeded text")
        rects = abs_rects_of(ta.snapshot(source="paint", viewport=VIEWPORT))
        assert TA_TAG in rects, "the field has a rect"
        assert FMT in rects, "the toolbar strip carries its primary scope tag"
        for i in range(6):
            assert ctrl(i) in rects, f"control {ctrl(i)} has a routed rect"
        assert_eq(
            len([k for k in rects if k.startswith(f"{FMT}#")]), 6, "exactly six routed controls"
        )
        xs = [rects[ctrl(i)][0] for i in range(6)]
        assert xs == sorted(xs) and len(set(xs)) == 6, f"six controls in a row ({xs})"

        ta.request("focus/set", {"tag": TA_TAG})
        wait_query(ta, "/external/state", "Focused", desc="field focused")

        # ── (B) routed bold click: applies, keeps focus + selection ───
        select(ta, 0, 5)  # the red "first"
        assert_eq(ink(run_at(ta, 0)), RED, "'first' is red before")
        assert_eq(run_at(ta, 0)["style"]["font_weight"], 400, "'first' normal weight before")
        click(ta, rects[ctrl(0)])  # bold
        wait_until(
            lambda: (r := run_at(ta, 0)) is not None and r["style"]["font_weight"] == 700,
            desc="routed click bolded the selection",
        )
        assert_eq(ink(run_at(ta, 0)), RED, "merge kept the red colour (mergeCharFormat)")
        assert_eq(ta.query("/external/state"), "Focused", "the toolbar click did not steal focus")
        assert_eq(ta.query("/external/selection"), {"start": 0, "end": 5}, "selection survived the click")
        assert_eq(text(ta), SEED, "formatting never edits the text buffer")
        click(ta, rects[ctrl(0)])  # bold again → off
        wait_until(
            lambda: (r := run_at(ta, 0)) is not None and r["style"]["font_weight"] == 400,
            desc="a second click toggles bold off",
        )

        # ── (C) italic / colour swatch / clear all route the same way ──
        select(ta, 0, 5)
        click(ta, rects[ctrl(1)])  # italic
        wait_until(
            lambda: (r := run_at(ta, 0)) is not None and r["style"]["font_style"] == "Italic",
            desc="routed italic click",
        )
        assert_eq(ink(run_at(ta, 0)), RED, "italic kept the colour too")
        click(ta, rects[ctrl(1)])  # italic off (reset)
        wait_until(
            lambda: (r := run_at(ta, 0)) is not None and r["style"]["font_style"] == "Normal",
            desc="italic toggled back off",
        )

        select(ta, 11, 17)  # the green "second"
        assert_eq(ink(run_at(ta, 11)), GREEN, "'second' is green before the swatch")
        click(ta, rects[ctrl(2)])  # red swatch
        wait_until(
            lambda: (r := run_at(ta, 11)) is not None and ink(r) == RED,
            desc="red swatch recoloured the selection",
        )
        assert_eq(text(ta), SEED, "the colour swatch never edits the text")
        select(ta, 11, 17)
        click(ta, rects[ctrl(5)])  # clear swatch
        wait_until(
            lambda: run_at(ta, 11) is None,
            desc="clear swatch stripped the run",
        )

        # ── (D) reflective pressed: the B cell tracks the selection ───
        select(ta, 23, 28)  # the blue "third" (not bold)
        inactive = cell_fill(ta, 0)
        assert_eq(cell_fill(ta, 1), inactive, "B and I share the inactive fill when neither applies")
        click(ta, rects[ctrl(0)])  # bold the selection
        wait_until(
            lambda: (r := run_at(ta, 23)) is not None and r["style"]["font_weight"] == 700,
            desc="'third' bolded",
        )
        active = cell_fill(ta, 0)
        assert active != inactive, f"B cell paints a reflective active fill ({active} vs {inactive})"
        assert_eq(cell_fill(ta, 1), inactive, "the I cell stays inactive (selection is not italic)")
        click(ta, rects[ctrl(0)])  # un-bold (reset)
        wait_until(
            lambda: cell_fill(ta, 0) == inactive,
            desc="un-bolding returns the B cell to the inactive fill",
        )

        # ── (E) a click with no selection is a no-op ──────────────────
        ta.invoke("/external/key", {"key": "Home", "ctrl": True})  # collapse to caret 0
        wait_until(
            lambda: ta.query("/external/selection") is None,
            desc="no active selection",
        )
        before = len(field_runs(ta))
        click(ta, rects[ctrl(0)])  # bold with no selection
        # No-op verification: the dispatch commits before the response,
        # so the unchanged runs are readable directly (no gate possible).
        assert_eq(len(field_runs(ta)), before, "a no-selection format click changes nothing")
        assert_eq(ta.query("/external/state"), "Focused", "still focused after the no-op click")


if __name__ == "__main__":
    sys.exit(run_demo("R799 §5.36 — framework-routed reflective format toolbar", body))
