#!/usr/bin/env python3
"""R864 §5.27 §5.50 — tree-grid (scene-outliner) KEYBOARD ROVING E2E.

Drives the `hello-tree-grid` binding via JSON-RPC. R860 landed the columned
outliner (frozen name column + scrolling metadata) and R863 its WAI-ARIA
`treegrid` a11y; R864 makes it keyboard-navigable by composing the lifted
`apply_tree_key` + `tree_typeahead_jump` substrate (shared with the plain
`hello-virtual-tree`). The outliner is a single tab stop with an
`aria-activedescendant` roving cursor; the metadata columns are display cells
the row cursor rides past, not separate tab stops.

Witness (§2 #7 scene-as-data) — the keyboard cursor is read as DATA from the
query-only `tgrid_state` introspection (cursor / cursor_index / row_count),
not scraped from pixels:

  (A) boot — the cursor sits on the first row (`f0`); the tree root is the
      single focusable tab stop; the body is windowed (rendered < visible).
  (B) Arrow Down/Up — the cursor moves one visible row at a time over the
      flattening (f0 -> f0-o0 -> f0) and clamps at the first row (no wrap).
  (C) Arrow Left/Right — collapse / expand the focused folder; the visible
      row_count falls / rises by the folder's child count.
  (D) keyboard PERP virtualization — driving the cursor far down scrolls the
      shared body window so the cursor row materializes in BOTH panes (the
      name cell AND its metadata strip), and the cursor index stays inside
      the re-derived window.

  Phase 2 — PIXELS (PINION_SCREENSHOT): the cursor row paints the M3 focus
      highlight across BOTH panes — its name cell AND its metadata cell are
      tonally distinct from a non-cursor row's (the R860 cross-pane focus fill,
      now driven by the keyboard).
"""

from __future__ import annotations

import os
import subprocess
import sys
import tempfile
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    WORKSPACE_ROOT,
    abs_rects_of,
    assert_eq,
    read_png_rgba8,
    run_demo,
    sample_png_points,
    wait_until,
)

EXAMPLE = "hello-tree-grid"
WIN = (480, 460)
TREE_TAG = "tgrid"
ROOT_TAG = "tgrid_root"
SCROLL_TAG = "tgrid_scroll"
STATE_TAG = "tgrid_state"
PITCH = 48
OVERSCAN = 3
FOLDERS = 24
OBJECTS_PER = 12
EXPANDED_AT_BOOT = 3
BOOT_ROWS = FOLDERS + EXPANDED_AT_BOOT * OBJECTS_PER
AT = (12.0, 80.0)  # any in-window point; named keys route to the focused root
PAUSE = 0.05


def name_cell(rid: str) -> str:
    return f"{TREE_TAG}#{rid}"


def data_strip(rid: str) -> str:
    return f"{TREE_TAG}_drow{rid}"


def data_cell(rid: str, col: int) -> str:
    return f"{TREE_TAG}_dcell{rid}_{col}"


def scroll_node(snap):
    found = []

    def visit(n):
        if isinstance(n, dict):
            if n.get("type") == "Scroll" and n.get("tag") == SCROLL_TAG:
                found.append(n)
            if n.get("type") == "Scroll":
                visit(n.get("content"))
            for ch in n.get("children") or []:
                visit(ch)

    visit(snap)
    return found[0] if found else None


def offset_y(snap) -> int:
    node = scroll_node(snap)
    return int(node.get("offset_y", 0)) if node else 0


def present_name_ids(snap) -> set[str]:
    out: set[str] = set()

    def visit(n):
        if isinstance(n, dict):
            tag = n.get("tag")
            if isinstance(tag, str) and tag.startswith(f"{TREE_TAG}#"):
                out.add(tag.split("#", 1)[1])
            if n.get("type") == "Scroll":
                visit(n.get("content"))
            for ch in n.get("children") or []:
                visit(ch)

    visit(snap)
    return out


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:

        def snap_now():
            return tf.snapshot(source="paint", viewport=WIN)

        def cursor():
            return tf.query(f"/{STATE_TAG}/external/cursor")

        def cursor_index():
            return tf.query(f"/{STATE_TAG}/external/cursor_index")

        def row_count() -> int:
            return int(tf.query(f"/{STATE_TAG}/external/row_count"))

        def press(name: str):
            tf.key(at=AT, name=name)
            time.sleep(PAUSE)

        # ── (A) boot: cursor on the first row, single tab stop, windowed ──
        snap = wait_until(
            lambda: (lambda s: s if present_name_ids(s) else None)(snap_now()),
            desc="boot window renders",
        )
        assert ROOT_TAG in abs_rects_of(snap), "focusable treegrid root present"
        assert_eq(offset_y(snap), 0, "boot vertical offset 0")
        assert_eq(cursor(), "f0", "boot cursor on the first row (aria-activedescendant)")
        assert_eq(row_count(), BOOT_ROWS, "introspection row_count = boot visible rows")
        rendered = len(present_name_ids(snap))
        assert 0 < rendered < BOOT_ROWS, f"virtualized: window {rendered} < {BOOT_ROWS} visible"

        # Single tab stop with a focus gate — focus the root before driving keys.
        tf.request("focus/set", {"tag": ROOT_TAG})

        # ── (B) Arrow Down/Up move the row cursor + clamp ────────────────
        press("ArrowDown")  # f0 (expanded) -> its first child f0-o0
        assert_eq(cursor(), "f0-o0", "ArrowDown -> first child")
        press("ArrowUp")
        assert_eq(cursor(), "f0", "ArrowUp -> back to the parent row")
        press("ArrowUp")
        assert_eq(cursor(), "f0", "ArrowUp clamps at the first row (no wrap)")

        # ── (C) Arrow Left/Right collapse + expand the focused folder ────
        before = row_count()
        press("ArrowLeft")  # collapse f0 (expanded at boot)
        assert_eq(row_count(), before - OBJECTS_PER, "ArrowLeft collapsed f0")
        snap = snap_now()
        assert "f0-o0" not in present_name_ids(snap), "f0's children left the visible set"
        press("ArrowRight")  # re-expand f0
        assert_eq(row_count(), before, "ArrowRight re-expanded f0")

        # ── (D) keyboard PERP virtualization: drive past the window ───────
        for _ in range(40):
            press("ArrowDown")
        snap = wait_until(
            lambda: (lambda s: s if offset_y(s) > 0 else None)(snap_now()),
            desc="driving the cursor down scrolled the body window",
        )
        cur = cursor()
        assert cur not in (None, "f0"), f"cursor advanced deep into the list (got {cur})"
        # The cursor row materialized in BOTH panes (cross-pane lockstep).
        present = present_name_ids(snap)
        assert cur in present, "cursor name cell materialized in the window"
        rects = abs_rects_of(snap)
        assert data_strip(cur) in rects, "cursor metadata strip materialized too (both panes)"
        # The cursor index stays inside the re-derived window.
        idx = cursor_index()
        first = max(0, offset_y(snap) // PITCH - OVERSCAN)
        last = (offset_y(snap) + WIN[1]) // PITCH + OVERSCAN
        assert first <= int(idx) <= last, f"cursor index {idx} inside window [{first},{last}]"

        # ── Phase 2 — PIXELS: the cursor highlights BOTH panes ───────────
        press("Home")  # cursor back to f0, window to the top
        assert_eq(cursor(), "f0", "Home -> cursor f0")
        snap = snap_now()
        rects = abs_rects_of(snap)
        img = read_png_rgba8(capture_screenshot())
        assert (img.width, img.height) == WIN, f"screenshot {img.width}x{img.height} != {WIN}"

        def right_edge(tag):
            # Sample near the right edge of a bounded, in-window cell (past the
            # left-aligned text/glyph), so the point lands on the row's fill,
            # not a glyph — the r820 right-edge pattern. The frozen name cell
            # (200px) and the first metadata cell (160px at x=200) both sit
            # inside the 480px window; the full metadata STRIP (480px) would
            # overflow it, so sample its leading cell instead.
            x, y, w, h = rects[tag]
            return (x + w - 6, y + h // 2)

        # f0 is the cursor row; its first child f0-o0 (rendered right below it,
        # since f0 boots expanded) is not. The R860 focus fill makes the cursor
        # row's NAME cell (frozen pane) AND its METADATA cell (scrolling pane,
        # transparent so the highlighted strip shows through) tonally distinct
        # from a non-cursor row's — the cross-pane highlight, now keyboard-driven.
        other = "f0-o0"
        assert other in present_name_ids(snap), "a non-cursor sibling row is rendered for contrast"
        cur_name, oth_name, cur_meta, oth_meta = sample_png_points(
            img,
            [right_edge(name_cell("f0")), right_edge(name_cell(other)),
             right_edge(data_cell("f0", 0)), right_edge(data_cell(other, 0))],
        )
        assert cur_name != oth_name, \
            f"cursor name cell highlighted vs non-cursor, got {cur_name} vs {oth_name}"
        assert cur_meta != oth_meta, \
            f"R860 cross-pane: cursor metadata strip highlighted too, got {cur_meta} vs {oth_meta}"


def capture_screenshot() -> Path:
    out = Path(tempfile.mkdtemp(prefix="pinion-r864-")) / "treegrid_kbd.png"
    binary = WORKSPACE_ROOT / "target" / "release" / EXAMPLE
    cmd = [str(binary)] if binary.exists() else [
        "cargo", "run", "-p", EXAMPLE, "--quiet", "--release",
    ]
    env = os.environ.copy()
    env["PINION_SCREENSHOT"] = str(out)
    # Boot the cursor on f0 and let the focus highlight paint (no key needed —
    # the cursor boots on the first row), so the captured boot frame shows it.
    res = subprocess.run(
        cmd, cwd=WORKSPACE_ROOT, env=env,
        capture_output=True, text=True, check=False, timeout=120.0,
    )
    if res.returncode != 0:
        raise AssertionError(
            f"PINION_SCREENSHOT capture exited {res.returncode}:\n  stderr: {res.stderr!r}"
        )
    if not out.exists():
        raise AssertionError(f"PINION_SCREENSHOT produced no file at {out}")
    return out


if __name__ == "__main__":
    sys.exit(run_demo("R864 §5.27 — tree-grid keyboard roving", body))
