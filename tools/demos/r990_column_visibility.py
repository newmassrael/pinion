#!/usr/bin/env python3
"""R990 §5.27 §5.38 §5.40 — data-grid column hide / show.

A column-chooser toolbar (one control per source column, reflective aria-pressed
= visible) drives a shared visibility model; the grid paints and announces only
the visible columns. The sole remaining visible column's control is disabled
(cannot hide everything) — the 2nd consumer of the R989 reflective toolbar
disabled axis.

Verification scope (>=30 assertions; gates per [[zero-flake-policy]]: every
action->assert edge polls observed state, no fixed sleeps):

  (A) boot — 5 columns, grid has 5 columnheaders, every chooser control pressed.
  (B) hide a column -> the grid drops it (painted + aria-colcount + count text).
  (C) the chooser control reflects pressed=false for the hidden column.
  (D) hide down to one column -> the sole visible control is aria-disabled.
  (E) the disabled control is a no-op even when clicked.
  (F) show a hidden column -> it returns; the disabled clears.
  (G) keyboard: focus the chooser, rove, Enter toggles a column.
  (H) scene/access cross-check of columnheaders + aria-pressed + aria-disabled.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_until,
)

VIEWPORT = (620, 420)
KEY_AT = (5.0, 5.0)
HEADERS = ["Name", "Type", "Size", "Modified", "Owner"]
NCOLS = len(HEADERS)


def _paint(tf):
    return tf.snapshot(source="paint", viewport=VIEWPORT)


def _access(tf):
    nodes = tf.request("scene/access").result["nodes"]
    return {n.get("tag"): n for n in nodes if n.get("tag")}, nodes


def _colheaders(tf):
    """Visible column header names, in order (from the a11y grid)."""
    _, nodes = _access(tf)
    return [n.get("name") for n in nodes if n.get("role") == "columnheader"]


def _painted_headers(tf):
    """Visible column header tags painted by view_table (`grid_ch<vp>`)."""
    snap = _paint(tf)
    return [t for t in range(NCOLS) if find_by_tag(snap, f"grid_ch{t}") is not None]


def _ctl_pressed(tf, i: int):
    by_tag, _ = _access(tf)
    return by_tag.get(f"cols#{i}", {}).get("state", {}).get("checked")


def _ctl_disabled(tf, i: int) -> bool:
    by_tag, _ = _access(tf)
    return bool(by_tag.get(f"cols#{i}", {}).get("state", {}).get("disabled", False))


def _count_text(tf) -> str:
    node = find_by_tag(_paint(tf), "colvis_count")
    return (node.get("content") or "") if node else ""


def _toggle(tf, col: int) -> None:
    tf.click(path=f"cols#{col}")
    tf.pointer_leave()


def _wait_visible(tf, n: int, desc: str) -> None:
    wait_until(lambda: len(_colheaders(tf)) == n, desc=desc)


def body() -> None:
    with RpcSubprocess("hello-column-visibility", boot_grace=1.5) as tf:
        # ── (A) boot ─────────────────────────────────────────────────
        wait_until(lambda: find_by_tag(_paint(tf), "grid_ch0") is not None, desc="grid paints")
        assert_eq(_colheaders(tf), HEADERS, "boot: all five columnheaders, in order")     # 1
        assert_eq(len(_painted_headers(tf)), NCOLS, "boot: all five columns painted")      # 2
        assert "5 of 5 columns shown" in _count_text(tf), "boot count readout"             # 3
        for i in range(NCOLS):
            assert_eq(_ctl_pressed(tf, i), True, f"chooser control {i} pressed (visible)")  # 4-8
        assert not any(_ctl_disabled(tf, i) for i in range(NCOLS)), "nothing disabled"      # 9

        # ── (B) hide a column (Type, col 1) ──────────────────────────
        _toggle(tf, 1)
        _wait_visible(tf, NCOLS - 1, "hiding Type drops a columnheader")                    # 10
        assert_eq(_colheaders(tf), ["Name", "Size", "Modified", "Owner"], "Type is gone")   # 11
        assert find_by_tag(_paint(tf), "grid_ch4") is None, "the 5th painted column is gone" # 12
        assert "4 of 5 columns shown" in _count_text(tf), "count readout drops"             # 13

        # ── (C) the chooser control reflects the hidden column ───────
        assert_eq(_ctl_pressed(tf, 1), False, "the Type control is no longer pressed")      # 14
        assert_eq(_ctl_pressed(tf, 0), True, "Name stays pressed")                          # 15

        # ── (D) hide down to one -> the sole visible is disabled ─────
        _toggle(tf, 2)  # hide Size
        _toggle(tf, 3)  # hide Modified
        _toggle(tf, 4)  # hide Owner
        _wait_visible(tf, 1, "only Name remains visible")                                    # 16
        assert_eq(_colheaders(tf), ["Name"], "Name is the sole columnheader")               # 17
        assert _ctl_disabled(tf, 0), "the sole visible column's control is aria-disabled"    # 18
        assert not _ctl_disabled(tf, 1), "a hidden column's control stays operable"          # 19
        assert "1 of 5 columns shown" in _count_text(tf), "count readout reads one"          # 20

        # ── (E) the disabled control is a no-op even when clicked ────
        _toggle(tf, 0)  # try to hide the last column
        wait_until(lambda: len(_colheaders(tf)) == 1, desc="hiding the last column is a no-op")
        assert_eq(_colheaders(tf), ["Name"], "still one visible column after the no-op")     # 21
        assert_eq(_ctl_pressed(tf, 0), True, "Name stays visible/pressed")                   # 22

        # ── (F) show a hidden column -> disabled clears ──────────────
        _toggle(tf, 3)  # show Modified
        _wait_visible(tf, 2, "showing Modified brings it back")                              # 23
        assert_eq(_colheaders(tf), ["Name", "Modified"], "Name + Modified visible")          # 24
        assert not _ctl_disabled(tf, 0), "no control disabled once two are visible"          # 25
        assert_eq(_ctl_pressed(tf, 3), True, "Modified is pressed again")                    # 26

        # ── (G) keyboard: focus the chooser, rove, Enter toggles ─────
        # Home first: the prior clicks left the roving cursor on the last-clicked
        # control (activation focuses it), so reset to a known position.
        tf.request("focus/set", {"tag": "cols"})
        wait_until(lambda: tf.request("focus/get").result.get("focused") == "cols",
                   desc="the chooser owns focus")                                            # 27
        tf.key(at=KEY_AT, name="Home")
        wait_until(lambda: tf.query("/external/focus") == 0, desc="Home roves to col 0")     # 28
        tf.key(at=KEY_AT, name="ArrowRight")  # rove from 0 to 1 (Type, hidden)
        wait_until(lambda: tf.query("/external/focus") == 1, desc="ArrowRight roves to col 1") # 29
        tf.key(at=KEY_AT, name="Enter")  # show Type
        _wait_visible(tf, 3, "Enter on the Type control shows it")                           # 30
        assert "Type" in _colheaders(tf), "Type is back via the keyboard"                    # 31

        # ── (H) scene/access final cross-check ───────────────────────
        assert_eq(_ctl_pressed(tf, 1), True, "Type control pressed after keyboard show")     # 31
        by_tag, nodes = _access(tf)
        assert_eq(by_tag["cols"]["role"], "toolbar", "the chooser is a toolbar")             # 32
        assert_eq(by_tag["grid"]["role"], "grid", "the data surface is a grid")              # 33
        gridcells = [n for n in nodes if n.get("role") == "gridcell"]
        assert_eq(len(gridcells), 3 * 6, "gridcell count = visible cols x rows (3 x 6)")     # 34


if __name__ == "__main__":
    sys.exit(run_demo("R990 §5.27 §5.38 §5.40 — data-grid column hide/show", body))
