#!/usr/bin/env python3
"""R869 §5.38 §5.16 §5.40 — property-grid colour cell editor.

The Inspector gains a colour cell: a cell holding an sRGB `Color`, edited by
a popup swatch palette anchored to the cell. Built on the R867 coordinator-
owned-popup pattern (the swatches are selectable items like the choice
options), with the cell showing a filled swatch chip + the `#RRGGBB` hex.

An arbitrary colour is entered in the GUI through the popup's hex field
(R870) or programmatically via `intervene value.<i>` with a hex string (the
AI-first path). A 2-D HSV pad in the cell is a deferred follow-up — it needs
a `Color`->HSV decomposition (the inverse of `from_hsv`) to seed from a
stored colour, which the substrate does not yet have.

Three drive paths, one model:
  * keyboard — the grid opens the popup; arrows rove the swatch cursor over
    the 2-D palette grid, Enter/Space commit, Escape dismisses.
  * mouse — single-click the colour cell opens it; click a swatch to commit;
    click outside to dismiss.
  * RPC — `invoke begin` opens, `invoke pick_color` commits a swatch,
    `invoke close_popup` dismisses, `intervene value.<i> = "#RRGGBB"` sets
    an arbitrary colour directly.

Coordinator slots (`property_grid`, the primary external):
  /external/value.<i>     -> colour: { hex, r, g, b, a }
  /external/popup_cursor -> the popup's swatch cursor (shared with choice)
  /external/begin         -> invoke(int): open the colour popup
  /external/pick_color    -> invoke(int): commit a swatch + close
  /external/close_popup  -> invoke: dismiss without committing

Verified (>= 30 assertions):
  (A) boot taxonomy — the colour row, its hex value
  (B) keyboard — open, rove + clamp, Enter commits the swatch
  (C) keyboard — Escape dismisses, value untouched
  (D) mouse — click cell opens, click swatch commits
  (E) RPC — pick_color + close_popup + intervene-by-hex (arbitrary colour)
  (F) mouse — click outside dismisses
  (G) paint — swatch panel + swatches + barrier paint only while open
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
    wait_until,
)

VIEWPORT = (460, 820)

GRID = "property_grid"
EDIT = "property_grid_edit"
POPUP = "property_grid_color"
DISMISS = "property_grid#dismiss"

TINT = 11  # the colour row; boots Blue (#1e88e5), swatch index 4


def _focus_grid(tf) -> None:
    tf.request("focus/set", {"tag": GRID})
    wait_until(
        lambda: tf.request("focus/get").result.get("focused") == GRID,
        timeout=4.0,
        interval=0.03,
        desc="grid owns keyboard focus",
    )


def _hex(tf) -> str:
    return tf.query(f"/external/value.{TINT}")["hex"]


def _editing(tf):
    # R931 — `editing` now reports the editing leaf's node id (a string, the same
    # vocabulary `cursor` uses). This demo's rows are scalar leaves, so map a
    # numeric id back to its int value index for comparison with the constants.
    e = tf.query("/external/editing")
    return int(e) if isinstance(e, str) and e.isdigit() else e


def body() -> None:
    with RpcSubprocess("hello-property-grid", boot_grace=1.5) as tf:
        # ── (A) boot taxonomy ────────────────────────────────────────
        assert_eq(tf.query("/external/row_count"), 16, "16 value slots (incl. struct fields)")
        assert_eq(tf.query("/external/kind.11"), "color", "Tint is a colour")
        tint = tf.query("/external/value.11")
        assert_eq(tint["hex"], "#1e88e5", "Tint boots Blue")
        assert_eq(tint["r"], 0x1E, "Blue red byte")
        assert_eq(tint["b"], 0xE5, "Blue blue byte")
        assert_eq(tf.query("/external/popup_cursor"), None, "no popup at boot")

        # ── (B) keyboard: open, rove, Enter commits ──────────────────
        _focus_grid(tf)
        tf.intervene("/external/cursor", str(TINT))
        tf.key(path=GRID, name="Enter")  # open
        wait_until(lambda: _editing(tf) == TINT, timeout=4.0, interval=0.03,
                   desc="Enter opens the colour popup")
        assert_eq(tf.query("/external/popup_cursor"), 4, "cursor seeded at Blue (swatch 4)")
        tf.key(path=GRID, name="ArrowRight")  # Blue(4) -> Yellow(5)
        wait_until(lambda: tf.query("/external/popup_cursor") == 5, timeout=4.0,
                   interval=0.03, desc="ArrowRight roves the swatch cursor")
        tf.key(path=GRID, name="Enter")  # commit Yellow
        wait_until(lambda: _hex(tf) == "#fdd835", timeout=4.0,
                   interval=0.03, desc="Enter commits the swatch cursor (Yellow)")
        assert_eq(_editing(tf), None, "popup closed after commit")

        # ── (C) keyboard: Escape dismisses, value untouched ──────────
        tf.key(path=GRID, name="Enter")  # re-open (Yellow committed)
        wait_until(lambda: _editing(tf) == TINT, timeout=4.0, interval=0.03,
                   desc="re-open the colour popup")
        tf.key(path=GRID, name="ArrowLeft")  # move off Yellow
        tf.key(path=GRID, name="Escape")
        wait_until(lambda: _editing(tf) is None, timeout=4.0, interval=0.03,
                   desc="Escape dismisses")
        assert_eq(_hex(tf), "#fdd835", "Escape leaves the committed Yellow")

        # ── (D) mouse: click cell opens, click swatch commits ────────
        tf.click(path=f"{GRID}#{TINT}")
        wait_until(lambda: _editing(tf) == TINT, timeout=4.0, interval=0.03,
                   desc="single-click opens the colour popup")
        assert find_by_tag(tf.snapshot(source="paint", viewport=VIEWPORT), POPUP) is not None, \
            "swatch panel painted"
        tf.click(path=f"{GRID}#sw2")  # Red
        wait_until(lambda: _hex(tf) == "#e53935", timeout=4.0,
                   interval=0.03, desc="clicking a swatch commits it (Red)")
        assert_eq(_editing(tf), None, "popup closed after swatch click")

        # ── (E) RPC ──────────────────────────────────────────────────
        assert_eq(tf.invoke("/external/begin", TINT), True, "RPC begin opens")
        assert_eq(tf.invoke("/external/pick_color", 0), True, "RPC pick_color commits")
        assert_eq(_hex(tf), "#ffffff", "pick_color 0 -> White")
        assert_eq(_editing(tf), None, "pick_color closed the popup")
        # close_popup dismisses without committing.
        tf.invoke("/external/begin", TINT)
        tf.invoke("/external/close_popup", None)
        assert_eq(_editing(tf), None, "close_popup dismissed")
        assert_eq(_hex(tf), "#ffffff", "close_popup did not commit")
        # intervene sets an arbitrary colour by hex (the AI-first path).
        tf.intervene("/external/value.11", "#abcdef")
        assert_eq(_hex(tf), "#abcdef", "intervene sets an arbitrary hex colour")

        # ── (E2) GUI hex field: type an arbitrary colour + Enter ─────
        tf.invoke("/external/begin", TINT)  # open the popup (seeds the hex field)
        wait_until(lambda: _editing(tf) == TINT, timeout=4.0, interval=0.03,
                   desc="popup open for hex entry")
        tf.request("focus/set", {"tag": EDIT})  # focus the hex field
        wait_until(lambda: tf.request("focus/get").result.get("focused") == EDIT,
                   timeout=4.0, interval=0.03, desc="hex field focused")
        for _ in range(8):  # clear the seeded "#abcdef"
            tf.key(path=EDIT, name="Backspace")
        tf.text("#123456", path=EDIT)
        tf.key(path=EDIT, name="Enter")
        wait_until(lambda: _hex(tf) == "#123456", timeout=4.0, interval=0.03,
                   desc="hex field commits an arbitrary colour")
        assert_eq(_editing(tf), None, "hex commit closed the popup")

        # ── (F) mouse: click outside dismisses ───────────────────────
        tf.click(path=f"{GRID}#{TINT}")
        wait_until(lambda: _editing(tf) == TINT, timeout=4.0, interval=0.03,
                   desc="re-open for dismiss test")
        tf.click(at=(8.0, 8.0))  # the dismiss barrier corner, clear of the panel
        wait_until(lambda: _editing(tf) is None, timeout=4.0, interval=0.03,
                   desc="clicking outside dismisses")
        assert_eq(_hex(tf), "#123456", "dismiss leaves the value untouched")

        # ── (G) paint: panel + swatches + barrier only while open ────
        closed = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert find_by_tag(closed, POPUP) is None, "no panel when closed"
        assert find_by_tag(closed, DISMISS) is None, "no barrier when closed"
        tf.invoke("/external/begin", TINT)
        wait_until(lambda: _editing(tf) == TINT, timeout=4.0, interval=0.03,
                   desc="open for paint check")
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert find_by_tag(snap, POPUP) is not None, "panel painted when open"
        assert find_by_tag(snap, DISMISS) is not None, "dismiss barrier painted"
        for i in range(8):
            assert find_by_tag(snap, f"{GRID}#sw{i}") is not None, f"swatch {i} painted"
        rects = abs_rects_of(snap)
        assert POPUP in rects, "popup rect resolved"
        px, py, _pw, ph = rects[POPUP]
        assert 150 <= px <= 230, f"popup anchored near the value column (x={px})"
        assert py >= 0 and py + ph <= VIEWPORT[1], f"popup fully on screen (y={py} h={ph})"
        tf.invoke("/external/close_popup", None)


if __name__ == "__main__":
    sys.exit(run_demo("hello-property-grid R869 §5.38 colour cell editor", body))
