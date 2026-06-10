#!/usr/bin/env python3
"""R782 §5.40 — multi-select data-grid at scale.

Drives the `hello-grid-multi-select` binding via JSON-RPC. A 10,000-row
virtualized data-grid (frozen header + flex-viewport body) whose selection is
an arbitrary **set** of data rows. It composes the existing substrate with
NO coordinator changes:

  * R780 `VirtualSelectExternal::new_multi` — the index-model selection set
    (anchor + cursor), the SAME coordinator the list uses; a grid cell click
    selects its ROW (column irrelevant — WAI-ARIA / Qt `SelectRows`).
  * R781 modifier-click wire — the held `Modifiers` ride the composite send
    wire's third segment, so `scene/modifiers` + `scene/click` toggles /
    extends exactly as a native modified click does.
  * R775 `view_virtual_table` — generalized this round so its row selection
    is a predicate; several visible rows wash accent at once.

The decisive witness is scene-as-data (§2 #7): a `Ctrl`-click toggles one row
into the set without disturbing the rest, and `Shift`-click / `Ctrl+A` grow a
span / the whole 10,000-row set — all observed through `query("selection")`,
never pixels.

  (A) boot — multi mode, empty selection; frozen header + a small window.
  (B) plain cell click — selects one row, sets the anchor.
  (C) Ctrl-click — accumulate rows, then toggle one back off.
  (D) Shift-click — extend the contiguous range from the anchor.
  (E) plain click resets, fresh Shift span.
  (F) keyboard composes (focus grid; Ctrl+A selects all; End scrolls a deep
      unmaterialized row into view while keeping the set).
  (G) AI-first invoke paths — select_all / clear over the open schema.

  Phase 2 — PIXELS (PINION_SCREENSHOT): the grid really paints (header vs a
      data cell distinct). The multi-Accent wash is unit-proven
      (`several_selected_rows_wash_accent_at_once`); boot has no selection by
      design (mirrors r780/r781).
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
    sample_png_points,
    wait_until,
)

EXAMPLE = "hello-grid-multi-select"
WIN = (400, 480)
N = 10_000
ROW_H = 36
SCROLL_TAG = "vtbl_scroll"
TABLE_TAG = "vtbl"


def selection(d) -> list[int]:
    return list(d.query("/external/selection"))


def cursor(d):
    return d.query("/external/selected")


def anchor(d):
    return d.query("/external/anchor")


def scroll_offset(snap) -> int:
    node = find_by_tag(snap, SCROLL_TAG)
    assert node is not None, "scroll node present"
    return int(node.get("offset_y", -1))


def present_rows(snap) -> set[int]:
    """Rendered data-row ids (from the `vtbl_row<id>` strips)."""
    out: set[int] = set()
    for tag in abs_rects_of(snap):
        if tag.startswith("vtbl_row"):
            suffix = tag[len("vtbl_row"):]
            if suffix.isdigit():
                out.add(int(suffix))
    return out


def click_cell(tf, row: int, col: int, *, shift: bool = False, ctrl: bool = False) -> None:
    """A `scene/click` on a windowed grid cell with held modifiers, released
    after — the R763 `scene/modifiers` press/release pair the shell reads at
    the activate edge. The column is irrelevant to row selection."""
    if shift or ctrl:
        tf.modifiers(shift=shift, ctrl=ctrl)
    tf.click(path=f"{TABLE_TAG}#{row}_{col}")
    if shift or ctrl:
        tf.modifiers()  # release


def wait_selection(tf, expected: list[int], desc: str) -> None:
    """Gate on the observed selection set (R883 zero-flake)."""
    wait_until(lambda: selection(tf) == expected, desc=desc)


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        snap = tf.snapshot(source="paint", viewport=WIN)
        rects = abs_rects_of(snap)

        # ── (A) boot: multi mode, frozen header, empty ──────────────
        assert TABLE_TAG in rects, "grid container present at boot"
        assert "vtbl_hrow" in rects, "frozen header row present"
        assert "vtbl_ch0" in rects, "column header cell present"
        assert_eq(scroll_offset(snap), 0, "boot offset is 0")
        assert_eq(tf.query("/external/mode"), "multi", "multi-select mode")
        assert_eq(tf.query("/external/item_count"), N, "coordinator knows the dataset size")
        assert_eq(selection(tf), [], "nothing selected at boot")
        assert_eq(cursor(tf), None, "no active row at boot")
        assert_eq(anchor(tf), None, "no anchor at boot")
        boot_rows = present_rows(snap)
        assert len(boot_rows) < 30, f"virtualized: small window, got {len(boot_rows)} of {N}"
        for r in (0, 2, 4, 5, 6, 7, 9):
            assert f"{TABLE_TAG}#{r}_0" in rects, f"cell of row {r} rendered for the click test"

        # ── (B) plain cell click selects one + sets the anchor ──────
        click_cell(tf, 4, 1)  # column 1 — irrelevant to row selection
        wait_selection(tf, [4], "plain click on any cell selects just row 4")
        assert_eq(cursor(tf), 4, "the active row is the clicked row")
        assert_eq(anchor(tf), 4, "plain click sets the anchor to 4")
        click_cell(tf, 4, 2)  # re-click a different column of the same row
        assert_eq(selection(tf), [4], "re-clicking the selected row is a no-op")

        # ── (C) Ctrl-click accumulates, then toggles off ────────────
        click_cell(tf, 9, 0, ctrl=True)
        wait_selection(tf, [4, 9], "Ctrl-click accumulates row 9")
        assert_eq(cursor(tf), 9, "Ctrl-click moves the active row to 9")
        assert_eq(anchor(tf), 9, "Ctrl-click re-anchors at 9")
        click_cell(tf, 7, 2, ctrl=True)
        wait_selection(tf, [4, 7, 9], "Ctrl-click accumulates row 7")
        assert_eq(anchor(tf), 7, "Ctrl-click re-anchors at 7")
        click_cell(tf, 7, 0, ctrl=True)
        wait_selection(tf, [4, 9], "Ctrl-click an existing member toggles it off")
        assert_eq(cursor(tf), 7, "the toggled row stays the active row")

        # ── (D) Shift-click extends the range from the anchor (7) ───
        click_cell(tf, 5, 1, shift=True)
        wait_selection(tf, [5, 6, 7], "Shift-click extends 5..7 from anchor 7, replacing")
        assert_eq(anchor(tf), 7, "the anchor stays put while extending")
        assert_eq(cursor(tf), 5, "the navigated end is the active row")
        click_cell(tf, 9, 0, shift=True)
        wait_selection(tf, [7, 8, 9], "Shift-click re-extends 7..9, anchor still 7")
        assert_eq(cursor(tf), 9, "the active row follows the new end")

        # ── (E) plain click resets, fresh Shift span ────────────────
        click_cell(tf, 2, 0)
        wait_selection(tf, [2], "a plain click replaces the whole set + re-anchors")
        assert_eq(anchor(tf), 2, "plain click re-anchors at 2")
        click_cell(tf, 0, 2, shift=True)
        wait_selection(tf, [0, 1, 2], "Shift-click spans 0..2 from the new anchor 2")
        assert_eq(cursor(tf), 0, "the active row is the shift-clicked end")

        # ── (F) keyboard composes with the mouse + scrolls at scale ─
        tf.request("focus/set", {"tag": TABLE_TAG})
        wait_until(
            lambda: tf.request("focus/get").result.get("focused") == TABLE_TAG,
            desc="grid owns focus",
        )
        tf.modifiers(ctrl=True)
        tf.key(path=TABLE_TAG, name="a")  # Ctrl+A
        tf.modifiers()
        wait_until(
            lambda: len(selection(tf)) == N,
            desc="Ctrl+A selects all 10,000 rows alongside the mouse path",
        )
        # End moves the active row to 9999 and scrolls there (the set stays).
        tf.key(path=TABLE_TAG, name="End")
        wait_until(
            lambda: cursor(tf) == N - 1,
            desc="End moves the active row to the last row",
        )
        assert_eq(len(selection(tf)), 1, "plain nav (End) collapses the set to the active row")
        snap = tf.snapshot(source="paint", viewport=WIN)
        deep_offset = scroll_offset(snap)
        assert deep_offset > 300_000, f"End scrolled deep into the grid, offset {deep_offset}"
        deep_rows = present_rows(snap)
        assert (N - 1) in deep_rows, "row 9999 is now a RENDERED node (scrolled into view)"
        assert 0 not in deep_rows, "row 0 has left the window entirely"
        assert "vtbl_hrow" in abs_rects_of(snap), "header stays FROZEN at the bottom of the list"
        # Shift+Home extends the full span from the anchor (9999) to the top.
        tf.modifiers(shift=True)
        tf.key(path=TABLE_TAG, name="Home")
        tf.modifiers()
        wait_until(
            lambda: len(selection(tf)) == N,
            desc="Shift+Home extends the full 0..9999 span",
        )
        assert_eq(cursor(tf), 0, "the active row is the top after Shift+Home")
        snap = tf.snapshot(source="paint", viewport=WIN)
        assert_eq(scroll_offset(snap), 0, "Shift+Home scrolled the active row back to the top")

        # ── (G) AI-first invoke paths over the open schema ──────────
        assert_eq(
            len(tf.invoke("/external/select_all", None)), N,
            "invoke select_all returns the full index array",
        )
        assert_eq(tf.invoke("/external/clear", None), None, "invoke clear returns null")
        assert_eq(selection(tf), [], "selection cleared")
        assert_eq(cursor(tf), None, "no active row after clear")

    # ── Phase 2 — live pixels (boot frame: grid paints) ────────────
    snap, rects = _boot_snapshot_and_rects()
    img = read_png_rgba8(capture_screenshot())
    assert (img.width, img.height) == WIN, \
        f"screenshot {img.width}x{img.height} != window {WIN}"

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
    out = Path(tempfile.mkdtemp(prefix="pinion-r782-")) / "gridmulti.png"
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
    sys.exit(run_demo("R782 §5.40 — multi-select data-grid at scale", body))
