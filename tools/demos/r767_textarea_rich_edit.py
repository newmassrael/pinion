#!/usr/bin/env python3
"""R767 §5.36 §5.22 — rich-text editing: styled runs over an editable field.

R713 landed styled-run *display* (`RichText`). R767 makes styled runs
**editable**: `TextEditState` carries a `style_runs` list (the toolkit
`FormatRange` model), the field paint threads it through the
`field_shaping` SSOT so paint *and* caret / hit-test geometry shape one
identical run-aware `Layout`, and every edit maintains the runs — insert
shifts runs at/after the caret, delete clips them against the removed
range (a styled span tracks *its text*, not a fixed offset).

hello-textarea seeds three colour runs over the leading word of each
line ("first" red, "second" green, "third" blue). This demo proves, all
without OCR:

1. **Structural** (`scene/snapshot` from:paint) — the painted text node
   carries the three runs with the seeded byte ranges + inks
   (§2 #7 scene-as-data).
2. **Maintenance** (live RPC edits) — typing before "first" shifts all
   three runs right; backspacing restores them; deleting the whole
   "first" word drops its run and shifts the rest left.
3. **Live-pixel** — a `PINION_SCREENSHOT` capture renders three distinct
   inks in the three line bands (a single-style regression would show
   one ink) — the rasterizer guard a structural query cannot replace
   ([[introspection-from-paint-not-screen]], R713 precedent).

Run from the workspace root:
    cargo build -p hello-textarea --release
    python3 tools/demos/r767_textarea_rich_edit.py
"""

from __future__ import annotations

import os
import subprocess
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    find_by_tag,
    read_png_rgba8,
    png_pixel,
    run_demo,
    wait_query,
    wait_until,
)

WORKSPACE_ROOT = Path(__file__).resolve().parent.parent.parent
TA_TAG = "main_textarea"
VIEWPORT = (480, 320)

# Seeded runs (byte ranges over "first line\nsecond line\nthird line").
RED = (0xD0, 0x28, 0x28)    # "first"  [0, 5)
GREEN = (0x1F, 0x8A, 0x34)  # "second" [11, 17)
BLUE = (0x26, 0x4C, 0xD8)   # "third"  [23, 28)
SEED_RUNS = [(0, 5, RED), (11, 17, GREEN), (23, 28, BLUE)]


def caret(ta: RpcSubprocess) -> int:
    return ta.query("/external/caret")


def text(ta: RpcSubprocess) -> str:
    return ta.query("/external/text")


def key(ta: RpcSubprocess, name: str) -> None:
    ta.key(path=TA_TAG, name=name)


def key_ctrl(ta: RpcSubprocess, name: str) -> None:
    ta.modifiers(ctrl=True)
    key(ta, name)
    ta.modifiers()


def _walk(node, out):
    out.append(node)
    for c in node.get("children", []) or []:
        _walk(c, out)
    if isinstance(node.get("content"), dict):
        _walk(node["content"], out)
    return out


def _color(style: dict) -> tuple[int, int, int]:
    c = style["fg_color"]
    return (c["r"], c["g"], c["b"])


def field_runs(ta: RpcSubprocess) -> list[dict]:
    """The styled runs on the field's painted text node (from:paint)."""
    snap = ta.snapshot(source="paint", viewport=VIEWPORT)
    field = find_by_tag(snap, TA_TAG)
    assert field is not None, "textarea present in paint scene"
    texts = [
        n for n in _walk(field, [])
        if n.get("type") == "Text" and n.get("runs")
    ]
    assert texts, "a painted Text node carries styled runs"
    return texts[0]["runs"]


def run_spans(ta: RpcSubprocess) -> list[tuple[int, int]]:
    return [(r["start"], r["end"]) for r in field_runs(ta)]


def capture_screenshot() -> Path:
    """Render hello-textarea's initial frame to RGBA8 PNG via
    PINION_SCREENSHOT (same `to_vello_cached` rasterizer the live window
    uses — producer parity)."""
    out = Path(tempfile.mkdtemp(prefix="pinion-r767-")) / "rich.png"
    binary = WORKSPACE_ROOT / "target" / "release" / "hello-textarea"
    cmd = [str(binary)] if binary.exists() else [
        "cargo", "run", "-p", "hello-textarea", "--quiet", "--release",
    ]
    env = os.environ.copy()
    env["PINION_SCREENSHOT"] = str(out)
    res = subprocess.run(
        cmd, cwd=WORKSPACE_ROOT, env=env,
        capture_output=True, text=True, check=False, timeout=120.0,
    )
    if res.returncode != 0:
        raise AssertionError(f"PINION_SCREENSHOT exited {res.returncode}: {res.stderr!r}")
    if not out.exists():
        raise AssertionError(f"PINION_SCREENSHOT produced no file at {out}")
    return out


def body() -> None:
    field_rect = None
    with RpcSubprocess("hello-textarea", request_timeout=12.0) as ta:
        # ── Phase 1 — structural: the seeded runs are in the paint scene
        assert_eq(text(ta), "first line\nsecond line\nthird line", "seeded text")
        runs = field_runs(ta)
        assert_eq(len(runs), 3, "three styled runs painted")
        for i, (s, e, ink) in enumerate(SEED_RUNS):
            assert_eq(runs[i]["start"], s, f"run {i} start byte")
            assert_eq(runs[i]["end"], e, f"run {i} end byte")
            assert_eq(_color(runs[i]["style"]), ink, f"run {i} ink")
        assert runs[0]["end"] <= runs[1]["start"], "runs are non-overlapping, ascending"
        assert runs[1]["end"] <= runs[2]["start"], "runs ascend in byte order"

        # Colour-only seed: every run shares the base font size + a normal
        # weight/style — only fg_color differs (so glyph metrics, hence
        # the caret/hit-test geometry, are unchanged from single-style).
        base_size = runs[0]["style"]["font_size_px"]
        for i in range(3):
            assert_eq(runs[i]["style"]["font_size_px"], base_size, f"run {i} matches base size")
            assert_eq(runs[i]["style"]["font_weight"], 400, f"run {i} is normal weight")
            assert_eq(runs[i]["style"]["font_style"], "Normal", f"run {i} is upright")
        # The three inks are mutually distinct (a real rich paragraph).
        inks = {_color(r["style"]) for r in runs}
        assert_eq(len(inks), 3, "three distinct run inks")

        rects = abs_rects_of(ta.snapshot(source="paint", viewport=VIEWPORT))
        assert TA_TAG in rects, "field has an absolute rect"
        field_rect = rects[TA_TAG]

        ta.request("focus/set", {"tag": TA_TAG})
        wait_query(ta, "/external/state", "Focused", desc="focused")

        # Run-aware geometry: a click on the second line's styled word
        # ("second", bytes 11..17) resolves to a byte inside that span —
        # the hit-test shapes the *same* run-aware Layout the paint does
        # ([[two-text-layouts-paint-vs-geometry]]).
        fx, fy = field_rect[0], field_rect[1]
        ta.click(at=(fx + 28, fy + 8 + 25 + 12))  # line 1, ~into "second"
        wait_until(
            lambda: 11 <= caret(ta) <= 17,
            desc="click on the green 'second' word lands inside its run",
        )
        c_second = caret(ta)
        assert 11 <= c_second <= 17, (
            f"click on the green 'second' word lands inside its run (byte {c_second} in 11..17)"
        )

        # ── Phase 2 — run maintenance under live edits ────────────────
        key_ctrl(ta, "Home")  # caret to byte 0 (before "first")
        assert_eq(caret(ta), 0, "caret at document start")

        # Insert a glyph before "first": every run shifts right by 1.
        ta.text("Z", path=TA_TAG)
        wait_until(
            lambda: text(ta) == "Zfirst line\nsecond line\nthird line",
            desc="Z inserted at the start",
        )
        assert_eq(caret(ta), 1, "caret advanced past the inserted glyph")
        assert_eq(
            run_spans(ta),
            [(1, 6), (12, 18), (24, 29)],
            "insert before all runs shifts every run right by 1",
        )
        # Inks ride along unchanged through the shift.
        assert_eq(_color(field_runs(ta)[0]["style"]), RED, "first-word run keeps red after shift")

        # Backspace restores the seeded spans (clip + shift cancel out).
        key(ta, "Backspace")
        assert_eq(text(ta)[:5], "first", "Z removed")
        assert_eq(
            run_spans(ta),
            [(0, 5), (11, 17), (23, 28)],
            "backspace restores the original run spans",
        )

        # Delete the whole "first" word: its run drops, the rest shift -5.
        for _ in range(5):
            key(ta, "Delete")
        assert_eq(text(ta), " line\nsecond line\nthird line", "the word 'first' is deleted")
        assert_eq(len(field_runs(ta)), 2, "the fully-deleted styled word's run is dropped")
        assert_eq(
            run_spans(ta),
            [(6, 12), (18, 23)],
            "deleting the styled word drops its run; the rest shift left by 5",
        )
        # The surviving runs still carry their seeded inks (green, blue).
        runs2 = field_runs(ta)
        assert_eq(_color(runs2[0]["style"]), GREEN, "second-word run keeps its green ink")
        assert_eq(_color(runs2[1]["style"]), BLUE, "third-word run keeps its blue ink")

    # ── Phase 3 — live-pixel: three distinct inks render ──────────────
    assert field_rect is not None
    img = read_png_rgba8(capture_screenshot())
    assert_eq((img.width, img.height), VIEWPORT, "screenshot matches viewport")
    fx, fy, _fw, _fh = field_rect

    def min_dist_in_band(target, row: int) -> float:
        # Leading word of the given line, ~pad in from the left edge.
        cy = fy + 8 + 25 * row + 12
        best = 1e9
        for yy in range(max(0, cy - 9), min(img.height, cy + 9)):
            for xx in range(max(0, fx + 9), min(img.width, fx + 60)):
                r, g, b, _a = png_pixel(img, xx, yy)
                d2 = (r - target[0]) ** 2 + (g - target[1]) ** 2 + (b - target[2]) ** 2
                best = min(best, d2)
        return best ** 0.5

    # The three inks are mutually >150 apart in RGB; tol 120 cannot
    # cross-match. A single-style regression would leave one base ink and
    # at least two of these would fail.
    for (target, label, row) in (
        (RED, "red 'first'", 0),
        (GREEN, "green 'second'", 1),
        (BLUE, "blue 'third'", 2),
    ):
        dist = min_dist_in_band(target, row)
        assert dist < 120.0, (
            f"{label} ink not found in its line band (nearest distance {dist:.1f} >= 120)"
        )


if __name__ == "__main__":
    sys.exit(run_demo("R767 TextArea rich-text editing", body))
