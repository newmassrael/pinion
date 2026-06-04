#!/usr/bin/env python3
"""R783 §5.40 — data-grid filter at scale E2E.

Drives `hello-grid-filter` via JSON-RPC. A 10,000-row virtualized data-grid
whose AI-first column filter (`invoke "set_filter" "<col>=<value>"`) shrinks
the view to matching rows, composing with the clickable numeric-aware sort
and the data-indexed selection:

  * primary `VirtualSelectExternal` (R746) at `vtbl` — single row selection
    held by SOURCE data index (a cell click selects its row);
  * extra `GridSortExternal` (R778 + R783) at `vsort` — the sort proxy now
    carrying a single-facet column FILTER (filter-then-sort over `cell_cmp`);
  * R775 `view_virtual_table` — frozen header + windowed body over the
    `(filter, sort)` order permutation.

The decisive Model/View witness (§2 #7 scene-as-data): set a filter and
`view_len` shrinks, the rendered window holds only matching rows, yet a
selected SOURCE row survives even when filtered out of view (selection ⊥
filter ⊥ ordering). The numeric sort then orders only the survivors.

  Status column (col 2) cycles Idle / Active / Done by `id % 3`
  (Idle=id%3==0, Active==1, Done==2), so Active = 3333 of 10000.

  (A) boot — unfiltered, view_len == N, small window, full count.
  (B) filter Status=Active — `invoke set_filter "2=Active"` returns the new
      view_len (3333); the window holds only Active source rows.
  (C) filter ⟂ sort composes — `cycle_sort 1` orders the FILTERED survivors by
      numeric Score (monotonic, still all Active).
  (D) selection ⊥ filter — select a source row, filter it out, `selected`
      survives; clearing the filter shows it again.
  (E) clear filter — `set_filter` Null restores the full 10000 view.
  (F) admin restore — `intervene filter "2=Done"` / Null round-trips; a
      phantom column clamps to unfiltered.

  Phase 2 — PIXELS (PINION_SCREENSHOT): the grid paints (header vs cell
      distinct bands).
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

EXAMPLE = "hello-grid-filter"
WIN = (400, 480)
N = 10_000
NCOLS = 3
ROW_H = 36
STATUS_COL = 2
GRID_TAG = "vtbl"
SORT_TAG = "vsort"
SCROLL_TAG = "vtbl_scroll"
PAUSE = 0.12

STATUS = ["Idle", "Active", "Done"]


def score(i: int) -> int:
    return (i * 7919) % 997


def status_of(i: int) -> str:
    return STATUS[i % 3]


def filter_str(d):
    return d.query(f"/{SORT_TAG}/external/filter")


def view_len(d):
    return d.query(f"/{SORT_TAG}/external/view_len")


def source_at(d, pos: int):
    return d.query(f"/{SORT_TAG}/external/source_at.{pos}")


def selected(d):
    return d.query("/external/selected")


def present_rows(snap) -> list[int]:
    """Rendered data-row ids (from the `vtbl_row<id>` strips)."""
    out: list[int] = []
    for tag in abs_rects_of(snap):
        if tag.startswith("vtbl_row"):
            suffix = tag[len("vtbl_row"):]
            if suffix.isdigit():
                out.append(int(suffix))
    return out


def scroll_offset(snap) -> int:
    node = find_by_tag(snap, SCROLL_TAG)
    assert node is not None, "scroll node present"
    return int(node.get("offset_y", -1))


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        snap = tf.snapshot(source="paint", viewport=WIN)
        rects = abs_rects_of(snap)

        # ── (A) boot: unfiltered, full view ─────────────────────────
        assert GRID_TAG in rects, "grid container present at boot"
        assert "vtbl_hrow" in rects, "frozen header row present"
        assert "vsort#h0" in rects, "clickable header routes to the sort/filter anchor"
        assert_eq(filter_str(tf), "none", "unfiltered at boot")
        assert_eq(view_len(tf), N, "boot view is the full dataset")
        assert_eq(tf.query(f"/{SORT_TAG}/external/count"), N, "proxy knows the source count")
        assert_eq(tf.query("/external/item_count"), N, "selection bound is the source count")
        assert_eq(source_at(tf, 0), 0, "unfiltered: visual 0 == source 0")
        boot_rows = present_rows(snap)
        assert len(boot_rows) < 30, f"virtualized: small window, got {len(boot_rows)} of {N}"

        # ── (B) filter Status=Active shrinks the view ───────────────
        active_count = sum(1 for i in range(N) if status_of(i) == "Active")
        assert_eq(active_count, 3333, "Active = id%3==1 over 0..10000")
        assert_eq(
            tf.invoke(f"/{SORT_TAG}/external/set_filter", "2=Active"),
            active_count,
            "set_filter returns the resulting view_len in one round-trip",
        )
        assert_eq(filter_str(tf), "2=Active", "filter query reflects the active facet")
        assert_eq(view_len(tf), active_count, "view_len shrank to the Active rows")
        snap = tf.snapshot(source="paint", viewport=WIN)
        filtered_rows = present_rows(snap)
        assert filtered_rows, "the filtered window still renders rows"
        assert all(status_of(i) == "Active" for i in filtered_rows), \
            f"every rendered row is Status=Active, got {sorted(filtered_rows)[:8]}"
        # The filtered view is dense in visual space: visual 0 is the first
        # Active source row (id 1).
        assert_eq(source_at(tf, 0), 1, "filtered visual 0 is the first Active source (id 1)")
        assert_eq(status_of(source_at(tf, 5)), "Active", "deep filtered visual is Active too")

        # ── (C) filter ⟂ sort composes (numeric over survivors) ─────
        tf.invoke(f"/{SORT_TAG}/external/cycle_sort", 1)  # Score ascending
        assert_eq(tf.query(f"/{SORT_TAG}/external/sort"), "1:ascending", "sort applied")
        assert_eq(view_len(tf), active_count, "sorting does not change the view size")
        prev = -1
        for pos in range(6):
            src = source_at(tf, pos)
            assert status_of(src) == "Active", f"survivor at visual {pos} is still Active"
            assert score(src) >= prev, f"filtered survivors sort by numeric Score at visual {pos}"
            prev = score(src)
        tf.invoke(f"/{SORT_TAG}/external/cycle_sort", 1)  # desc
        tf.invoke(f"/{SORT_TAG}/external/cycle_sort", 1)  # back to none

        # ── (D) selection ⊥ filter (survives being filtered out) ────
        # Source row 0 is Status=Idle — select it while UNFILTERED, then filter
        # to Active (which excludes it); the selection survives off-view.
        tf.invoke(f"/{SORT_TAG}/external/set_filter", None)  # clear first
        assert_eq(view_len(tf), N, "cleared filter restores the full view")
        assert_eq(tf.invoke("/external/select", 0), 0, "select source row 0 (Idle)")
        assert_eq(selected(tf), 0, "row 0 selected")
        kept = tf.invoke(f"/{SORT_TAG}/external/set_filter", "2=Active")
        assert_eq(kept, active_count, "re-filter to Active")
        assert_eq(selected(tf), 0, "the Idle selection SURVIVES being filtered out of view")
        assert 0 not in present_rows(tf.snapshot(source="paint", viewport=WIN)), \
            "the selected Idle row is not rendered while filtered to Active"
        tf.invoke(f"/{SORT_TAG}/external/set_filter", None)
        assert_eq(selected(tf), 0, "clearing the filter keeps the selection")

        # ── (E) clear via invoke restores full view ─────────────────
        tf.invoke(f"/{SORT_TAG}/external/set_filter", "2=Done")
        done_count = sum(1 for i in range(N) if status_of(i) == "Done")
        assert_eq(view_len(tf), done_count, "Done filter view_len")
        assert_eq(tf.invoke(f"/{SORT_TAG}/external/set_filter", None), N, "Null clears → full view")
        assert_eq(filter_str(tf), "none", "filter cleared")

        # ── (F) admin restore via intervene + phantom-column clamp ──
        tf.intervene(f"/{SORT_TAG}/external/filter", "2=Done")
        assert_eq(filter_str(tf), "2=Done", "intervene sets the filter (admin/restore)")
        assert_eq(view_len(tf), done_count, "Done view_len via intervene")
        tf.intervene(f"/{SORT_TAG}/external/filter", None)
        assert_eq(filter_str(tf), "none", "intervene Null clears the filter")
        # A phantom column clamps to unfiltered (col_count == 3).
        tf.invoke(f"/{SORT_TAG}/external/set_filter", "9=Active")
        assert_eq(filter_str(tf), "none", "filter on a phantom column clamps to unfiltered")
        assert_eq(view_len(tf), N, "phantom-column filter keeps the full view")

    # ── Phase 2 — live pixels (boot frame: grid paints) ────────────
    snap, rects = _boot_snapshot_and_rects()
    img = read_png_rgba8(capture_screenshot())
    assert (img.width, img.height) == WIN, \
        f"screenshot {img.width}x{img.height} != window {WIN}"
    hx, hy, hw, hh = rects["vtbl_ch0"]
    rx, ry, rw, rh = rects["vtbl#0_0"]
    h_col, r_col = sample_png_points(img, [(hx + hw // 2, hy + hh // 2), (rx + rw // 2, ry + rh // 2)])
    assert h_col is not None and r_col is not None, "header + data cell sampled"


def _boot_snapshot_and_rects():
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        snap = tf.snapshot(source="paint", viewport=WIN)
        return snap, abs_rects_of(snap)


def capture_screenshot() -> Path:
    out = Path(tempfile.mkdtemp(prefix="pinion-r783-")) / "gridfilter.png"
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
    sys.exit(run_demo("R783 §5.40 — data-grid filter at scale", body))
