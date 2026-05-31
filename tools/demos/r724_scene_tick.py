#!/usr/bin/env python3
"""R724 §5.28 scene/tick demo — deterministic animation time-advance.

Before R724 an AI client could read animated state (theme-fade, springs, caret
blink) but could not *advance the clock on demand*: real-frame ticks fire
non-deterministically between RPC calls, so R723's theme-fade demo had to assert
a time-tolerant ("lighter, not the exact tone") result. `scene/tick {dt}` closes
that gap — it advances the addressed window's animation clock by `dt` seconds
(CoreShell::tick_animations_for_window), the time peer to scene/hover's pointer
peer (§2 invariant #2 + #3 dry-run). The caller-injected dt keeps determinism.

This drives hello-theme's R57.X theme-fade to its *exact* settled palette in
both directions:

  - boot light: inverse_swatch fill is exactly the light inverseSurface (#322F35);
  - toggle dark, scene/tick(1.0) >> the ~200 ms fade: fill settles to the EXACT
    dark inverseSurface (#E6E1E5) and the panel to the EXACT dark surface
    (#121212) — exact, not time-tolerant, because a settled spring returns its
    target verbatim;
  - toggle back light, scene/tick(1.0): fill returns to the EXACT light tone
    (round-trip), proving scene/tick settles the fade reliably both ways.

Run from the workspace root:
    cargo build -p hello-theme --release
    python3 tools/demos/r724_scene_tick.py
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
)

VIEWPORT = (360, 300)

# Mirror the inverse/surface `const`s in crates/pinion-core/src/theme.rs.
INVERSE_SURFACE_LIGHT = (0x32, 0x2F, 0x35)
INVERSE_SURFACE_DARK = (0xE6, 0xE1, 0xE5)
SURFACE_LIGHT = (0xFF, 0xFF, 0xFF)
SURFACE_DARK = (0x12, 0x12, 0x12)


def rgb(c: dict) -> tuple[int, int, int]:
    return (c["r"], c["g"], c["b"])


def swatch_fill(snap) -> tuple[int, int, int]:
    node = find_by_tag(snap, "inverse_swatch")
    assert node is not None, "inverse_swatch present"
    return rgb(node["style"]["fill"])


def approx(actual, expected, label, tol=1):
    for ch, a, e in zip("rgb", actual, expected):
        assert abs(a - e) <= tol, f"{label}: {ch} {a} != {e} (+/-{tol})"


def body() -> None:
    with RpcSubprocess("hello-theme") as d:
        # Boot is light and un-animated — exact light inverseSurface.
        snap = d.snapshot(source="paint", viewport=VIEWPORT)
        assert_eq(swatch_fill(snap), INVERSE_SURFACE_LIGHT, "boot swatch = light inverseSurface")
        assert_eq(rgb(snap["style"]["fill"]), SURFACE_LIGHT, "boot panel = light surface")

        # ── light -> dark, then settle the fade deterministically ────
        d.click(path="theme_toggle")
        d.tick(1.0)  # >> ~200 ms M3 short4 fade
        snap_dark = d.snapshot(source="paint", viewport=VIEWPORT)
        approx(swatch_fill(snap_dark), INVERSE_SURFACE_DARK,
               "after tick: swatch settled to dark inverseSurface")
        approx(rgb(snap_dark["style"]["fill"]), SURFACE_DARK,
               "after tick: panel settled to dark surface")
        # Distinct from the light tone — the fade genuinely crossed.
        assert swatch_fill(snap_dark) != INVERSE_SURFACE_LIGHT, "dark != light tone"

        # ── dark -> light round-trip, settle again ───────────────────
        d.click(path="theme_toggle")
        d.tick(1.0)
        snap_light = d.snapshot(source="paint", viewport=VIEWPORT)
        approx(swatch_fill(snap_light), INVERSE_SURFACE_LIGHT,
               "round-trip: swatch back to exact light inverseSurface")
        approx(rgb(snap_light["style"]["fill"]), SURFACE_LIGHT,
               "round-trip: panel back to exact light surface")

        # ── a zero tick is a well-defined no-op (clock does not move) ─
        before = swatch_fill(d.snapshot(source="paint", viewport=VIEWPORT))
        d.tick(0.0)
        after = swatch_fill(d.snapshot(source="paint", viewport=VIEWPORT))
        assert_eq(after, before, "tick(0.0) leaves a settled palette unchanged")


if __name__ == "__main__":
    sys.exit(run_demo("R724 scene/tick deterministic time-advance", body))
