#!/usr/bin/env python3
"""R746 §5.27 — selectable virtualized list E2E.

Drives the `hello-virtual-select` binding via JSON-RPC. Consumer of
`pinion_core::widgets::virtual_select::VirtualSelectExternal` — selection
on a virtualized list held by DATA INDEX (not per-rendered-row), decoupled
from materialization. The R735.1 leaf-based selection substrate cannot do
this (it needs a slice of all N leaves); a 10,000-row windowed list has
~15 leaves at a time.

The decisive witness is scene-as-data (§2 #7): selection survives in the
coordinator while the rendered rows window in and out.

  (A) boot — nothing selected; only a small window of 10,000 rows exists.
  (B) click-to-select — a real `scene/click` on a visible row selects it;
      `selected` reports that index; clicking another row moves it.
  (C) selection ⊥ virtualization — select row 3, scroll 4,000px away (row 3
      leaves the tree entirely), `selected` is STILL 3; scroll back and row
      3 is rendered again. Select a DEEP row (5,000) via the AI-first
      `invoke select` though it never existed at boot; scroll to it and it
      renders.
  (D) clear + out-of-range guard — `invoke clear` deselects; an
      out-of-range `invoke select` is ignored.

  Phase 2 — PIXELS (PINION_SCREENSHOT): the windowed rows really paint
      (zebra tones distinct). Selection colour is proven live in the native
      XTEST click pass (boot frame has no selection by design).
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
    read_png_rgba8,
    run_demo,
    wait_snap,
    wait_until,
    sample_png_points,
)

EXAMPLE = "hello-virtual-select"
WIN = (360, 460)
N = 10_000
PITCH = 32
VP_H = 384  # 12 rows
OVERSCAN = 3
SCROLL_TAG = "vlist_scroll"
LIST_TAG = "vlist"


def visible_window(offset: int, vp_h: int, n: int, pitch: int, overscan: int) -> set[int]:
    if n == 0 or vp_h == 0 or pitch == 0:
        return set()
    offset = max(0, offset)
    bottom = offset + vp_h
    max_index = n - 1
    first_visible = min(offset // pitch, max_index)
    last_visible = min((bottom - 1) // pitch, max_index)
    first = max(0, first_visible - overscan)
    last = min(last_visible + overscan, max_index)
    return set(range(first, last + 1))


def present_rows(snap) -> set[int]:
    out: set[int] = set()
    for tag in abs_rects_of(snap):
        if tag.startswith("vlist#"):
            out.add(int(tag.split("#", 1)[1]))
    return out


def scroll_offset(snap) -> int:
    node = find_by_tag(snap, SCROLL_TAG)
    assert node is not None, "scroll node present"
    return int(node.get("offset_y", -1))


def selected(d):
    """The coordinator's selected data index (None when unselected)."""
    return d.query("/external/selected")


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        snap = tf.snapshot(source="paint", viewport=WIN)

        # ── (A) boot: nothing selected, small window ────────────────
        rects = abs_rects_of(snap)
        assert LIST_TAG in rects, "list container present at boot"
        assert_eq(scroll_offset(snap), 0, "boot offset is 0")
        assert_eq(selected(tf), None, "nothing selected at boot")
        assert_eq(tf.query("/external/item_count"), N, "coordinator knows full dataset size")

        rows = present_rows(snap)
        expected = visible_window(0, VP_H, N, PITCH, OVERSCAN)
        assert_eq(rows, expected, "boot rendered band == windowing math")
        assert len(rows) < 30, f"virtualized: small window, got {len(rows)} of {N}"

        # ── (B) click-to-select a visible row ───────────────────────
        tf.click(path=f"{LIST_TAG}#3")
        wait_until(lambda: selected(tf) == 3, desc="click selected row 3")
        # Clicking another visible row MOVES the single selection.
        tf.click(path=f"{LIST_TAG}#6")
        wait_until(
            lambda: selected(tf) == 6,
            desc="click row 6 moves selection (single-select)",
        )
        # Re-select row 3 for the orthogonality test.
        tf.click(path=f"{LIST_TAG}#3")
        wait_until(lambda: selected(tf) == 3, desc="back to row 3")

        # ── (C) selection ⊥ virtualization ─────────────────────────
        # Scroll far enough that row 3 leaves the window AND the tree.
        tf.scroll(SCROLL_TAG, to=(0, 4000))  # 125 rows down
        snap = wait_snap(
            tf, lambda s: 3 not in present_rows(s), viewport=WIN,
            desc="row 3 has left the rendered window (and the tree)",
        )
        rows_deep = present_rows(snap)
        assert_eq(selected(tf), 3, "selection SURVIVES though the row is unmaterialized")

        # AI-first: select a deep row that never existed at boot.
        assert_eq(tf.invoke("/external/select", 5000), 5000, "invoke select 5000 returns 5000")
        assert_eq(selected(tf), 5000, "deep row 5000 selected without materialization")
        # Scroll to it; it renders, and the selection is consistent.
        tf.scroll(SCROLL_TAG, to=(0, 5000 * PITCH - VP_H // 2))
        snap = wait_snap(
            tf, lambda s: 5000 in present_rows(s), viewport=WIN,
            desc="row 5000 now rendered after scrolling to it",
        )
        assert_eq(selected(tf), 5000, "selection still 5000")

        # Scroll back to the top; row 3 renders again, selection unchanged
        # (we re-select 3 first to prove the round-trip from a deep state).
        tf.invoke("/external/select", 3)
        tf.scroll(SCROLL_TAG, to=(0, 0))
        snap = wait_snap(
            tf, lambda s: 3 in present_rows(s), viewport=WIN,
            desc="row 3 rendered again at the top",
        )
        assert_eq(selected(tf), 3, "selection intact across the full scroll round-trip")
        assert_eq(present_rows(snap), expected, "top window restored exactly")

        # ── (D) clear + out-of-range guard ──────────────────────────
        assert_eq(tf.invoke("/external/clear", None), None, "clear returns null")
        assert_eq(selected(tf), None, "selection cleared")
        # An out-of-range select is a no-op (malformed AI/automation input).
        tf.invoke("/external/select", 999_999)
        assert_eq(selected(tf), None, "out-of-range select ignored, still none")
        # A valid composite send (the same wire the click produces) selects.
        tf.invoke("/external/send", "8:PointerUp")
        assert_eq(selected(tf), 8, "composite send wire selects row 8")

    # ── Phase 2 — live pixels (boot frame: rows render) ─────────────
    snap, rects = _boot_snapshot_and_rects()
    img = read_png_rgba8(capture_screenshot())
    assert (img.width, img.height) == WIN, \
        f"screenshot {img.width}x{img.height} != window {WIN}"

    def row_bg_point(i: int):
        x, y, w, h = rects[f"vlist#{i}"]
        return (x + w - 8, y + h // 2)

    bg0, bg1, bg2 = sample_png_points(img, [row_bg_point(0), row_bg_point(1), row_bg_point(2)])
    assert bg0 != bg1, f"zebra tones differ (rows painted), got {bg0} vs {bg1}"
    assert bg0 == bg2, f"even rows share a tone (zebra parity), got {bg0} vs {bg2}"


def _boot_snapshot_and_rects():
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        snap = tf.snapshot(source="paint", viewport=WIN)
        return snap, abs_rects_of(snap)


def capture_screenshot() -> Path:
    out = Path(tempfile.mkdtemp(prefix="pinion-r746-")) / "vselect.png"
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
    sys.exit(run_demo("R746 §5.27 — selectable virtualized list", body))
