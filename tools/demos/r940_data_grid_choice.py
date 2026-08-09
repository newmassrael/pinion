#!/usr/bin/env python3
"""R940 §5.38 §5.40 — data-grid Choice column (enum dropdown cell editor).

Drives hello-data-grid over JSON-RPC. The editable grid's cells could be edited
as text / int / float / bool, but not picked from a closed enum — a core DCC /
inspector affordance (an asset's Type, a material's blend mode). R940 converts
the `Type` column (col 1) into a `CellKind::Choice` enum edited through a
floating dropdown popup — the property-grid popup pattern, the data-grid being
its 2nd consumer, adapted to the virtualized 2-D grid:

  * the choice VALUE layer was already complete in pinion-core (`CellValue::Choice`
    sorts / filters / groups by its selected label, and `with_intervene` takes an
    option Int index), so the filter / group / sort proxies index the column for
    free — the conversion preserves every prior Type-column assertion;
  * the dropdown floats in GRID-LOCAL coordinates (a sibling of the scroll
    viewport), scroll-aware (it tracks the cell as the body scrolls) and
    flip-above near the bottom — the toolkit combobox-delegate behaviour;
  * one path commits a pick — a pointer option click, the keyboard Enter/Space,
    and the RPC `choose` all journal ONE cell edit through `edit_cell` (so a
    dropdown pick re-anchors + undoes exactly like every other cell edit).

  (A) boot — Type is a choice column; the other columns keep their kinds.
  (B) a click opens the dropdown; it paints as a listbox anchored under the cell.
  (C) keyboard roves the option cursor + Enter commits one undo step.
  (D) an option click commits; the dismiss barrier closes without committing.
  (E) the RPC `choose` verb commits; one symmetric undo / redo.
  (F) the choice column composes with the filter (degrades to its selected label).

Run from the workspace root:
    cargo build -p hello-data-grid --release
    python3 tools/demos/r940_data_grid_choice.py

>= 35 assertions.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_query,
)

EXAMPLE = "hello-data-grid"
GRID = "data_grid"
UNDO = "/data_grid_undo/external"
POPUP = "data_grid_choice"
VIEWPORT = (460, 380)


def q(tf, path: str):
    return tf.query(f"/external/{path}")


def inv(tf, verb: str, arg):
    return tf.invoke(f"/external/{verb}", arg)


def painted(tf, tag: str) -> bool:
    return find_by_tag(tf.snapshot(source="paint", viewport=VIEWPORT), tag) is not None


def open_via_click(tf, row: int) -> None:
    """Click the Type cell of `row` through the real pointer pipeline (so the
    shell focuses the grid — the keyboard then routes here), opening its
    dropdown; wait for the popup."""
    tf.click(path=f"{GRID}#{row}_1")
    wait_query(tf, "/external/popup_open", True, desc=f"row {row} dropdown opened on click")


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) boot — Type is now a Choice column ──────────────────
        assert_eq(q(tf, "row_count"), 4, "4 seed rows")
        assert_eq(q(tf, "col_count"), 6, "6 columns (R943 added the Tint colour column)")
        assert_eq(q(tf, "col_kind.0"), "text", "Asset stays text")
        assert_eq(q(tf, "col_kind.1"), "choice", "Type is a choice column")
        assert_eq(q(tf, "col_kind.2"), "int", "Count stays int")
        assert_eq(q(tf, "col_kind.4"), "bool", "Active stays bool")
        # The choice cells carry {selected, label, options}; the label degrades
        # to the option text (the filter / group / sort SSOT).
        assert_eq(q(tf, "value.0.1")["label"], "sprite", "Hero Type = sprite")
        assert_eq(q(tf, "value.1.1")["label"], "mesh", "Tree Type = mesh")
        assert_eq(q(tf, "value.0.1")["selected"], 0, "sprite is option 0")
        assert_eq(q(tf, "value.0.1")["options"], ["sprite", "mesh", "material", "audio", "script"],
                  "the full asset-type enum")
        assert_eq(q(tf, "popup_open"), False, "no dropdown open at boot")
        assert not painted(tf, POPUP), "no dropdown panel painted at boot"

        # ── (B) a click opens the dropdown (paints as an anchored listbox) ──
        open_via_click(tf, 0)
        assert_eq(q(tf, "editing_row"), 0, "the latch is on row 0")
        assert_eq(q(tf, "editing_col"), 1, "... the Type column")
        assert_eq(q(tf, "popup_cursor"), 0, "the cursor seeds at the committed option (sprite)")
        assert painted(tf, POPUP), "the dropdown panel paints when open"
        for i in range(5):
            assert painted(tf, f"{GRID}#opt{i}"), f"option {i} row paints"
        # The panel anchors under the Type cell (GRID-LOCAL, scroll-aware): same
        # x as the cell, dropping just below it.
        rects = abs_rects_of(tf.snapshot(source="paint", viewport=VIEWPORT))
        cell = rects[f"{GRID}#0_1"]
        panel = rects[POPUP]
        assert abs(panel[0] - cell[0]) <= 2, f"panel x aligns under the Type cell ({panel[0]} vs {cell[0]})"
        assert panel[1] >= cell[1] + cell[3] - 2, "panel drops below the cell"
        # Close it again (the dismiss barrier) before the keyboard test.
        inv(tf, "send", "dismiss:PointerUp")
        wait_query(tf, "/external/popup_open", False, desc="dismiss closed the dropdown")

        # ── (C) keyboard rove + Enter commits one undo step ─────────
        open_via_click(tf, 0)  # Hero, cursor at sprite (0)
        tf.key(path=GRID, name="ArrowDown")
        tf.key(path=GRID, name="ArrowDown")
        wait_query(tf, "/external/popup_cursor", 2, desc="ArrowDown roved the cursor to material")
        tf.key(path=GRID, name="End")
        wait_query(tf, "/external/popup_cursor", 4, desc="End clamps to the last option")
        tf.key(path=GRID, name="ArrowUp")
        wait_query(tf, "/external/popup_cursor", 3, desc="ArrowUp roves back (audio)")
        tf.key(path=GRID, name="Enter")
        wait_query(tf, "/external/popup_open", False, desc="Enter committed + closed the dropdown")
        assert_eq(q(tf, "value.0.1")["label"], "audio", "Enter committed the cursor option")
        # The keyboard pick journals exactly one cell edit.
        tf.invoke(f"{UNDO}/undo", None)
        wait_query(tf, "/external/value.0.1", {"selected": 0, "label": "sprite",
                   "options": ["sprite", "mesh", "material", "audio", "script"]},
                   desc="undo reverts the dropdown pick in one step")

        # ── (D) option click commits; Escape-equivalent dismiss does not ──
        open_via_click(tf, 1)  # Tree = mesh (1)
        inv(tf, "send", "opt3:PointerUp")  # click "audio"
        wait_query(tf, "/external/popup_open", False, desc="option click committed + closed")
        assert_eq(q(tf, "value.1.1")["label"], "audio", "the option click committed audio")
        open_via_click(tf, 1)
        inv(tf, "send", "dismiss:PointerUp")
        wait_query(tf, "/external/popup_open", False, desc="dismiss closed the dropdown")
        assert_eq(q(tf, "value.1.1")["label"], "audio", "dismiss kept the prior value (no commit)")

        # ── (E) RPC choose verb + symmetric undo / redo ─────────────
        # open_choice rejects a non-choice focused cell (the Asset text column).
        tf.intervene("/external/focused_row", 2)
        tf.intervene("/external/focused_col", 0)
        assert_eq(inv(tf, "open_choice", None), False, "open_choice needs a focused CHOICE cell")
        tf.intervene("/external/focused_col", 1)
        assert_eq(inv(tf, "open_choice", None), True, "open_choice opens on the focused Type cell")
        assert_eq(q(tf, "popup_open"), True, "the RPC opened the dropdown")
        assert_eq(inv(tf, "choose", 2), True, "choose committed option 2 (material)")
        assert_eq(q(tf, "popup_open"), False, "choose closed the dropdown")
        assert_eq(q(tf, "value.2.1")["label"], "material", "Coin Type = material")
        tf.invoke(f"{UNDO}/undo", None)
        wait_query(tf, "/external/value.2.1", {"selected": 0, "label": "sprite",
                   "options": ["sprite", "mesh", "material", "audio", "script"]},
                   desc="undo restored Coin's sprite")
        tf.invoke(f"{UNDO}/redo", None)
        wait_query(tf, "/external/value.2.1", {"selected": 2, "label": "material",
                   "options": ["sprite", "mesh", "material", "audio", "script"]},
                   desc="redo re-applied material")
        # An out-of-range choose with the popup open commits nothing.
        tf.intervene("/external/focused_row", 2)
        tf.intervene("/external/focused_col", 1)
        assert_eq(inv(tf, "open_choice", None), True, "re-open Coin's dropdown")
        assert_eq(inv(tf, "choose", 99), False, "an out-of-range choose is a no-op")
        assert_eq(q(tf, "value.2.1")["label"], "material", "the value is unchanged")

        # ── (F) the choice column composes with the filter ──────────
        # CellValue::Choice degrades to its selected label, so the existing
        # equality filter indexes it with no special-casing.
        assert_eq(inv(tf, "set_filter", "1=mesh"), 1, "filter Type=mesh keeps the one mesh row (Tree was edited away)")
        assert_eq(inv(tf, "set_filter", "1=material"), 1, "filter Type=material keeps Coin")
        assert_eq(q(tf, "source_at.0"), 2, "Coin (material) is the only match")
        assert_eq(inv(tf, "set_filter", None), 4, "clearing the filter restores every row")


if __name__ == "__main__":
    sys.exit(run_demo("R940 §5.38 §5.40 — data-grid Choice column", body))
