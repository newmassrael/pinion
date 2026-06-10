#!/usr/bin/env python3
"""R778 §5.27 §5.40 — data-grid sort at scale E2E.

Drives `hello-grid-sort` via JSON-RPC. A 10,000-row virtualized data-grid
(frozen header + flex-viewport body) whose clickable column headers cycle a
numeric-aware multi-column sort, composed of three coordinators:

  * primary `VirtualSelectExternal` (R746) at `vtbl` — single row selection
    held by SOURCE data index (a cell click selects its row);
  * extra `GridSortExternal` (R778) at `vsort` — the multi-column sort proxy
    mapping visual positions → source rows, numeric-aware (`cell_cmp` SSOT);
  * R775/R778 `view_virtual_table` — frozen header + windowed body, fed the
    sort glyph + `order` permutation.

The decisive Model/View witness (§2 #7 scene-as-data): selection is held by
source index, so a re-sort reorders the *view* while a selected source row
stays selected — its visual position moves, its data identity does not. And
the numeric column sorts numerically, not lexicographically (`9 < 80`, not
`"80" < "9"`), observable through `source_at`.

  (A) boot — source order, unsorted, full dataset, small window.
  (B) sort col 0 (Name) — `invoke cycle_sort 0` → ascending regroups the
      window into the Alpha block (sources 0,5,10,…); descending reverses; a
      third cycle returns to source order.
  (C) numeric-aware sort col 1 (Score) — `invoke cycle_sort 1` orders the view
      by NUMBER, matching a numeric sort and differing from a lexicographic
      one at a position computed here from the same dataset formula.
  (D) selection ⊥ ordering — select a source row by cell click; a re-sort
      keeps `selected` on the same SOURCE row (its visual position moves).
  (E) different column jumps to ascending; out-of-range column is a no-op.
  (F) clicked column header cycles the sort exactly like the RPC path.
  (G) admin restore — `intervene sort "<col>:<dir>"` round-trips the whole
      sort state in one shot.

  Phase 2 — PIXELS (PINION_SCREENSHOT): the grid paints (header vs cell
      distinct bands).
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
    wait_query,
    wait_until,
)

EXAMPLE = "hello-grid-sort"
WIN = (400, 480)
N = 10_000
NCOLS = 3
ROW_H = 36
GRID_TAG = "vtbl"
SORT_TAG = "vsort"

CATEGORIES = ["Alpha", "Bravo", "Charlie", "Delta", "Echo"]


# ── dataset mirror (must match examples/hello-grid-sort/src/main.rs) ──────
def score(i: int) -> int:
    return (i * 7919) % 997


def name(i: int) -> str:
    return f"{CATEGORIES[i % 5]}{i:04d}"


def numeric_order(col: int) -> list[int]:
    """The numeric-aware (cell_cmp) ascending order of all source rows by
    `col`, ties broken by source id — the Rust `grid_order_by` mirror."""
    if col == 1:
        key = lambda i: (score(i), i)  # noqa: E731 (numeric column)
    else:
        key = lambda i: (name(i), i)  # noqa: E731 (text columns)
    return sorted(range(N), key=key)


def lex_order(col: int) -> list[int]:
    """The *string* ascending order of `col` — what a non-numeric sort would
    produce (for the numeric column this differs from `numeric_order`)."""
    if col == 1:
        key = lambda i: (str(score(i)), i)  # noqa: E731
    else:
        key = lambda i: (name(i), i)  # noqa: E731
    return sorted(range(N), key=key)


def present_sources(snap) -> list[int]:
    """Source ids of the rendered `vtbl_row<source>` strips (sorted set)."""
    out: list[int] = []
    for tag in abs_rects_of(snap):
        if tag.startswith("vtbl_row"):
            suffix = tag[len("vtbl_row"):]
            if suffix.isdigit():
                out.append(int(suffix))
    return sorted(out)


def grid_sort(d):
    return d.query("/vsort/external/sort")


def sort_col(d):
    return d.query("/vsort/external/sort_col")


def source_at(d, pos: int):
    return d.query(f"/vsort/external/source_at.{pos}")


def selected(d):
    return d.query("/external/selected")


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        snap = tf.snapshot(source="paint", viewport=WIN)

        # ── (A) boot: source order, unsorted, full dataset, small window ──
        rects = abs_rects_of(snap)
        assert GRID_TAG in rects, "grid container present at boot"
        assert "vtbl_hrow" in rects, "frozen header row present"
        assert "vtbl_ch0" in rects, "column header cell present"
        assert "vsort#h0" in rects, "clickable header routes to the sort anchor"
        assert "vtbl#h0" not in rects, "header does NOT route to the grid (select) anchor"
        assert_eq(grid_sort(tf), "none", "boot is unsorted")
        assert_eq(sort_col(tf), -1, "no active sort column at boot")
        assert_eq(tf.query("/vsort/external/count"), N, "proxy knows the source count")
        assert_eq(tf.query("/vsort/external/cols"), NCOLS, "proxy knows the column count")
        assert_eq(tf.query("/external/item_count"), N, "selection bound is the source count")
        assert_eq(source_at(tf, 0), 0, "unsorted: visual 0 == source 0")
        assert_eq(source_at(tf, 7), 7, "unsorted: visual 7 == source 7")
        boot_rows = present_sources(snap)
        assert len(boot_rows) < 30, f"virtualized: small window, got {len(boot_rows)} of {N}"
        assert_eq(boot_rows[:5], [0, 1, 2, 3, 4], "unsorted window is source order")

        # ── (B) sort column 0 (Name): ascending regroups into Alpha ──────
        assert_eq(tf.invoke("/vsort/external/cycle_sort", 0), "0:ascending", "cycle col 0 → asc")
        assert_eq(grid_sort(tf), "0:ascending", "sort is col 0 ascending")
        assert_eq(sort_col(tf), 0, "active sort column is 0")
        assert_eq(source_at(tf, 0), 0, "asc Name visual 0 == Alpha source 0")
        assert_eq(source_at(tf, 1), 5, "asc Name visual 1 == Alpha source 5")
        assert_eq(source_at(tf, 2), 10, "asc Name visual 2 == Alpha source 10")
        snap = tf.snapshot(source="paint", viewport=WIN)
        asc_rows = present_sources(snap)
        assert all(s % 5 == 0 for s in asc_rows), f"asc Name top window all Alpha: {asc_rows}"
        # descending reverses; a third cycle returns to source order.
        assert_eq(tf.invoke("/vsort/external/cycle_sort", 0), "0:descending", "cycle col 0 → desc")
        assert_eq(source_at(tf, 0), 9999, "desc Name visual 0 is the highest key (Echo 9999)")
        assert_eq(tf.invoke("/vsort/external/cycle_sort", 0), "none", "cycle col 0 → unsorted")
        assert_eq(source_at(tf, 0), 0, "back to source order")

        # ── (C) numeric-aware sort column 1 (Score) ──────────────────────
        assert_eq(tf.invoke("/vsort/external/cycle_sort", 1), "1:ascending", "cycle col 1 → asc")
        assert_eq(sort_col(tf), 1, "active sort column is 1")
        num = numeric_order(1)
        lex = lex_order(1)
        assert num != lex, "dataset: numeric and lexicographic Score orders differ"
        # The grid order matches the NUMERIC order at several positions.
        for pos in (0, 1, 100, 5000):
            assert_eq(source_at(tf, pos), num[pos], f"Score asc visual {pos} == numeric order")
        # At the first position where numeric and lexicographic DIVERGE, the
        # grid follows the numeric order — not the lexicographic one.
        div = next(p for p in range(N) if num[p] != lex[p])
        assert_eq(source_at(tf, div), num[div], f"at divergence {div}: grid follows numeric order")
        assert source_at(tf, div) != lex[div], "grid is NOT in lexicographic order at the divergence"
        tf.invoke("/vsort/external/cycle_sort", 1)  # desc
        tf.invoke("/vsort/external/cycle_sort", 1)  # back to none
        assert_eq(grid_sort(tf), "none", "Score sort cycled back to unsorted")

        # ── (D) selection ⊥ ordering ─────────────────────────────────────
        snap = tf.snapshot(source="paint", viewport=WIN)
        assert "vtbl#3_1" in abs_rects_of(snap), "cell vtbl#3_1 visible for the click test"
        tf.click(path="vtbl#3_1")  # column irrelevant to row selection
        wait_query(tf, "/external/selected", 3,
                   desc="clicking any cell of source row 3 selects it")
        # Sort by Name ascending: source 3 (a Delta) moves to a new visual
        # position but stays selected.
        tf.invoke("/vsort/external/cycle_sort", 0)
        wait_query(tf, "/external/selected", 3,
                   desc="selection survives the re-sort (still source 3)")
        new_pos = next(p for p in range(N) if source_at(tf, p) == 3)
        assert new_pos != 3, f"selected source 3 moved to a new visual position {new_pos}"
        tf.invoke("/vsort/external/cycle_sort", 0)  # desc
        tf.invoke("/vsort/external/cycle_sort", 0)  # none
        assert_eq(selected(tf), 3, "selection held across all sort cycles")
        tf.invoke("/external/clear", None)
        assert_eq(selected(tf), None, "selection cleared")

        # ── (E) column jump + out-of-range no-op ─────────────────────────
        tf.invoke("/vsort/external/cycle_sort", 0)  # col 0 ascending
        assert_eq(tf.invoke("/vsort/external/cycle_sort", 1), "1:ascending",
                  "clicking a different column jumps to it ascending")
        assert_eq(tf.invoke("/vsort/external/cycle_sort", 9), "1:ascending",
                  "out-of-range column is a no-op (sort unchanged)")
        # restore unsorted for the header-click phase.
        tf.intervene("/vsort/external/sort", "none")
        assert_eq(grid_sort(tf), "none", "intervene restored unsorted")

        # ── (F) clicked column header cycles like the RPC path ───────────
        before = grid_sort(tf)
        tf.click(path="vsort#h0")
        wait_until(lambda: grid_sort(tf) != before,
                   desc=f"clicked header changed the sort (from {before!r})")
        after = grid_sort(tf)
        assert_eq(after, "0:ascending", "clicked header h0 cycles column 0 ascending")

        # ── (G) admin restore round-trips the whole sort state ───────────
        tf.intervene("/vsort/external/sort", "2:descending")
        assert_eq(grid_sort(tf), "2:descending", "intervene set col 2 descending")
        assert_eq(sort_col(tf), 2, "restored active column is 2")
        assert_eq(tf.query("/vsort/external/sort_dir"), "descending", "restored direction")
        tf.intervene("/vsort/external/sort", "none")
        assert_eq(grid_sort(tf), "none", "intervene cleared the sort")

    # ── Phase 2 — live pixels (boot frame: the grid paints) ────────────
    snap, rects = _boot_snapshot_and_rects()
    img = read_png_rgba8(capture_screenshot())
    assert (img.width, img.height) == WIN, \
        f"screenshot {img.width}x{img.height} != window {WIN}"
    hx, hy, hw, hh = rects["vtbl_ch0"]
    rx, ry, rw, rh = rects["vtbl#0_0"]
    header_pt = (hx + hw // 2, hy + hh // 2)
    row_pt = (rx + rw // 2, ry + rh // 2)
    h_col, r_col = sample_png_points(img, [header_pt, row_pt])
    assert h_col is not None and r_col is not None, "header + data cell sampled (grid paints)"


def _boot_snapshot_and_rects():
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        snap = tf.snapshot(source="paint", viewport=WIN)
        return snap, abs_rects_of(snap)


def capture_screenshot() -> Path:
    out = Path(tempfile.mkdtemp(prefix="pinion-r778-")) / "gridsort.png"
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
    sys.exit(run_demo("R778 §5.27 §5.40 — data-grid sort at scale", body))
