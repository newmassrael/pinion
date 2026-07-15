#!/usr/bin/env python3
"""R1347 §5.20 §2 #2 §2 #7 — column-resize drag-end commit reaches the binding.

SCOPE. R1347 gave `ColumnResizeExternal` the same `onChangeEnd` channel R1346
gave the splitter: a `"width_committed"` intent carrying the settled column
width, emitted on the `PointerUp` of a drag that actually changed a width.
Before it, the column-resize drag wrote the shared `ColumnWidths` model live
(60 Hz) and the release was silent — so a grid persisting its column layout
(an IDE, sprag's tables) had no settle edge to write from, identical to the
pre-R1346 splitter. `ColumnResizeExternal` is the splitter's structural twin
(same `DragCalibration`, same capture lock, same silent release); this closes
the divergence PR-56's report flagged as `DragCalibration`-family-wide.

This proves the two things crate tests can't:

  1. §2 #2 — the commit is reachable over the REAL RPC wire. `scene/drag`
     synthesizes press -> interpolated moves -> release into the very same
     `InputRouter` capture arm a physical mouse takes.
  2. The `CoreShell::tail` -> `V::update` reducer link. `hello-grid-hscroll`
     now has a real `ghs_ch<col>.width_committed` reducer arm writing a
     committed-width mirror (count + col + width) — the state a grid persists.

Observed as §2 #7 scene-as-data via the `ghs_width_commit_log` witness row.

  (A) Boot — 0 commits.
  (B) A real drag of column 0's border -> EXACTLY ONE commit, and the
      committed width matches the live model's column-0 width.
  (C) A CLICK on the grabber (zero-distance drag) -> still one commit. The
      press-time `pointer_move` (R51.35) arms the calibration, so a gate on
      `DragCalibration::end()`'s bool would emit a spurious persist write for
      a click that resized nothing. Non-tautological: same widget, same verb,
      only the travel differs from (B).
  (D) A second real drag on a DIFFERENT column -> commits advance to two and
      the mirror reports the new column + width.
  (E) Determinism — re-reading changes nothing.

The live FEEL is HW-gated; this pins what is observable as scene-as-data.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path
from typing import Any, Optional

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import RpcSubprocess, run_demo  # noqa: E402

# hello-grid-hscroll is `SizeStrategy::Fixed { 520, 440 }`. MUST match: the
# InputRouter hit-tests the live window, so a snapshot at any other viewport
# reports rects the pointer path does not use.
_MAIN_W = 520
_MAIN_H = 440

_TABLE = "ghs"
_COMMIT_LOG_TAG = "ghs_width_commit_log"
_COLS_EXTERNAL = "/ghs_cols/external"  # the ColumnWidthExternal (query widths)

_LOG_RE = re.compile(r"committed width: (\d+) commits, last col (\d+) = (\d+)px")


def _find_tagged(node: Any, tag: str) -> Optional[dict]:
    if isinstance(node, dict):
        if node.get("tag") == tag:
            return node
        for child in node.get("children") or []:
            hit = _find_tagged(child, tag)
            if hit is not None:
                return hit
    return None


def _commit_log(tf: RpcSubprocess) -> tuple[int, int, int]:
    """`(commit_count, last_col, last_width)` off the witness row (paint scene).

    Read from PAINT (not `state`): the mirror is a view projection of the
    reducer's signal, so a non-zero count proves the intent completed the round
    trip through `V::update`, not merely that the External queued something.
    """
    paint = tf.snapshot(source="paint", viewport=(_MAIN_W, _MAIN_H))
    node = _find_tagged(paint, _COMMIT_LOG_TAG)
    assert node is not None, f"no {_COMMIT_LOG_TAG!r} node in the paint scene"
    text = node.get("content") or node.get("text") or ""
    m = _LOG_RE.search(text)
    assert m is not None, f"unparsable commit log: {text!r}"
    return int(m.group(1)), int(m.group(2)), int(m.group(3))


def _grabber_path(col: int) -> str:
    """`from_path` of column `col`'s resize grabber (`ghs_ch<col>#resize`).

    Targeted by tag, not coordinate: `scene/drag`'s `from_path` resolves against
    the LIVE paint scene the router hit-tests, which carries the grabber even
    though `scene/snapshot` truncates the virtual-table subtree below the root
    container (only `ghs` / `ghs_hscroll` / the witness serialize). A computed
    x would also have to track the h-scroll offset and the grabber moving after
    each resize; the tag is stable and lets the framework do the geometry.
    """
    return f"{_TABLE}_ch{col}#resize"


def _col_width(tf: RpcSubprocess, col: int) -> int:
    # The ColumnWidthExternal answers `width.<col>` with the live width.
    return int(tf.query(f"{_COLS_EXTERNAL}/width.{col}"))


# TableStyle::block_pad — the grid's outer padding; column x starts after it.
_BLOCK_PAD = 8


def _border_x(tf: RpcSubprocess, col: int) -> float:
    """Live x of the border after column `col` (its resize grabber's position).

    `block_pad + Σ widths[0..=col]`, read from the live model so it tracks
    earlier resizes. The demo never scrolls horizontally, so the h-scroll
    offset stays 0 and this is the on-screen x. Kept modest on purpose: dragging
    a column so wide that a later column's header scrolls off-screen would make
    that column's grabber un-hit-testable (correct virtualization, but not what
    this demo is probing).
    """
    return float(_BLOCK_PAD + sum(_col_width(tf, c) for c in range(col + 1)))


def body() -> None:
    with RpcSubprocess("hello-grid-hscroll", boot_grace=2.0) as tf:
        # (A) Boot.
        commits, _, _ = _commit_log(tf)
        assert commits == 0, f"(A) fresh boot must have 0 commits, got {commits}"

        # (B) A real drag of column 0's border, +70px right (widen). Grab the
        # handle by tag; drop 70px to the right of its live border x.
        w0_before = _col_width(tf, 0)
        tf.drag(from_path=_grabber_path(0), to_at=(_border_x(tf, 0) + 70.0, 28.0))
        commits, col, width = _commit_log(tf)
        assert commits == 1, f"(B) one real drag commits once, got {commits}"
        assert col == 0, f"(B) committed column must be 0, got {col}"
        live = _col_width(tf, 0)
        assert abs(live - width) <= 1, (
            f"(B) committed width must match the live model: live={live} committed={width}"
        )
        assert width > w0_before, (
            f"(B) dragging right must widen past {w0_before}px, got {width}"
        )

        # (C) ★ A CLICK on the grabber — press + release, no travel. from_path
        # and to_path resolve to the same live grabber rect, so zero travel.
        tf.drag(from_path=_grabber_path(0), to_path=_grabber_path(0), steps=1)
        commits2, _, width2 = _commit_log(tf)
        assert commits2 == 1, (
            f"(C) a click that resized nothing must NOT commit — count stays 1, "
            f"got {commits2} (the spurious-persist regression)"
        )
        assert width2 == width, f"(C) a click must not rewrite the committed width: {width} -> {width2}"

        # (D) A second real drag, on column 1 (adjacent, stays on-screen).
        w1_before = _col_width(tf, 1)
        tf.drag(from_path=_grabber_path(1), to_at=(_border_x(tf, 1) + 55.0, 28.0))
        commits, col, width = _commit_log(tf)
        assert commits == 2, f"(D) the second drag must commit, got {commits}"
        assert col == 1, f"(D) committed column must be 1, got {col}"
        live = _col_width(tf, 1)
        assert abs(live - width) <= 1, (
            f"(D) mirror must track the live model: live={live} committed={width}"
        )
        assert width > w1_before, f"(D) column 1 must have widened past {w1_before}px, got {width}"

        # (E) Determinism.
        again = _commit_log(tf)
        assert again == (commits, col, width), f"(E) re-read drifted: {again}"

        print(
            f"[demo] ok — 2 real drags committed twice (cols 0, 1), "
            f"1 click committed zero times; last col {col} = {width}px"
        )


if __name__ == "__main__":
    raise SystemExit(run_demo("r1347_column_width_commit", body))
