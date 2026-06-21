#!/usr/bin/env python3
"""R972 §5.41 — cell-native `TextGrid` geometry scaffold introspection.

The first real consumer of the `CellMetric` coordinate substrate (R968
node-local ratify -> R970 metric type -> R971 authoritative dims). Two
`Scene::TextGrid` geometry leaves sit at fixed absolute positions:

  * `htg_default`  — 8x16 baseline (`CellMetric::DEFAULT`),  640x384 px;
  * `htg_measured` — a measured 9x18 metric (`CellMetric::new`), 360x360.

The proof is pure DATA over RPC (paint + cell data model are follow-up
rounds, so the window renders only its surface). `scene/snapshot`
exposes each grid's `rect`, node-local `cell_w` / `cell_h`, and derived
winsize `cols` / `rows`. From those an AI client reconstructs the whole
cell<->pixel mapping with NO OCR (the §2 #7 scene-as-data thesis):

  * the winsize round-trip `cols == floor(rect.w / cell_w)` (the R969
    one-directional `(rows, cols)` SSOT — layout pixel rect -> dims);
  * the cell-span fits its rect (`cols * cell_w <= rect.w`);
  * the two grids carry DIFFERENT node-local metrics (R968), proving the
    metric travels with the node, not a global coordinate space.

Run from the workspace root:
    cargo build -p hello-textgrid --release
    python3 tools/demos/r972_textgrid.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (
    RpcSubprocess,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_snap,
)

# Mirror of the example's layout constants (examples/hello-textgrid).
WIN = (680, 840)
DEFAULT = {"tag": "htg_default", "x": 16, "y": 16, "w": 640, "h": 384, "cw": 8, "ch": 16, "fixed": True}
# (R1031 §5.37) htg_measured uses a font-DERIVED cell (measured_monospace_cell),
# so cell_w/cell_h vary with the resolved monospace (DejaVu vs Noto CJK) — do NOT
# hardcode them. The winsize round-trip is asserted against the node's OWN
# cell_w/cell_h (invariants) below. htg_default is CellMetric::DEFAULT (8x16,
# font-independent), so its absolute cell metric IS pinned.
MEASURED = {"tag": "htg_measured", "x": 16, "y": 432, "w": 360, "h": 360, "fixed": False}


def check_grid(tf, spec: dict) -> dict:
    """Snapshot one grid and assert its geometry round-trips. Returns the
    node so the caller can do cross-grid comparisons."""
    tag = spec["tag"]
    # ZERO-FLAKE gate: poll the paint snapshot until the layout pass has
    # resolved this grid's rect (cols becomes non-zero only once `rect`
    # is filled — winsize is strictly layout-derived). No fixed sleep.
    # cols becomes positive only once the layout pass fills `rect` (winsize is
    # strictly layout-derived). Poll for that — font-robustly, without assuming
    # the cell width (htg_measured's is font-derived).
    snap = wait_snap(
        tf,
        lambda s: ((find_by_tag(s, tag) or {}).get("cols") or 0) > 0,
        source="paint",
        viewport=WIN,
        desc=f"{tag} layout-resolved",
    )
    node = find_by_tag(snap, tag)
    assert node is not None, f"{tag} present in paint scene"

    # --- scene-as-data identity (§2 #7) ---
    assert_eq(node["type"], "TextGrid", f"{tag} type")
    assert_eq(node["tag"], tag, f"{tag} tag")
    rect = node["rect"]
    assert_eq(rect["x"], spec["x"], f"{tag} rect.x")
    assert_eq(rect["y"], spec["y"], f"{tag} rect.y")
    assert_eq(rect["w"], spec["w"], f"{tag} rect.w")
    assert_eq(rect["h"], spec["h"], f"{tag} rect.h")
    # Absolute cell metric only for the fixed-metric grid; the measured grid's
    # cell is font-derived and verified via the winsize invariants below.
    if spec.get("fixed"):
        assert_eq(node["cell_w"], spec["cw"], f"{tag} cell_w")
        assert_eq(node["cell_h"], spec["ch"], f"{tag} cell_h")

    # --- winsize round-trip (R969 layout-derived (rows, cols) SSOT) ---
    cols = node["cols"]
    rows = node["rows"]
    assert_eq(cols, rect["w"] // node["cell_w"], f"{tag} cols == floor(w/cell_w)")
    assert_eq(rows, rect["h"] // node["cell_h"], f"{tag} rows == floor(h/cell_h)")
    # the whole-cell span never exceeds the pixel rect, and the trailing
    # remainder is strictly less than one cell (a partial cell is unusable).
    assert cols * node["cell_w"] <= rect["w"], f"{tag} col span fits rect.w"
    assert rect["w"] - cols * node["cell_w"] < node["cell_w"], f"{tag} <1 col remainder"
    assert rows * node["cell_h"] <= rect["h"], f"{tag} row span fits rect.h"
    assert rect["h"] - rows * node["cell_h"] < node["cell_h"], f"{tag} <1 row remainder"

    # --- cell<->pixel round-trip the AI reconstructs from the snapshot ---
    # For sample cells, forward-map to a pixel (cell origin) then invert;
    # the metric's exactness on integer origins makes this the identity.
    for (c, r) in [(0, 0), (cols // 3, rows // 3), (cols - 1, rows - 1)]:
        px = rect["x"] + c * node["cell_w"]
        py = rect["y"] + r * node["cell_h"]
        assert px < rect["x"] + rect["w"], f"{tag} cell({c},{r}) px in-bounds"
        assert py < rect["y"] + rect["h"], f"{tag} cell({c},{r}) py in-bounds"
        back_c = (px - rect["x"]) // node["cell_w"]
        back_r = (py - rect["y"]) // node["cell_h"]
        assert_eq((back_c, back_r), (c, r), f"{tag} cell({c},{r}) round-trips")
    return node


def body() -> None:
    with RpcSubprocess("hello-textgrid") as tf:
        default = check_grid(tf, DEFAULT)
        measured = check_grid(tf, MEASURED)

        # The fixed-metric grid derives exactly the dims its rect +
        # CellMetric::DEFAULT imply (80x24 @ 8x16). The measured grid's dims are
        # font-derived (its cell varies with the resolved monospace), so assert
        # the round-trip against its OWN cell metric rather than absolute px.
        assert_eq((default["cols"], default["rows"]), (80, 24), "default dims 80x24")
        assert_eq(
            (measured["cols"], measured["rows"]),
            (
                measured["rect"]["w"] // measured["cell_w"],
                measured["rect"]["h"] // measured["cell_h"],
            ),
            "measured dims = rect // its font-derived cell",
        )

        # Node-local metric (R968): the two grids carry DIFFERENT cell
        # metrics — the metric travels with the node, not a global space.
        assert default["cell_w"] != measured["cell_w"], "node-local metric differs (w)"
        assert default["cell_h"] != measured["cell_h"], "node-local metric differs (h)"
        assert_eq(default["type"], measured["type"], "both are TextGrid")

        # The two grids do not overlap vertically (default ends above the
        # measured grid's top) — distinct addressable geometry regions.
        d_bottom = default["rect"]["y"] + default["rect"]["h"]
        assert d_bottom <= measured["rect"]["y"], "grids are disjoint vertically"

        # Same metric, different rect ⇒ different dims is the winsize
        # contract: a hypothetical wider rect would report more columns.
        assert_eq(640 // default["cell_w"], default["cols"], "winsize is rect-driven")


def main() -> int:
    return run_demo("r972_textgrid", body)


if __name__ == "__main__":
    sys.exit(main())
