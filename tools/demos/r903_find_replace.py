#!/usr/bin/env python3
"""R903 §5.22 §5.52 — find & replace over the TextFieldExternal find surface.

Find & replace is the editor-grade depth slice on the R56 text substrate. The
find session (needle + case / whole-word flags) lives on the editor's own
`TextEditState`, so the editor is self-describing to AI introspection:
`scene/<tag>/external/find_matches` reports the editor's own count / ranges /
current index. The "current match" is the selection (the browser / VS Code
model) — `find-next` searches forward from the selection and lands it on the
hit, so repeated calls walk every match and wrap.

This demo drives the whole feature over RPC — §2 #2's "RPC headless is the
primary path" — and verifies, all without OCR:

1. **Self-describing find state** — `find_matches` reports count + every byte
   range + the current index; the visible `find_status` bar (scene-as-data)
   shows the same "{n} of {N}".
2. **Toggle semantics** — one needle "red" against the seed
   "Red bored credit red end" yields 4 substring matches, 3 case-sensitive
   (drops "Red"), 2 whole-word (drops the "red" inside "bored" / "credit"),
   1 case-sensitive AND whole-word.
3. **Navigation** — find-next / find-prev walk the matches and wrap; the
   selection (and `find_matches.current`) tracks the active match.
4. **Replace** — replace-current replaces the match under the selection and
   advances (and selects-then-replaces in two presses when off a match);
   replace-all rewrites every match.
5. **Replace All is ONE undo step** — the first consumer of the undo
   substrate's macro-transaction axis (`begin_macro` / `end_macro`): a single
   Ctrl+Z reverses every replacement, and Ctrl+Shift+Z redoes them.
6. **Live-pixel** — a `PINION_SCREENSHOT` boot frame rasterises (the seed text
   renders in the field with the boot needle active); the highlight bands' data
   is `find_matches` itself, verified structurally above
   ([[introspection-from-paint-not-screen]]).

Run from the workspace root:
    cargo build -p hello-find-replace --release
    python3 tools/demos/r903_find_replace.py
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
FIND_STATUS_TAG = "find_status"
EDITOR_TAG = "find_editor"
VIEWPORT = (480, 200)

SEED = "Red bored credit red end"
# Substring "red" (case-insensitive): "Red", the "red" inside "bo[red]" and
# "c[red]it", and the standalone "red".
SUBSTRING_RANGES = [[0, 3], [6, 9], [11, 14], [17, 20]]
# Case-sensitive drops "Red".
CASE_RANGES = [[6, 9], [11, 14], [17, 20]]
# Whole-word keeps only the two standalone words.
WORD_RANGES = [[0, 3], [17, 20]]
# Case-sensitive AND whole-word → just the lowercase standalone "red".
WORD_CASE_RANGES = [[17, 20]]


def find_matches(ed: RpcSubprocess) -> dict:
    return ed.query("/external/find_matches")


def match_count(ed: RpcSubprocess) -> int:
    return find_matches(ed)["count"]


def text(ed: RpcSubprocess) -> str:
    return ed.query("/external/text")


def status_text(ed: RpcSubprocess) -> str | None:
    """The visible find-status bar content (scene-as-data)."""
    snap = ed.snapshot(source="paint", viewport=VIEWPORT)
    node = find_by_tag(snap, FIND_STATUS_TAG)
    assert node is not None, "find-status bar present in the paint scene"
    stack = [node]
    while stack:
        n = stack.pop()
        if isinstance(n, dict):
            if n.get("type") == "Text" and isinstance(n.get("content"), str):
                return n["content"]
            stack.extend(n.get("children") or [])
    return None


def ctrl_key(ed: RpcSubprocess, key: str, shift: bool = False) -> None:
    ed.invoke("/external/key", {"key": key, "ctrl": True, "shift": shift})


def capture_screenshot() -> Path:
    out = Path(tempfile.mkdtemp(prefix="pinion-r903-")) / "find.png"
    binary = WORKSPACE_ROOT / "target" / "release" / "hello-find-replace"
    cmd = [str(binary)] if binary.exists() else [
        "cargo", "run", "-p", "hello-find-replace", "--quiet", "--release",
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
    with RpcSubprocess("hello-find-replace", request_timeout=12.0) as ed:
        # ── Phase 1 — seed + boot find state (self-describing) ────────────
        assert_eq(text(ed), SEED, "seeded document")
        assert_eq(ed.query("/external/find_query"), "red", "boot needle 'red'")
        assert_eq(ed.query("/external/find_case_sensitive"), False, "case off at boot")
        assert_eq(ed.query("/external/find_whole_word"), False, "whole-word off at boot")
        m = find_matches(ed)
        assert_eq(m["count"], 4, "4 substring matches at boot")
        assert_eq(m["ranges"], SUBSTRING_RANGES, "boot match ranges")
        assert_eq(m["current"], None, "no current match before navigating")
        assert_eq(status_text(ed), 'Find "red": 4 matches', "status bar shows bare count")
        field_rect = abs_rects_of(ed.snapshot(source="paint", viewport=VIEWPORT))[EDITOR_TAG]

        # ── Phase 2 — toggle semantics (one needle, four answers) ─────────
        ed.intervene("/external/find_case_sensitive", True)
        wait_until(lambda: match_count(ed) == 3, desc="case-sensitive drops 'Red' → 3")
        assert_eq(find_matches(ed)["ranges"], CASE_RANGES, "case-sensitive ranges")

        ed.intervene("/external/find_case_sensitive", False)
        ed.intervene("/external/find_whole_word", True)
        wait_until(lambda: match_count(ed) == 2, desc="whole-word → 2")
        assert_eq(find_matches(ed)["ranges"], WORD_RANGES, "whole-word ranges")
        assert_eq(status_text(ed), 'Find "red": 2 matches', "status reflects whole-word count")

        ed.intervene("/external/find_case_sensitive", True)
        wait_until(lambda: match_count(ed) == 1, desc="case-sensitive + whole-word → 1")
        assert_eq(find_matches(ed)["ranges"], WORD_CASE_RANGES, "case+word range")

        # Reset both toggles → back to the 4 substring matches.
        ed.intervene("/external/find_case_sensitive", False)
        ed.intervene("/external/find_whole_word", False)
        wait_until(lambda: match_count(ed) == 4, desc="toggles reset → 4")

        # ── Phase 3 — navigation (the selection IS the cursor) ────────────
        ed.intervene("/external/caret", 0)
        ed.intervene("/external/selection", None)
        assert_eq(
            ed.invoke("/external/find-next", None),
            {"start": 0, "end": 3},
            "find-next selects the first match from caret 0",
        )
        assert_eq(ed.query("/external/selection"), {"start": 0, "end": 3}, "selection on match 0")
        assert_eq(find_matches(ed)["current"], 0, "current index 0")
        assert_eq(status_text(ed), 'Find "red": 1 of 4', "status shows 1 of 4")
        assert_eq(ed.invoke("/external/find-next", None), {"start": 6, "end": 9}, "advance to 2nd")
        assert_eq(ed.invoke("/external/find-next", None), {"start": 11, "end": 14}, "3rd")
        assert_eq(ed.invoke("/external/find-next", None), {"start": 17, "end": 20}, "4th")
        assert_eq(ed.invoke("/external/find-next", None), {"start": 0, "end": 3}, "wraps to 1st")
        assert_eq(ed.invoke("/external/find-prev", None), {"start": 17, "end": 20}, "prev wraps to last")
        assert_eq(find_matches(ed)["current"], 3, "current index 3 after prev-wrap")

        # ── Phase 4 — replace current (two-press find-then-replace) ───────
        ed.intervene("/external/caret", 4)  # 'b' of bored — not on a match
        ed.intervene("/external/selection", None)
        assert_eq(
            ed.invoke("/external/replace", "Q"),
            False,
            "off a match: replace only selects the next match (no edit)",
        )
        assert_eq(text(ed), SEED, "text unchanged by the selecting press")
        assert_eq(ed.query("/external/selection"), {"start": 6, "end": 9}, "selected next match")
        assert_eq(
            ed.invoke("/external/replace", "Q"),
            True,
            "on a match: replace performs the edit",
        )
        assert_eq(text(ed), "Red boQ credit red end", "replaced the match under the selection")
        ctrl_key(ed, "z")
        wait_until(lambda: text(ed) == SEED, desc="Ctrl+Z reverts the single replace")

        # ── Phase 5 — replace all is ONE undo step (the macro axis) ───────
        assert_eq(match_count(ed), 4, "back to 4 matches after undo")
        assert_eq(ed.invoke("/external/replace-all", "X"), 4, "replace-all returns the count")
        assert_eq(text(ed), "X boX cXit X end", "every match rewritten")
        assert_eq(ed.invoke("/external/replace-all", "X"), 0, "idempotent: no 'red' left")
        # ONE Ctrl+Z reverses all four replacements — the begin/end_macro fold.
        ctrl_key(ed, "z")
        wait_until(
            lambda: text(ed) == SEED,
            desc="a single Ctrl+Z reverses the whole Replace All (macro transaction)",
        )
        # And one Ctrl+Shift+Z redoes the whole batch.
        ctrl_key(ed, "z", shift=True)
        wait_until(lambda: text(ed) == "X boX cXit X end", desc="Ctrl+Shift+Z redoes the batch")
        ctrl_key(ed, "z")  # leave the buffer at the seed
        wait_until(lambda: text(ed) == SEED, desc="restored to the seed")

        # ── Phase 6 — bare-needle clears the find state ───────────────────
        ed.intervene("/external/find_query", "")
        wait_until(lambda: match_count(ed) == 0, desc="empty needle → no matches")
        assert status_text(ed).startswith("Find: (set a needle"), "status shows the hint"
        ed.intervene("/external/find_query", "red")
        wait_until(lambda: match_count(ed) == 4, desc="restoring the needle re-finds 4")

    # ── Phase 7 — live-pixel: the boot frame rasterises ───────────────────
    assert field_rect is not None
    img = read_png_rgba8(capture_screenshot())
    assert_eq((img.width, img.height), VIEWPORT, "screenshot matches viewport")
    # The field renders its seed text: the field band carries non-background
    # ink (a real rasterisation smoke test — the highlight bands' correctness is
    # verified structurally above, their data being `find_matches` itself).
    fx, fy, fw, fh = field_rect
    bg = png_pixel(img, max(0, fx + fw - 4), fy + fh // 2)[:3]
    ink_pixels = 0
    for yy in range(max(0, fy + 4), min(img.height, fy + fh - 4)):
        for xx in range(max(0, fx + 4), min(img.width, fx + fw - 4)):
            r, g, b, _a = png_pixel(img, xx, yy)
            if (r - bg[0]) ** 2 + (g - bg[1]) ** 2 + (b - bg[2]) ** 2 > 900:
                ink_pixels += 1
    assert ink_pixels > 50, f"field renders glyph/highlight ink ({ink_pixels} non-bg pixels)"


if __name__ == "__main__":
    sys.exit(run_demo("R903 find & replace", body))
