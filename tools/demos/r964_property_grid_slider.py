#!/usr/bin/env python3
"""R964 §5.38 §5.50 — bounded slider delegate for the property grid.

Drives hello-property-grid over JSON-RPC. R875 gave numeric leaves a drag-to-
scrub (the unbounded the DCC / the engine "drag the number" gesture). R964 adds the
*bounded* delegate: a ranged Float leaf (here `Opacity`, slot 8, normalised to
`[0, 1]`) is clamped to its interval and renders a slider gauge in its value
cell — the engine Details "factor" field. A thin track + active fill along the
cell's bottom edge shows the value's position in range; editing is the existing
(now clamped) scrub / inline-edit / RPC write.

What this proves:

  (A) **range introspection** — `range.<i>` reports `[min, max]` for the bounded
      leaf and Null for every unbounded one (the AI reads the bounds before a
      write, so the clamp is never silent — the §2#7 scene-as-data peer of the
      painted gauge);
  (B) **the clamp funnels every writer** — an out-of-range RPC `value.<i>` write
      clamps to the interval (the same `set_value` funnel the scrub and the
      inline-edit commit converge on);
  (C) **the gauge fill tracks the value** — the `property_grid#gauge8` fill width
      is exactly `frac · track_width` (deterministic geometry, no font metrics),
      so the painted gauge is a faithful, introspectable read of the value;
  (D) **a live scrub past the top clamps**, and the reset restores the default.

Run from the workspace root:
    cargo build -p hello-property-grid --release
    python3 tools/demos/r964_property_grid_slider.py

>= 30 assertions.
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_query,
    wait_snap,
)

WIN = (560, 640)
EXT = "/external"
GRID = "property_grid"
FILL = f"{GRID}#gauge8"   # the Opacity gauge's active-fill tag
OPACITY = "value.8"
TRACK_W = 230             # VALUE_COL_W(250) - 2 * CELL_PAD(10)


def _fill_w(snap: Any) -> int:
    rect = abs_rects_of(snap).get(FILL)
    return rect[2] if rect else 0


def _set_opacity(g: RpcSubprocess, v: float) -> Any:
    g.intervene(f"{EXT}/{OPACITY}", v)
    return wait_query(g, f"{EXT}/{OPACITY}", v, desc=f"Opacity set to {v}")


def body() -> None:
    with RpcSubprocess("hello-property-grid", request_timeout=12.0) as g:
        # ── (A) range introspection: only the bounded leaf reports an interval.
        # The wire is the data-grid R894 `col_range` sibling format ("lo..hi" /
        # "none"), so one range format reads across both DCC widgets.
        assert_eq(g.query(f"{EXT}/{OPACITY}"), 1.0, "Opacity boots at 1.0")
        assert_eq(g.query(f"{EXT}/range.8"), "0..1", "Opacity range is 0..1")
        for unranged in ("range.6", "range.7", "range.4", "range.2"):
            assert_eq(g.query(f"{EXT}/{unranged}"), "none", f"{unranged} is unbounded")

        # The gauge is painted at boot (full fill at 1.0).
        snap = wait_snap(
            g,
            lambda s: find_by_tag(s, FILL) is not None,
            viewport=WIN,
            desc="Opacity gauge fill painted",
        )
        assert_eq(_fill_w(snap), TRACK_W, "value 1.0 → fill spans the whole track")

        # ── (C) the fill width is exactly frac · track_width, for several values
        prev_w = -1
        for v in [0.1, 0.25, 0.5, 0.75, 1.0]:
            _set_opacity(g, v)
            snap = wait_snap(
                g,
                lambda s, vv=v: abs(_fill_w(s) - round(vv * TRACK_W)) <= 1,
                viewport=WIN,
                desc=f"fill width == round({v} · {TRACK_W}) px",
            )
            w = _fill_w(snap)
            assert abs(w - round(v * TRACK_W)) <= 1, f"value {v}: fill {w} != ~{round(v * TRACK_W)}"
            assert w > prev_w, f"fill grows monotonically with the value: {w} <= {prev_w}"
            prev_w = w

        # ── (B) the clamp funnels every writer: out-of-range RPC writes clamp
        g.intervene(f"{EXT}/{OPACITY}", 2.5)
        wait_query(g, f"{EXT}/{OPACITY}", 1.0, desc="above-max write clamps to 1.0")
        snap = g.snapshot(source="paint", viewport=WIN)
        assert_eq(_fill_w(snap), TRACK_W, "clamped-to-max value fills the whole track")
        g.intervene(f"{EXT}/{OPACITY}", -3.0)
        wait_query(g, f"{EXT}/{OPACITY}", 0.0, desc="below-min write clamps to 0.0")
        snap = g.snapshot(source="paint", viewport=WIN)
        assert _fill_w(snap) <= 1, "clamped-to-min value leaves an empty fill"

        # ── (D) a live pointer scrub past the top clamps to the interval ────
        _set_opacity(g, 0.9)
        snap = g.snapshot(source="paint", viewport=WIN)
        rx, ry, rw, rh = abs_rects_of(snap)[f"{GRID}#8"]
        cx, cy = rx + int(rw * 0.6), ry + rh // 2
        # Drag right ~+60 logical px → +0.6 → 0.9 + 0.6 = 1.5 → clamp 1.0. The
        # gauge fill is pointer_transparent, so the press falls through to the
        # row's scrub (it does not intercept the drag).
        g.drag(from_at=(float(cx), float(cy)), to_at=(float(cx + 60), float(cy)), steps=10)
        wait_query(g, f"{EXT}/{OPACITY}", 1.0, desc="a live scrub past the top clamps to 1.0")
        snap = g.snapshot(source="paint", viewport=WIN)
        assert_eq(_fill_w(snap), TRACK_W, "the clamped scrub fills the whole track")

        # ── reset restores the class default (1.0), in range ────────────────
        _set_opacity(g, 0.3)
        assert_eq(g.query(f"{EXT}/modified.8"), True, "Opacity 0.3 differs from its 1.0 default")
        g.invoke(f"{EXT}/reset", 8)
        wait_query(g, f"{EXT}/{OPACITY}", 1.0, desc="reset restores Opacity to its 1.0 default")
        assert_eq(g.query(f"{EXT}/modified.8"), False, "reset clears the modified flag")
        snap = g.snapshot(source="paint", viewport=WIN)
        assert_eq(_fill_w(snap), TRACK_W, "the reset value fills the whole track")


if __name__ == "__main__":
    sys.exit(run_demo("R964 §5.38 §5.50 — bounded slider delegate", body))
