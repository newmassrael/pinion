#!/usr/bin/env python3
"""R926 §5.22 — matching-bracket end-to-end RPC verification demo.

Bracket matching is the depth slice that turns a text field into a
code-navigable editor: when the caret sits next to a bracket, the editor
knows where its mate is. R926 rides the existing R56 text substrate plus
one new derive-on-read accessor —
[`TextEditState::matching_bracket`] — the third member of the
`find_matches` (R903) / `style_runs` (R904) family: a pure accessor that
re-derives `(open, close)` from the live buffer + caret, read by the
paint highlight AND the `scene/<tag>/external/bracket_match` RPC, so the
rendered outline boxes and the wire answer can never disagree.

This demo drives the existing `hello-syntax-highlight` code editor
entirely over RPC — sets the buffer + caret with `intervene`, reads the
matched pair with `query` — §2 #2's "RPC headless is the primary path".

## The verification idea: a pure derivation, no wall-clock

`matching_bracket` is a deterministic function of (text, caret), so the
demo asserts exact pairs, not timing:

  - **forward / backward**: caret after an opener matches the closer;
    caret after a closer matches back to the opener.
  - **nesting**: same-type depth counting reaches the correct mate
    through nested pairs of the same and other types.
  - **adjacency precedence**: the bracket immediately *before* the caret
    wins over one *at* the caret (the just-typed bracket).
  - **all three pairs**: (), [], {}.
  - **no match**: caret not next to a bracket, or an unbalanced bracket,
    reports `null` (distinct from a bare field, which omits the slot).
  - **multibyte safety**: a multi-byte char between brackets does not
    shift the byte offsets.

## Demo scope (>=30 assertions, sections A-G)

  (A) Wire/shape — bracket_match present, null off a bracket.
  (B) Forward + backward from a single pair.
  (C) Nesting (same + mixed bracket types).
  (D) Adjacency precedence (before-caret wins).
  (E) All three pair types + unbalanced -> null.
  (F) Multibyte offsets stay exact.
  (G) Paint: a focused caret on a bracket adds exactly two outline
      boxes to the rendered scene (delta-checked so any constant
      overlay cancels).
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    run_demo,
    wait_until,
)

# The editor widget tag (hello-syntax-highlight's TF_TAG = "code_editor").
_EDITOR_TAG = "code_editor"
_EXT = "/external"


def _set_text(ed: RpcSubprocess, text: str) -> None:
    """Set the whole buffer and wait until the readback confirms it."""
    ed.intervene(f"{_EXT}/text", text)
    wait_until(
        lambda: ed.query(f"{_EXT}/text") == text,
        desc=f"buffer becomes {text!r}",
    )


def _bracket_at(ed: RpcSubprocess, caret: int) -> Optional[dict[str, int]]:
    """Place the caret and read the matched bracket pair (or None)."""
    ed.intervene(f"{_EXT}/caret", caret)
    wait_until(
        lambda: ed.query(f"{_EXT}/caret") == caret,
        desc=f"caret moves to {caret}",
    )
    return ed.query(f"{_EXT}/bracket_match")


def _assert_pair(got: Any, open_byte: int, close_byte: int, ctx: str) -> None:
    assert isinstance(got, dict), f"{ctx}: expected a pair object, got {got!r}"
    assert got.get("open") == open_byte, f"{ctx}: open {got.get('open')} != {open_byte}"
    assert got.get("close") == close_byte, f"{ctx}: close {got.get('close')} != {close_byte}"


def _count_bordered_boxes(node: Any) -> int:
    """Count Box nodes carrying a non-null border in a paint snapshot —
    the bracket outline boxes (every other field overlay is a filled
    box). Used as a delta so any constant bordered overlay cancels."""
    total = 0
    stack = [node]
    while stack:
        n = stack.pop()
        if not isinstance(n, dict):
            continue
        if n.get("type") == "Box":
            style = n.get("style")
            if isinstance(style, dict) and style.get("border") is not None:
                total += 1
        stack.extend(n.get("children") or [])
        # Scroll nodes nest their content under "content".
        content = n.get("content")
        if isinstance(content, dict):
            stack.append(content)
    return total


def body() -> None:
    with RpcSubprocess("hello-syntax-highlight", request_timeout=12.0) as ed:
        # ── (A) Wire shape — null off a bracket ──────────────────────
        _set_text(ed, "f(x)")
        assert _bracket_at(ed, 0) is None, "caret on 'f' is not next to a bracket"
        # A bare position deep in an identifier is also null.
        _set_text(ed, "abcde")
        assert _bracket_at(ed, 3) is None, "no brackets in the buffer -> null"

        # ── (B) Forward + backward from one pair ─────────────────────
        _set_text(ed, "f(x)")
        # '(' at 1, ')' at 3.
        _assert_pair(_bracket_at(ed, 1), 1, 3, "caret after '('")
        _assert_pair(_bracket_at(ed, 2), 1, 3, "caret after '(' (before 'x') still on '('")
        _assert_pair(_bracket_at(ed, 4), 1, 3, "caret after ')' scans back to '('")
        _assert_pair(_bracket_at(ed, 3), 1, 3, "caret at ')' matches the opener")

        # ── (C) Nesting — same + mixed types ─────────────────────────
        _set_text(ed, "((()))")
        _assert_pair(_bracket_at(ed, 1), 0, 5, "outer opener -> outer closer")
        _assert_pair(_bracket_at(ed, 3), 2, 3, "innermost opener -> innermost closer")
        _set_text(ed, "{[()]}")
        _assert_pair(_bracket_at(ed, 1), 0, 5, "'{' through balanced inner pairs -> '}'")
        _assert_pair(_bracket_at(ed, 2), 1, 4, "'[' -> ']'")
        _assert_pair(_bracket_at(ed, 3), 2, 3, "'(' -> ')'")

        # ── (D) Adjacency precedence — before-caret wins ─────────────
        _set_text(ed, "(){}")
        # caret 2 sits between ')' (before) and '{' (at): the closer wins.
        _assert_pair(_bracket_at(ed, 2), 0, 1, "before-caret ')' matches back to '('")
        # caret 1 (after '(') matches its ')'.
        _assert_pair(_bracket_at(ed, 1), 0, 1, "after '(' matches ')'")

        # ── (E) All three pair types + unbalanced -> null ────────────
        _set_text(ed, "[x]")
        _assert_pair(_bracket_at(ed, 1), 0, 2, "square pair")
        _set_text(ed, "{x}")
        _assert_pair(_bracket_at(ed, 1), 0, 2, "curly pair")
        _set_text(ed, "(x)")
        _assert_pair(_bracket_at(ed, 1), 0, 2, "round pair")
        _set_text(ed, "(a")
        assert _bracket_at(ed, 1) is None, "lone opener -> null"
        _set_text(ed, "a)")
        assert _bracket_at(ed, 2) is None, "lone closer -> null"

        # ── (F) Multibyte offsets stay exact ─────────────────────────
        # "(é)" — '(' at 0, 'é' at bytes 1..3, ')' at byte 3.
        _set_text(ed, "(é)")
        _assert_pair(_bracket_at(ed, 1), 0, 3, "multibyte: '(' -> ')' across 'é'")
        _assert_pair(_bracket_at(ed, 4), 0, 3, "multibyte: caret after ')' scans back")

        # ── (G) Paint — a focused caret on a bracket outlines its pair ─
        _set_text(ed, "f(x)")
        # Focus via a click, then drive the caret precisely. The outline
        # only paints in the focused posture (a caret affordance).
        ed.click(path=_EDITOR_TAG)

        def bordered_count(caret: int) -> int:
            ed.intervene(f"{_EXT}/caret", caret)
            wait_until(
                lambda: ed.query(f"{_EXT}/caret") == caret,
                desc=f"caret -> {caret} for paint check",
            )
            snap = ed.snapshot(source="paint")
            return _count_bordered_boxes(snap)

        # Caret away from any bracket: a baseline (focus ring etc. cancel).
        off = bordered_count(0)
        # Caret adjacent to '(': the matched ( and ) each gain an outline.
        on = bordered_count(2)
        assert on - off == 2, (
            f"a focused caret on a bracket must add exactly two outline boxes; "
            f"off={off} on={on}"
        )


if __name__ == "__main__":
    raise SystemExit(run_demo("r926_bracket_match", body))
