#!/usr/bin/env python3
"""R713 §5.36 styled-run text substrate demo — RPC introspection + live-pixel.

The first RichText (`Text.rich`) widget: a single `TextNode` whose `content` is
one string painted with inline multi-style runs via `StyleRun`. The base style
owns the uncovered bytes; each run fully restyles its UTF-8 byte range (colour /
weight / italic / size). The paint adapter already emitted one Vello glyph run
per parley run, so R713 only added the styled-run *build* path to
`pinion-text::LayoutCache` — this demo proves both the build and the wire.

Two complementary verifications, per [[ai-first-rpc-introspection-obligation]]
+ [[introspection-from-paint-not-screen]]:

  Phase 1 — STRUCTURE (scene/snapshot, >=30 assertions). The paragraph's
    `runs` array is read back as data — each span's byte range and resolved
    style (colour, weight, italic, size) — with no OCR (§2 #7). Toggling the
    fox-emphasis bit is observed as the third run's colour + weight flipping
    in the snapshot, proving the Off/On bit reaches the styled-run build.

  Phase 2 — PIXELS (PINION_SCREENSHOT, producer-parity headless render). The
    rendered paragraph band is scanned and the three literal run colours
    (purple / saddle-brown / teal) are each located within an AA tolerance. A
    single-style regression would paint one ink colour and these asserts would
    fail loudly. The headless screenshot uses the same `to_vello_cached`
    rasterizer the live window does (producer parity), so a pixel is a
    faithful witness.

Run from the workspace root:
    cargo build -p hello-richtext --release
    python3 tools/demos/r713_richtext.py
"""

from __future__ import annotations

import os
import subprocess
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    WORKSPACE_ROOT,
    abs_rects_of,
    assert_eq,
    find_by_tag,
    png_pixel,
    read_png_rgba8,
    run_demo,
)

VIEWPORT = (480, 240)
PARA_TAG = "rich_para"

# SSOT mirror of examples/hello-richtext/src/main.rs byte ranges + colours.
QUICK = (4, 9)
BROWN = (10, 15)
FOX = (16, 19)
PARA_FONT_PX = 22
FOX_FONT_PX = 30

EMPH_PURPLE = (0x7C, 0x3A, 0xED)
SADDLE_BROWN = (0x8B, 0x45, 0x13)
FOX_TEAL = (0x0D, 0x94, 0x88)
FOX_RED = (0xD1, 0x1D, 0x2A)

WEIGHT_NORMAL = 400
WEIGHT_BOLD = 700


def _color(style: dict) -> tuple[int, int, int]:
    c = style["fg_color"]
    return (c["r"], c["g"], c["b"])


def _para(snap) -> dict:
    node = find_by_tag(snap, PARA_TAG)
    assert node is not None, f"tag {PARA_TAG!r} present in paint snapshot"
    return node


def capture_screenshot() -> Path:
    """Render hello-richtext's initial (Off / Idle) frame to RGBA8 PNG via
    `PINION_SCREENSHOT`, bypassing winit. Producer parity: same
    `to_vello_cached` rasterizer the live window uses. Off => fox is teal."""
    out = Path(tempfile.mkdtemp(prefix="pinion-r713-")) / "richtext.png"
    binary = WORKSPACE_ROOT / "target" / "release" / "hello-richtext"
    cmd = [str(binary)] if binary.exists() else [
        "cargo", "run", "-p", "hello-richtext", "--quiet", "--release",
    ]
    env = os.environ.copy()
    env["PINION_SCREENSHOT"] = str(out)
    res = subprocess.run(
        cmd, cwd=WORKSPACE_ROOT, env=env,
        capture_output=True, text=True, check=False, timeout=120.0,
    )
    if res.returncode != 0:
        raise AssertionError(
            f"PINION_SCREENSHOT capture exited {res.returncode}:\n"
            f"  stderr: {res.stderr!r}"
        )
    if not out.exists():
        raise AssertionError(f"PINION_SCREENSHOT produced no file at {out}")
    return out


def body() -> None:
    # ── Phase 1 — structural introspection ───────────────────────────
    para_rect = None
    with RpcSubprocess("hello-richtext") as d:
        snap = d.snapshot(source="paint", viewport=VIEWPORT)
        para = _para(snap)

        # One logical string, three inline styled runs.
        assert_eq(para["content"], "The quick brown fox", "paragraph content")
        runs = para["runs"]
        assert_eq(len(runs), 3, "styled-run count")

        # Base style owns the uncovered bytes ("The " + spaces).
        assert_eq(para["style"]["font_size_px"], PARA_FONT_PX, "base font size")

        # Run 0 — "quick": bold purple.
        assert_eq(runs[0]["start"], QUICK[0], "quick start byte")
        assert_eq(runs[0]["end"], QUICK[1], "quick end byte")
        assert_eq(_color(runs[0]["style"]), EMPH_PURPLE, "quick colour (purple)")
        assert_eq(runs[0]["style"]["font_weight"], WEIGHT_BOLD, "quick weight (bold)")
        assert_eq(runs[0]["style"]["font_style"], "Normal", "quick is upright")

        # Run 1 — "brown": italic saddle-brown.
        assert_eq(runs[1]["start"], BROWN[0], "brown start byte")
        assert_eq(runs[1]["end"], BROWN[1], "brown end byte")
        assert_eq(_color(runs[1]["style"]), SADDLE_BROWN, "brown colour")
        assert_eq(runs[1]["style"]["font_style"], "Italic", "brown is italic")
        assert_eq(runs[1]["style"]["font_weight"], WEIGHT_NORMAL, "brown weight (regular)")

        # Run 2 — "fox": larger; teal + regular weight while Off.
        assert_eq(runs[2]["start"], FOX[0], "fox start byte")
        assert_eq(runs[2]["end"], FOX[1], "fox end byte")
        assert_eq(runs[2]["style"]["font_size_px"], FOX_FONT_PX, "fox is larger")
        assert_eq(_color(runs[2]["style"]), FOX_TEAL, "fox colour Off (teal)")
        assert_eq(runs[2]["style"]["font_weight"], WEIGHT_NORMAL, "fox weight Off (regular)")

        # The runs are non-overlapping and ascend in byte order.
        assert runs[0]["end"] <= runs[1]["start"], "quick precedes brown"
        assert runs[1]["end"] <= runs[2]["start"], "brown precedes fox"

        # Toggle handle state before flipping.
        assert_eq(d.query("/external/state"), "Idle", "initial /external/state")
        assert_eq(d.query("/external/value"), False, "initial /external/value (Off)")

        # Capture the paragraph's absolute rect for the pixel phase.
        rects = abs_rects_of(snap)
        assert PARA_TAG in rects, "paragraph has an absolute rect"
        para_rect = rects[PARA_TAG]

        # ── Toggle Off -> On: only the fox run flips ──────────────────
        d.click(path="main_toggle")
        assert_eq(d.query("/external/value"), True, "/external/value after click (On)")

        snap_on = d.snapshot(source="paint", viewport=VIEWPORT)
        para_on = _para(snap_on)
        runs_on = para_on["runs"]
        assert_eq(len(runs_on), 3, "run count unchanged after toggle")
        # Fox run flipped to bold red.
        assert_eq(_color(runs_on[2]["style"]), FOX_RED, "fox colour On (red)")
        assert_eq(runs_on[2]["style"]["font_weight"], WEIGHT_BOLD, "fox weight On (bold)")
        # The other two runs are mode-independent.
        assert_eq(_color(runs_on[0]["style"]), EMPH_PURPLE, "quick unchanged after toggle")
        assert_eq(_color(runs_on[1]["style"]), SADDLE_BROWN, "brown unchanged after toggle")
        assert_eq(para_on["content"], "The quick brown fox", "content unchanged")

    # ── Phase 2 — live-pixel: three distinct run inks render ─────────
    assert para_rect is not None
    png = capture_screenshot()
    img = read_png_rgba8(png)
    assert_eq((img.width, img.height), VIEWPORT, "screenshot matches viewport")

    px, py, pw, ph = para_rect
    x0, y0 = max(0, px), max(0, py)
    x1, y1 = min(img.width, px + pw), min(img.height, py + ph)

    def min_dist_to(target: tuple[int, int, int]) -> float:
        best = 1e9
        for yy in range(y0, y1):
            for xx in range(x0, x1):
                r, g, b, _a = png_pixel(img, xx, yy)
                d2 = (r - target[0]) ** 2 + (g - target[1]) ** 2 + (b - target[2]) ** 2
                if d2 < best:
                    best = d2
        return best ** 0.5

    # The three literal run colours are mutually >170 apart in RGB, so a
    # tolerance of 110 cannot cross-match; a glyph core within 110 of a
    # target is genuinely that ink. A single-style regression would leave
    # only the base ink and these would fail.
    for target, label in (
        (EMPH_PURPLE, "purple 'quick'"),
        (SADDLE_BROWN, "brown 'brown'"),
        (FOX_TEAL, "teal 'fox' (Off frame)"),
    ):
        dist = min_dist_to(target)
        assert dist < 110.0, (
            f"{label} ink not found in paragraph band "
            f"(nearest pixel distance {dist:.1f} >= 110)"
        )


if __name__ == "__main__":
    sys.exit(run_demo("R713 styled-run text substrate", body))
