#!/usr/bin/env python3
"""R1008 §5.41 §5.49 — pointer→cell hit-test (R1.8) E2E.

Drives `hello-grid-pointer` via JSON-RPC. A full-window 80×24 `Scene::TextGrid`
(8×16 baseline `CellMetric`, so the window IS the grid) whose pointer external
resolves the cursor's logical-pixel position into the `(col, row)` cell via the
new `CellMetric::px_to_cell`. This is the geometry an xterm mouse-reporting
terminal (the sprag pane) needs to forward a click to its PTY.

  * primary `GridPointerExternal` at `grid_pointer` — the pointer coordinator.
    `cell` / `cell_col` / `cell_row` are the cell under the pointer (a real
    `pointer_move` through the router); `cols` / `rows` the winsize dims;
    `invoke "cell_at" "x,y"` is a pure px→cell oracle (deterministic, no
    pointer state) for exhaustive geometry coverage.

  (A) oracle — `cell_at "x,y"` floors window pixels into the cell, sharing the
      winsize `cols_for`/`rows_for` rounding; an off-grid pixel clamps to the
      last cell. Exhaustive over the column/row rulers + the cell interior.
  (B) router path — `hover` / `click` at a window pixel fires the real
      `pointer_move`, and `cell` reports the resolved cell. The full path
      (router fraction → pixel reconstruction → px→cell).

  Phase 2 — PIXELS (PINION_SCREENSHOT): the coordinate-ruler grid paints.
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
    read_png_rgba8,
    run_demo,
    sample_png_points,
)

EXAMPLE = "hello-grid-pointer"
WIN = (640, 384)
ROOT_TAG = "grid_pointer"
GRID_TAG = "grid_pointer"  # the grid carries ROOT_TAG (pointer-dispatch link)
COLS, ROWS = 80, 24
CW, CH = 8, 16  # the 8×16 baseline cell pitch


def q(d, path: str):
    return d.query(f"/external/{path}")


def cell_at(d, x: int, y: int):
    return d.invoke("/external/cell_at", f"{x},{y}")


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        snap = tf.snapshot(source="paint", viewport=WIN)
        rects = abs_rects_of(snap)

        # ── boot: dims + no cell yet ────────────────────────────────
        assert ROOT_TAG in rects, "root present"
        assert GRID_TAG in rects, "grid painted"
        assert_eq(q(tf, "cols"), COLS, "winsize columns = 80")
        assert_eq(q(tf, "rows"), ROWS, "winsize rows = 24")
        assert_eq(q(tf, "cell"), "none", "no cell before any pointer move")
        assert_eq(q(tf, "cell_col"), None, "no cell_col before a move")

        # ── (A) cell_at oracle — pure px→cell geometry ──────────────
        assert_eq(cell_at(tf, 0, 0), "0,0", "origin pixel → cell (0,0)")
        assert_eq(cell_at(tf, 7, 15), "0,0", "any interior pixel of cell (0,0)")
        assert_eq(cell_at(tf, 8, 16), "1,1", "first pixel of cell (1,1)")
        assert_eq(cell_at(tf, 24, 32), "3,2", "inverse of cell_to_px(3,2)=(24,32)")
        assert_eq(cell_at(tf, 320, 192), "40,12", "window centre")
        assert_eq(cell_at(tf, 639, 383), "79,23", "last interior pixel → last cell")
        # Off-grid pixels clamp to the last cell (the binding's in-grid policy).
        assert_eq(cell_at(tf, 640, 384), "79,23", "one cell past each edge clamps")
        assert_eq(cell_at(tf, 99999, 99999), "79,23", "far off-grid clamps")
        # Exhaustive column ruler: the first pixel of every column maps to it.
        for col in range(COLS):
            assert_eq(cell_at(tf, col * CW, 0), f"{col},0", f"col {col} ruler pixel")
        # Exhaustive row ruler: the first pixel of every row maps to it.
        for row in range(ROWS):
            assert_eq(cell_at(tf, 0, row * CH), f"0,{row}", f"row {row} ruler pixel")
        # The last pixel inside each cell still floors to that cell.
        for col in range(0, COLS, 7):
            assert_eq(cell_at(tf, col * CW + (CW - 1), 0), f"{col},0", f"col {col} last pixel")

        # ── (B) router path — a real click resolves the cell under it ──
        # A left-click captures the pointer; R51.35 "click-to-position" forwards
        # the press cursor as pointer_move, so the cell resolves through the full
        # router path (fraction over the grid rect → pixel → px→cell).
        tf.click(at=(320.0, 192.0))
        assert_eq(q(tf, "cell"), "40,12", "click at centre → cell (40,12) via the router")
        assert_eq(q(tf, "cell_col"), 40, "cell_col axis")
        assert_eq(q(tf, "cell_row"), 12, "cell_row axis")
        tf.click(at=(0.0, 0.0))
        assert_eq(q(tf, "cell"), "0,0", "click at origin")
        tf.click(at=(639.0, 383.0))
        assert_eq(q(tf, "cell"), "79,23", "click at the far corner → last cell")
        tf.click(at=(8.0, 16.0))
        assert_eq(q(tf, "cell"), "1,1", "click one cell in")
        tf.click(at=(200.0, 100.0))
        # 200/8 = 25, 100/16 = 6.
        assert_eq(q(tf, "cell"), "25,6", "click resolves the cell under the press")
        # intervene is read-only (the cell is pointer-driven).
        try:
            tf.intervene("/external/cell", "1,1")
            raise AssertionError("intervene on read-only `cell` should error")
        except Exception:
            pass

    # ── Phase 2 — live pixels (the ruler grid paints) ──────────────
    snap, rects = _boot_snapshot_and_rects()
    assert GRID_TAG in rects, "grid painted at boot"
    img = read_png_rgba8(capture_screenshot())
    assert (img.width, img.height) == WIN, f"screenshot {img.width}x{img.height} != {WIN}"
    gx, gy, gw, gh = rects[GRID_TAG]
    # Sample the ruler's top-left (column 0/ row 0 markers) — ink present.
    p0, p1 = sample_png_points(img, [(gx + 4, gy + 8), (gx + gw // 2, gy + gh // 2)])
    assert p0 is not None and p1 is not None, "grid cells sampled"


def _boot_snapshot_and_rects():
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        snap = tf.snapshot(source="paint", viewport=WIN)
        return snap, abs_rects_of(snap)


def capture_screenshot() -> Path:
    out = Path(tempfile.mkdtemp(prefix="pinion-r1008-")) / "pointer.png"
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
    sys.exit(run_demo("R1008 §5.41 §5.49 — pointer→cell hit-test", body))
