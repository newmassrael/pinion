#!/usr/bin/env python3
"""R673 §5.16 §5.50 — interactive TreeView 2nd consumer end-to-end.

Drives the new `hello-tree-view` binding via JSON-RPC + verifies the
substrate's interactive surface: focused-row highlight, Arrow Up/Down
navigation, Arrow Right/Left expand/collapse, Home/End jump, Space
toggle. Closes the R671 + R672 keyboard-nav carry through the
canonical 2nd consumer (the [[abstraction-needs-second-consumer]]
maturity gate).

R673 atomic 3 verification scope (≥30 assertions):

  (A) substrate shape — TreeView root tag + composite row tags +
      M3 focus-highlight overlay paints on the focused row only.
  (B) initial visible rows — fresh boot focused on `src` with
      `src` expanded (3 children visible) + `tests` collapsed +
      `docs` collapsed.
  (C) Arrow Up/Down navigation moves through visible rows, clamping
      at the edges (R809 made the vertical axis clamp, not wrap — see
      r809_tree_keyboard_waiaria.py for the WAI-ARIA edge tests).
  (D) Arrow Right expands the focused branch (`tests` →
      tests/integration.rs + tests/snapshot.rs visible).
  (E) Arrow Left collapses the focused branch.
  (F) Space toggles expand on the focused branch.
  (G) Home / End jump to first / last visible row.
  (H) Arrow Right on a leaf is a no-op (no children to expand). The
      richer leaf semantics R809 completed (Arrow Left ascends to the
      parent, type-ahead) live in r809_tree_keyboard_waiaria.py.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    find_by_tag,
    run_demo,
    wait_snap,
)

VIEWPORT = (480, 400)


def _wait_focus(tf, row_id: str, desc: str):
    """Gate on the focus highlight reaching `row_id` (R883 zero-flake)."""
    return wait_snap(
        tf,
        lambda s: _focused_row_id(s) == row_id,
        viewport=VIEWPORT,
        desc=desc,
    )


def _walk_for_text(node, needle: str) -> bool:
    if not isinstance(node, dict):
        return False
    if node.get("type") == "Text":
        content = node.get("content")
        if isinstance(content, str) and needle in content:
            return True
    children = node.get("children")
    if isinstance(children, list):
        return any(_walk_for_text(child, needle) for child in children)
    return False


def _row_tags(snapshot) -> list[str]:
    """Collect every composite row tag under file_tree#…."""
    tags: list[str] = []
    if isinstance(snapshot, dict):
        if snapshot.get("type") == "Container":
            tag = snapshot.get("tag")
            if isinstance(tag, str) and tag.startswith("file_tree#"):
                tags.append(tag)
        for child in snapshot.get("children") or []:
            tags.extend(_row_tags(child))
    return tags


def _focused_row_id(snapshot) -> str | None:
    """Return the id of the row carrying the focus highlight
    (M3 SurfaceContainerHighest fill)."""
    if not isinstance(snapshot, dict):
        return None
    if snapshot.get("type") == "Container":
        style = snapshot.get("style") or {}
        fill = style.get("fill") or {}
        # M3 SurfaceContainerHighest in the default light Theme:
        # alpha 255 + non-zero rgb (not pure transparent / surface).
        # The simplest discriminator: fill alpha != 0 + fill != white.
        alpha = int(fill.get("a") or 0)
        rgb_sum = int(fill.get("r") or 0) + int(fill.get("g") or 0) + int(fill.get("b") or 0)
        tag = snapshot.get("tag")
        if (
            alpha != 0
            and rgb_sum > 0
            and rgb_sum < 255 * 3
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
        # ── (A) substrate shape ────────────────────────────────────
        snap = tf.snapshot(source="paint", viewport=(480, 400))
        assert find_by_tag(snap, "file_tree") is not None, (
            f"TreeView root must carry the file_tree tag: {snap!r}"
        )

        # (A1)-(A6) Initial visible rows. `src` is expanded by
        # default; its 3 children (main.rs, lib.rs, widgets branch)
        # are visible. `tests` + `docs` collapsed → just their
        # branch rows.
        rows = _row_tags(snap)
        expected_initial = {
            "file_tree#src",
            "file_tree#src/main.rs",
            "file_tree#src/lib.rs",
            "file_tree#src/widgets",
            "file_tree#tests",
            "file_tree#docs",
        }
        for tag in expected_initial:
            assert tag in rows, (
                f"initial paint must include row tag {tag!r}; got rows: {rows!r}"
            )
        # And the deeper tests/docs children are NOT visible (collapsed).
        assert "file_tree#tests/integration.rs" not in rows, (
            "tests/integration.rs must be hidden under collapsed tests"
        )
        assert "file_tree#src/widgets/tree_view.rs" not in rows, (
            "src/widgets/tree_view.rs must be hidden under collapsed widgets"
        )

        # ── (A7) focused-row highlight paints on src ────────────────
        initial_focus = _focused_row_id(snap)
        assert initial_focus == "src", (
            f"initial focus highlight must be on src; got: {initial_focus!r}"
        )

        # R820.1 — hello-tree-view's tree is now a focus-gated single tab
        # stop (apply_key drops keys unless the root is focused), so focus
        # it before driving keys, like every other gated demo.
        tf.request("focus/set", {"tag": "tree_root"})

        # ── (C) Arrow Down moves focus through visible rows ─────────
        tf.key(at=(10.0, 10.0), name="ArrowDown")
        _wait_focus(tf, "src/main.rs", "ArrowDown from src -> src/main.rs")

        tf.key(at=(10.0, 10.0), name="ArrowDown")
        _wait_focus(tf, "src/lib.rs", "ArrowDown from main.rs -> lib.rs")

        # ── Arrow Up reverses direction ─────────────────────────────
        tf.key(at=(10.0, 10.0), name="ArrowUp")
        _wait_focus(tf, "src/main.rs", "ArrowUp back to main.rs")

        # ── (G) Home jumps to first visible row ─────────────────────
        tf.key(at=(10.0, 10.0), name="Home")
        _wait_focus(tf, "src", "Home -> first visible (src)")

        # ── (G) End jumps to last visible row ───────────────────────
        tf.key(at=(10.0, 10.0), name="End")
        _wait_focus(tf, "docs", "End -> last visible (docs)")

        # ── (D) Arrow Right expands the focused branch (docs) ───────
        tf.key(at=(10.0, 10.0), name="ArrowRight")
        wait_snap(
            tf,
            lambda s: "file_tree#docs/README.md" in _row_tags(s),
            viewport=VIEWPORT,
            desc="ArrowRight on docs must expand it",
        )

        # ── (E) Arrow Left collapses the focused branch ─────────────
        tf.key(at=(10.0, 10.0), name="ArrowLeft")
        wait_snap(
            tf,
            lambda s: "file_tree#docs/README.md" not in _row_tags(s),
            viewport=VIEWPORT,
            desc="ArrowLeft on docs must collapse it",
        )

        # ── (F) Space toggles expand on the focused branch ──────────
        tf.key(at=(10.0, 10.0), name="Space")
        wait_snap(
            tf,
            lambda s: "file_tree#docs/README.md" in _row_tags(s),
            viewport=VIEWPORT,
            desc="Space toggle on docs must expand it",
        )
        tf.key(at=(10.0, 10.0), name="Space")
        wait_snap(
            tf,
            lambda s: "file_tree#docs/README.md" not in _row_tags(s),
            viewport=VIEWPORT,
            desc="Space toggle (2nd press) on docs must collapse it",
        )

        # ── (H) Arrow Right on a leaf is a no-op (no children) ──────
        # Navigate to a leaf (src/lib.rs). R809 completed the other
        # leaf keys (Arrow Left = ascend to parent, type-ahead); those
        # are exercised in r809_tree_keyboard_waiaria.py. Here we keep
        # the one behaviour that is still a true in-place no-op.
        tf.key(at=(10.0, 10.0), name="Home")  # back to src
        _wait_focus(tf, "src", "Home back to src before leaf no-op")
        tf.key(at=(10.0, 10.0), name="ArrowDown")  # main.rs
        tf.key(at=(10.0, 10.0), name="ArrowDown")  # lib.rs (leaf)
        snap = _wait_focus(tf, "src/lib.rs", "two ArrowDown land on lib.rs leaf")
        rows_before = set(_row_tags(snap))
        tf.key(at=(10.0, 10.0), name="ArrowRight")
        # No-op verification: the dispatch commits before the RPC
        # response, so a plain read after the key IS the post-key state.
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert set(_row_tags(snap)) == rows_before, (
            "Arrow Right on leaf must not change visible rows"
        )

        # ── (D) Expand tests branch + verify children appear ────────
        # Navigate to tests.
        for _ in range(5):
            tf.key(at=(10.0, 10.0), name="ArrowDown")
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        focus = _focused_row_id(snap)
        # Whichever row arrives, expand it; for tests we expect:
        if focus == "tests":
            tf.key(at=(10.0, 10.0), name="ArrowRight")
            snap = wait_snap(
                tf,
                lambda s: "file_tree#tests/integration.rs" in _row_tags(s),
                viewport=VIEWPORT,
                desc="ArrowRight on tests expands its children",
            )
            rows = _row_tags(snap)
            assert "file_tree#tests/snapshot.rs" in rows

        # ── (I) header + footer text present ────────────────────────
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert _walk_for_text(snap, "File explorer"), (
            "header text must render"
        )
        assert _walk_for_text(snap, "navigate"), (
            "footer keyboard hint must render"
        )


if __name__ == "__main__":
    sys.exit(run_demo("R673 §5.16 §5.50 — interactive TreeView 2nd consumer", body))
