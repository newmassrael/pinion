#!/usr/bin/env python3
"""R809 §5.16 §5.50 — WAI-ARIA APG 6.13 Tree keyboard model completed.

Drives `hello-tree-view` over JSON-RPC and verifies the keyboard axes
R673 declared as *future work*, now completed at R809, plus the
clamp-not-wrap vertical motion (delegated to the shared `clamp_nav`
SSOT) and type-ahead (delegated to the `pinion_shell::typeahead`
substrate the TreeView is the named future consumer of):

  (A) substrate shape + initial visible rows + initial focus on `src`.
  (B) Arrow Up / Down clamp at the ends (NO wrap — WAI-ARIA: a tree
      has ends).
  (C) Arrow Right on an already-expanded branch descends to the first
      child (R673 future axis).
  (D) Arrow Left on a leaf / collapsed branch ascends to the parent
      (R673 future axis); on an expanded branch it collapses.
  (E) Arrow Right on a collapsed branch expands it.
  (F) Page Up / Page Down jump a viewport-ful, clamped.
  (G) Type-ahead jumps to the next visible row whose label matches.
  (H) Space toggles a branch; leaves are no-ops on expand keys.

Focus is observed via the M3 focused-row state-layer fill (the same
discriminator the R673 demo uses); navigation effects are polled with
`wait_until` so the demo is deterministic on the observed post-key
frame rather than wall-clock (R705 / R694 flake lesson).
"""

from __future__ import annotations

import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    find_by_tag,
    run_demo,
    wait_until,
)

VIEWPORT = (480, 400)
# Send every key at a fixed in-window point; apply_key routes to the
# focused widget (the tree's primary), so the exact coordinate is
# irrelevant — it only has to land inside the window.
AT = (10.0, 10.0)
# The typeahead substrate clears its search buffer after 500 ms
# (TYPEAHEAD_TIMEOUT); space distinct single-letter searches beyond
# that so each is a fresh cyclic search rather than a growing prefix.
TYPEAHEAD_GAP = 0.6


def _walk_for_text(node, needle: str) -> bool:
    if not isinstance(node, dict):
        return False
    if node.get("type") == "Text":
        content = node.get("content")
        if isinstance(content, str) and needle in content:
            return True
    return any(_walk_for_text(c, needle) for c in node.get("children") or [])


def _row_tags(snapshot) -> set[str]:
    tags: set[str] = set()
    if isinstance(snapshot, dict):
        if snapshot.get("type") == "Container":
            tag = snapshot.get("tag")
            if isinstance(tag, str) and tag.startswith("file_tree#"):
                tags.add(tag)
        for child in snapshot.get("children") or []:
            tags |= _row_tags(child)
    return tags


def _focused_row_id(snapshot) -> str | None:
    """Id of the row carrying the M3 focus-highlight fill (non-zero,
    non-white background) — the keyboard cursor row."""
    if not isinstance(snapshot, dict):
        return None
    if snapshot.get("type") == "Container":
        style = snapshot.get("style") or {}
        fill = style.get("fill") or {}
        alpha = int(fill.get("a") or 0)
        rgb_sum = int(fill.get("r") or 0) + int(fill.get("g") or 0) + int(fill.get("b") or 0)
        tag = snapshot.get("tag")
        if (
            alpha != 0
            and 0 < rgb_sum < 255 * 3
            and isinstance(tag, str)
            and tag.startswith("file_tree#")
        ):
            return tag.removeprefix("file_tree#")
    for child in snapshot.get("children") or []:
        found = _focused_row_id(child)
        if found is not None:
            return found
    return None


def body() -> None:
    with RpcSubprocess("hello-tree-view", boot_grace=1.5) as tf:

        def snap():
            return tf.snapshot(source="paint", viewport=VIEWPORT)

        def focus():
            return _focused_row_id(snap())

        def rows():
            return _row_tags(snap())

        def press(name: str) -> None:
            tf.key(at=AT, name=name)

        def await_focus(expected: str) -> None:
            wait_until(lambda: focus() == expected, desc=f"focus == {expected}")
            assert focus() == expected, f"focus must be {expected}; got {focus()!r}"

        def await_row_present(tag: str) -> None:
            wait_until(lambda: tag in rows(), desc=f"{tag} visible")
            assert tag in rows(), f"{tag} must be visible; rows={sorted(rows())!r}"

        def await_row_absent(tag: str) -> None:
            wait_until(lambda: tag not in rows(), desc=f"{tag} hidden")
            assert tag not in rows(), f"{tag} must be hidden; rows={sorted(rows())!r}"

        # ── (A) substrate shape + initial state ─────────────────────
        assert find_by_tag(snap(), "file_tree") is not None, "TreeView root tag present"
        for tag in (
            "file_tree#src",
            "file_tree#src/main.rs",
            "file_tree#src/lib.rs",
            "file_tree#src/widgets",
            "file_tree#tests",
            "file_tree#docs",
        ):
            assert tag in rows(), f"initial paint must include {tag}; rows={sorted(rows())!r}"
        assert "file_tree#tests/integration.rs" not in rows(), "collapsed tests hides children"
        assert "file_tree#src/widgets/tree_view.rs" not in rows(), "collapsed widgets hides children"
        assert focus() == "src", f"initial focus on src; got {focus()!r}"

        # R820.1 — the tree is a single tab stop with a focus gate
        # (apply_key returns false unless the tree root is focused), so
        # focus it before driving keys, like every other gated demo.
        tf.request("focus/set", {"tag": "tree_root"})

        # ── (B) Arrow Up clamps at the first row (no wrap) ──────────
        press("ArrowUp")
        await_focus("src")  # stays on src, does NOT wrap to docs

        # ── Arrow Down advances ─────────────────────────────────────
        press("ArrowDown")
        await_focus("src/main.rs")

        # ── (C) Arrow Right on the expanded src descends to 1st child
        press("Home")
        await_focus("src")
        press("ArrowRight")  # src already expanded → first child
        await_focus("src/main.rs")

        # ── (D) Arrow Left on a leaf ascends to the parent ──────────
        press("ArrowLeft")
        await_focus("src")

        # ── (D) Arrow Left on the expanded src collapses it ─────────
        press("ArrowLeft")
        await_row_absent("file_tree#src/main.rs")
        await_focus("src")

        # ── (E) Arrow Right on the collapsed src re-expands it ──────
        press("ArrowRight")
        await_row_present("file_tree#src/main.rs")

        # ── (D) Arrow Left on a collapsed branch ascends to parent ──
        press("ArrowDown")  # src/main.rs
        press("ArrowDown")  # src/lib.rs
        press("ArrowDown")  # src/widgets (collapsed branch under src)
        await_focus("src/widgets")
        press("ArrowLeft")  # collapsed branch → parent
        await_focus("src")

        # ── End clamps to last; Down on last clamps (no wrap) ───────
        press("End")
        await_focus("docs")
        press("ArrowDown")
        await_focus("docs")  # clamp at the bottom

        # ── (F) Page Up / Page Down jump clamped across the listing ─
        press("Home")
        await_focus("src")
        press("PageDown")
        await_focus("docs")  # page > listing → clamp to last
        press("PageUp")
        await_focus("src")  # clamp to first

        # ── (G) type-ahead jumps by label ───────────────────────────
        # wall-clock semantic: each gap lets the widget's 500 ms
        # TYPEAHEAD_TIMEOUT (Instant-based, pinion-shell::typeahead)
        # expire so every press below is a FRESH single-letter search,
        # not a growing prefix. This is test INPUT, not a settle wait.
        time.sleep(TYPEAHEAD_GAP)
        press("t")
        await_focus("tests")
        time.sleep(TYPEAHEAD_GAP)
        press("d")
        await_focus("docs")
        time.sleep(TYPEAHEAD_GAP)
        press("w")
        await_focus("src/widgets")  # label "widgets"
        time.sleep(TYPEAHEAD_GAP)
        press("s")
        await_focus("src")  # label "src"

        # ── (H) Space toggles the focused branch ────────────────────
        await_focus("src")
        press("Space")
        await_row_absent("file_tree#src/main.rs")  # collapse
        press("Space")
        await_row_present("file_tree#src/main.rs")  # re-expand

        # ── (H) leaves are no-ops on expand keys ────────────────────
        press("Home")
        await_focus("src")
        press("ArrowDown")
        await_focus("src/main.rs")
        rows_before = rows()
        press("ArrowRight")  # leaf → no row change
        # No-op verification: the dispatch commits before the RPC
        # response, so a plain read after the key IS the post-key state.
        assert rows() == rows_before, "Arrow Right on a leaf must not change visible rows"
        press("Space")  # leaf → no row change
        assert rows() == rows_before, "Space on a leaf must not change visible rows"

        # ── (I) header + footer chrome ──────────────────────────────
        s = snap()
        assert _walk_for_text(s, "File explorer"), "header text renders"
        assert _walk_for_text(s, "navigate"), "footer navigate hint renders"
        assert _walk_for_text(s, "PgUp"), "footer advertises the page keys"


if __name__ == "__main__":
    sys.exit(run_demo("R809 §5.16 §5.50 — WAI-ARIA Tree keyboard complete", body))
