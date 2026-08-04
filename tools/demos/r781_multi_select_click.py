#!/usr/bin/env python3
"""R781 §5.35 §5.41 — mouse Shift/Ctrl-click multi-select.

Drives the `hello-multi-select` binding via JSON-RPC, completing the
multi-select funnel R780 left as a carry: the **mouse** modifier-click.
R780 wired keyboard (Shift-range / Ctrl+A / Ctrl+Space) + RPC; the pointer
path could not carry modifiers because the router's `pointer_up` →
`dispatch_send` composite wire only emitted `"<key>:<Event>"`. R781 threads
the held `Modifiers` through to a third wire segment
(`"<key>:PointerUp:<token>"`, e.g. `:sc`), decoded by the migrated
`split_send_payload` SSOT (all 11 composite consumers updated, R773
all-or-none), and consumed by `VirtualSelect`:

  * **plain click** — move + replace (anchor follows the click).
  * **Ctrl-click** — toggle the row's membership, rest intact.
  * **Shift-click** — extend the contiguous range from the anchor.

The same `scene/modifiers` cache drives keyboard multi-select, so RPC
`scene/modifiers` + `scene/click` exercises the identical shell path a
native modified click takes — keyboard, RPC, and mouse are one funnel.

  (A) boot — mode "multi", empty selection.
  (B) plain click — selects one row, sets the anchor.
  (C) Ctrl-click — accumulate then toggle a row off.
  (D) Shift-click — extend the contiguous range from the anchor.
  (E) plain click resets, Shift-click a fresh span.
  (F) keyboard still composes (Ctrl+A) alongside the mouse.

  Phase 2 — PIXELS (PINION_SCREENSHOT): the windowed rows paint (zebra).
      The multi-Accent visual is unit-proven (hello-multi-select tests);
      boot has no selection by design (mirrors r780).
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
    chord_click,
    read_png_rgba8,
    run_demo,
    sample_png_points,
    selection_rows,
    wait_until,
)

EXAMPLE = "hello-multi-select"
WIN = (380, 480)
N = 10_000
LIST_TAG = "vlist"


def selection(d) -> list[int]:
    """The coordinator's selected ROWS, ascending — decoded from the R1561 run
    form (`[[first, last], …]`) the slot answers with, through the shared
    decoder rather than a private one."""
    return selection_rows(d.query("/external/selection"))


def cursor(d):
    return d.query("/external/selected")


def anchor(d):
    return d.query("/external/anchor")


def click_row(tf, row: int, *, shift: bool = False, ctrl: bool = False) -> None:
    """A `scene/click` on a windowed row with held modifiers, through the
    shared `chord_click` (R1562 lift)."""
    chord_click(tf, f"{LIST_TAG}#{row}", shift=shift, ctrl=ctrl)


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        snap = tf.snapshot(source="paint", viewport=WIN)
        rects = abs_rects_of(snap)

        # ── (A) boot: multi mode, empty ─────────────────────────────
        assert LIST_TAG in rects, "list container present at boot"
        assert_eq(tf.query("/external/mode"), "multi", "multi-select mode")
        assert_eq(tf.query("/external/item_count"), N, "coordinator knows the dataset size")
        assert_eq(selection(tf), [], "nothing selected at boot")
        assert_eq(cursor(tf), None, "no active row at boot")
        assert_eq(anchor(tf), None, "no anchor at boot")
        # Rows 0..9 are in the boot window for the click tests.
        for r in (0, 2, 4, 5, 6, 7, 9):
            assert f"{LIST_TAG}#{r}" in rects, f"row {r} rendered for the click test"

        # ── (B) plain click selects one + sets the anchor ───────────
        click_row(tf, 4)
        assert_eq(selection(tf), [4], "plain click selects just row 4")
        assert_eq(cursor(tf), 4, "the active row is the clicked row")
        assert_eq(anchor(tf), 4, "plain click sets the anchor to 4")
        # A re-click of the same row is idempotent.
        click_row(tf, 4)
        assert_eq(selection(tf), [4], "re-clicking the selected row is a no-op")

        # ── (C) Ctrl-click accumulates, then toggles off ────────────
        click_row(tf, 9, ctrl=True)
        assert_eq(selection(tf), [4, 9], "Ctrl-click accumulates row 9")
        assert_eq(cursor(tf), 9, "Ctrl-click moves the active row to 9")
        assert_eq(anchor(tf), 9, "Ctrl-click re-anchors at 9")
        click_row(tf, 7, ctrl=True)
        assert_eq(selection(tf), [4, 7, 9], "Ctrl-click accumulates row 7")
        assert_eq(anchor(tf), 7, "Ctrl-click re-anchors at 7")
        click_row(tf, 7, ctrl=True)
        assert_eq(selection(tf), [4, 9], "Ctrl-click an existing member toggles it off")
        assert_eq(cursor(tf), 7, "the toggled row stays the active row")

        # ── (D) Shift-click extends the range from the anchor (7) ───
        click_row(tf, 5, shift=True)
        assert_eq(selection(tf), [5, 6, 7], "Shift-click extends 5..7 from anchor 7, replacing")
        assert_eq(anchor(tf), 7, "the anchor stays put while extending")
        assert_eq(cursor(tf), 5, "the navigated end is the active row")
        # Re-extend the other side: anchor unchanged, range replaced.
        click_row(tf, 9, shift=True)
        assert_eq(selection(tf), [7, 8, 9], "Shift-click re-extends 7..9, anchor still 7")
        assert_eq(cursor(tf), 9, "the active row follows the new end")

        # ── (E) plain click resets, fresh Shift span ────────────────
        click_row(tf, 2)
        assert_eq(selection(tf), [2], "a plain click replaces the whole set + re-anchors")
        assert_eq(anchor(tf), 2, "plain click re-anchors at 2")
        click_row(tf, 0, shift=True)
        assert_eq(selection(tf), [0, 1, 2], "Shift-click spans 0..2 from the new anchor 2")
        assert_eq(cursor(tf), 0, "the active row is the shift-clicked end")

        # ── (F) keyboard composes with the mouse (R780 + R781) ──────
        tf.request("focus/set", {"tag": LIST_TAG})
        wait_until(
            lambda: tf.request("focus/get").result.get("focused") == LIST_TAG,
            desc="list owns focus",
        )
        tf.modifiers(ctrl=True)
        tf.key(path=LIST_TAG, name="a")  # Ctrl+A
        tf.modifiers()
        wait_until(
            lambda: len(selection(tf)) == N,
            desc="Ctrl+A still selects all alongside the mouse path",
        )
        # A plain click then collapses the keyboard select-all back to one.
        click_row(tf, 3)
        assert_eq(selection(tf), [3], "a plain click after Ctrl+A replaces with one row")

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
    out = Path(tempfile.mkdtemp(prefix="pinion-r781-")) / "multiclick.png"
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
    sys.exit(run_demo("R781 §5.35 §5.41 — mouse Shift/Ctrl-click multi-select", body))
