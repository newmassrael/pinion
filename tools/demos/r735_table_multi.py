#!/usr/bin/env python3
"""R735 §5.38 multi-select Table (aria-multiselectable).

R735 widens the R707 single-select data grid with a **multi-select mode**
(`TableExternal::with_multiselect`) — the 2-D grid analog of the R51.98
ListBox multi-select and the **second consumer** of `aria-multiselectable`.
Activating a row *toggles only that row's* selection bit (no
sibling-deselect), so any subset of rows can report `aria-selected` at
once — the use case behind a DCC asset browser's multi-pick, an IDE's
batch file selection, a layer panel's multi-layer selection.

`hello-table-multi` boots with rows 0 ("Tabs") and 2 ("Toolbar") seeded
selected, so the defining feature (multiple selected rows) is visible in
the boot frame.

Phase 1 — RPC introspection / behaviour (the AI-first surface):
  * boot: `multiselect == true`, rows {0, 2} selected, `selected_row` is
    the `-1` sentinel (no single selected row in multi-mode);
  * click an UNSELECTED row -> ADDS it (siblings untouched);
  * click a SELECTED row -> TOGGLES it off (siblings untouched);
  * 2-D keyboard roving (arrows / Home / End / PageUp / PageDown) + Enter
    / Space TOGGLE the active-descendant row;
  * per-row `selected.<r>` intervene admin write (multi-mode only);
  * `invoke "send"` toggle wire returns Null in multi-mode;
  * negatives: out-of-range cell + unknown slot reject cleanly.

Phase 2 — live-pixel (boot frame): the two seeded rows (0, 2) render the
accent selection wash; the unselected row 1 between them renders the
zebra/neutral strip. The boot frame renders every row Idle (no lingering
interaction state), so the two seeded rows are pixel-identical there and
both differ from the unselected strip — the defining multi-select visual
(distinct selected rows, not one selected block). The live session's row
fills are NOT used as the pixel reference because the Phase-1 toggle
tests leave interaction state on some rows; the boot PNG is the artifact
under test.

Run from the workspace root:
    cargo build -p hello-table-multi --release
    python3 tools/demos/r735_table_multi.py
"""

from __future__ import annotations

import os
import subprocess
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcError,
    RpcSubprocess,
    WORKSPACE_ROOT,
    abs_rects_of,
    assert_eq,
    sample_png_points,
    widest_flat_run,
    read_png_rgba8,
    run_demo,
)

EXAMPLE = "hello-table-multi"
T = "table"
VIEWPORT = (540, 360)


def q(d, slot):
    return d.query(f"/external/{slot}")


def body() -> None:
    with RpcSubprocess(EXAMPLE) as d:
        # ── boot: 6x4 grid, multi-select mode, rows {0, 2} seeded ───────
        assert_eq(q(d, "rows"), 6, "6 data rows")
        assert_eq(q(d, "cols"), 4, "4 columns")
        assert_eq(q(d, "multiselect"), True, "multiselect mode flag set")
        assert_eq(q(d, "header.0"), "Widget", "column 0 header")
        assert_eq(q(d, "header.3"), "Role", "column 3 header")
        assert_eq(q(d, "cell.5.0"), "Table", "row 5 col 0 cell")

        assert_eq(q(d, "selected"), True, "aggregate selected true at boot")
        assert_eq(q(d, "selected.0"), True, "row 0 seeded selected")
        assert_eq(q(d, "selected.1"), False, "row 1 unselected")
        assert_eq(q(d, "selected.2"), True, "row 2 seeded selected")
        assert_eq(q(d, "selected.3"), False, "row 3 unselected")
        assert_eq(q(d, "selected.4"), False, "row 4 unselected")
        assert_eq(q(d, "selected.5"), False, "row 5 unselected")
        # Multi-mode has no single selected row: the -1 sentinel.
        assert_eq(q(d, "selected_row"), -1, "selected_row is -1 in multi-mode")

        # ── click an UNSELECTED row -> ADD it (no sibling-deselect) ─────
        d.click(path=f"{T}#4_0")
        d.pointer_leave()
        assert_eq(q(d, "selected.4"), True, "click row 4 adds it")
        assert_eq(q(d, "selected.0"), True, "row 0 still selected (no exclusion)")
        assert_eq(q(d, "selected.2"), True, "row 2 still selected (no exclusion)")

        # ── click a SELECTED row -> TOGGLE off (siblings untouched) ─────
        d.click(path=f"{T}#0_2")
        d.pointer_leave()
        assert_eq(q(d, "selected.0"), False, "click selected row 0 toggles it off")
        assert_eq(q(d, "selected.2"), True, "row 2 untouched by row-0 toggle")
        assert_eq(q(d, "selected.4"), True, "row 4 untouched by row-0 toggle")

        # ── 2-D keyboard roving + Enter/Space TOGGLE ────────────────────
        assert_eq(d.request("focus/set", {"tag": T}).result.get("focused"), T,
                  "focus set on grid root")
        # The clicks above synced the active descendant (WAI-ARIA "click
        # moves focus to the cell"); clear it via the admin intervene so
        # the keyboard section starts from the clean "no cursor" baseline
        # where the first vertical arrow enters the grid at (0, 0).
        d.intervene("/external/focused_row", None)
        d.intervene("/external/focused_col", 0)
        assert_eq(q(d, "focused_row"), -1, "active descendant row cleared")
        d.key(path=T, name="ArrowDown")
        assert_eq(q(d, "focused_row"), 0, "first ArrowDown enters at row 0")
        assert_eq(q(d, "focused_col"), 0, "active descendant col 0")
        # Row 0 is OFF; Enter toggles it back ON.
        d.key(path=T, name="Enter")
        assert_eq(q(d, "selected.0"), True, "Enter toggles active row 0 on")
        # Space toggles it OFF again (Enter/Space both toggle in multi).
        d.key(path=T, name="Space")
        assert_eq(q(d, "selected.0"), False, "Space toggles active row 0 off")
        d.key(path=T, name="ArrowRight")
        assert_eq(q(d, "focused_col"), 1, "ArrowRight -> col 1")
        d.key(path=T, name="End")
        assert_eq(q(d, "focused_col"), 3, "End -> last col")
        d.key(path=T, name="PageDown")
        assert_eq(q(d, "focused_row"), 5, "PageDown -> last row")
        d.key(path=T, name="ArrowDown")
        assert_eq(q(d, "focused_row"), 5, "ArrowDown clamps at last row")

        # ── per-row intervene admin write (multi-mode only) ─────────────
        d.intervene("/external/selected.1", True)
        assert_eq(q(d, "selected.1"), True, "intervene selects row 1")
        d.intervene("/external/selected.1", False)
        assert_eq(q(d, "selected.1"), False, "intervene clears row 1")

        # ── AI invoke "send" toggle wire (returns Null in multi-mode) ───
        out = None
        for ev in ("PointerEnter", "PointerDown", "PointerUp", "PointerLeave"):
            out = d.invoke("/external/send", f"3_0:{ev}")
        assert_eq(out, None, "invoke send returns Null in multi-mode")
        assert_eq(q(d, "selected.3"), True, "invoke send toggled row 3 on")

        # ── negatives: bad send + unknown slot reject cleanly ───────────
        raised = False
        try:
            d.invoke("/external/send", "9_9:PointerUp")
        except RpcError:
            raised = True
        assert raised, "out-of-range cell index must be rejected"
        raised = False
        try:
            q(d, "no_such_slot")
        except RpcError:
            raised = True
        assert raised, "unknown introspect slot must raise, not silently pass"

        # ── geometry for the pixel phase (row-strip absolute rects) ─────
        # Only the geometry is taken from the live snapshot; the strip
        # *fills* are read from the boot PNG below (the live session's
        # interaction state would tint them — see the module docstring).
        snap = d.snapshot(source="paint", viewport=VIEWPORT)
        rects = abs_rects_of(snap)
        assert T in rects, "grid block has an absolute rect"
        for r in range(3):
            assert f"{T}_row{r}" in rects, f"row strip {r} has an absolute rect"
        row0_rect = rects[f"{T}_row0"]
        row1_rect = rects[f"{T}_row1"]
        row2_rect = rects[f"{T}_row2"]

    # ── Phase 2 — live-pixel: boot frame reflects the seeded {0, 2} set ──
    png = read_png_rgba8(capture_screenshot())
    assert (png.width, png.height) == VIEWPORT, \
        f"screenshot {png.width}x{png.height} != viewport {VIEWPORT}"

    def strip_spot(rect):
        # ★ R1670 — the WIDEST run of whatever the strip is painted in, at
        # mid-height, rather than a constant three pixels in from the left
        # edge. `r760_fab` measured what a constant inset into a derived
        # geometry is worth: it sat three pixels from a glyph's antialiased
        # edge and failed in CI twice while passing on two rasterisers here.
        # A row's padding is wider than a FAB's, so this one was not failing --
        # it was relying on the same thing.
        x, y, _w, h = rect
        start, length = widest_flat_run(
            png, rect, sample_png_points(png, [(x + 3, y + h // 2)])[0][:3]
        )
        return (start + length // 2, y + h // 2)

    p0, p1, p2 = sample_png_points(
        png, [strip_spot(row0_rect), strip_spot(row1_rect), strip_spot(row2_rect)]
    )

    def chan_dist(a, b):
        return abs(a[0] - b[0]) + abs(a[1] - b[1]) + abs(a[2] - b[2])

    # Boot frame renders every row Idle, so the two seeded selected rows
    # (0, 2) render the same accent wash and both differ from the
    # unselected zebra strip (row 1) — the defining multi-select visual.
    assert chan_dist(p0, p2) <= 12, (
        f"both seeded rows render the same accent wash in the boot frame "
        f"(row0={p0}, row2={p2})"
    )
    assert chan_dist(p0, p1) > 8, (
        f"selected row 0 wash {p0} must differ from unselected strip {p1}"
    )
    assert chan_dist(p2, p1) > 8, (
        f"selected row 2 wash {p2} must differ from unselected strip {p1}"
    )


def capture_screenshot() -> Path:
    out = Path(tempfile.mkdtemp(prefix="pinion-r735-")) / "table-multi.png"
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
    sys.exit(run_demo("R735 multi-select data Table (aria-multiselectable)", body))
