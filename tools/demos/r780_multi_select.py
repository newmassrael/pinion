#!/usr/bin/env python3
"""R780 §5.40 — multi-select index model (Shift-range / Ctrl+A / Ctrl+Space).

Drives the `hello-multi-select` binding via JSON-RPC. It extends the R746
`VirtualSelectExternal` single-select coordinator into a Qt
`QItemSelectionModel` `ExtendedSelection` analogue: a selection *set* of
data indices with an `anchor` for range extension, over the unchanged R774
`view_flex_virtual_list` AutoSizer windowing.

The decisive witness is scene-as-data (§2 #7): the keyboard grows / shrinks
an arbitrary index *set*, and the full set survives even though almost none
of its members were ever materialized — `query("selection")` reports the
whole span after a `Shift`-extend, and `Ctrl+A` reports all 10,000.

  (A) boot — mode "multi", empty selection, no anchor; small window.
  (B) plain nav — Arrow/Home/End move the active row and *replace* the
      selection with just it (and reset the anchor), exactly as R776.
  (C) Shift-range — Shift+nav extends the contiguous range from the anchor.
  (D) Ctrl+A — select all 10,000; a plain move then collapses back to one.
  (E) Ctrl+Space — toggle the active row's membership, rest intact.
  (F) RPC funnel — invoke toggle / extend_to / select_all / clear / select
      and intervene the whole set; keyboard and RPC are one funnel.
  (G) scroll-into-view at scale — Shift+End extends the span to row 9,999
      and the offset jumps so it becomes a rendered node.

  Phase 2 — PIXELS (PINION_SCREENSHOT): the windowed rows really paint
      (zebra tones distinct). The multi-Accent selection visual (several
      rows Accent at once) is pixel-proven by the unit
      `several_selected_rows_paint_accent_at_once`; the boot frame has no
      selection by design (mirrors r776_virtual_nav).
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
    find_by_tag,
    read_png_rgba8,
    run_demo,
    runs_of,
    sample_png_points,
    selection_rows,
    wait_query,
    wait_until,
)

EXAMPLE = "hello-multi-select"
WIN = (380, 480)
N = 10_000
PITCH = 32
SCROLL_TAG = "vlist_scroll"
LIST_TAG = "vlist"


def present_rows(snap) -> set[int]:
    out: set[int] = set()
    for tag in abs_rects_of(snap):
        if tag.startswith("vlist#"):
            out.add(int(tag.split("#", 1)[1]))
    return out


def scroll_offset(snap) -> int:
    node = find_by_tag(snap, SCROLL_TAG)
    assert node is not None, "scroll node present"
    return int(node.get("offset_y", -1))


def selection(d) -> list[int]:
    """The coordinator's selected ROWS, ascending.

    R1561 — the slot answers with the RUNS the selection is made of, so this
    goes through the shared `selection_rows` decoder rather than a private one.
    Assertions about the representation itself (a whole-model span is ONE run)
    read the raw answer, because that is the property, not an encoding detail.
    """
    return selection_rows(d.query("/external/selection"))


def cursor(d):
    """The active row index (the `selected` / currentIndex slot)."""
    return d.query("/external/selected")


def mod_key(tf, name: str, *, shift: bool = False, ctrl: bool = False) -> None:
    """Send one `scene/key` with held modifiers, then release them — the
    R763 `scene/modifiers` press/release pair the harness documents."""
    tf.modifiers(shift=shift, ctrl=ctrl)
    tf.key(path=LIST_TAG, name=name)
    tf.modifiers()  # release so the next plain key is unmodified


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        snap = tf.snapshot(source="paint", viewport=WIN)

        # ── (A) boot: multi mode, empty set, no anchor ──────────────
        rects = abs_rects_of(snap)
        assert LIST_TAG in rects, "list container present at boot"
        assert_eq(scroll_offset(snap), 0, "boot offset is 0")
        assert_eq(tf.query("/external/mode"), "multi", "coordinator is in multi-select mode")
        assert_eq(selection(tf), [], "nothing selected at boot")
        assert_eq(cursor(tf), None, "no active row at boot")
        assert_eq(tf.query("/external/anchor"), None, "no anchor at boot")
        assert_eq(tf.query("/external/item_count"), N, "coordinator knows full dataset size")
        boot_rows = present_rows(snap)
        assert len(boot_rows) < 30, f"virtualized: small window, got {len(boot_rows)} of {N}"
        assert 0 in boot_rows, "row 0 rendered at the top"

        # ── (B) plain nav: move + replace (single-select-style) ─────
        tf.request("focus/set", {"tag": LIST_TAG})
        wait_until(
            lambda: tf.request("focus/get").result.get("focused") == LIST_TAG,
            desc="list focused for keyboard nav",
        )
        tf.key(path=LIST_TAG, name="ArrowDown")  # from nothing → row 0
        wait_query(tf, "/external/selection", runs_of([0]),
                   desc="first ArrowDown selects just row 0")
        tf.key(path=LIST_TAG, name="ArrowDown")
        tf.key(path=LIST_TAG, name="ArrowDown")
        wait_query(tf, "/external/selection", runs_of([2]),
                   desc="plain nav replaces — never accumulates")
        assert_eq(cursor(tf), 2, "active row is 2")
        assert_eq(tf.query("/external/anchor"), 2, "plain move sets the anchor to 2")

        # ── (C) Shift-range: extend the contiguous span from anchor ─
        mod_key(tf, "ArrowDown", shift=True)
        wait_query(tf, "/external/selection", runs_of([2, 3]),
                   desc="Shift+Down extends the range from anchor 2")
        mod_key(tf, "ArrowDown", shift=True)
        wait_query(tf, "/external/selection", runs_of([2, 3, 4]),
                   desc="Shift+Down grows the span")
        assert_eq(tf.query("/external/anchor"), 2, "the anchor stays put while extending")
        assert_eq(cursor(tf), 4, "the navigated end is the active row")
        mod_key(tf, "ArrowUp", shift=True)
        wait_query(tf, "/external/selection", runs_of([2, 3]),
                   desc="Shift+Up shrinks the span back toward the anchor")
        # End the anchored span at the bottom: contiguous from 2 to last.
        mod_key(tf, "End", shift=True)
        wait_until(lambda: len(selection(tf)) == N - 2,
                   desc="Shift+End extends the span to the last row")
        assert_eq(cursor(tf), N - 1, "active row is the last")

        # ── (D) Ctrl+A select-all, then a plain move collapses ──────
        mod_key(tf, "a", ctrl=True)
        wait_until(lambda: len(selection(tf)) == N, desc="Ctrl+A selects every row")
        sel = selection(tf)
        assert sel[0] == 0 and sel[-1] == N - 1, "the full span 0..9999 is selected"
        tf.key(path=LIST_TAG, name="Home")  # plain move
        wait_query(tf, "/external/selection", runs_of([0]),
                   desc="a plain move replaces the whole set with one row")

        # ── (E) Ctrl+Space toggles the active row's membership ──────
        mod_key(tf, " ", ctrl=True)
        wait_query(tf, "/external/selection", [],
                   desc="Ctrl+Space toggles the active row 0 OFF")
        mod_key(tf, " ", ctrl=True)
        wait_query(tf, "/external/selection", runs_of([0]),
                   desc="Ctrl+Space toggles row 0 back ON")

        # ── (F) RPC funnel: invoke + intervene the set ──────────────
        assert_eq(tf.invoke("/external/clear", None), None, "clear empties the set")
        assert_eq(selection(tf), [], "cleared")
        assert_eq(tf.invoke("/external/toggle", 5), [[5, 5]], "invoke toggle 5 adds it")
        assert_eq(tf.invoke("/external/toggle", 9), [[5, 5], [9, 9]],
                  "invoke toggle 9 accumulates — two apart rows are two runs")
        assert_eq(tf.invoke("/external/toggle", 5), [[9, 9]],
                  "invoke toggle 5 again removes it")
        # toggling 5 off reset the anchor to 5, so extend_to spans 5..11.
        assert_eq(
            tf.invoke("/external/extend_to", 11),
            [[5, 11]],
            "extend_to replaces with the contiguous span from the anchor (5)",
        )
        # A fresh single toggle re-anchors for a clean range.
        tf.invoke("/external/clear", None)
        assert_eq(tf.invoke("/external/toggle", 9), [[9, 9]], "re-anchor at 9")
        assert_eq(tf.invoke("/external/extend_to", 11), [[9, 11]],
                  "extend_to 11 spans 9..11, as the one run it is")
        assert_eq(tf.invoke("/external/select", 3), 3, "invoke select 3 (single move) returns 3")
        assert_eq(selection(tf), [3], "select replaced the set with one row")
        assert_eq(tf.query("/external/selection"), [[3, 3]],
                  "one row is a run of one, not a bare index")
        assert_eq(tf.invoke("/external/select_all", None), [[0, N - 1]],
                  "invoke select_all picks all — and answers in one run")
        assert_eq(tf.query("/external/selection_count"), N,
                  "the row count is answerable without materialising the rows")
        tf.intervene("/external/selection", [[1, 1], [4, 4], [7, 7], [99_999, 99_999]])
        assert_eq(selection(tf), [1, 4, 7], "intervene replaces the set, out-of-range dropped")
        assert_eq(tf.query("/external/anchor"), 7, "anchor follows the greatest restored index")
        tf.intervene("/external/selection", None)
        assert_eq(selection(tf), [], "intervene null clears the set")

        # ── (G) scroll-into-view at scale (the headline) ────────────
        tf.key(path=LIST_TAG, name="Home")  # anchor = 0
        wait_query(tf, "/external/anchor", 0, desc="Home re-anchored at row 0")
        mod_key(tf, "End", shift=True)  # span the whole dataset
        wait_until(lambda: len(selection(tf)) == N,
                   desc="Shift+End spans all 10,000 from the top anchor")
        snap = tf.snapshot(source="paint", viewport=WIN)
        deep_offset = scroll_offset(snap)
        assert deep_offset > 300_000, f"Shift+End scrolled deep, offset {deep_offset}"
        deep_rows = present_rows(snap)
        assert (N - 1) in deep_rows, "row 9999 is now a RENDERED node (scrolled into view)"
        assert 0 not in deep_rows, "row 0 has left the window entirely"
        assert len(deep_rows) < 30, f"still virtualized at the bottom, {len(deep_rows)} rows"

    # ── Phase 2 — live pixels (boot frame: rows render, zebra) ──────
    snap, rects = _boot_snapshot_and_rects()
    img = read_png_rgba8(capture_screenshot())
    assert (img.width, img.height) == WIN, \
        f"screenshot {img.width}x{img.height} != window {WIN}"

    def row_bg_point(i: int):
        x, y, w, h = rects[f"vlist#{i}"]
        return (x + w - 8, y + h // 2)

    bg0, bg1, bg2 = sample_png_points(img, [row_bg_point(0), row_bg_point(1), row_bg_point(2)])
    assert bg0 != bg1, f"zebra tones differ (rows painted), got {bg0} vs {bg1}"
    assert bg0 == bg2, f"even rows share a tone (zebra parity), got {bg0} vs {bg2}"


def _boot_snapshot_and_rects():
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        snap = tf.snapshot(source="paint", viewport=WIN)
        return snap, abs_rects_of(snap)


def capture_screenshot() -> Path:
    out = Path(tempfile.mkdtemp(prefix="pinion-r780-")) / "multisel.png"
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
    sys.exit(run_demo("R780 §5.40 — multi-select index model", body))
