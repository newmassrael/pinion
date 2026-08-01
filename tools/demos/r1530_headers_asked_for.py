#!/usr/bin/env python3
"""R1530 §5.27 — the grid asks for the header sections it paints, E2E.

Drives the `hello-virtual-columns` binding via JSON-RPC. R1523 windowed the
data grid's COLUMN axis and R1524 made the cell contract per-cell — but the
column *labels* were never asked for at all. They arrived as `headers:
&[&str]`, a slice of the whole table, and `VirtualTableData` read its column
count off that slice's length. The extent was welded to the labels: a grid
could only learn how many columns it had by being handed every one of their
names, so this 200-column binding built 200 strings a frame to paint five, and
built them a second time for the a11y pass.

R1530 splits the two the way Qt's `QAbstractItemModel` does — `columnCount()`
is a number, `headerData(section, Qt::Horizontal, Qt::DisplayRole)` is an
accessor — and the grid asks the accessor once per painted header.

Like its R1524 peer, that change is invisible in the painted scene: the same
labels appear either way. So the count is published INTO the scene, at
`vcol_hstatus`, where `scene/snapshot` reads it with no pixels (§2 #7), and the
demo holds it against a second, independent observable: the header cells
actually in the tree. Their EQUALITY is the round's claim, and it breaks in
both directions:

  - hand over the whole table again and `asked` pins at 200 while the painted
    count stays at ~5 (the pre-R1530 state);
  - ask for too few and the header band paints blanks under real columns.

Sections:

  (A) boot — the readout exists, parses, and its count equals the painted
      header-cell count.
  (B) the section is answered, not merely counted — each painted header carries
      the label of the absolute column it is tagged with, so a pane answering
      with a pane-relative index would keep every count intact and fail here.
  (C) magnitude — `asked` is an order of magnitude below the 200 a whole-table
      slice cost, and the declared extent is still 200.
  (D) the horizontal axis — scrolling replaces the section set, and the identity
      holds at the new offset and at the clamp.
  (E) axis independence — scrolling ROWS changes the cell count and leaves the
      section count alone: the header axis tracks one window, the cell axis two.
      A single summed counter could not show this, which is why there are two.
  (F) resize — a wider viewport asks for strictly more sections, a narrower one
      for strictly fewer, and the identity survives both.
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
HEADER_STATUS_TAG = "vcol_hstatus"
V_SCROLL_TAG = "vcol_scroll"
H_SCROLL_TAG = "vcol_hscroll"

# `· asked {n} headers`
HSTATUS_RE = re.compile(r"^· asked (\d+) headers$")
# `asked {n} cells · table {N}×{NCOLS}` — R1524's readout, read here only to
# show the two axes move independently.
STATUS_RE = re.compile(r"^asked (\d+) cells · table (\d+)×(\d+)$")


# ── snapshot readers ────────────────────────────────────────────────

def readout(snap, tag: str) -> str:
    node = find_by_tag(snap, tag)
    assert node is not None, f"the {tag} readout is in the paint tree"
    texts = texts_of(node)
    assert texts, f"the {tag} readout carries text"
    return texts[0]


def asked_sections(snap) -> int:
    """How many header sections the grid asked for, building this frame."""
    text = readout(snap, HEADER_STATUS_TAG)
    m = HSTATUS_RE.match(text)
    assert m is not None, f"header readout parses: {text!r}"
    return int(m.group(1))


def asked_cells(snap) -> int:
    text = readout(snap, STATUS_TAG)
    m = STATUS_RE.match(text)
    assert m is not None, f"cell readout parses: {text!r}"
    return int(m.group(1))


def painted_sections(rects) -> list[int]:
    """Header cells in the tree — the independent half of the identity."""
    return indexed_tags(rects, f"{TABLE_TAG}_ch")


def section_label(snap, col: int) -> str:
    node = find_by_tag(snap, f"{TABLE_TAG}_ch{col}")
    assert node is not None, f"header section {col} is in the tree"
    texts = texts_of(node)
    assert texts, f"header section {col} carries a label"
    return texts[0]


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

        def assert_identity(snap, label: str) -> tuple[int, list[int]]:
            """`asked` == painted header cells. The round's whole claim."""
            rects = abs_rects_of(snap)
            a, p = asked_sections(snap), painted_sections(rects)
            assert p, f"{label}: the window holds header cells"
            assert_eq(a, len(p), f"{label}: asked {a} sections, painted {len(p)}")
            return a, p

        def assert_labels(snap, cols: list[int], label: str) -> None:
            """Each painted section carries the label of its ABSOLUTE column."""
            for col in cols:
                assert_eq(
                    section_label(snap, col), f"C{col:03}",
                    f"{label}: section {col} carries its own label",
                )

        # ── (A) boot: the readout and the identity ──────────────────
        snap = snap_now()
        rects = abs_rects_of(snap)
        assert TABLE_TAG in rects, "grid root present at boot"
        assert HSTATUS_RE.match(readout(snap, HEADER_STATUS_TAG)) is not None, \
            f"the header readout parses: {readout(snap, HEADER_STATUS_TAG)!r}"
        assert_eq(offset_x(snap), 0, "boot horizontal offset is 0")
        assert_eq(offset_y(snap), 0, "boot vertical offset is 0")
        boot_asked, boot_cols = assert_identity(snap, "boot")
        assert boot_cols[0] == 0, "the boot window starts at column 0"
        assert len(boot_cols) < NCOLS // 10, \
            f"the header axis windows: {len(boot_cols)} of {NCOLS}"

        # ── (B) the SECTION was answered, not just counted ──────────
        # `header_text` renders `C{col:03}`, so the label names the section it
        # was handed. A pane answering with its own pane-relative index would
        # keep every count above intact and fail here.
        assert_labels(snap, boot_cols, "boot")

        # ── (C) magnitude against the pre-R1530 cost ────────────────
        vp_w = hviewport_w(snap)
        assert vp_w > 0, f"horizontal viewport measured, got {vp_w}"
        assert boot_asked * 10 < NCOLS, \
            (f"asking for {boot_asked} sections is an order of magnitude below the "
             f"{NCOLS} a whole-table slice cost")
        # The extent survives the windowing: R1524's readout still states it, and
        # it is a number the grid is TOLD, not one derived from the labels.
        m = STATUS_RE.match(readout(snap, STATUS_TAG))
        assert m is not None, "the cell readout parses"
        assert_eq(int(m.group(3)), NCOLS, "the declared column extent is still 200")
        assert_eq(int(m.group(2)), N, "and the row extent is untouched")

        # ── (D) the horizontal axis ─────────────────────────────────
        D = 6_000
        tf.scroll(H_SCROLL_TAG, to=(D, 0))
        snap = wait_snap(
            tf, lambda s: offset_x(s) == D, viewport=WIN,
            desc="horizontal scroll advanced offset_x",
        )
        far_asked, far_cols = assert_identity(snap, "h-scrolled")
        assert far_cols[0] > boot_cols[-1], \
            f"the boot section set has left the tree: {boot_cols} -> {far_cols}"
        assert_labels(snap, far_cols, "h-scrolled")
        rects2 = abs_rects_of(snap)
        for c in boot_cols:
            assert f"{TABLE_TAG}_ch{c}" not in rects2, \
                f"boot section {c} is absent from the tree, not merely off-screen"

        tf.scroll(H_SCROLL_TAG, to=(10 ** 9, 0))
        snap = wait_snap(
            tf, lambda s: offset_x(s) > D, viewport=WIN,
            desc="horizontal scroll clamped past the midpoint",
        )
        end_asked, end_cols = assert_identity(snap, "h-clamped")
        assert (NCOLS - 1) in end_cols, "the last section is in the tree at the clamp"
        assert 0 not in end_cols, "and the first has left it"
        assert_labels(snap, end_cols, "h-clamped")
        assert end_asked * 10 < NCOLS, "the clamp windows too, it does not fall back"

        # ── (E) axis independence: rows move cells, not sections ────
        tf.scroll(H_SCROLL_TAG, to=(0, 0))
        snap = wait_snap(tf, lambda s: offset_x(s) == 0, viewport=WIN,
                         desc="horizontal scrolled back to 0")
        home_asked, _ = assert_identity(snap, "h-home")
        assert_eq(home_asked, boot_asked, "back at the boot offset, the same sections")
        home_cells = asked_cells(snap)

        tf.scroll(V_SCROLL_TAG, to=(0, 4_000))
        snap = wait_snap(
            tf, lambda s: offset_y(s) == 4_000, viewport=WIN,
            desc="vertical scroll advanced offset_y",
        )
        v_asked, v_cols = assert_identity(snap, "v-scrolled")
        assert_eq(v_asked, boot_asked,
                  "scrolling ROWS does not change which sections exist")
        assert_eq(v_cols, boot_cols, "the same sections, by index")
        rows = indexed_tags(abs_rects_of(snap), f"{TABLE_TAG}_row")
        assert rows and rows[0] > 0, "the row window did move"
        assert asked_cells(snap) > 0, "and cells are still being asked for"
        # The header band is frozen against the vertical scroll, so its labels
        # are unchanged while the rows beneath them are entirely different.
        assert_labels(snap, v_cols, "v-scrolled")

        tf.scroll(V_SCROLL_TAG, to=(0, 0))
        snap = wait_snap(tf, lambda s: offset_y(s) == 0, viewport=WIN,
                         desc="vertical scrolled back to 0")
        assert_identity(snap, "v-home")
        assert_eq(asked_cells(snap), home_cells, "the cell count returns with it")

        # ── (F) resize re-windows, and the identity survives ────────
        wide = (WIN[0] + 400, WIN[1])
        resp = tf.request("scene/resize", {"width": wide[0], "height": wide[1]})
        assert resp is not None and resp.result is not None, "scene/resize accepted"

        def widened():
            s = snap_now(wide)
            return s if hviewport_w(s) > vp_w else None

        snap = wait_until(widened, desc="the grid re-measures its wider viewport")
        wide_asked, wide_cols = assert_identity(snap, "widened")
        assert wide_asked > boot_asked, \
            f"a wider viewport asks for more sections: {boot_asked} -> {wide_asked}"
        assert_labels(snap, wide_cols, "widened")

        narrow = (WIN[0] - 200, WIN[1])
        resp = tf.request("scene/resize", {"width": narrow[0], "height": narrow[1]})
        assert resp is not None and resp.result is not None, "second scene/resize accepted"

        def narrowed():
            s = snap_now(narrow)
            return s if hviewport_w(s) < vp_w else None

        snap = wait_until(narrowed, desc="the grid re-measures its narrower viewport")
        narrow_asked, narrow_cols = assert_identity(snap, "narrowed")
        assert narrow_asked < wide_asked, \
            f"a narrower viewport asks for fewer sections: {wide_asked} -> {narrow_asked}"
        assert_labels(snap, narrow_cols, "narrowed")


if __name__ == "__main__":
    sys.exit(run_demo("R1530 §5.27 — the grid asks for the sections it paints", body))
