#!/usr/bin/env python3
"""R730 §5.40 sortable Table columns (aria-sort).

R730 widens the R707 single-select data grid with **sortable columns** —
the canonical #1 data-grid feature. Clicking a column header cycles the
sort (unsorted -> ascending -> descending -> unsorted); the sorted column
header carries a glyph + WAI-ARIA `aria-sort`. The sort is additive in
`TableExternal`: selection stays **data-indexed**, so a sorted view needs
no remap — a selected data row simply paints at its new visual position.

Phase 1 — RPC introspection / behaviour:
  * boot: unsorted (`sort_col == -1`, `sort_dir == "none"`, identity order);
  * click the Widget header -> ascending: `sort_dir`, the `order`
    permutation (Dialog/Menu/Table/Tabs/Toolbar/Tooltip = data [3,1,5,0,2,4]),
    and the ascending glyph appear;
  * click again -> descending (reversed order, descending glyph);
  * click again -> unsorted (identity, no glyph);
  * a selected data row survives a sort (data-indexed selection): it stays
    `selected.<data>` and moves to its sorted visual position;
  * `invoke "sort"` is the direct AI path (no synthesised pointer event);
  * a different column jumps straight to ascending.

Phase 2 — live-pixel (boot frame): the unsorted grid renders its
SurfaceContainerLow block (the sort glyph is post-click, so it is verified
structurally in phase 1, not in the boot frame — the overlay-pixel rule).

Run from the workspace root:
    cargo build -p hello-table --release
    python3 tools/demos/r730_table_sort.py
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
    assert_pixel_eq,
    find_by_tag,
    read_png_rgba8,
    run_demo,
    sample_png_points,
)

EXAMPLE = "hello-table"
T = "table"
VIEWPORT = (540, 360)
ASC = "▲"
DESC = "▼"


def q(d, slot):
    return d.query(f"/external/{slot}")


def order(d, n=6):
    return [q(d, f"order.{v}") for v in range(n)]


def header_glyphs(d):
    """Collect the glyph text nodes painted in the header band."""
    snap = d.snapshot(source="paint", viewport=VIEWPORT)
    hrow = find_by_tag(snap, f"{T}_hrow")
    assert hrow is not None, "header band present"
    out = []

    def walk(node):
        if not isinstance(node, dict):
            return
        if node.get("type") == "Text":
            c = node.get("content")
            if c in (ASC, DESC):
                out.append(c)
        for ch in node.get("children") or []:
            walk(ch)

    walk(hrow)
    return out


def click_header(d, col):
    d.click(path=f"{T}#h{col}")
    d.pointer_leave()


def body() -> None:
    with RpcSubprocess(EXAMPLE) as d:
        # ── boot: unsorted ─────────────────────────────────────────────
        assert_eq(q(d, "sort_col"), -1, "boot unsorted (sort_col -1)")
        assert_eq(q(d, "sort_dir"), "none", "boot sort_dir none")
        assert_eq(order(d), [0, 1, 2, 3, 4, 5], "boot identity order")
        assert_eq(header_glyphs(d), [], "no sort glyph when unsorted")

        # ── select data row 0 ("Tabs") so we can prove it survives sort ─
        d.click(path=f"{T}#0_0")
        d.pointer_leave()
        assert_eq(q(d, "selected_row"), 0, "data row 0 selected")

        # ── click Widget header -> ascending ───────────────────────────
        click_header(d, 0)
        assert_eq(q(d, "sort_col"), 0, "sorted by column 0")
        assert_eq(q(d, "sort_dir"), "ascending", "ascending after 1st click")
        # Lexicographic ascending of Widget: Dialog Menu Table Tabs Toolbar
        # Tooltip = data rows [3, 1, 5, 0, 2, 4].
        assert_eq(order(d), [3, 1, 5, 0, 2, 4], "ascending Widget order")
        assert_eq(q(d, "cell.3.0"), "Dialog", "data row 3 is Dialog (now visual 0)")
        assert_eq(header_glyphs(d), [ASC], "one ascending glyph")
        # Selection survived (data-indexed): row 0 still selected, now at
        # visual position 3 (order[3] == 0).
        assert_eq(q(d, "selected.0"), True, "selected data row survives the sort")
        assert_eq(q(d, "selected_row"), 0, "selection is data-indexed, no remap")
        assert_eq(q(d, "order.3"), 0, "the selected data row moved to visual 3")

        # ── click again -> descending ──────────────────────────────────
        click_header(d, 0)
        assert_eq(q(d, "sort_dir"), "descending", "descending after 2nd click")
        assert_eq(order(d), [4, 2, 0, 5, 1, 3], "descending = reversed ascending")
        assert_eq(header_glyphs(d), [DESC], "one descending glyph")

        # ── click again -> unsorted ────────────────────────────────────
        click_header(d, 0)
        assert_eq(q(d, "sort_col"), -1, "3rd click clears the sort")
        assert_eq(q(d, "sort_dir"), "none", "back to unsorted")
        assert_eq(order(d), [0, 1, 2, 3, 4, 5], "identity order restored")
        assert_eq(header_glyphs(d), [], "glyph gone when unsorted")

        # ── invoke "sort" — the direct AI path (no pointer synthesis) ──
        assert_eq(d.invoke("/external/sort", 1), "ascending", "invoke sort returns dir")
        assert_eq(q(d, "sort_col"), 1, "AI sorted column 1 (Round)")
        assert_eq(q(d, "sort_dir"), "ascending", "Round ascending")
        # Round = R690..R707, lexicographic ascending == data order here.
        assert_eq(order(d), [0, 1, 2, 3, 4, 5], "Round already in ascending data order")

        # ── a different column jumps straight to ascending ─────────────
        assert_eq(d.invoke("/external/sort", 2), "ascending", "new column -> ascending")
        assert_eq(q(d, "sort_col"), 2, "sort key moved to column 2")
        assert_eq(q(d, "sort_dir"), "ascending", "ascending, not toggled from prior")

        # ── geometry for the pixel phase (boot-visible grid) ───────────
        # Reset to unsorted so the screenshot's boot frame is the canonical
        # grid (the screenshot is captured fresh, but assert tags exist).
        snap = d.snapshot(source="paint", viewport=VIEWPORT)
        rects = abs_rects_of(snap)
        assert T in rects, "grid block has an absolute rect"
        block = find_by_tag(snap, T)
        block_fill = block["style"]["fill"]
        block_rgb = (block_fill["r"], block_fill["g"], block_fill["b"])
        bx, by, _bw, _bh = rects[T]

    # ── Phase 2 — live-pixel: the boot grid block renders ─────────────
    png = read_png_rgba8(capture_screenshot())
    assert (png.width, png.height) == VIEWPORT, \
        f"screenshot {png.width}x{png.height} != viewport {VIEWPORT}"
    # Sample the block's top-left padding strip (SurfaceContainerLow),
    # clear of the header band / cells.
    spot = (bx + 3, by + 3)
    px = sample_png_points(png, [spot])[0]
    assert_pixel_eq(px, (*block_rgb, 255), f"grid block tone {spot}", tolerance=12)


def capture_screenshot() -> Path:
    out = Path(tempfile.mkdtemp(prefix="pinion-r730-")) / "table.png"
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
    sys.exit(run_demo("R730 sortable Table columns", body))
