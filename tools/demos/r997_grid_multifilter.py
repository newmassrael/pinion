#!/usr/bin/env python3
"""R997 §5.40 — multi-facet predicate filter at scale E2E.

Drives `hello-grid-multifilter` via JSON-RPC. A 10,000-row virtualized
data-grid whose AI-first column filter is a CONJUNCTION (AND) of per-column
facets with comparison operators — `invoke "set_filter"
"0~Alpha&1>=500&2=Active"` (Name contains "Alpha" AND Score >= 500 AND
Status == Active). Generalizes the R783 single exact-match `GridFilter`; the
ordered ops are numeric-aware (the `cell_cmp` SSOT the sort uses), composing
with the clickable sort headers and the data-indexed selection:

  * primary `VirtualSelectExternal` (R746) at `vtbl` — single row selection
    held by SOURCE data index;
  * extra `GridSortExternal` (R778 + R783 + R997) at `vsort` — the sort proxy
    now carrying a MULTI-FACET conjunction filter (filter-then-sort);
  * R775 `view_virtual_table` — frozen header + windowed body over the
    `(filter, sort)` order permutation.

  Data (per data row id):
    col 0 Name   = "<Category><id>", Category = ["Alpha","Bravo","Charlie",
                   "Delta","Echo"][id % 5] — so `~Alpha` keeps id%5==0.
    col 1 Score  = (id * 7919) % 1000 — the numeric `>=` column.
    col 2 Status = ["Idle","Active","Done"][id % 3] — the exact `=` column.

  (A) boot — unfiltered, view_len == N, small window, full count.
  (B) multi-facet conjunction — `set_filter "0~Alpha&1>=500&2=Active"` returns
      the survivor count; every rendered row satisfies ALL three facets; the
      wire string round-trips through `query "filter"`.
  (C) relaxing a facet grows the view (3-facet ⊂ 2-facet ⊂ 1-facet).
  (D) the numeric `>=` op is numeric-aware (a low-score row is excluded, a
      high-score row kept; not lexicographic).
  (E) the `~` substring op keeps a category by name containment.
  (F) filter ⟂ sort composes — survivors sort by numeric Score, still satisfy.
  (G) selection ⊥ filter — select a non-matching source row, apply the
      conjunction, the selection survives off-view; clearing shows it again.
  (H) admin restore via `intervene`; a phantom-column facet is dropped, an
      all-phantom conjunction clamps to unfiltered.
  (I) clear via Null restores the full 10000 view.

  Phase 2 — PIXELS (PINION_SCREENSHOT): the grid paints (header vs cell bands).
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
)

EXAMPLE = "hello-grid-multifilter"
WIN = (400, 480)
N = 10_000
NCOLS = 3
ROW_H = 36
NAME_COL = 0
SCORE_COL = 1
STATUS_COL = 2
GRID_TAG = "vtbl"
SORT_TAG = "vsort"
SCROLL_TAG = "vtbl_scroll"

CATEGORIES = ["Alpha", "Bravo", "Charlie", "Delta", "Echo"]
STATUS = ["Idle", "Active", "Done"]


def score(i: int) -> int:
    return (i * 7919) % 1000


def name_of(i: int) -> str:
    return f"{CATEGORIES[i % 5]}{i:04d}"


def status_of(i: int) -> str:
    return STATUS[i % 3]


def satisfies_conjunction(i: int) -> bool:
    """Name contains "Alpha" AND Score >= 500 AND Status == Active."""
    return "Alpha" in name_of(i) and score(i) >= 500 and status_of(i) == "Active"


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

        # ── (B) multi-facet conjunction shrinks the view ────────────
        conj_count = sum(1 for i in range(N) if satisfies_conjunction(i))
        assert conj_count > 0, "the demo conjunction keeps a non-empty subset"
        wire = "0~Alpha&1>=500&2=Active"
        assert_eq(
            tf.invoke(f"/{SORT_TAG}/external/set_filter", wire),
            conj_count,
            "set_filter returns the conjunction survivor count in one round-trip",
        )
        assert_eq(filter_str(tf), wire, "the multi-facet wire string round-trips")
        assert_eq(view_len(tf), conj_count, "view_len shrank to the conjunction survivors")
        snap = tf.snapshot(source="paint", viewport=WIN)
        rows = present_rows(snap)
        assert rows, "the filtered window still renders rows"
        for i in rows:
            assert "Alpha" in name_of(i), f"rendered row {i} Name contains Alpha"
            assert score(i) >= 500, f"rendered row {i} Score >= 500"
            assert status_of(i) == "Active", f"rendered row {i} Status == Active"
        # Visual 0 is the first SOURCE row that satisfies every facet.
        first = next(i for i in range(N) if satisfies_conjunction(i))
        assert_eq(source_at(tf, 0), first, "filtered visual 0 is the first conjunction survivor")

        # ── (C) relaxing a facet grows the view ─────────────────────
        two_facet = sum(1 for i in range(N) if "Alpha" in name_of(i) and status_of(i) == "Active")
        assert_eq(
            tf.invoke(f"/{SORT_TAG}/external/set_filter", "0~Alpha&2=Active"),
            two_facet,
            "dropping the Score facet grows the view",
        )
        assert two_facet > conj_count, "a 2-facet superset is larger than the 3-facet conjunction"
        one_facet = sum(1 for i in range(N) if status_of(i) == "Active")
        assert_eq(
            tf.invoke(f"/{SORT_TAG}/external/set_filter", "2=Active"),
            one_facet,
            "a single Eq facet (back-compat) is the widest",
        )
        assert one_facet > two_facet, "fewer facets keep at least as many rows"

        # ── (D) the numeric `>=` op is numeric-aware ────────────────
        ge_count = sum(1 for i in range(N) if score(i) >= 500)
        assert_eq(
            tf.invoke(f"/{SORT_TAG}/external/set_filter", "1>=500"),
            ge_count,
            "numeric `>= 500` keeps every row with Score >= 500",
        )
        for pos in range(6):
            src = source_at(tf, pos)
            assert score(src) >= 500, f"survivor at visual {pos} has Score >= 500 (numeric, not text)"
        # A strict `<` over the same column is the complement (no row is both).
        lt_count = sum(1 for i in range(N) if score(i) < 500)
        assert_eq(
            tf.invoke(f"/{SORT_TAG}/external/set_filter", "1<500"),
            lt_count,
            "`< 500` is the complement of `>= 500`",
        )
        assert ge_count + lt_count == N, "the two halves partition the dataset"

        # ── (E) the `~` substring op keeps a category ───────────────
        bravo_count = sum(1 for i in range(N) if "Bravo" in name_of(i))
        assert_eq(
            tf.invoke(f"/{SORT_TAG}/external/set_filter", "0~Bravo"),
            bravo_count,
            "`~Bravo` keeps rows whose Name contains Bravo",
        )
        snap = tf.snapshot(source="paint", viewport=WIN)
        for i in present_rows(snap):
            assert "Bravo" in name_of(i), f"rendered row {i} Name contains Bravo"

        # ── (F) filter ⟂ sort composes (numeric over survivors) ─────
        tf.invoke(f"/{SORT_TAG}/external/set_filter", wire)  # back to the conjunction
        tf.invoke(f"/{SORT_TAG}/external/cycle_sort", SCORE_COL)  # Score ascending
        assert_eq(tf.query(f"/{SORT_TAG}/external/sort"), "1:ascending", "sort applied")
        assert_eq(view_len(tf), conj_count, "sorting does not change the view size")
        prev = -1
        for pos in range(min(6, conj_count)):
            src = source_at(tf, pos)
            assert satisfies_conjunction(src), f"survivor at visual {pos} still satisfies all facets"
            assert score(src) >= prev, f"filtered survivors sort by numeric Score at visual {pos}"
            prev = score(src)
        tf.invoke(f"/{SORT_TAG}/external/cycle_sort", SCORE_COL)  # desc
        tf.invoke(f"/{SORT_TAG}/external/cycle_sort", SCORE_COL)  # back to none

        # ── (G) selection ⊥ filter (survives being filtered out) ────
        tf.invoke(f"/{SORT_TAG}/external/set_filter", None)  # clear first
        assert_eq(view_len(tf), N, "cleared filter restores the full view")
        # Source row 0 is Alpha0000 / Idle — fails the conjunction (Status).
        assert not satisfies_conjunction(0), "row 0 is excluded by the conjunction"
        assert_eq(tf.invoke("/external/select", 0), 0, "select source row 0")
        assert_eq(selected(tf), 0, "row 0 selected")
        assert_eq(
            tf.invoke(f"/{SORT_TAG}/external/set_filter", wire),
            conj_count,
            "re-apply the conjunction",
        )
        assert_eq(selected(tf), 0, "the non-matching selection SURVIVES being filtered out")
        assert 0 not in present_rows(tf.snapshot(source="paint", viewport=WIN)), \
            "the selected row is not rendered while the conjunction excludes it"
        tf.invoke(f"/{SORT_TAG}/external/set_filter", None)
        assert_eq(selected(tf), 0, "clearing the filter keeps the selection")

        # ── (H) admin restore via intervene + phantom-facet drop ────
        tf.intervene(f"/{SORT_TAG}/external/filter", "2=Done")
        done_count = sum(1 for i in range(N) if status_of(i) == "Done")
        assert_eq(filter_str(tf), "2=Done", "intervene sets the filter (admin/restore)")
        assert_eq(view_len(tf), done_count, "Done view_len via intervene")
        tf.intervene(f"/{SORT_TAG}/external/filter", None)
        assert_eq(filter_str(tf), "none", "intervene Null clears the filter")
        # A phantom-column facet is dropped; the valid facet still applies.
        tf.invoke(f"/{SORT_TAG}/external/set_filter", "9=x&2=Active")
        assert_eq(filter_str(tf), "2=Active", "phantom facet dropped, valid facet kept")
        assert_eq(view_len(tf), one_facet, "the surviving facet drives the view")
        # An all-phantom conjunction clamps to unfiltered.
        tf.invoke(f"/{SORT_TAG}/external/set_filter", "9=x")
        assert_eq(filter_str(tf), "none", "all-phantom conjunction clamps to unfiltered")
        assert_eq(view_len(tf), N, "all-phantom filter keeps the full view")

        # ── (I) clear via invoke restores full view ─────────────────
        tf.invoke(f"/{SORT_TAG}/external/set_filter", wire)
        assert_eq(view_len(tf), conj_count, "conjunction re-applied")
        assert_eq(tf.invoke(f"/{SORT_TAG}/external/set_filter", None), N, "Null clears → full view")
        assert_eq(filter_str(tf), "none", "filter cleared")

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
    out = Path(tempfile.mkdtemp(prefix="pinion-r997-")) / "multifilter.png"
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
    sys.exit(run_demo("R997 §5.40 — multi-facet predicate filter at scale", body))
