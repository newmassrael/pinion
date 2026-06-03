#!/usr/bin/env python3
"""R769 §5.36 §5.22 — field-level merge (mergeCharFormat): bold/italic toggle.

R768 added wholesale `setCharFormat` (a colour swatch *replaces* a range's
style). R769 adds the canonical "make the selection bold while keeping its
colour" — `TextEditState::merge_style_run`, which transforms only the
field(s) the toggle touches: a covered byte keeps its run's other style,
an unstyled byte resolves against the field base before the transform.

hello-textarea's toolbar gains **B** / **I** toggle buttons next to the
colour swatches. This demo proves, all without OCR:

1. **Toolbar present** (`scene/snapshot` from:paint) — bold + italic
   buttons and four swatches in a row.
2. **Merge keeps colour** (`scene/click` a toggle over a coloured word)
   — the run's `font_weight` flips to 700 while `fg_color` is unchanged
   (the mergeCharFormat point a wholesale overlay would destroy).
3. **Toggle is reversible** — a second **B** click returns weight to 400.
4. **Orthogonal fields** — bold then italic the same word leaves both set.
5. **Unstyled bytes resolve against base** — bolding plain text
   materialises a run (base ink + weight 700).
6. **Metric-aware geometry** — after bolding the whole document a click
   still lands on the right byte (the `field_shaping` SSOT keeps paint +
   hit-test one layout even when a run changes glyph metrics).
7. **Runs-only** — every toggle leaves the text buffer byte-identical.
8. **Live-pixel** — a boot-frame `PINION_SCREENSHOT` renders the swatch
   inks + the three seed inks ([[introspection-from-paint-not-screen]]).

Run from the workspace root:
    cargo build -p hello-textarea --release
    python3 tools/demos/r769_textarea_toggle.py
"""

from __future__ import annotations

import os
import subprocess
import sys
import tempfile
import time
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
)

WORKSPACE_ROOT = Path(__file__).resolve().parent.parent.parent
TA_TAG = "main_textarea"
VIEWPORT = (480, 320)
SETTLE = 0.06

RED = (0xD0, 0x28, 0x28)
GREEN = (0x1F, 0x8A, 0x34)
BLUE = (0x26, 0x4C, 0xD8)
SWATCHES = [("swatch_red", RED), ("swatch_green", GREEN), ("swatch_blue", BLUE)]
SW_CLEAR = "swatch_clear"
TB_BOLD = "toggle_bold"
TB_ITALIC = "toggle_italic"

SEED_TEXT = "first line\nsecond line\nthird line"


def text(ta: RpcSubprocess) -> str:
    return ta.query("/external/text")


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
    snap = ta.snapshot(source="paint", viewport=VIEWPORT)
    field = find_by_tag(snap, TA_TAG)
    assert field is not None, "textarea present in paint scene"
    texts = [n for n in _walk(field, []) if n.get("type") == "Text" and n.get("runs")]
    assert texts, "a painted Text node carries styled runs"
    return texts[0]["runs"]


def run_at(ta: RpcSubprocess, byte: int) -> dict | None:
    for r in field_runs(ta):
        if r["start"] <= byte < r["end"]:
            return r
    return None


def caret(ta: RpcSubprocess) -> int:
    return ta.query("/external/caret")


def select(ta: RpcSubprocess, start: int, end: int) -> None:
    ta.intervene("/external/selection", {"start": start, "end": end})
    time.sleep(SETTLE)


def click_tag(ta: RpcSubprocess, rect: tuple[int, int, int, int]) -> None:
    x, y, w, h = rect
    ta.click(at=(x + w / 2, y + h / 2))
    time.sleep(SETTLE)


def capture_screenshot() -> Path:
    out = Path(tempfile.mkdtemp(prefix="pinion-r769-")) / "toggle.png"
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
    sw_rects: dict[str, tuple[int, int, int, int]] = {}
    field_rect = None
    with RpcSubprocess("hello-textarea", request_timeout=12.0) as ta:
        # ── Phase 1 — toolbar present (toggles + swatches) ────────────
        assert_eq(text(ta), SEED_TEXT, "seeded text")
        rects = abs_rects_of(ta.snapshot(source="paint", viewport=VIEWPORT))
        for tag in (TB_BOLD, TB_ITALIC):
            assert tag in rects, f"{tag} toggle button has a rect"
            sw_rects[tag] = rects[tag]
        for tag, _ink in SWATCHES:
            assert tag in rects, f"{tag} swatch has a rect"
            sw_rects[tag] = rects[tag]
        sw_rects[SW_CLEAR] = rects[SW_CLEAR]
        # Bold + italic sit left of the colour swatches (ascending x row).
        order = [TB_BOLD, TB_ITALIC, "swatch_red", "swatch_green", "swatch_blue", SW_CLEAR]
        xs = [sw_rects[t][0] for t in order]
        assert xs == sorted(xs) and len(set(xs)) == 6, f"six toolbar controls in a row ({xs})"
        field_rect = rects[TA_TAG]

        assert_eq(_color(run_at(ta, 0)["style"]), RED, "'first' seed is red")
        assert_eq(run_at(ta, 0)["style"]["font_weight"], 400, "'first' seed is normal weight")

        ta.request("focus/set", {"tag": TA_TAG})
        time.sleep(0.05)
        assert_eq(ta.query("/external/state"), "Focused", "field focused")

        # ── Phase 2 — bold the red "first": weight flips, colour kept ──
        select(ta, 0, 5)
        click_tag(ta, sw_rects[TB_BOLD])
        first = run_at(ta, 0)
        assert_eq(first["style"]["font_weight"], 700, "merge made 'first' bold")
        assert_eq(_color(first["style"]), RED, "bold KEPT the red colour (mergeCharFormat point)")
        assert_eq((first["start"], first["end"]), (0, 5), "the run span is unchanged")
        assert_eq(text(ta), SEED_TEXT, "toggle never edits the text")

        # ── Phase 3 — second B click toggles bold back off ────────────
        select(ta, 0, 5)
        click_tag(ta, sw_rects[TB_BOLD])
        assert_eq(run_at(ta, 0)["style"]["font_weight"], 400, "second B click un-bolds")
        assert_eq(_color(run_at(ta, 0)["style"]), RED, "un-bold still keeps red")

        # ── Phase 4 — bold + italic the green "second": both set ──────
        select(ta, 11, 17)
        click_tag(ta, sw_rects[TB_BOLD])
        select(ta, 11, 17)
        click_tag(ta, sw_rects[TB_ITALIC])
        second = run_at(ta, 11)
        assert_eq(second["style"]["font_weight"], 700, "'second' is bold")
        assert_eq(second["style"]["font_style"], "Italic", "'second' is also italic (orthogonal)")
        assert_eq(_color(second["style"]), GREEN, "'second' kept its green through both merges")

        # ── Phase 5 — bold UNSTYLED bytes resolves against base ───────
        # " line" after "first" (bytes 5..10) carries no seed run.
        assert run_at(ta, 7) is None, "byte 7 starts unstyled"
        select(ta, 5, 10)
        click_tag(ta, sw_rects[TB_BOLD])
        gap = run_at(ta, 7)
        assert gap is not None, "bolding plain text materialised a run"
        assert_eq(gap["style"]["font_weight"], 700, "materialised run is bold")
        assert _color(gap["style"]) not in (RED, GREEN, BLUE), "gap took the field base ink, not a seed colour"

        # ── Phase 6 — metric-aware geometry: bold all, click still lands
        select(ta, 0, len(SEED_TEXT))
        click_tag(ta, sw_rects[TB_BOLD])  # toggle: some already bold → policy by start (byte 0 bold→unbold)
        select(ta, 0, len(SEED_TEXT))
        click_tag(ta, sw_rects[TB_BOLD])  # ensure all bold now
        fx, fy = field_rect[0], field_rect[1]
        ta.click(at=(fx + 28, fy + 8 + 25 + 12))  # into bold "second" line
        time.sleep(SETTLE)
        c = caret(ta)
        assert 11 <= c <= 17, f"click on bold 'second' lands inside its run (byte {c} in 11..17)"
        assert_eq(text(ta), SEED_TEXT, "every toggle left the text byte-identical")

    # ── Phase 7 — live-pixel: swatches + seed inks render at boot ─────
    assert field_rect is not None
    img = read_png_rgba8(capture_screenshot())
    assert_eq((img.width, img.height), VIEWPORT, "screenshot matches viewport")

    def min_dist(target, rect, pad=4) -> float:
        x0, y0, w, h = rect
        best = 1e9
        for yy in range(max(0, y0 + pad), min(img.height, y0 + h - pad)):
            for xx in range(max(0, x0 + pad), min(img.width, x0 + w - pad)):
                r, g, b, _a = png_pixel(img, xx, yy)
                best = min(best, (r - target[0]) ** 2 + (g - target[1]) ** 2 + (b - target[2]) ** 2)
        return best ** 0.5

    for tag, ink in SWATCHES:
        dist = min_dist(ink, sw_rects[tag])
        assert dist < 60.0, f"{tag} renders its ink (nearest {dist:.1f} >= 60)"

    fx, fy, _fw, _fh = field_rect

    def min_dist_band(target, row: int) -> float:
        cy = fy + 8 + 25 * row + 12
        best = 1e9
        for yy in range(max(0, cy - 9), min(img.height, cy + 9)):
            for xx in range(max(0, fx + 9), min(img.width, fx + 60)):
                r, g, b, _a = png_pixel(img, xx, yy)
                best = min(best, (r - target[0]) ** 2 + (g - target[1]) ** 2 + (b - target[2]) ** 2)
        return best ** 0.5

    for (target, label, row) in ((RED, "red 'first'", 0), (GREEN, "green 'second'", 1), (BLUE, "blue 'third'", 2)):
        dist = min_dist_band(target, row)
        assert dist < 120.0, f"{label} seed ink not in its band (nearest {dist:.1f} >= 120)"


if __name__ == "__main__":
    sys.exit(run_demo("R769 TextArea bold/italic toggle (mergeCharFormat)", body))
