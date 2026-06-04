#!/usr/bin/env python3
"""R776 §5.27 — keyboard navigation at scale (selectable flex-viewport list).

Drives the `hello-virtual-nav` binding via JSON-RPC. It composes three
existing substrates — the R746 `VirtualSelectExternal` index-model
selection, the R774 `view_flex_virtual_list` AutoSizer windowing, and the
shared `ScrollState` — plus the one new primitive this round adds:
`scroll_offset_to_reveal` (scroll-into-view by index).

The decisive witness is scene-as-data (§2 #7): a keyboard key moves the
selection to a row that was never materialized, and the list *scrolls
there* — the row becomes a rendered node only because the offset was
computed from its known slot, not from any existing DOM node.

  (A) boot — nothing selected; a small window of 10,000 rows; offset 0.
  (B) keyboard nav (selection-follows-focus) — focus the list, then
      ArrowDown/Up step one row (clamped at the ends, no wrap); Home/End
      jump to the extremes; PageDown/PageUp step by a viewport-ful.
  (C) scroll-into-view at scale — End selects row 9,999 and the offset
      jumps so 9,999 is now rendered, though it did not exist at offset 0;
      Home brings it back to the top.
  (D) pointer + RPC unaffected — a real `scene/click` still selects; the
      AI-first `invoke select` / `clear` paths still work alongside the
      keyboard.

  Phase 2 — PIXELS (PINION_SCREENSHOT): the windowed rows really paint
      (zebra tones distinct). Selection colour is pixel-proven by the unit
      `selected_row_paints_accent` + the R746 XTEST pass (the boot frame
      has no selection by design).
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

EXAMPLE = "hello-virtual-nav"
WIN = (380, 480)
N = 10_000
PITCH = 32
OVERSCAN = 3
SCROLL_TAG = "vlist_scroll"
LIST_TAG = "vlist"
PAUSE = 0.12


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
        boot_rows = present_rows(snap)
        assert len(boot_rows) < 30, f"virtualized: small window, got {len(boot_rows)} of {N}"
        assert 0 in boot_rows, "row 0 rendered at the top"

        # ── (B) keyboard navigation (selection-follows-focus) ───────
        tf.request("focus/set", {"tag": LIST_TAG})
        time.sleep(PAUSE)

        # First ArrowDown from no selection lands on row 0.
        tf.key(path=LIST_TAG, name="ArrowDown")
        time.sleep(PAUSE)
        assert_eq(selected(tf), 0, "first ArrowDown selects row 0")
        tf.key(path=LIST_TAG, name="ArrowDown")
        tf.key(path=LIST_TAG, name="ArrowDown")
        time.sleep(PAUSE)
        assert_eq(selected(tf), 2, "two more ArrowDowns advance to row 2")
        tf.key(path=LIST_TAG, name="ArrowUp")
        time.sleep(PAUSE)
        assert_eq(selected(tf), 1, "ArrowUp steps back to row 1")

        # ArrowUp clamps at the top (no wrap to the last row).
        tf.key(path=LIST_TAG, name="Home")
        time.sleep(PAUSE)
        assert_eq(selected(tf), 0, "Home jumps to the first row")
        tf.key(path=LIST_TAG, name="ArrowUp")
        time.sleep(PAUSE)
        assert_eq(selected(tf), 0, "ArrowUp at the top clamps (no wrap)")
        assert_eq(scroll_offset(tf.snapshot(source="paint", viewport=WIN)), 0, "still at top")

        # PageDown steps by a whole viewport-ful (more than one row), stays
        # in range; PageUp is the symmetric reverse.
        tf.key(path=LIST_TAG, name="PageDown")
        time.sleep(PAUSE)
        after_page = selected(tf)
        assert isinstance(after_page, int) and after_page > 1, \
            f"PageDown steps by a page (>1 row), got {after_page}"
        assert after_page < N, "PageDown stays in range"
        tf.key(path=LIST_TAG, name="PageUp")
        time.sleep(PAUSE)
        assert_eq(selected(tf), 0, "PageUp from one page down returns to the top")

        # ── (C) scroll-into-view at scale (the headline) ────────────
        # End selects the last row — never materialized at offset 0 — and
        # the offset jumps so it becomes a rendered node.
        tf.key(path=LIST_TAG, name="End")
        time.sleep(PAUSE)
        assert_eq(selected(tf), N - 1, "End selects the last row 9999")
        snap = tf.snapshot(source="paint", viewport=WIN)
        deep_offset = scroll_offset(snap)
        assert deep_offset > 300_000, f"End scrolled deep into the list, offset {deep_offset}"
        deep_rows = present_rows(snap)
        assert (N - 1) in deep_rows, "row 9999 is now a RENDERED node (scrolled into view)"
        assert 0 not in deep_rows, "row 0 has left the window entirely"
        assert len(deep_rows) < 30, f"still virtualized at the bottom, {len(deep_rows)} rows"

        # Home brings the deep selection back to the top in one key.
        tf.key(path=LIST_TAG, name="Home")
        time.sleep(PAUSE)
        assert_eq(selected(tf), 0, "Home selects row 0 from the bottom")
        snap = tf.snapshot(source="paint", viewport=WIN)
        assert_eq(scroll_offset(snap), 0, "Home scrolled the selection back into view (top)")
        assert 0 in present_rows(snap), "row 0 rendered again at the top"

        # ArrowDown from the last row clamps (prove the End clamp too).
        tf.key(path=LIST_TAG, name="End")
        time.sleep(PAUSE)
        tf.key(path=LIST_TAG, name="ArrowDown")
        time.sleep(PAUSE)
        assert_eq(selected(tf), N - 1, "ArrowDown at the last row clamps (no wrap)")

        # ── (D) pointer + RPC paths still work alongside the keyboard ─
        tf.key(path=LIST_TAG, name="Home")  # bring the window back up
        time.sleep(PAUSE)
        snap = tf.snapshot(source="paint", viewport=WIN)
        assert 4 in present_rows(snap), "row 4 visible for the click test"
        tf.click(path=f"{LIST_TAG}#4")
        time.sleep(PAUSE)
        assert_eq(selected(tf), 4, "pointer click still selects (unaffected by keyboard wiring)")
        # AI-first deep select via invoke, then clear.
        assert_eq(tf.invoke("/external/select", 5000), 5000, "invoke select 5000 returns 5000")
        assert_eq(selected(tf), 5000, "deep row selected via RPC without materialization")
        assert_eq(tf.invoke("/external/clear", None), None, "clear returns null")
        assert_eq(selected(tf), None, "selection cleared")
        # A composite send (the same wire a click produces) still selects.
        tf.invoke("/external/send", "7:PointerUp")
        assert_eq(selected(tf), 7, "composite send wire selects row 7")

    # ── Phase 2 — live pixels (boot frame: rows render, zebra) ──────
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
    out = Path(tempfile.mkdtemp(prefix="pinion-r776-")) / "vnav.png"
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
    sys.exit(run_demo("R776 §5.27 — keyboard navigation at scale", body))
