#!/usr/bin/env python3
"""R933 §5.36 — code-folding end-to-end RPC verification demo.

Code folding lets brace-delimited blocks collapse so a reader hides a body
and keeps the structure. R933 rides the existing R56 text substrate plus
one stored
view-state field (the collapsed set) and a derive-on-read accessor,
[`TextEditState::fold_regions`] — the latest member of the
`find_matches` (R903) / `style_runs` (R904) / `matching_bracket` (R926)
family: it re-derives the foldable regions from the live buffer (reusing
the R926 bracket scan as `match_forward`), read by the paint gutter AND
the `scene/<tag>/external/fold_regions` RPC, so the rendered fold and the
wire answer can never disagree.

This demo drives the `hello-code-fold` fold viewer entirely over RPC —
sets the buffer with `intervene`, folds with `invoke`, reads the fold set
with `query`, and confirms the *painted* code panel actually drops the
hidden rows — §2 #2's "RPC headless is the primary path".

## The verification idea: a deterministic view transform, no wall-clock

`fold_regions` is a pure function of (text, collapsed-set), so the demo
asserts exact regions / visible rows, never timing — every state read is
gated through `wait_until` (observed-state polling, ZERO-FLAKE):

  (A) Wire shape — fold_regions lists the two brace blocks (outer fn body,
      inner if body) with exact start/end lines, all expanded.
  (B) toggle-fold collapses a block; the read AND the painted gutter drop
      the interior rows, keeping the opener row (with its `…` placeholder).
  (C) toggling again expands; rows return.
  (D) the inner block folds independently of the outer.
  (E) collapsing over the caret reanchors it to the opener line (a fold
      never strands the caret on a hidden line).
  (F) fold-all / unfold-all return the resulting collapsed count and the
      paint reflects it.
  (G) toggle-fold on a non-opener line is a no-op (False); a negative line
      is rejected.
  (H) a stale anchor (its `{` edited away) prunes — no panic, no region.
  (I) the status line reads "{visible}/{total} lines visible, {k} folded".
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any, Iterator, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    run_demo,
    wait_until,
)

# hello-code-fold's TF_TAG; the single primary External answers at /external.
_EXT = "/external"
_EDITOR_TAG = "code_editor"

# The example's seed (set explicitly for determinism). Logical lines:
#   0: fn main() {        <- outer opener
#   1:     let x = 1;
#   2:     if x > 0 {      <- inner opener
#   3:         log(x);
#   4:     }               <- inner closer
#   5: }                   <- outer closer
_SEED = "fn main() {\n    let x = 1;\n    if x > 0 {\n        log(x);\n    }\n}"
# Byte offset of the end of logical line 0 ("fn main() {" is 11 bytes), the
# spot the caret reanchors to when the outer block collapses over it.
_LINE0_END = 11


def _set_text(ed: RpcSubprocess, text: str) -> None:
    ed.intervene(f"{_EXT}/text", text)
    wait_until(lambda: ed.query(f"{_EXT}/text") == text, desc=f"buffer becomes {text!r}")


def _regions(ed: RpcSubprocess) -> list[dict[str, Any]]:
    r = ed.query(f"{_EXT}/fold_regions")
    assert isinstance(r, list), f"fold_regions must be a JSON array, got {r!r}"
    return r


def _collapsed_lines(ed: RpcSubprocess) -> set[int]:
    return {r["start_line"] for r in _regions(ed) if r["collapsed"]}


def _wait_collapsed(ed: RpcSubprocess, expected: set[int]) -> None:
    wait_until(
        lambda: _collapsed_lines(ed) == expected,
        desc=f"collapsed openers == {sorted(expected)}",
    )


def _walk(node: Any) -> Iterator[dict[str, Any]]:
    stack = [node]
    while stack:
        n = stack.pop()
        if not isinstance(n, dict):
            continue
        yield n
        stack.extend(n.get("children") or [])
        content = n.get("content")
        if isinstance(content, dict):
            stack.append(content)


def _visible_rows(ed: RpcSubprocess) -> set[int]:
    """The logical-line indices whose row container is in the paint scene."""
    snap = ed.snapshot(source="paint")
    rows: set[int] = set()
    for n in _walk(snap):
        tag = n.get("tag")
        if isinstance(tag, str) and tag.startswith("fold_row_"):
            rows.add(int(tag[len("fold_row_") :]))
    return rows


def _wait_visible(ed: RpcSubprocess, expected: set[int]) -> None:
    wait_until(
        lambda: _visible_rows(ed) == expected,
        desc=f"painted rows == {sorted(expected)}",
    )


def _status_text(ed: RpcSubprocess) -> Optional[str]:
    snap = ed.snapshot(source="paint")
    for n in _walk(snap):
        if n.get("tag") == "fold_status":
            for m in _walk(n):
                if m.get("type") == "Text":
                    return m.get("content")
    return None


def _region_by_start(regions: list[dict[str, Any]], start: int) -> dict[str, Any]:
    for r in regions:
        if r["start_line"] == start:
            return r
    raise AssertionError(f"no fold region opens on line {start}: {regions!r}")


_ALL_ROWS = {0, 1, 2, 3, 4, 5}


def body() -> None:
    with RpcSubprocess("hello-code-fold", request_timeout=12.0) as ed:
        _set_text(ed, _SEED)

        # ── (A) Wire shape — two brace blocks, both expanded ─────────
        regions = _regions(ed)
        assert len(regions) == 2, f"expected outer+inner blocks, got {regions!r}"
        outer = _region_by_start(regions, 0)
        inner = _region_by_start(regions, 2)
        assert outer["end_line"] == 5, f"outer closes on line 5: {outer!r}"
        assert inner["end_line"] == 4, f"inner closes on line 4: {inner!r}"
        assert outer["collapsed"] is False, "outer starts expanded"
        assert inner["collapsed"] is False, "inner starts expanded"
        # Opener byte offsets land on a real '{'.
        assert _SEED.encode()[outer["open"]] == ord("{"), "outer open byte is '{'"
        assert _SEED.encode()[outer["close"]] == ord("}"), "outer close byte is '}'"
        # Nothing folded yet → every row painted.
        _wait_visible(ed, _ALL_ROWS)

        # ── (B) Collapse the outer block ─────────────────────────────
        assert ed.invoke(f"{_EXT}/toggle-fold", 0) is True, "outer toggled"
        _wait_collapsed(ed, {0})
        assert _region_by_start(_regions(ed), 0)["collapsed"] is True, "outer now collapsed"
        # Interior lines 1..5 hide; only the opener row remains painted.
        _wait_visible(ed, {0})

        # ── (C) Expand again — rows return ───────────────────────────
        assert ed.invoke(f"{_EXT}/toggle-fold", 0) is True, "outer toggled back"
        _wait_collapsed(ed, set())
        _wait_visible(ed, _ALL_ROWS)

        # ── (D) Inner block folds independently ──────────────────────
        assert ed.invoke(f"{_EXT}/toggle-fold", 2) is True, "inner toggled"
        _wait_collapsed(ed, {2})
        # Inner hides lines 3..4; the rest (0,1,2,5) stay visible.
        _wait_visible(ed, {0, 1, 2, 5})
        assert ed.invoke(f"{_EXT}/toggle-fold", 2) is True, "inner toggled back"
        _wait_collapsed(ed, set())

        # ── (E) Collapse reanchors the caret out of the hidden body ──
        ed.intervene(f"{_EXT}/caret", 45)  # a byte inside the interior (line 3)
        wait_until(lambda: ed.query(f"{_EXT}/caret") == 45, desc="caret in interior")
        assert ed.invoke(f"{_EXT}/toggle-fold", 0) is True, "collapse over caret"
        wait_until(
            lambda: ed.query(f"{_EXT}/caret") == _LINE0_END,
            desc="caret reanchored to the opener line end",
        )
        assert ed.query(f"{_EXT}/caret") == _LINE0_END, "caret pulled to a visible line"
        # restore
        assert ed.invoke(f"{_EXT}/toggle-fold", 0) is True, "expand again"
        _wait_collapsed(ed, set())

        # ── (F) fold-all / unfold-all return the collapsed count ─────
        assert ed.invoke(f"{_EXT}/fold-all", None) == 2, "fold-all collapses both blocks"
        _wait_collapsed(ed, {0, 2})
        # The outer collapse subsumes everything → only the opener row shows.
        _wait_visible(ed, {0})
        assert ed.invoke(f"{_EXT}/unfold-all", None) == 0, "unfold-all expands all"
        _wait_collapsed(ed, set())
        _wait_visible(ed, _ALL_ROWS)

        # ── (G) Non-opener line is a no-op; negative line rejected ───
        assert ed.invoke(f"{_EXT}/toggle-fold", 1) is False, "line 1 opens no block"
        assert _collapsed_lines(ed) == set(), "the no-op folded nothing"
        rejected = False
        try:
            ed.invoke(f"{_EXT}/toggle-fold", -1)
        except RpcError:
            rejected = True
        assert rejected, "a negative line is rejected, not silently folded"

        # ── (H) Stale anchor prunes when its brace is edited away ────
        assert ed.invoke(f"{_EXT}/fold-all", None) == 2, "collapse both"
        _wait_collapsed(ed, {0, 2})
        _set_text(ed, "let y = 1;")  # no braces at all
        wait_until(lambda: _regions(ed) == [], desc="stale anchors pruned")
        assert _regions(ed) == [], "no braces → no foldable region, no panic"
        # The single line paints normally.
        _wait_visible(ed, {0})

        # ── (I) Status line is the visible mirror of the fold set ────
        _set_text(ed, _SEED)
        _wait_visible(ed, _ALL_ROWS)
        wait_until(
            lambda: _status_text(ed) == "6/6 lines visible, 0 folded",
            desc="status shows all lines, none folded",
        )
        assert ed.invoke(f"{_EXT}/toggle-fold", 0) is True, "fold for status check"
        _wait_visible(ed, {0})
        wait_until(
            lambda: _status_text(ed) == "1/6 lines visible, 1 folded",
            desc="status shows the collapse",
        )
        assert _status_text(ed) == "1/6 lines visible, 1 folded", "status mirrors the fold"


if __name__ == "__main__":
    raise SystemExit(run_demo("r933_code_fold", body))
