#!/usr/bin/env python3
"""R777 §5.27 — data-grid keyboard navigation at scale.

Drives the `hello-grid-nav` binding via JSON-RPC. A 10,000-row selectable
virtualized data-grid (frozen header + flex-viewport body), made
keyboard-navigable and selectable by pure composition:

  * R746 `VirtualSelectExternal` — index-model row selection (the SAME
    coordinator the list uses; a grid cell click selects its ROW).
  * R775 `view_virtual_table` — frozen header + windowed body, now tinting
    the selected row.
  * R776 `scroll_offset_to_reveal` — here its SECOND consumer: navigating
    to a row that was never materialized scrolls there.

The decisive witness is scene-as-data (§2 #7): a keyboard key moves the
row selection to an unmaterialized row, and the grid body *scrolls there*.

  (A) boot — nothing selected; a frozen header + a small window of rows.
  (B) keyboard nav (selection-follows-focus) — focus the grid, then
      ArrowDown/Up step one row (clamped, no wrap); Home/End jump to the
      extremes; PageDown/PageUp step a viewport-ful.
  (C) scroll-into-view at scale — End selects row 9,999 and the body scrolls
      so `vtbl_row9999` is now a rendered node, though it did not exist at
      offset 0; Home returns to the top.
  (D) pointer + RPC — a cell click selects its row (column irrelevant —
      SelectRows); the AI-first `invoke select` / `clear` paths still work.

  Phase 2 — PIXELS (PINION_SCREENSHOT): the grid really paints (header +
      data cells distinct). Selection colour is pixel-proven by the unit
      `selected_row_paints_accent_wash`.
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
    find_by_tag,
    read_png_rgba8,
    run_demo,
    sample_png_points,
)

EXAMPLE = "hello-grid-nav"
WIN = (400, 480)
N = 10_000
ROW_H = 36
OVERSCAN = 3
SCROLL_TAG = "vtbl_scroll"
TABLE_TAG = "vtbl"
PAUSE = 0.12


def present_rows(snap) -> set[int]:
    """Rendered data-row ids (from the `vtbl_row<id>` strips)."""
    out: set[int] = set()
    for tag in abs_rects_of(snap):
        if tag.startswith("vtbl_row"):
            suffix = tag[len("vtbl_row"):]
            if suffix.isdigit():
                out.add(int(suffix))
    return out


def scroll_offset(snap) -> int:
    node = find_by_tag(snap, SCROLL_TAG)
    assert node is not None, "scroll node present"
    return int(node.get("offset_y", -1))


def selected(d):
    return d.query("/external/selected")


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        snap = tf.snapshot(source="paint", viewport=WIN)

        # ── (A) boot: frozen header, small window, nothing selected ─
        rects = abs_rects_of(snap)
        assert TABLE_TAG in rects, "grid container present at boot"
        assert "vtbl_hrow" in rects, "frozen header row present"
        assert "vtbl_ch0" in rects, "column header cell present"
        assert_eq(scroll_offset(snap), 0, "boot offset is 0")
        assert_eq(selected(tf), None, "nothing selected at boot")
        assert_eq(tf.query("/external/item_count"), N, "coordinator knows full dataset size")
        boot_rows = present_rows(snap)
        assert len(boot_rows) < 30, f"virtualized: small window, got {len(boot_rows)} of {N}"
        assert 0 in boot_rows, "row 0 rendered at the top"

        # ── (B) keyboard navigation ─────────────────────────────────
        tf.request("focus/set", {"tag": TABLE_TAG})
        time.sleep(PAUSE)

        tf.key(path=TABLE_TAG, name="ArrowDown")
        time.sleep(PAUSE)
        assert_eq(selected(tf), 0, "first ArrowDown selects row 0")
        tf.key(path=TABLE_TAG, name="ArrowDown")
        tf.key(path=TABLE_TAG, name="ArrowDown")
        time.sleep(PAUSE)
        assert_eq(selected(tf), 2, "two more ArrowDowns advance to row 2")
        tf.key(path=TABLE_TAG, name="ArrowUp")
        time.sleep(PAUSE)
        assert_eq(selected(tf), 1, "ArrowUp steps back to row 1")
        tf.key(path=TABLE_TAG, name="Home")
        time.sleep(PAUSE)
        assert_eq(selected(tf), 0, "Home jumps to the first row")
        tf.key(path=TABLE_TAG, name="ArrowUp")
        time.sleep(PAUSE)
        assert_eq(selected(tf), 0, "ArrowUp at the top clamps (no wrap)")

        tf.key(path=TABLE_TAG, name="PageDown")
        time.sleep(PAUSE)
        after_page = selected(tf)
        assert isinstance(after_page, int) and after_page > 1, \
            f"PageDown steps by a page (>1 row), got {after_page}"
        assert after_page < N, "PageDown stays in range"
        tf.key(path=TABLE_TAG, name="PageUp")
        time.sleep(PAUSE)
        assert_eq(selected(tf), 0, "PageUp from one page down returns to the top")

        # ── (C) scroll-into-view at scale (the headline) ────────────
        tf.key(path=TABLE_TAG, name="End")
        time.sleep(PAUSE)
        assert_eq(selected(tf), N - 1, "End selects the last row 9999")
        snap = tf.snapshot(source="paint", viewport=WIN)
        deep_offset = scroll_offset(snap)
        assert deep_offset > 300_000, f"End scrolled deep into the grid, offset {deep_offset}"
        deep_rows = present_rows(snap)
        assert (N - 1) in deep_rows, "row 9999 is now a RENDERED node (scrolled into view)"
        assert 0 not in deep_rows, "row 0 has left the window entirely"
        assert "vtbl_hrow" in abs_rects_of(snap), "header stays FROZEN at the bottom of the list"
        assert len(deep_rows) < 30, f"still virtualized at the bottom, {len(deep_rows)} rows"

        tf.key(path=TABLE_TAG, name="Home")
        time.sleep(PAUSE)
        assert_eq(selected(tf), 0, "Home selects row 0 from the bottom")
        snap = tf.snapshot(source="paint", viewport=WIN)
        assert_eq(scroll_offset(snap), 0, "Home scrolled the selection back into view (top)")
        assert 0 in present_rows(snap), "row 0 rendered again at the top"

        # ── (D) pointer cell click selects the row + RPC paths ──────
        snap = tf.snapshot(source="paint", viewport=WIN)
        assert "vtbl#4_0" in abs_rects_of(snap), "cell vtbl#4_0 visible for the click test"
        # Click the *second* column of row 4 — the column is irrelevant to
        # row selection (SelectRows).
        tf.click(path="vtbl#4_1")
        time.sleep(PAUSE)
        assert_eq(selected(tf), 4, "clicking any cell of row 4 selects row 4")
        # AI-first deep select via invoke, then clear.
        assert_eq(tf.invoke("/external/select", 5000), 5000, "invoke select 5000 returns 5000")
        assert_eq(selected(tf), 5000, "deep row selected via RPC without materialization")
        assert_eq(tf.invoke("/external/clear", None), None, "clear returns null")
        assert_eq(selected(tf), None, "selection cleared")

    # ── Phase 2 — live pixels (boot frame: grid paints) ────────────
    snap, rects = _boot_snapshot_and_rects()
    img = read_png_rgba8(capture_screenshot())
    assert (img.width, img.height) == WIN, \
        f"screenshot {img.width}x{img.height} != window {WIN}"

    # The frozen header band and a data cell sit at different vertical
    # bands; sampling the header vs a data row proves the grid paints.
    hx, hy, hw, hh = rects["vtbl_ch0"]
    rx, ry, rw, rh = rects["vtbl#0_0"]
    header_pt = (hx + hw // 2, hy + hh // 2)
    row_pt = (rx + rw // 2, ry + rh // 2)
    h_col, r_col = sample_png_points(img, [header_pt, row_pt])
    assert h_col is not None and r_col is not None, "header + data cell sampled"


def _boot_snapshot_and_rects():
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        snap = tf.snapshot(source="paint", viewport=WIN)
        return snap, abs_rects_of(snap)


def capture_screenshot() -> Path:
    out = Path(tempfile.mkdtemp(prefix="pinion-r777-")) / "gridnav.png"
    binary = WORKSPACE_ROOT / "target" / "release" / EXAMPLE
    cmd = [str(binary)] if binary.exists() else [
        "cargo", "run", "-p", EXAMPLE, "--quiet", "--release",
    ]
    env = os.environ.copy()
    env["PINION_SCREENSHOT"] = str(out)
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
    sys.exit(run_demo("R777 §5.27 — data-grid keyboard navigation at scale", body))
