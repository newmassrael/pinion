#!/usr/bin/env python3
"""R860 §5.27 §5.50 — tree-grid (scene-outliner) E2E.

Drives the `hello-tree-grid` binding via JSON-RPC. First consumer of
`pinion_widget_paint::tree_view::view_virtual_treegrid`: a hierarchical
outliner whose FROZEN name column (indent + expand glyph + label) is pinned
via the R859 frozen-split substrate while the metadata columns (Type /
Visible / Layer) scroll horizontally. Both panes share the vertical body
scroll (the R859 linked-scroll follower), so the tree + its metadata scroll
in vertical lockstep; clicking a folder expands / collapses it (re-windowing
the grid).

Witness (§2 #7 scene-as-data), no pixels required:

  (A) structure — the grid root, the two header bands (frozen tree
      `tgrid_fhrow` + scrolling metadata `tgrid_hrow`), the windowed name
      cells (`tgrid#{id}`) + metadata strips (`tgrid_drow{id}`), and both
      scroll axes are present; the query-only introspection reports the FULL
      visible-row count (the virtualization paints only a window).
  (B) horizontal scroll => the FREEZE — `scene/scroll` on the horizontal
      scroll shifts the metadata strips left by exactly `offset_x` while the
      FROZEN name cells DO NOT MOVE.
  (C) vertical scroll => LOCKSTEP — `scene/scroll` on the body moves the name
      cell AND the metadata strip of the same row by the SAME amount (equal
      y), while neither header moves.
  (D) expand / collapse — `tf.click` a boot-expanded folder collapses it: the
      introspection row_count drops by the folder's child count and its child
      rows vanish from the scene; clicking again restores them.

  Phase 2 — PIXELS (PINION_SCREENSHOT): the header band is tonally distinct
      from a name cell (the grid really painted, header framed).
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
    wait_until,
)

EXAMPLE = "hello-tree-grid"
WIN = (480, 460)
FOLDERS = 24
OBJECTS_PER = 12
EXPANDED_AT_BOOT = 3
BOOT_ROWS = FOLDERS + EXPANDED_AT_BOOT * OBJECTS_PER  # 60
TREE_TAG = "tgrid"
STATE_TAG = "tgrid_state"
CELLS_TAG = "tgrid_cells"  # R865 — off-window metadata-cell introspection
FROZEN_HEADER_TAG = "tgrid_fhrow"
SCROLL_HEADER_TAG = "tgrid_hrow"
V_SCROLL_TAG = "tgrid_scroll"
H_SCROLL_TAG = "tgrid_hscroll"
PAUSE = 0.12


def hscroll_node(snap):
    node = find_by_tag(snap, H_SCROLL_TAG)
    assert node is not None, "horizontal scroll node present"
    return node


def offset_x(snap) -> int:
    return int(hscroll_node(snap).get("offset_x", -1))


def offset_y(snap) -> int:
    node = find_by_tag(snap, V_SCROLL_TAG)
    assert node is not None, "vertical body scroll node present"
    return int(node.get("offset_y", -1))


def x_of(rects, tag) -> int:
    assert tag in rects, f"{tag} present"
    return rects[tag][0]


def y_of(rects, tag) -> int:
    assert tag in rects, f"{tag} present"
    return rects[tag][1]


def name_cell(rid: str) -> str:
    return f"{TREE_TAG}#{rid}"


def data_strip(rid: str) -> str:
    return f"{TREE_TAG}_drow{rid}"


def data_cell(rid: str, col: int) -> str:
    return f"{TREE_TAG}_dcell{rid}_{col}"


def row_count(tf) -> int:
    return int(tf.query(f"/{STATE_TAG}/external/row_count"))


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:

        def snap_now():
            return tf.snapshot(source="paint", viewport=WIN)

        # ── (A) structure at boot ───────────────────────────────────
        snap = snap_now()
        rects = abs_rects_of(snap)
        assert "tgrid_root" in rects, "outliner root anchor present at boot"
        assert FROZEN_HEADER_TAG in rects, "frozen tree header band present"
        assert SCROLL_HEADER_TAG in rects, "scrolling metadata header band present"
        # f0 boots expanded, so f0 + its first child render at the top.
        assert name_cell("f0") in rects, "folder f0 name cell present"
        assert data_strip("f0") in rects, "folder f0 metadata strip present"
        assert name_cell("f0-o0") in rects, "expanded folder f0's first child present"

        assert_eq(hscroll_node(snap).get("axis"), "horizontal", "metadata pane scroll axis=horizontal")
        assert_eq(offset_x(snap), 0, "boot horizontal offset 0")
        assert_eq(offset_y(snap), 0, "boot vertical offset 0")

        # AI-first: the introspection reports the FULL visible-row count, not
        # the windowed render, plus the per-row structure (id / depth /
        # expanded) the virtualization paint cannot expose for off-window rows.
        assert_eq(row_count(tf), BOOT_ROWS, "introspection row_count = boot visible rows")
        rendered = sum(1 for t in rects if t.startswith(f"{TREE_TAG}#"))
        assert rendered < BOOT_ROWS, f"only a window of name cells renders ({rendered} < {BOOT_ROWS})"
        assert_eq(tf.query(f"/{STATE_TAG}/external/id_at.0"), "f0", "introspection row 0 id = f0")
        assert_eq(tf.query(f"/{STATE_TAG}/external/level_at.0"), 1, "f0 is a root (aria-level 1)")
        assert_eq(tf.query(f"/{STATE_TAG}/external/expanded_at.0"), True, "f0 boots expanded")
        assert_eq(tf.query(f"/{STATE_TAG}/external/id_at.1"), "f0-o0", "row 1 is f0's first child")
        assert_eq(tf.query(f"/{STATE_TAG}/external/level_at.1"), 2, "f0's child is aria-level 2")

        # R865 — the metadata-cell peer: the AI reads cell values by (pos, col),
        # including OFF-WINDOW rows the paint never realizes. f0 is a folder, so
        # its Type (col 0) is the deterministic "Folder".
        assert_eq(tf.query(f"/{CELLS_TAG}/external/col_count"), 3, "3 metadata columns")
        assert_eq(tf.query(f"/{CELLS_TAG}/external/cell_at.0.0"), "Folder", "f0 Type = Folder")
        # A row deep in the flattening (past the rendered window) still resolves.
        deep = BOOT_ROWS - 1
        assert deep > rendered, f"row {deep} is off the rendered window ({rendered})"
        deep_type = tf.query(f"/{CELLS_TAG}/external/cell_at.{deep}.0")
        assert isinstance(deep_type, str) and deep_type, \
            f"off-window cell_at reports a value (AI reads what paint cannot), got {deep_type!r}"
        # Out-of-range column / position report Null (present-but-empty).
        assert tf.query(f"/{CELLS_TAG}/external/cell_at.0.99") is None, "OOR column -> null"
        assert tf.query(f"/{CELLS_TAG}/external/cell_at.99999.0") is None, "OOR position -> null"

        # Frozen name cell left of its metadata strip; headers above rows.
        assert x_of(rects, name_cell("f0")) < x_of(rects, data_strip("f0")), \
            "name column left of metadata column"
        assert y_of(rects, FROZEN_HEADER_TAG) < y_of(rects, name_cell("f0")), \
            "frozen header above the first name cell"

        # ── (A2) R863 — the metadata cells + headers are individually
        # addressable. Each `{TREE_TAG}_dcell{id}_{col}` cell + `{TREE_TAG}_ch{col}`
        # / `{TREE_TAG}_chtree` header is a tagged scene node, the geometry the
        # a11y `treegrid` resolves its `gridcell` / `columnheader` bounds from
        # (the metadata columns were AT-invisible under the prior `tree` /
        # `treeitem` topology). The cells ascend left-to-right inside their row's
        # strip, and each cell sits within the strip's horizontal extent.
        assert "tgrid_chtree" in rects, "R863 name-column header is tagged"
        assert all(f"{TREE_TAG}_ch{c}" in rects for c in range(3)), \
            "R863 each metadata column header is tagged"
        for col in range(3):
            assert data_cell("f0", col) in rects, f"R863 metadata cell col {col} is tagged"
        cell_xs = [x_of(rects, data_cell("f0", c)) for c in range(3)]
        assert cell_xs == sorted(cell_xs) and len(set(cell_xs)) == 3, \
            f"R863 metadata cells ascend left-to-right ({cell_xs})"
        strip_x = x_of(rects, data_strip("f0"))
        assert cell_xs[0] >= strip_x, "R863 first metadata cell sits inside its row strip"

        f0_name_x_boot = x_of(rects, name_cell("f0"))
        f0_strip_x_boot = x_of(rects, data_strip("f0"))
        f0_name_y_boot = y_of(rects, name_cell("f0"))
        # A child row's boot positions, to prove the freeze on a second row.
        child_name_x_boot = x_of(rects, name_cell("f0-o0"))
        child_strip_x_boot = x_of(rects, data_strip("f0-o0"))

        # ── (B) horizontal scroll => the FREEZE ─────────────────────
        # The metadata pane overflows by ~200px (3×160 cols vs the window
        # minus the frozen tree column); scroll a meaningful fraction first.
        D = 80
        tf.scroll(H_SCROLL_TAG, to=(D, 0))
        time.sleep(PAUSE)
        snap = snap_now()
        assert_eq(offset_x(snap), D, "horizontal scroll advanced offset_x")
        assert_eq(offset_y(snap), 0, "vertical offset unchanged by horizontal scroll")
        rects2 = abs_rects_of(snap)
        # Frozen name column DID NOT MOVE.
        assert_eq(x_of(rects2, name_cell("f0")), f0_name_x_boot,
                  "FROZEN name cell x unchanged after horizontal scroll")
        assert_eq(y_of(rects2, name_cell("f0")), f0_name_y_boot,
                  "name cell y unchanged by horizontal scroll")
        # Metadata strip shifted LEFT by exactly offset_x.
        assert_eq(x_of(rects2, data_strip("f0")), f0_strip_x_boot - D,
                  "SCROLLING metadata strip shifted left by exactly offset_x")
        # The freeze holds for a SECOND row too: child name cell fixed, child
        # metadata strip shifted by the same offset_x.
        assert_eq(x_of(rects2, name_cell("f0-o0")), child_name_x_boot,
                  "FROZEN child name cell x unchanged after horizontal scroll")
        assert_eq(x_of(rects2, data_strip("f0-o0")), child_strip_x_boot - D,
                  "child metadata strip shifted left by exactly offset_x")
        # Frozen header pinned, scrolling header tracked h-scroll.
        assert FROZEN_HEADER_TAG in rects2, "frozen header still present after h-scroll"

        # Scroll-to-max: offset_x clamps PAST D (the range is real — a widget
        # that silently clamped at D, or dropped the rightmost column, would
        # fail here), and the frozen name column is STILL pinned at boot x
        # while the metadata strip shifts by the full max offset.
        tf.scroll(H_SCROLL_TAG, to=(10 ** 9, 0))
        time.sleep(PAUSE)
        snap = snap_now()
        max_x = offset_x(snap)
        assert max_x > D, f"scroll-to-max advanced past D ({max_x} > {D})"
        rects_max = abs_rects_of(snap)
        assert_eq(x_of(rects_max, name_cell("f0")), f0_name_x_boot,
                  "FROZEN name cell STILL at boot x at max horizontal scroll")
        assert_eq(x_of(rects_max, data_strip("f0")), f0_strip_x_boot - max_x,
                  "metadata strip shifted left by the full max offset")

        # Reset horizontal.
        tf.scroll(H_SCROLL_TAG, to=(0, 0))
        time.sleep(PAUSE)
        snap = snap_now()
        assert_eq(offset_x(snap), 0, "horizontal scrolled back to 0")
        rects_reset = abs_rects_of(snap)
        assert_eq(x_of(rects_reset, data_strip("f0")), f0_strip_x_boot,
                  "metadata strip back at boot x after reset")

        # ── (C) vertical scroll => LOCKSTEP ─────────────────────────
        header_x = x_of(rects_reset, FROZEN_HEADER_TAG)
        header_y = y_of(rects_reset, FROZEN_HEADER_TAG)
        ROW_PITCH = 48
        tf.scroll(V_SCROLL_TAG, to=(0, ROW_PITCH * 4))
        time.sleep(PAUSE)
        snap = snap_now()
        assert_eq(offset_y(snap), ROW_PITCH * 4, "vertical scroll advanced offset_y")
        assert_eq(offset_x(snap), 0, "horizontal offset unaffected by vertical scroll")
        rects4 = abs_rects_of(snap)
        # A row visible in both panes keeps EQUAL y across name cell + strip.
        rid = "f0-o6"  # well within the window after a 4-row scroll
        assert name_cell(rid) in rects4, f"name cell for {rid} present after v-scroll"
        assert data_strip(rid) in rects4, f"metadata strip for {rid} present after v-scroll"
        assert_eq(y_of(rects4, name_cell(rid)), y_of(rects4, data_strip(rid)),
                  f"row {rid} name cell + metadata strip share y (vertical lockstep)")
        # Neither header moved on either axis.
        assert_eq(x_of(rects4, FROZEN_HEADER_TAG), header_x, "frozen header X unchanged by v-scroll")
        assert_eq(y_of(rects4, FROZEN_HEADER_TAG), header_y, "frozen header Y unchanged by v-scroll")

        tf.scroll(V_SCROLL_TAG, to=(0, 0))
        time.sleep(PAUSE)
        snap = snap_now()
        assert_eq(offset_y(snap), 0, "vertical scrolled back to top")

        # ── (D) expand / collapse ───────────────────────────────────
        assert_eq(row_count(tf), BOOT_ROWS, "row_count back at boot before collapse")
        # Collapse the boot-expanded folder f0.
        tf.click(path=name_cell("f0"))
        wait_until(
            lambda: row_count(tf) == BOOT_ROWS - OBJECTS_PER,
            desc="collapse f0 -> row_count falls by OBJECTS_PER",
        )
        assert_eq(tf.query(f"/{STATE_TAG}/external/expanded_at.0"), False,
                  "introspection reports f0 collapsed after the click")
        snap = snap_now()
        rects5 = abs_rects_of(snap)
        assert name_cell("f0") in rects5, "f0 itself still rendered when collapsed"
        assert name_cell("f0-o0") not in rects5, "collapsed folder f0's first child vanishes"
        assert name_cell("f0-o5") not in rects5, "collapsed folder f0's later child vanishes"
        assert name_cell("f1") in rects5, "sibling f1 slid up after f0 collapsed"
        # f1 now occupies row 1 in the flattening.
        assert_eq(tf.query(f"/{STATE_TAG}/external/id_at.1"), "f1",
                  "after collapse, row 1 is the next sibling f1")

        # Re-expand restores the children + the count.
        tf.click(path=name_cell("f0"))
        wait_until(
            lambda: row_count(tf) == BOOT_ROWS,
            desc="re-expand f0 -> row_count restored",
        )
        assert_eq(tf.query(f"/{STATE_TAG}/external/expanded_at.0"), True,
                  "introspection reports f0 expanded again")
        snap = snap_now()
        rects6 = abs_rects_of(snap)
        assert name_cell("f0-o0") in rects6, "f0's children restored after re-expand"

    # ── Phase 2 — live pixels (boot frame) ──────────────────────────
    snap, rects = _boot_snapshot_and_rects()
    img = read_png_rgba8(capture_screenshot())
    assert (img.width, img.height) == WIN, \
        f"screenshot {img.width}x{img.height} != window {WIN}"

    # Header band tonally distinct from a name cell (grid really painted).
    _, hy, _, hh = rects[FROZEN_HEADER_TAG]
    _, ry, _, rh = rects[name_cell("f0")]
    on_screen_x = 30
    header_pt = (on_screen_x, hy + hh // 2)
    row_pt = (on_screen_x, ry + rh // 2)
    header_px, row_px = sample_png_points(img, [header_pt, row_pt])
    assert header_px != row_px, \
        f"header band tonally distinct from a name cell, got {header_px} vs {row_px}"


def _boot_snapshot_and_rects():
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        snap = tf.snapshot(source="paint", viewport=WIN)
        return snap, abs_rects_of(snap)


def capture_screenshot() -> Path:
    out = Path(tempfile.mkdtemp(prefix="pinion-r860-")) / "treegrid.png"
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
    sys.exit(run_demo("R860 §5.27 §5.50 — tree-grid (scene-outliner)", body))
