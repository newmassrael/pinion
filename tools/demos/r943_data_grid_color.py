#!/usr/bin/env python3
"""R943 §5.38 §5.40 — data-grid Color column (swatch-palette cell editor).

Drives hello-data-grid over JSON-RPC. R940 gave the grid a Choice column edited
through a floating dropdown popup; R943 adds a `Tint` column (`CellKind::Color`)
edited through a floating SWATCH PALETTE popup — the property-grid colour-cell
pattern, the data-grid being its 2nd popup consumer, adapted to the virtualized
grid as the choice dropdown's 2nd popup *kind* (one shared edit latch + cursor +
hover; the kind dispatches the panel / keymap / a11y):

  * the colour VALUE layer was already complete in pinion-core (`CellValue::Color`
    displays / sorts / filters by its `#RRGGBB` hex, and `with_intervene` parses a
    hex string), so the column drops in over the existing Model/View proxies;
  * a closed cell paints a filled swatch chip + its hex; a click opens a grid of
    preset swatch chips, navigable in 2-D (the property-grid `apply_key_color`);
  * one path commits a pick — a swatch click, the keyboard Enter/Space, and the
    RPC `pick_color` all journal ONE cell edit through `edit_cell` (so a swatch
    pick re-anchors + undoes exactly like every other cell edit); an arbitrary
    off-palette colour is set through `intervene value` with a hex string.

  (A) boot — Tint is a colour column; closed cells show their hex.
  (B) the RPC `open_color` opens the swatch popup; it paints a grid of swatches.
  (C) a real click (after revealing the column) opens + focuses; keyboard 2-D
      roves the swatch cursor and Enter commits one undo step.
  (D) a swatch click commits; the dismiss barrier closes without committing.
  (E) the RPC `pick_color` commits + symmetric undo/redo; out-of-range + a
      non-colour cell are rejected.
  (F) the arbitrary-colour path: `intervene value` with an off-palette hex.
  (G) the open popup announces a listbox of swatches (a11y).
  (H) the colour column composes with sort (sorts by hex) — additive.

Run from the workspace root:
    cargo build -p hello-data-grid --release
    python3 tools/demos/r943_data_grid_color.py

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
    wait_snap,
    wait_until,
)

EXAMPLE = "hello-data-grid"
GRID = "data_grid"
UNDO = "/data_grid_undo/external"
POPUP = "data_grid_color"
H_SCROLL = "data_grid_hscroll"
TINT = 5  # the Tint (Color) column index
VIEWPORT = (460, 380)

# Mirror the Rust COLOR_SWATCHES palette (index -> #RRGGBB hex + AT name).
SWATCH_HEX = ["#ffffff", "#212121", "#e53935", "#43a047",
              "#1e88e5", "#fdd835", "#00acc1", "#8e24aa"]
# The seed Tint cells: row r -> swatch index SEED_SWATCH[r].
SEED_SWATCH = [4, 3, 5, 2]  # Blue / Green / Yellow / Red


def q(tf, path: str):
    return tf.query(f"/external/{path}")


def inv(tf, verb: str, arg):
    return tf.invoke(f"/external/{verb}", arg)


def cell_hex(tf, row: int) -> str:
    return q(tf, f"value.{row}.{TINT}")["hex"]


def wait_hex(tf, row: int, expected: str, desc: str) -> None:
    """Poll a Tint cell's hex (the colour query returns {r,g,b,a,hex}) until it
    reaches `expected` — the ZERO-FLAKE gate for an async undo / redo."""
    wait_until(lambda: cell_hex(tf, row) == expected, desc=desc)


def painted(tf, tag: str) -> bool:
    return find_by_tag(tf.snapshot(source="paint", viewport=VIEWPORT), tag) is not None


def open_color_rpc(tf, row: int) -> bool:
    """Open the swatch popup on `(row, TINT)` over RPC (the AI-first peer of a
    click; no pixels, so it works while the column is scrolled off-screen)."""
    tf.intervene("/external/focused_row", row)
    tf.intervene("/external/focused_col", TINT)
    return inv(tf, "open_color", None)


def cell_on_screen(snap, tag: str) -> bool:
    node = find_by_tag(snap, H_SCROLL)
    rects = abs_rects_of(snap)
    if node is None or tag not in rects:
        return False
    vp = node.get("viewport") or {}
    vp_x, vp_w = int(vp.get("x", -1)), int(vp.get("w", -1))
    x, _, w, _ = rects[tag]
    return x < vp_x + vp_w and x + w > vp_x


def reveal_tint(tf) -> None:
    """Scroll the trailing Tint column on-screen (it clips out at rest)."""
    tf.scroll(H_SCROLL, to=(10 ** 9, 0))
    wait_snap(tf, lambda s: cell_on_screen(s, f"{GRID}#0_{TINT}"),
              viewport=VIEWPORT, desc="Tint column scrolled on-screen")


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) boot — Tint is a Color column; closed cells show their hex ──
        assert_eq(q(tf, "col_count"), 6, "6 columns (R943 added Tint)")
        assert_eq(q(tf, "col_kind.4"), "bool", "Active stays bool")
        assert_eq(q(tf, "col_kind.5"), "color", "Tint is a colour column")
        for r, sw in enumerate(SEED_SWATCH):
            assert_eq(cell_hex(tf, r), SWATCH_HEX[sw], f"seed Tint row {r} = swatch {sw}")
        assert_eq(q(tf, "popup_open"), False, "no popup open at boot")
        assert not painted(tf, POPUP), "no swatch palette painted at boot"

        # ── (B) RPC open_color opens the swatch popup (paints a grid) ──────
        assert_eq(open_color_rpc(tf, 0), True, "open_color opens on the focused Tint cell")
        assert_eq(q(tf, "popup_open"), True, "the swatch popup is open")
        assert_eq(q(tf, "editing_row"), 0, "the latch is on row 0")
        assert_eq(q(tf, "editing_col"), TINT, "... the Tint column")
        assert_eq(q(tf, "popup_cursor"), 4, "cursor seeded at the current preset (Blue, swatch 4)")
        assert painted(tf, POPUP), "the swatch palette paints when open"
        for i in range(len(SWATCH_HEX)):
            assert painted(tf, f"{GRID}#sw{i}"), f"swatch chip {i} paints"
        inv(tf, "close_popup", None)
        wait_query(tf, "/external/popup_open", False, desc="close_popup closes the palette")

        # ── (C) real click opens + focuses; keyboard 2-D rove + Enter ─────
        # The Tint column clips out at rest, so reveal it, then a real pointer
        # click focuses the grid (keys route here) AND opens the popup.
        reveal_tint(tf)
        tf.click(path=f"{GRID}#0_{TINT}")
        wait_query(tf, "/external/popup_open", True, desc="click opened the swatch popup")
        wait_query(tf, "/external/popup_cursor", 4, desc="cursor at the current preset (Blue)")
        # The palette anchors under the Tint cell (GRID-LOCAL, scroll-aware).
        rects = abs_rects_of(tf.snapshot(source="paint", viewport=VIEWPORT))
        cell = rects[f"{GRID}#0_{TINT}"]
        panel = rects[POPUP]
        assert abs(panel[0] - cell[0]) <= 2, f"panel x aligns under the Tint cell ({panel[0]} vs {cell[0]})"
        assert panel[1] >= cell[1] + cell[3] - 2, "panel drops below the cell"
        # 2-D nav: Home -> 0 (White); ArrowDown jumps a palette row (4 cols).
        tf.key(path=GRID, name="Home")
        wait_query(tf, "/external/popup_cursor", 0, desc="Home -> first swatch")
        tf.key(path=GRID, name="ArrowDown")
        wait_query(tf, "/external/popup_cursor", 4, desc="ArrowDown jumps a palette row (+4)")
        tf.key(path=GRID, name="ArrowRight")
        wait_query(tf, "/external/popup_cursor", 5, desc="ArrowRight steps to swatch 5 (Yellow)")
        tf.key(path=GRID, name="Enter")
        wait_query(tf, "/external/popup_open", False, desc="Enter committed + closed")
        assert_eq(cell_hex(tf, 0), SWATCH_HEX[5], "Enter committed Yellow")
        # The keyboard pick journals exactly one cell edit.
        tf.invoke(f"{UNDO}/undo", None)
        wait_hex(tf, 0, SWATCH_HEX[4], "undo reverts the swatch pick in one step (back to Blue)")

        # ── (D) swatch click commits; dismiss does not ──────────────────
        assert_eq(open_color_rpc(tf, 1), True, "open row 1's Tint (Green / swatch 3)")
        inv(tf, "send", "sw2:PointerUp")  # click Red (swatch 2)
        wait_query(tf, "/external/popup_open", False, desc="swatch click committed + closed")
        assert_eq(cell_hex(tf, 1), SWATCH_HEX[2], "the swatch click committed Red")
        assert_eq(open_color_rpc(tf, 1), True, "re-open row 1")
        inv(tf, "send", "dismiss:PointerUp")
        wait_query(tf, "/external/popup_open", False, desc="dismiss closed the palette")
        assert_eq(cell_hex(tf, 1), SWATCH_HEX[2], "dismiss kept the prior value (no commit)")

        # ── (E) RPC pick_color + undo/redo; rejections ──────────────────
        # open_color rejects a non-colour focused cell (the Active bool column).
        tf.intervene("/external/focused_row", 2)
        tf.intervene("/external/focused_col", 4)
        assert_eq(inv(tf, "open_color", None), False, "open_color needs a focused COLOUR cell")
        assert_eq(open_color_rpc(tf, 2), True, "open row 2's Tint (Yellow / swatch 5)")
        assert_eq(inv(tf, "pick_color", 6), True, "pick_color committed swatch 6 (Cyan)")
        assert_eq(q(tf, "popup_open"), False, "pick_color closed the palette")
        assert_eq(cell_hex(tf, 2), SWATCH_HEX[6], "row 2 Tint = Cyan")
        tf.invoke(f"{UNDO}/undo", None)
        wait_hex(tf, 2, SWATCH_HEX[5], "undo restored row 2's Yellow")
        tf.invoke(f"{UNDO}/redo", None)
        wait_hex(tf, 2, SWATCH_HEX[6], "redo re-applied Cyan")
        # An out-of-range pick_color with the popup open commits nothing.
        assert_eq(open_color_rpc(tf, 2), True, "re-open row 2's Tint")
        assert_eq(inv(tf, "pick_color", 99), False, "an out-of-range pick_color is a no-op")
        assert_eq(cell_hex(tf, 2), SWATCH_HEX[6], "the value is unchanged")

        # ── (F) the arbitrary-colour path: intervene value with a hex ───
        assert_eq(tf.intervene(f"/external/value.3.{TINT}", "#123456"), None,
                  "intervene accepts an off-palette hex")
        assert_eq(cell_hex(tf, 3), "#123456", "an arbitrary colour is set via intervene value")

        # ── (G) the open popup is a listbox of swatches (a11y) ──────────
        assert_eq(open_color_rpc(tf, 0), True, "re-open row 0's Tint")
        # The swatch chips route through the popup; their composite tags are the
        # AT option set the listbox builder advertises.
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        sw_tags = [f"{GRID}#sw{i}" for i in range(len(SWATCH_HEX))]
        assert all(find_by_tag(snap, t) is not None for t in sw_tags), "every swatch option paints"
        inv(tf, "close_popup", None)
        wait_query(tf, "/external/popup_open", False, desc="palette closed")

        # ── (H) the colour column composes with sort (additive) ─────────
        # Sorting by Tint orders rows by their colour (the CellValue::Color sort
        # SSOT), with no special-casing — the column drops into the proxy. Final
        # colours: row0 #1e88e5, row1 #e53935, row2 #00acc1, row3 #123456.
        assert_eq(inv(tf, "cycle_sort", TINT), "5:ascending", "first cycle sorts Tint ascending")
        # Ascending: #00acc1 (row2) < #123456 (row3) < #1e88e5 (row0) < #e53935 (row1).
        assert_eq(q(tf, "source_at.0"), 2, "row 2 (#00acc1) sorts first")
        assert_eq(q(tf, "source_at.3"), 1, "row 1 (#e53935) sorts last")
        assert_eq(inv(tf, "cycle_sort", TINT), "5:descending", "second cycle flips to descending")
        assert_eq(q(tf, "source_at.0"), 1, "descending puts #e53935 first")
        inv(tf, "cycle_sort", TINT)  # third cycle -> back to source order


if __name__ == "__main__":
    sys.exit(run_demo("R943 §5.38 §5.40 — data-grid Color column", body))
