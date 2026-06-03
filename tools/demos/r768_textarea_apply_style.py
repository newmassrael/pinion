#!/usr/bin/env python3
"""R768 §5.36 §5.22 — rich-text apply-to-selection: a colour toolbar.

R767 made styled runs *editable* (insert shifts / delete clips them).
R768 adds the canonical "select text, click a format button" operation:
`TextEditState::apply_style_run` overlays one colour run over a byte
range (Qt `setCharFormat` — carve existing runs, lay in the new span,
merge abutting identical spans), and `clear_style_runs` strips formatting
back to the base. Both are reachable two ways that funnel to the *same*
substrate method:

  * the AI-first `apply-style` / `clear-style` invoke channel on
    `TextFieldExternal` (driven here via `scene/invoke`), and
  * a colour-swatch toolbar under the field — a press on a swatch routes
    through the binding's press hook to the live selection.

hello-textarea seeds three colour runs (first=red, second=green,
third=blue) and renders four swatches (red / green / blue / clear). This
demo proves, all without OCR:

1. **Toolbar present** (`scene/snapshot` from:paint) — four swatch rects
   laid out in a row under the field.
2. **Apply / split / merge / clear** (live RPC `scene/invoke`) — overlay
   semantics on the field's painted runs: a colour inside a run splits
   it; the same colour abutting merges; clear removes coverage with no
   text shift.
3. **Toolbar click** (`scene/click` on a swatch rect) — the press-hook
   router recolours the current selection (the click peer of the invoke
   funnel).
4. **Runs-only repaint** — every apply leaves the text buffer byte-for-
   byte unchanged (formatting ⊥ content).
5. **Live-pixel** — a `PINION_SCREENSHOT` boot frame renders the three
   swatch inks in the toolbar band and the three seed inks in the field
   (the rasterizer guard a structural query cannot replace,
   [[introspection-from-paint-not-screen]], R767 precedent).

Run from the workspace root:
    cargo build -p hello-textarea --release
    python3 tools/demos/r768_textarea_apply_style.py
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

SEED_TEXT = "first line\nsecond line\nthird line"


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


def _color(style: dict) -> tuple[int, int, int]:
    c = style["fg_color"]
    return (c["r"], c["g"], c["b"])


def field_runs(ta: RpcSubprocess) -> list[dict]:
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


def run_at(ta: RpcSubprocess, byte: int) -> dict | None:
    for r in field_runs(ta):
        if r["start"] <= byte < r["end"]:
            return r
    return None


def apply_style(ta: RpcSubprocess, start: int, end: int, rgb: tuple[int, int, int]) -> bool:
    handled = ta.invoke(
        "/external/apply-style",
        {"start": start, "end": end, "fg": hex_of(rgb), "size": 16},
    )
    time.sleep(SETTLE)
    return handled


def clear_style(ta: RpcSubprocess, start: int, end: int) -> bool:
    handled = ta.invoke("/external/clear-style", {"start": start, "end": end})
    time.sleep(SETTLE)
    return handled


def select(ta: RpcSubprocess, start: int, end: int) -> None:
    ta.intervene("/external/selection", {"start": start, "end": end})
    time.sleep(SETTLE)


def capture_screenshot() -> Path:
    out = Path(tempfile.mkdtemp(prefix="pinion-r768-")) / "apply.png"
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
        # ── Phase 1 — toolbar present in the paint scene ──────────────
        assert_eq(text(ta), SEED_TEXT, "seeded text")
        rects = abs_rects_of(ta.snapshot(source="paint", viewport=VIEWPORT))
        for tag, _ink in SWATCHES:
            assert tag in rects, f"{tag} swatch has an absolute rect"
            sw_rects[tag] = rects[tag]
        assert SW_CLEAR in rects, "clear swatch has an absolute rect"
        sw_rects[SW_CLEAR] = rects[SW_CLEAR]
        # The four swatches sit in a row (distinct, ascending x).
        xs = [sw_rects[t][0] for t, _ in SWATCHES] + [sw_rects[SW_CLEAR][0]]
        assert xs == sorted(xs) and len(set(xs)) == 4, f"four swatches in a row, ascending x ({xs})"
        field_rect = rects[TA_TAG]

        # Seed runs as R767 left them.
        assert_eq(run_spans(ta), [(0, 5), (11, 17), (23, 28)], "three seed runs")
        assert_eq(_color(run_at(ta, 0)["style"]), RED, "'first' is red")
        assert_eq(_color(run_at(ta, 11)["style"]), GREEN, "'second' is green")
        assert_eq(_color(run_at(ta, 23)["style"]), BLUE, "'third' is blue")

        # ── Phase 2 — apply / split / merge / clear via the invoke funnel
        # Split: blue over [1, 3) inside the red "first" run.
        assert apply_style(ta, 1, 3, BLUE) is True, "apply-style invoke handled"
        assert_eq(
            run_spans(ta)[:3],
            [(0, 1), (1, 3), (3, 5)],
            "overlay inside a run carves it into red | blue | red",
        )
        assert_eq(_color(run_at(ta, 1)["style"]), BLUE, "middle slice is the new blue ink")
        assert_eq(_color(run_at(ta, 0)["style"]), RED, "left flank keeps red")
        assert_eq(_color(run_at(ta, 4)["style"]), RED, "right flank keeps red")
        assert_eq(text(ta), SEED_TEXT, "apply never edits the text (split)")

        # Merge: red over [3, 10) abuts the [3, 5) red flank → one span.
        assert apply_style(ta, 3, 10, RED) is True, "apply-style merge invoke handled"
        assert run_at(ta, 3) is not None and run_at(ta, 9) is not None, "red now covers [3, 10)"
        assert_eq(
            (run_at(ta, 3)["start"], run_at(ta, 3)["end"]),
            (3, 10),
            "abutting identical red merges into one [3, 10) span (no seam)",
        )
        assert_eq(_color(run_at(ta, 9)["style"]), RED, "merged span is red through byte 9")
        assert_eq(text(ta), SEED_TEXT, "apply never edits the text (merge)")

        # Clear: strip all formatting over the first line "first line\n".
        before_clear = len(field_runs(ta))
        assert clear_style(ta, 0, 11) is True, "clear-style invoke handled"
        assert all(s >= 11 for s, _ in run_spans(ta)), "no run survives in the cleared [0, 11) range"
        assert len(field_runs(ta)) < before_clear, "clearing dropped the first-line runs"
        assert_eq(text(ta), SEED_TEXT, "clear never edits the text")
        # The untouched lines keep their seed runs.
        assert_eq(_color(run_at(ta, 11)["style"]), GREEN, "'second' still green after clear")
        assert_eq(_color(run_at(ta, 23)["style"]), BLUE, "'third' still blue after clear")

        # ── Phase 3 — toolbar click recolours the selection ───────────
        ta.request("focus/set", {"tag": TA_TAG})
        time.sleep(0.05)
        assert_eq(ta.query("/external/state"), "Focused", "field focused")
        select(ta, 11, 17)  # the green "second"
        rx, ry, rw, rh = sw_rects["swatch_red"]
        ta.click(at=(rx + rw / 2, ry + rh / 2))  # click the RED swatch
        time.sleep(SETTLE)
        assert_eq(
            _color(run_at(ta, 11)["style"]),
            RED,
            "clicking the red swatch recoloured the selected 'second' word",
        )
        assert_eq(ta.query("/external/state"), "Focused", "swatch click did not blur the field")
        assert_eq(text(ta), SEED_TEXT, "toolbar click never edits the text")

        # Clear swatch over the same selection strips it back to base.
        select(ta, 11, 17)
        cx, cy, cw, ch = sw_rects[SW_CLEAR]
        ta.click(at=(cx + cw / 2, cy + ch / 2))
        time.sleep(SETTLE)
        assert run_at(ta, 13) is None, "clear swatch stripped the 'second' run"

    # ── Phase 4 — live-pixel: swatches + seed inks render ─────────────
    assert field_rect is not None
    img = read_png_rgba8(capture_screenshot())
    assert_eq((img.width, img.height), VIEWPORT, "screenshot matches viewport")

    def min_dist(target, rect, pad=4) -> float:
        x0, y0, w, h = rect
        best = 1e9
        for yy in range(max(0, y0 + pad), min(img.height, y0 + h - pad)):
            for xx in range(max(0, x0 + pad), min(img.width, x0 + w - pad)):
                r, g, b, _a = png_pixel(img, xx, yy)
                d2 = (r - target[0]) ** 2 + (g - target[1]) ** 2 + (b - target[2]) ** 2
                best = min(best, d2)
        return best ** 0.5

    # Each colour swatch renders its ink (a solid box; tol 60 is tight).
    for tag, ink in SWATCHES:
        dist = min_dist(ink, sw_rects[tag])
        assert dist < 60.0, f"{tag} renders its ink (nearest distance {dist:.1f} >= 60)"

    # The three seed inks still render in the field's line bands (boot
    # frame = pre-RPC state, so the seed runs are intact).
    fx, fy, _fw, _fh = field_rect

    def min_dist_in_band(target, row: int) -> float:
        cy = fy + 8 + 25 * row + 12
        best = 1e9
        for yy in range(max(0, cy - 9), min(img.height, cy + 9)):
            for xx in range(max(0, fx + 9), min(img.width, fx + 60)):
                r, g, b, _a = png_pixel(img, xx, yy)
                d2 = (r - target[0]) ** 2 + (g - target[1]) ** 2 + (b - target[2]) ** 2
                best = min(best, d2)
        return best ** 0.5

    for (target, label, row) in (
        (RED, "red 'first'", 0),
        (GREEN, "green 'second'", 1),
        (BLUE, "blue 'third'", 2),
    ):
        dist = min_dist_in_band(target, row)
        assert dist < 120.0, f"{label} seed ink not found in its band (nearest {dist:.1f} >= 120)"


if __name__ == "__main__":
    sys.exit(run_demo("R768 TextArea apply-to-selection", body))
