#!/usr/bin/env python3
"""R867 §5.38 §5.16 §5.40 — property-grid enum/choice cell editor.

The self-hosted editor's Inspector gains the combobox-in-a-cell: an
enum/choice property whose value is one of a fixed option list, edited by a
popup listbox overlay anchored to the cell. The popup is owned by the grid
coordinator (the cell's model is the grid's typed `CellValue::Choice`, not a
separate model-owning `ListBoxExternal`); it reuses the lifted `view_option`
paint, the shared `dismiss_barrier`, and the `listbox_option_nodes` a11y.

Three drive paths, one model:
  * keyboard — the grid (one Tab stop) opens the popup; arrows rove the
    active descendant, Enter/Space commit, Escape dismisses.
  * mouse — single-click the choice cell opens it; click an option to
    commit; click outside to dismiss.
  * RPC (AI-first) — `invoke begin` opens, `invoke choose` commits, `invoke
    close_popup` dismisses, and `intervene value.<i> = <int>` sets the
    option directly without opening the popup.

Coordinator slots (`property_grid`, the primary external):
  /external/value.<i>     -> choice: { selected, label, options }
  /external/popup_cursor -> the popup's roving cursor (null when closed)
  /external/begin         -> invoke(int): open the choice popup
  /external/choose        -> invoke(int): commit an option + close
  /external/close_popup  -> invoke: dismiss without committing

Verified (>= 30 assertions):
  (A) boot taxonomy — two choice rows, their option lists + committed labels
  (B) keyboard — open, rove + clamp, Enter commits the cursor
  (C) keyboard — Escape dismisses, value untouched
  (D) mouse — click cell opens, click option commits
  (E) mouse — click outside dismisses, value untouched
  (F) RPC — begin/choose commit, close_popup dismiss, intervene-by-index
  (G) paint — popup panel + options + barrier paint only while open
  (H) anchor — the popup sits at the value column, on screen
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
POPUP = "property_grid_choice"
DISMISS = "property_grid#dismiss"

BLEND = 9   # 4 options: Normal / Additive / Multiply / Screen (default Normal)
BODY = 10   # 3 options: None / Trigger / Solid (default Solid)


def _focus_grid(tf) -> None:
    tf.request("focus/set", {"tag": GRID})
    wait_until(
        lambda: tf.request("focus/get").result.get("focused") == GRID,
        timeout=4.0,
        interval=0.03,
        desc="grid owns keyboard focus",
    )


def _label(tf, row: int) -> str:
    return tf.query(f"/external/value.{row}")["label"]


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
        assert_eq(tf.query("/external/kind.9"), "choice", "Blend is a choice")
        assert_eq(tf.query("/external/kind.10"), "choice", "Body is a choice")
        blend = tf.query("/external/value.9")
        assert_eq(blend["selected"], 0, "Blend boots at Normal")
        assert_eq(blend["label"], "Normal", "Blend label")
        assert_eq(
            blend["options"],
            ["Normal", "Additive", "Multiply", "Screen"],
            "Blend option list",
        )
        assert_eq(_label(tf, BODY), "Solid", "Body boots at Solid")
        assert_eq(tf.query("/external/popup_cursor"), None, "no popup at boot")
        assert_eq(_editing(tf), None, "nothing editing at boot")

        # ── (B) keyboard: open, rove + clamp, Enter commits ──────────
        _focus_grid(tf)
        tf.intervene("/external/cursor", str(BLEND))
        tf.key(path=GRID, name="Enter")  # open the popup
        wait_until(lambda: _editing(tf) == BLEND, timeout=4.0, interval=0.03,
                   desc="Enter opens the choice popup")
        assert_eq(tf.query("/external/popup_cursor"), 0, "cursor seeded at committed option")
        tf.key(path=GRID, name="ArrowDown")
        wait_until(lambda: tf.query("/external/popup_cursor") == 1, timeout=4.0,
                   interval=0.03, desc="ArrowDown roves the cursor")
        tf.key(path=GRID, name="ArrowDown")
        tf.key(path=GRID, name="ArrowDown")
        tf.key(path=GRID, name="ArrowDown")  # past the end
        wait_until(lambda: tf.query("/external/popup_cursor") == 3, timeout=4.0,
                   interval=0.03, desc="cursor clamps at the last option")
        tf.key(path=GRID, name="Enter")  # commit the cursor (Screen)
        wait_until(lambda: _label(tf, BLEND) == "Screen", timeout=4.0,
                   interval=0.03, desc="Enter commits the cursor")
        assert_eq(_editing(tf), None, "popup closed after commit")
        assert_eq(tf.query("/external/popup_cursor"), None, "cursor cleared after commit")

        # ── (C) keyboard: Escape dismisses, value untouched ──────────
        tf.intervene("/external/cursor", str(BODY))
        tf.key(path=GRID, name="Enter")
        wait_until(lambda: _editing(tf) == BODY, timeout=4.0, interval=0.03,
                   desc="open Body popup")
        tf.key(path=GRID, name="ArrowUp")  # move off Solid
        tf.key(path=GRID, name="Escape")
        wait_until(lambda: _editing(tf) is None, timeout=4.0, interval=0.03,
                   desc="Escape dismisses")
        assert_eq(_label(tf, BODY), "Solid", "Escape leaves Body untouched")

        # ── (D) mouse: click cell opens, click option commits ────────
        tf.click(path=f"{GRID}#{BLEND}")  # single-click opens
        wait_until(lambda: _editing(tf) == BLEND, timeout=4.0, interval=0.03,
                   desc="single-click opens the choice popup")
        assert find_by_tag(tf.snapshot(source="paint", viewport=VIEWPORT), POPUP) is not None, \
            "popup panel painted"
        tf.click(path=f"{GRID}#opt1")  # commit Additive
        wait_until(lambda: _label(tf, BLEND) == "Additive", timeout=4.0,
                   interval=0.03, desc="clicking an option commits it")
        assert_eq(_editing(tf), None, "popup closed after option click")

        # ── (E) mouse: click outside dismisses ───────────────────────
        tf.click(path=f"{GRID}#{BODY}")
        wait_until(lambda: _editing(tf) == BODY, timeout=4.0, interval=0.03,
                   desc="re-open Body popup")
        tf.click(at=(8.0, 8.0))  # the dismiss barrier corner, clear of the panel
        wait_until(lambda: _editing(tf) is None, timeout=4.0, interval=0.03,
                   desc="clicking outside dismisses")
        assert_eq(_label(tf, BODY), "Solid", "dismiss leaves Body untouched")

        # ── (F) RPC (AI-first) ───────────────────────────────────────
        assert_eq(tf.invoke("/external/begin", BLEND), True, "RPC begin opens")
        assert_eq(_editing(tf), BLEND, "popup open via RPC")
        assert_eq(tf.invoke("/external/choose", 2), True, "RPC choose commits")
        assert_eq(_label(tf, BLEND), "Multiply", "choose 2 -> Multiply")
        assert_eq(_editing(tf), None, "choose closed the popup")
        # close_popup dismisses without committing.
        tf.invoke("/external/begin", BODY)
        assert_eq(_editing(tf), BODY, "Body popup open")
        tf.invoke("/external/close_popup", None)
        assert_eq(_editing(tf), None, "close_popup dismissed")
        assert_eq(_label(tf, BODY), "Solid", "close_popup did not commit")
        # intervene-by-index sets the value directly (no popup needed).
        tf.intervene("/external/value.9", 3)
        assert_eq(_label(tf, BLEND), "Screen", "intervene sets the option by index")
        tf.intervene("/external/value.10", 1)
        assert_eq(_label(tf, BODY), "Trigger", "intervene sets Body to Trigger")

        # ── (G) paint: popup + options + barrier only while open ─────
        closed = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert find_by_tag(closed, POPUP) is None, "no panel when closed"
        assert find_by_tag(closed, DISMISS) is None, "no barrier when closed"
        tf.invoke("/external/begin", BLEND)
        wait_until(lambda: _editing(tf) == BLEND, timeout=4.0, interval=0.03,
                   desc="open for paint check")
        snap = tf.snapshot(source="paint", viewport=VIEWPORT)
        assert find_by_tag(snap, POPUP) is not None, "panel painted when open"
        assert find_by_tag(snap, DISMISS) is not None, "dismiss barrier painted"
        for i in range(4):
            assert find_by_tag(snap, f"{GRID}#opt{i}") is not None, f"option {i} painted"

        # ── (H) anchor: popup at the value column, on screen ─────────
        rects = abs_rects_of(snap)
        assert POPUP in rects, "popup rect resolved"
        px, py, pw, ph = rects[POPUP]
        assert 150 <= px <= 230, f"popup anchored near the value column (x={px})"
        assert py >= 0 and py + ph <= VIEWPORT[1], f"popup fully on screen (y={py} h={ph})"
        tf.invoke("/external/close_popup", None)


if __name__ == "__main__":
    sys.exit(run_demo("hello-property-grid R867 §5.38 enum/choice cell editor", body))
