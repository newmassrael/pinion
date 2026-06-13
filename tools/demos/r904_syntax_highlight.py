#!/usr/bin/env python3
"""R904 §5.22 §5.36 — syntax highlighting derived from buffer content.

A code editor styles its text by *tokenising the content*, not by manual
formatting. The field attaches a highlighter once
(`TextEditState::attach_highlighter` wrapping `pinion_core::syntax::highlight_code`
over a C-family keyword set), and from then on `style_runs()` re-derives the
colouring from the live text on every read — the single accessor paint, caret
geometry, hit-test, and this RPC all share, so the AI reads the editor's token
colouring directly.

This demo drives it over RPC and verifies, all without OCR:

1. **Derived runs** — `scene/style_runs` reports one run per coloured token of
   the seeded line "let n = 42 + \"x\" // sum": the `let` keyword, the `42`
   number, the `"x"` string, the `// sum` comment — each at the exact byte range
   and the exact palette colour, at the field's font size.
2. **Status mirror** — the visible "{n} tokens highlighted" bar (scene-as-data)
   matches the run count.
3. **Re-highlight on edit** — writing new text (intervene) and typing keys
   (invoke) re-derive the colouring; identifiers/punctuation get no run.
4. **Live-pixel** — a `PINION_SCREENSHOT` boot frame rasterises the highlighted
   code (the runs' data is `style_runs` itself, verified structurally above).

Run from the workspace root:
    cargo build -p hello-syntax-highlight --release
    python3 tools/demos/r904_syntax_highlight.py
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
    wait_until,
)

WORKSPACE_ROOT = Path(__file__).resolve().parent.parent.parent
STATUS_TAG = "syntax_status"
EDITOR_TAG = "code_editor"
VIEWPORT = (480, 200)

SEED = 'let n = 42 + "x" // sum'

# The classic palette (pinion_core::syntax::SyntaxPalette::classic), asserted by
# the exact bytes (theme-independent — the syntax scheme is fixed).
KEYWORD = (0x00, 0x00, 0xC0)
STRING = (0xA3, 0x15, 0x15)
COMMENT = (0x00, 0x80, 0x00)
NUMBER = (0x09, 0x86, 0x58)
FONT_PX = 18


def runs(ed: RpcSubprocess) -> list[dict]:
    return ed.query("/external/style_runs")


def spans(ed: RpcSubprocess) -> list[tuple[int, int]]:
    return [(r["start"], r["end"]) for r in runs(ed)]


def color(run: dict) -> tuple[int, int, int]:
    c = run["style"]["fg_color"]
    return (c["r"], c["g"], c["b"])


def set_text(ed: RpcSubprocess, text: str, expect_runs: int) -> None:
    ed.intervene("/external/text", text)
    wait_until(lambda: len(runs(ed)) == expect_runs, desc=f"{text!r} -> {expect_runs} runs")


def status_text(ed: RpcSubprocess) -> str | None:
    snap = ed.snapshot(source="paint", viewport=VIEWPORT)
    node = find_by_tag(snap, STATUS_TAG)
    assert node is not None, "syntax-status bar present in the paint scene"
    stack = [node]
    while stack:
        n = stack.pop()
        if isinstance(n, dict):
            if n.get("type") == "Text" and isinstance(n.get("content"), str):
                return n["content"]
            stack.extend(n.get("children") or [])
    return None


def capture_screenshot() -> Path:
    out = Path(tempfile.mkdtemp(prefix="pinion-r904-")) / "syntax.png"
    binary = WORKSPACE_ROOT / "target" / "release" / "hello-syntax-highlight"
    cmd = [str(binary)] if binary.exists() else [
        "cargo", "run", "-p", "hello-syntax-highlight", "--quiet", "--release",
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
    with RpcSubprocess("hello-syntax-highlight", request_timeout=12.0) as ed:
        # ── Phase 1 — derived runs for the seeded code ────────────────────
        assert_eq(ed.query("/external/text"), SEED, "seeded code line")
        rs = runs(ed)
        assert_eq(len(rs), 4, "four tokens coloured")
        assert_eq(
            spans(ed),
            [(0, 3), (8, 10), (13, 16), (17, 23)],
            "keyword / number / string / comment byte ranges",
        )
        assert_eq(color(rs[0]), KEYWORD, "'let' is the keyword colour")
        assert_eq(color(rs[1]), NUMBER, "'42' is the number colour")
        assert_eq(color(rs[2]), STRING, '"x" is the string colour')
        assert_eq(color(rs[3]), COMMENT, "'// sum' is the comment colour")
        assert_eq(rs[0]["style"]["font_size_px"], FONT_PX, "runs keep the field font size")
        assert_eq(status_text(ed), "4 tokens highlighted", "status mirrors the run count")
        field_rect = abs_rects_of(ed.snapshot(source="paint", viewport=VIEWPORT))[EDITOR_TAG]

        # ── Phase 2 — re-highlight on whole-text edit (intervene) ─────────
        set_text(ed, "fn add // go", 2)
        assert_eq(spans(ed), [(0, 2), (7, 12)], "'fn' keyword + '// go' comment")
        assert_eq(color(runs(ed)[0]), KEYWORD, "'fn' keyword colour")
        assert_eq(color(runs(ed)[1]), COMMENT, "comment colour")

        set_text(ed, "let x = 99", 2)
        assert_eq(spans(ed), [(0, 3), (8, 10)], "'let' keyword + '99' number")
        assert_eq(color(runs(ed)[1]), NUMBER, "'99' number colour")

        set_text(ed, "no_tokens_here + plain;", 0)
        assert_eq(runs(ed), [], "identifiers/punctuation get no run")
        assert_eq(status_text(ed), "0 tokens highlighted", "status shows zero")

        set_text(ed, '"only a string"', 1)
        assert_eq(spans(ed), [(0, 15)], "the whole string literal is one run")
        assert_eq(color(runs(ed)[0]), STRING, "string colour")

        # ── Phase 3 — re-highlight on typed keys (invoke) ─────────────────
        ed.intervene("/external/text", "")
        wait_until(lambda: runs(ed) == [], desc="cleared buffer, no tokens")
        ed.intervene("/external/caret", 0)
        for ch in "let":
            ed.invoke("/external/key", ch)
        wait_until(
            lambda: len(runs(ed)) == 1 and spans(ed) == [(0, 3)],
            desc="typing 'let' key-by-key re-highlights the keyword",
        )
        assert_eq(color(runs(ed)[0]), KEYWORD, "typed keyword takes the keyword colour")
        assert_eq(ed.query("/external/text"), "let", "buffer holds the typed text")

        # ── Phase 4 — restore the seed for the screenshot baseline ────────
        set_text(ed, SEED, 4)

    # ── Phase 5 — live-pixel: the boot frame rasterises the highlighted code
    assert field_rect is not None
    img = read_png_rgba8(capture_screenshot())
    assert_eq((img.width, img.height), VIEWPORT, "screenshot matches viewport")
    fx, fy, fw, fh = field_rect
    bg = png_pixel(img, max(0, fx + fw - 4), fy + fh // 2)[:3]
    ink_pixels = 0
    for yy in range(max(0, fy + 4), min(img.height, fy + fh - 4)):
        for xx in range(max(0, fx + 4), min(img.width, fx + fw - 4)):
            r, g, b, _a = png_pixel(img, xx, yy)
            if (r - bg[0]) ** 2 + (g - bg[1]) ** 2 + (b - bg[2]) ** 2 > 900:
                ink_pixels += 1
    assert ink_pixels > 50, f"field rasterises the highlighted code ({ink_pixels} ink pixels)"


if __name__ == "__main__":
    sys.exit(run_demo("R904 syntax highlighting", body))
