#!/usr/bin/env python3
"""R1523 §5.27 §5.45 — column-axis virtualization at 200 columns, E2E.

Drives the `hello-virtual-columns` binding via JSON-RPC. Before this round the
data grid windowed the ROW axis (R744/R775) and merely gave the column axis a
horizontal *viewport* (R784): every column was built for every windowed row and
positioned off-screen. At 8 columns that is invisible; at 200 it is 40x the
cells in the layout walk, the paint encode, the a11y tree and this very
snapshot — the exact list R744 gives as its own reason to window rows.

The columns are deliberately UNEQUAL (a five-width cycle), so this demo carries
an independent Python oracle for the window: prefix-sum the widths and
binary-search the viewport edges. A grid that derived its window from a uniform
pitch would satisfy neither the membership assertions nor the x-position ones.

The decisive witness is scene-as-data (§2 #7), no pixels required:

  (A) boot structure — both scroll axes present and reporting their axis, and
      the columns overflowing the viewport by ~43x. (The surviving EXTENT — the
      scroll still bounding against all 200 columns while ~7 are in the tree —
      is witnessed by the clamp in (D), because a scroll node's wire form
      carries offsets and viewport but not the bound. A windowed axis that let
      its bound shrink to its window would trap itself in the window it
      started in.)
  (B) the column window itself — the cells in row 0 are one contiguous run
      matching the oracle, the header band holds the SAME run (they share one
      scroll, so a divergence would paint labels over the wrong columns), and
      the far column is absent from the tree rather than merely off-screen.
  (C) geometry — every windowed cell sits at exactly `prefix(col) - offset_x`,
      the position it would have had with all 200 columns built. That is the
      leading/trailing pad's whole job; without it the windowed cells would
      bunch at the viewport's left edge.
  (D) scrolling the window — a 6000px scroll replaces the entire column set,
      the header follows, the geometry still matches the oracle, and scrolling
      to the clamp reveals the last column while the first leaves the tree.
  (E) a11y — `aria-colcount` on the grid reports 200 while the tree holds the
      window, and every windowed cell / header carries its one-based absolute
      `aria-colindex`. Windowing an axis without its extent pair would make the
      grid LESS readable than before it scaled.
  (F) resize — a wider window widens the column set (the window is derived from
      the measured viewport, not a constant).
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    access_node_by_tag,
    assert_eq,
    find_by_tag,
    indexed_tags,
    run_demo,
    wait_snap,
    wait_until,
)

EXAMPLE = "hello-virtual-columns"
WIN = (560, 420)
N = 10_000
NCOLS = 200
ROW_H = 36
OVERSCAN = 2
WIDTH_CYCLE = (150, 90, 120, 105, 135)
TABLE_TAG = "vcol"
HEADER_TAG = "vcol_hrow"
V_SCROLL_TAG = "vcol_scroll"
H_SCROLL_TAG = "vcol_hscroll"


# ── the independent oracle ──────────────────────────────────────────

def col_width(col: int) -> int:
    return WIDTH_CYCLE[col % len(WIDTH_CYCLE)]


WIDTHS = [col_width(c) for c in range(NCOLS)]
TOTAL_W = sum(WIDTHS)
PREFIX = [0]
for _w in WIDTHS:
    PREFIX.append(PREFIX[-1] + _w)


def col_at(pixel: int) -> int:
    """The column whose span contains `pixel`, clamped to the last column."""
    for c in range(NCOLS):
        if PREFIX[c] <= pixel < PREFIX[c + 1]:
            return c
    return NCOLS - 1


def oracle_window(offset_x: int, viewport_w: int) -> list[int]:
    """The columns a `viewport_w`-wide viewport at `offset_x` exposes, padded by
    OVERSCAN on each side and clamped — computed here from the widths alone, so
    it agrees with the binding only if the binding really prefix-sums."""
    if viewport_w == 0:
        return []
    first = col_at(offset_x)
    last = col_at(offset_x + viewport_w - 1)
    first = max(0, first - OVERSCAN)
    last = min(NCOLS - 1, last + OVERSCAN)
    return list(range(first, last + 1))


# ── snapshot readers ────────────────────────────────────────────────

def hscroll_node(snap):
    node = find_by_tag(snap, H_SCROLL_TAG)
    assert node is not None, "outer horizontal scroll node present"
    return node


def vscroll_node(snap):
    node = find_by_tag(snap, V_SCROLL_TAG)
    assert node is not None, "inner vertical body scroll node present"
    return node


def offset_x(snap) -> int:
    return int(hscroll_node(snap).get("offset_x", -1))


def offset_y(snap) -> int:
    return int(vscroll_node(snap).get("offset_y", -1))


def hviewport(snap) -> tuple[int, int]:
    vp = hscroll_node(snap).get("viewport") or {}
    return int(vp.get("x", -1)), int(vp.get("w", -1))


def header_cols(rects) -> list[int]:
    return indexed_tags(rects, f"{TABLE_TAG}_ch")


def row_cols(rects, row: int) -> list[int]:
    return indexed_tags(rects, f"{TABLE_TAG}#{row}_")


def rendered_rows(rects) -> list[int]:
    return indexed_tags(rects, f"{TABLE_TAG}_row")


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:

        def snap_now():
            return tf.snapshot(source="paint", viewport=WIN)

        # ── (A) boot structure + the surviving extent ────────────────
        snap = snap_now()
        rects = abs_rects_of(snap)
        assert TABLE_TAG in rects, "grid root present at boot"
        assert HEADER_TAG in rects, "frozen header row present at boot"
        assert_eq(hscroll_node(snap).get("axis"), "horizontal",
                  "outer scroll reports axis=horizontal")
        assert_eq(vscroll_node(snap).get("axis"), "vertical",
                  "inner body scroll reports axis=vertical")
        assert_eq(offset_x(snap), 0, "boot horizontal offset is 0")
        assert_eq(offset_y(snap), 0, "boot vertical offset is 0")

        vp_x, vp_w = hviewport(snap)
        assert vp_w > 0, f"horizontal viewport measured > 0, got {vp_w}"
        assert vp_w < TOTAL_W // 10, \
            f"viewport ({vp_w}) is a small fraction of the columns ({TOTAL_W})"
        # The scroll bound is not in the snapshot's wire form (a scroll node
        # reports axis / offsets / viewport), so the extent claim — that the
        # bound is the whole content and not the window — is witnessed by the
        # clamp in (D): `offset_x` stops at exactly `TOTAL_W - viewport_w`.
        # That is the same witness r784 uses, and it is the assertion that
        # would fail if the pad were missing: a row shrunk to its window would
        # bound the scroll to the window it started in.

        # ── (B) the column window ───────────────────────────────────
        cols = row_cols(rects, 0)
        assert cols, "the visible columns are built"
        assert len(cols) < NCOLS // 10, \
            f"the column axis must window: {len(cols)} of {NCOLS} cells in row 0"
        assert_eq(cols, oracle_window(0, vp_w),
                  "the boot column window matches the prefix-sum oracle")
        assert_eq(cols, list(range(cols[0], cols[-1] + 1)),
                  "the rendered columns are one contiguous run")
        assert_eq(header_cols(rects), cols,
                  "the header band holds the SAME columns as the body")
        assert (NCOLS - 1) not in cols, "the far column is not in the tree at boot"
        assert f"{TABLE_TAG}_ch{NCOLS - 1}" not in rects, \
            "and neither is its header — absent, not merely off-screen"
        # Both axes window at once: this is a 200x10,000 grid in ~7x11 cells.
        rows = rendered_rows(rects)
        assert 0 < len(rows) < N // 100, \
            f"the row axis still windows too: {len(rows)} of {N} rows"

        # ── (C) geometry: the pad puts cells where they belong ───────
        def assert_geometry(rs, off: int, label: str) -> None:
            present = row_cols(rs, 0)
            for c in (present[0], present[len(present) // 2], present[-1]):
                assert_eq(rs[f"{TABLE_TAG}#0_{c}"][0], vp_x + PREFIX[c] - off,
                          f"{label}: cell col {c} sits at prefix({c}) - offset_x")
                assert_eq(rs[f"{TABLE_TAG}#0_{c}"][2], col_width(c),
                          f"{label}: cell col {c} carries its own (unequal) width")
                assert_eq(rs[f"{TABLE_TAG}_ch{c}"][0], rs[f"{TABLE_TAG}#0_{c}"][0],
                          f"{label}: header col {c} x-aligned with its body cell")

        assert_geometry(rects, 0, "boot")
        # The widths really are unequal, or every assertion above is a test of
        # uniform-pitch arithmetic.
        assert len(set(WIDTH_CYCLE)) >= 3, "the fixture's column widths are unequal"

        # ── (D) scrolling moves the window ──────────────────────────
        D = 6_000  # = PREFIX[50] exactly (ten 600px cycles)
        assert_eq(PREFIX[50], D, "the oracle agrees column 50 starts at 6000")
        tf.scroll(H_SCROLL_TAG, to=(D, 0))
        snap = wait_snap(
            tf, lambda s: offset_x(s) == D, viewport=WIN,
            desc="horizontal scroll advanced offset_x",
        )
        assert_eq(offset_y(snap), 0, "vertical offset unchanged by horizontal scroll")
        rects2 = abs_rects_of(snap)
        far = row_cols(rects2, 0)
        assert_eq(far, oracle_window(D, vp_w),
                  "the scrolled column window matches the oracle")
        assert_eq(header_cols(rects2), far, "header still in lockstep after the scroll")
        assert far[0] > cols[-1], \
            f"the whole boot column set has left the tree: {cols} -> {far}"
        for c in cols:
            assert f"{TABLE_TAG}#0_{c}" not in rects2, \
                f"boot column {c} is gone from the tree, not just off-screen"
        assert_geometry(rects2, D, "scrolled")

        # Scroll to the clamp: the last column arrives, the first leaves.
        tf.scroll(H_SCROLL_TAG, to=(10 ** 9, 0))
        snap = wait_snap(
            tf, lambda s: offset_x(s) == TOTAL_W - vp_w, viewport=WIN,
            desc="offset_x clamps to total_w - viewport_w",
        )
        rects3 = abs_rects_of(snap)
        end = row_cols(rects3, 0)
        assert (NCOLS - 1) in end, "the last column is in the tree at max scroll"
        assert 0 not in end, "and the first column has left it"
        assert_eq(header_cols(rects3), end, "header in lockstep at the clamp")
        assert_geometry(rects3, TOTAL_W - vp_w, "clamped")

        # ── (E) a11y: the extent the AT needs ───────────────────────
        acc = tf.request("scene/access").result
        grid = access_node_by_tag(acc, TABLE_TAG)
        assert grid is not None, "the grid node is in the access tree"
        assert_eq(grid.get("column_count"), NCOLS,
                  "aria-colcount reports the FULL 200-column extent")
        assert_eq(grid.get("size_of_set"), N,
                  "aria-setsize still reports the full 10,000-row extent")
        at_cols = []
        for c in end:
            node = access_node_by_tag(acc, f"{TABLE_TAG}_ch{c}")
            assert node is not None, f"windowed column {c} has an access node"
            at_cols.append(node.get("column_index"))
        assert_eq(at_cols, [c + 1 for c in end],
                  "every windowed header carries its one-based absolute aria-colindex")
        cell_node = access_node_by_tag(acc, f"{TABLE_TAG}#{rendered_rows(rects3)[0]}_{end[0]}")
        assert cell_node is not None, "a windowed gridcell has an access node"
        assert_eq(cell_node.get("column_index"), end[0] + 1,
                  "and so does the gridcell")
        # The AT tree and the paint tree hold the same columns — the a11y window
        # is derived from the same measured viewport, not a second guess.
        at_headers = [
            c for c in range(NCOLS)
            if access_node_by_tag(acc, f"{TABLE_TAG}_ch{c}") is not None
        ]
        assert_eq(at_headers, end, "the AT column window IS the painted one")

        # ── (F) resize re-windows ───────────────────────────────────
        tf.scroll(H_SCROLL_TAG, to=(0, 0))
        wait_snap(tf, lambda s: offset_x(s) == 0, viewport=WIN,
                  desc="horizontal scrolled back to 0")
        wide = (WIN[0] + 400, WIN[1])
        resp = tf.request("scene/resize", {"width": wide[0], "height": wide[1]})
        assert resp is not None and resp.result is not None, "scene/resize accepted"

        def widened():
            s = tf.snapshot(source="paint", viewport=wide)
            _, w = hviewport(s)
            return s if w > vp_w else None

        snap = wait_until(widened, desc="the grid re-measures its wider viewport")
        rects4 = abs_rects_of(snap)
        _, vp_w2 = hviewport(snap)
        grown = row_cols(rects4, 0)
        assert len(grown) > len(cols), \
            f"a wider window exposes more columns: {len(cols)} -> {len(grown)}"
        assert_eq(grown, oracle_window(0, vp_w2),
                  "the widened window matches the oracle at the new viewport")
        assert_eq(header_cols(rects4), grown, "header in lockstep after the resize")


if __name__ == "__main__":
    sys.exit(run_demo("R1523 §5.27 §5.45 — column-axis virtualization", body))
