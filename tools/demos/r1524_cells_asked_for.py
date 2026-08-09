#!/usr/bin/env python3
"""R1524 §5.27 — the grid asks for the cells it paints, E2E.

Drives the `hello-virtual-columns` binding via JSON-RPC. R1523 windowed the
data grid's COLUMN axis, so the scene tree stopped holding 200 cells per row
and held the ~7 the viewport exposes. But the *contract* stayed per-row:
`build_cells(row) -> Vec<String>` still produced every column's text, and the
substrate sliced the window out of it. The tree had been relieved of the 40x;
the consumer had not. R1524 replaces that with the Model/View contract every
framework with a virtualized grid carries — the toolkit `data(model index)`, another retained-mode toolkit
`cellBuilder(TableVicinity)` — one call per painted cell, addressed by
`CellIndex { row, col }`.

That change is invisible in the painted scene: the same cells appear either
way. Which is exactly why the count is published INTO the scene, at
`vcol_status`, where `scene/snapshot` reads it with no pixels and no profiler
(§2 #7). The demo then never trusts that number on its own — it holds it
against a second, independent observable, the number of data cells actually in
the tree. Their EQUALITY is what "asks for the cells it paints" means, and it
is the assertion a regression breaks in both directions:

  - ask for the whole row again and `asked` climbs to rows x 200 while the
    painted count stays put (this is the pre-R1524 state: measured 2400 vs 84);
  - ask for too few and the painted count falls below `asked`'s window.

Sections:

  (A) boot — the readout exists, parses, states the full extent it drew from,
      and its count equals the painted data-cell count.
  (B) the address is answered, not merely counted — every sampled cell's text
      is `r{row}c{col}` for the tag it is painted under, so a consumer answering
      for a transposed or off-by-one address would show up here even though the
      counts matched.
  (C) magnitude — `asked` is an order of magnitude below the `rows x NCOLS` a
      per-row builder cost, computed from the row count in the tree rather than
      from a remembered constant.
  (D) the horizontal axis — scrolling replaces the column set, and the identity
      holds at the new offset and at the clamp.
  (E) the vertical axis — scrolling rows changes `asked` too: the contract is
      per cell, so it tracks BOTH windows, not just the one this round moved.
  (F) resize — a wider viewport asks for strictly more cells, a shorter one for
      strictly fewer, and the identity survives both.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    find_by_tag,
    indexed_tags,
    run_demo,
    texts_of,
    wait_snap,
    wait_until,
)

EXAMPLE = "hello-virtual-columns"
WIN = (560, 420)
N = 10_000
NCOLS = 200
TABLE_TAG = "vcol"
STATUS_TAG = "vcol_status"
V_SCROLL_TAG = "vcol_scroll"
H_SCROLL_TAG = "vcol_hscroll"

# `asked {n} cells · table {N}×{NCOLS}`
STATUS_RE = re.compile(r"^asked (\d+) cells · table (\d+)×(\d+)$")


# ── snapshot readers ────────────────────────────────────────────────

def status_text(snap) -> str:
    node = find_by_tag(snap, STATUS_TAG)
    assert node is not None, f"the {STATUS_TAG} readout is in the paint tree"
    texts = texts_of(node)
    assert texts, "the readout carries text"
    return texts[0]


def asked(snap) -> int:
    """How many cells the grid asked its consumer for, building this frame."""
    m = STATUS_RE.match(status_text(snap))
    assert m is not None, f"readout parses: {status_text(snap)!r}"
    return int(m.group(1))


def rendered_rows(rects) -> list[int]:
    return indexed_tags(rects, f"{TABLE_TAG}_row")


def row_cols(rects, row: int) -> list[int]:
    return indexed_tags(rects, f"{TABLE_TAG}#{row}_")


def painted_cells(rects) -> int:
    """Data cells in the tree — the independent half of the identity.

    Counted per data row, so the header band's cells (`vcol#h{col}`, the same
    composite space) cannot be mistaken for a further row of data cells.
    """
    return sum(len(row_cols(rects, r)) for r in rendered_rows(rects))


def offset_x(snap) -> int:
    node = find_by_tag(snap, H_SCROLL_TAG)
    assert node is not None, "horizontal scroll node present"
    return int(node.get("offset_x", -1))


def offset_y(snap) -> int:
    node = find_by_tag(snap, V_SCROLL_TAG)
    assert node is not None, "vertical body scroll node present"
    return int(node.get("offset_y", -1))


def hviewport_w(snap) -> int:
    node = find_by_tag(snap, H_SCROLL_TAG)
    assert node is not None, "horizontal scroll node present"
    return int((node.get("viewport") or {}).get("w", -1))


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:

        def snap_now(viewport=WIN):
            return tf.snapshot(source="paint", viewport=viewport)

        def assert_identity(snap, label: str) -> tuple[int, int]:
            """`asked` == painted data cells. The round's whole claim."""
            rects = abs_rects_of(snap)
            a, p = asked(snap), painted_cells(rects)
            assert p > 0, f"{label}: the window holds cells"
            assert_eq(a, p, f"{label}: asked {a} cells, painted {p}")
            return a, p

        # ── (A) boot: the readout and the identity ──────────────────
        snap = snap_now()
        rects = abs_rects_of(snap)
        assert TABLE_TAG in rects, "grid root present at boot"
        m = STATUS_RE.match(status_text(snap))
        assert m is not None, f"the readout parses: {status_text(snap)!r}"
        assert_eq(int(m.group(2)), N, "the readout states the full row extent")
        assert_eq(int(m.group(3)), NCOLS, "the readout states the full column extent")
        assert_eq(offset_x(snap), 0, "boot horizontal offset is 0")
        assert_eq(offset_y(snap), 0, "boot vertical offset is 0")
        boot_asked, _ = assert_identity(snap, "boot")
        boot_cols = row_cols(rects, 0)
        boot_rows = rendered_rows(rects)
        assert boot_cols, "row 0 holds its windowed cells"
        assert len(boot_cols) < NCOLS // 10, \
            f"the column axis windows: {len(boot_cols)} of {NCOLS}"
        assert 0 < len(boot_rows) < N // 100, \
            f"the row axis windows: {len(boot_rows)} of {N}"

        # ── (B) the ADDRESS was answered, not just the count ────────
        # `cell_text` renders `r{row}c{col}`, so the text names the CellIndex it
        # was handed. A consumer answering for a transposed (col, row) or an
        # off-by-one column would keep every count above intact and fail here.
        vp_w = hviewport_w(snap)
        assert vp_w > 0, f"horizontal viewport measured, got {vp_w}"
        sample_rows = [boot_rows[0], boot_rows[len(boot_rows) // 2], boot_rows[-1]]
        for r in sample_rows:
            cols = row_cols(rects, r)
            assert cols, f"row {r} holds cells"
            for c in (cols[0], cols[-1]):
                node = find_by_tag(snap, f"{TABLE_TAG}#{r}_{c}")
                assert node is not None, f"cell ({r},{c}) is in the tree"
                assert_eq(texts_of(node)[0], f"r{r}c{c}",
                          f"cell ({r},{c}) says the address it was asked for")

        # ── (C) magnitude against the pre-R1524 cost ────────────────
        full_row_cost = len(boot_rows) * NCOLS
        assert boot_asked * 10 < full_row_cost, \
            (f"asking {boot_asked} for {len(boot_rows)} windowed rows is an order of "
             f"magnitude below the {full_row_cost} a per-row builder cost")
        assert_eq(boot_asked, len(boot_rows) * len(boot_cols),
                  "and it is exactly the row window times the column window")

        # ── (D) the horizontal axis ─────────────────────────────────
        D = 6_000
        tf.scroll(H_SCROLL_TAG, to=(D, 0))
        snap = wait_snap(
            tf, lambda s: offset_x(s) == D, viewport=WIN,
            desc="horizontal scroll advanced offset_x",
        )
        rects2 = abs_rects_of(snap)
        far_cols = row_cols(rects2, 0)
        assert far_cols, "the scrolled window holds cells"
        assert far_cols[0] > boot_cols[-1], \
            f"the boot column set has left the tree: {boot_cols} -> {far_cols}"
        assert_identity(snap, "h-scrolled")
        for c in boot_cols:
            assert f"{TABLE_TAG}#0_{c}" not in rects2, \
                f"boot column {c} is absent from the tree, not merely off-screen"

        tf.scroll(H_SCROLL_TAG, to=(10 ** 9, 0))
        snap = wait_snap(
            tf, lambda s: offset_x(s) > D, viewport=WIN,
            desc="horizontal scroll clamped past the midpoint",
        )
        end_cols = row_cols(abs_rects_of(snap), 0)
        assert (NCOLS - 1) in end_cols, "the last column is in the tree at the clamp"
        assert 0 not in end_cols, "and the first has left it"
        assert_identity(snap, "h-clamped")

        # ── (E) the vertical axis: per-cell tracks BOTH windows ─────
        tf.scroll(H_SCROLL_TAG, to=(0, 0))
        snap = wait_snap(tf, lambda s: offset_x(s) == 0, viewport=WIN,
                         desc="horizontal scrolled back to 0")
        assert_identity(snap, "h-home")

        tf.scroll(V_SCROLL_TAG, to=(0, 4_000))
        snap = wait_snap(
            tf, lambda s: offset_y(s) == 4_000, viewport=WIN,
            desc="vertical scroll advanced offset_y",
        )
        rects3 = abs_rects_of(snap)
        far_rows = rendered_rows(rects3)
        assert far_rows, "the scrolled row window holds rows"
        assert far_rows[0] > boot_rows[-1], \
            f"the boot row set has left the tree: {boot_rows[-1]} -> {far_rows[0]}"
        v_asked, _ = assert_identity(snap, "v-scrolled")
        # A per-ROW contract would have asked for the same 200 columns of each of
        # these rows; a per-CELL one asks for the intersection of both windows.
        assert_eq(v_asked, len(far_rows) * len(row_cols(rects3, far_rows[0])),
                  "the count is the row window times the column window here too")
        node = find_by_tag(snap, f"{TABLE_TAG}#{far_rows[0]}_{row_cols(rects3, far_rows[0])[0]}")
        assert node is not None, "a scrolled-to cell is in the tree"
        assert texts_of(node)[0].startswith(f"r{far_rows[0]}c"), \
            "and it says the data row it was asked for, not its visual position"

        tf.scroll(V_SCROLL_TAG, to=(0, 0))
        snap = wait_snap(tf, lambda s: offset_y(s) == 0, viewport=WIN,
                         desc="vertical scrolled back to 0")
        assert_identity(snap, "v-home")

        # ── (F) resize re-windows, and the identity survives ────────
        wide = (WIN[0] + 400, WIN[1])
        resp = tf.request("scene/resize", {"width": wide[0], "height": wide[1]})
        assert resp is not None and resp.result is not None, "scene/resize accepted"

        def widened():
            s = snap_now(wide)
            return s if hviewport_w(s) > vp_w else None

        snap = wait_until(widened, desc="the grid re-measures its wider viewport")
        wide_asked, _ = assert_identity(snap, "widened")
        wide_cols = row_cols(abs_rects_of(snap), 0)
        assert len(wide_cols) > len(boot_cols), \
            f"a wider viewport exposes more columns: {len(boot_cols)} -> {len(wide_cols)}"
        assert wide_asked > boot_asked, \
            f"and the grid asks for more cells: {boot_asked} -> {wide_asked}"

        short = (wide[0], WIN[1] - 160)
        resp = tf.request("scene/resize", {"width": short[0], "height": short[1]})
        assert resp is not None and resp.result is not None, "second scene/resize accepted"

        def shortened():
            s = snap_now(short)
            return s if len(rendered_rows(abs_rects_of(s))) < len(boot_rows) else None

        snap = wait_until(shortened, desc="the grid re-measures its shorter viewport")
        short_asked, _ = assert_identity(snap, "shortened")
        assert short_asked < wide_asked, \
            f"a shorter viewport asks for fewer cells: {wide_asked} -> {short_asked}"


if __name__ == "__main__":
    sys.exit(run_demo("R1524 §5.27 — the grid asks for the cells it paints", body))
