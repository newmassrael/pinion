#!/usr/bin/env python3
"""R1406 §5.35 — a LineChart inspector crosshair that follows the BARE hover.

The 2nd consumer of the R1405 `External::wants_hover_move` seam (the first is
R1405's `HyperlinkOracle`, a TextGrid cell highlighter). A plain hover over the
plot — no button held — drives `wants_hover_move` end-to-end through the router,
which forwards the pointer position to the chart's `CrosshairExternal`; the view
feeds the stored x fraction to `LineChart::inspect`, so the crosshair / per-series
markers / value tooltip track the cursor with NO press:

  * hovering the plot lights a vertical crosshair (`scene/snapshot` reports the
    `chart.inspect.crosshair` / `.header` / `.marker.{i}` nodes — no pixel);
  * the header snaps to the nearest data x (`x = 0` at the far left, `x = 11`
    at the far right) and the readout names each series;
  * moving WITHIN the plot forwards each position (the R1405 within-target
    forward), so a second hover at a different x moves the crosshair;
  * hovering OFF the plot fires `Leave`, which clears the crosshair — it is a
    hover affordance, alive only while the pointer is over the plot;
  * `scene/intervene /external/x_frac` drives the crosshair no-pixel (the
    AI-first path), and rejects a fraction outside 0.0..=1.0.

Run from the workspace root:
    cargo build -p hello-crosshair --release
    python3 tools/demos/r1406_crosshair.py
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

WIN = (720, 420)
PLOT = "plot"
EXT = "/external"

# Mirror of the binding's CHART_RECT = Rect::new(16, 48, WIN_W - 32, WIN_H - 104).
RECT_X, RECT_Y, RECT_W, RECT_H = 16, 48, WIN[0] - 32, WIN[1] - 104


def hover_at(frac: float) -> tuple[float, float]:
    """The window pixel at fraction `frac` across the plot, vertically centred.

    The crosshair surface covers the plot rect exactly, so the router's `x_rel`
    equals `frac` — what the external stores as `x_frac`.
    """
    return (RECT_X + frac * RECT_W, RECT_Y + RECT_H / 2)


def assert_near(actual, expected: float, label: str, tol: float = 0.03) -> None:
    if actual is None or abs(float(actual) - expected) > tol:
        raise AssertionError(f"{label}: expected ~{expected}, got {actual}")
    print(f"  ok: {label} (~{expected})")


def has_crosshair(snap) -> bool:
    return find_by_tag(snap, "chart.inspect.crosshair") is not None


def header(snap):
    node = find_by_tag(snap, "chart.inspect.header")
    return node.get("content") if node else None


def readout(snap) -> str:
    node = find_by_tag(snap, "crosshair.readout")
    return node.get("content", "") if node else ""


def body() -> None:
    with RpcSubprocess("hello-crosshair") as tf:
        snap = wait_snap(
            tf,
            lambda s: find_by_tag(s, PLOT) is not None,
            source="paint",
            viewport=WIN,
            desc="plot surface resolved",
        )

        # --- boot: nothing hovered, no crosshair ---
        assert_eq(tf.query(f"{EXT}/x_frac"), None, "boot: no hover fraction")
        assert_eq(tf.query(f"{EXT}/has_crosshair"), False, "boot: no crosshair")
        assert not has_crosshair(snap), "boot: no crosshair node in the scene"
        assert "hover the plot" in readout(snap), "boot: the idle prompt is shown"

        # --- REAL hover at the far left: drives wants_hover_move end-to-end.
        #     The fraction snaps the header to the first bucket (x = 0). ---
        tf.hover(at=hover_at(0.02))
        left = wait_snap(
            tf,
            has_crosshair,
            source="paint",
            viewport=WIN,
            desc="hovering the plot lights a crosshair",
        )
        assert_near(tf.query(f"{EXT}/x_frac"), 0.02, "left hover fraction")
        assert_eq(tf.query(f"{EXT}/has_crosshair"), True, "crosshair is now live")
        assert has_crosshair(left), "the crosshair node is in the scene"
        assert header(left).startswith("x = 0"), f"left snaps to x=0, got {header(left)!r}"
        assert find_by_tag(left, "chart.inspect.marker.0") is not None, "series 0 marker"
        assert find_by_tag(left, "chart.inspect.marker.1") is not None, "series 1 marker"
        assert find_by_tag(left, "chart.inspect.marker.2") is None, "only two series"
        left_readout = readout(left)
        assert left_readout.startswith("x = 0"), f"readout names x, got {left_readout!r}"
        assert "requests" in left_readout, "readout names the requests series"
        assert "errors" in left_readout, "readout names the errors series"

        # --- hover the far right: header snaps to the last bucket (x = 11) ---
        tf.hover(at=hover_at(0.98))
        right = wait_snap(
            tf,
            lambda s: (header(s) or "").startswith("x = 11"),
            source="paint",
            viewport=WIN,
            desc="hovering right snaps to x=11",
        )
        assert_near(tf.query(f"{EXT}/x_frac"), 0.98, "right hover fraction")
        assert has_crosshair(right), "crosshair still drawn on the right"
        assert header(right).startswith("x = 11"), f"right snaps to x=11, got {header(right)!r}"

        # --- move WITHIN the plot (the R1405 within-target forward): a second
        #     hover at a different x moves the crosshair, no Enter to ride on. ---
        tf.hover(at=hover_at(0.35))
        wait_snap(
            tf,
            lambda s: abs(float(tf.query(f"{EXT}/x_frac")) - 0.35) < 0.03,
            source="paint",
            viewport=WIN,
            desc="a within-plot move forwards the new position",
        )
        assert_near(tf.query(f"{EXT}/x_frac"), 0.35, "first within-plot fraction")
        tf.hover(at=hover_at(0.65))
        mid = wait_snap(
            tf,
            lambda s: abs(float(tf.query(f"{EXT}/x_frac")) - 0.65) < 0.03,
            source="paint",
            viewport=WIN,
            desc="a second within-plot move moves the crosshair again",
        )
        assert_near(tf.query(f"{EXT}/x_frac"), 0.65, "second within-plot fraction")
        assert has_crosshair(mid), "the crosshair tracks the within-plot move"

        # --- hover OFF the plot -> Leave clears the crosshair ---
        tf.hover(at=(360.0, 12.0))
        gone = wait_snap(
            tf,
            lambda s: tf.query(f"{EXT}/x_frac") is None,
            source="paint",
            viewport=WIN,
            desc="hovering off the plot clears the crosshair",
        )
        assert_eq(tf.query(f"{EXT}/x_frac"), None, "leaving the plot cleared the fraction")
        assert_eq(tf.query(f"{EXT}/has_crosshair"), False, "no crosshair off the plot")
        assert not has_crosshair(gone), "the crosshair node is gone"

        # --- AI-first, no-pixel: intervene the fraction; the crosshair renders ---
        tf.intervene(f"{EXT}/x_frac", 0.5)
        drawn = wait_snap(
            tf,
            has_crosshair,
            source="paint",
            viewport=WIN,
            desc="intervening the fraction draws the crosshair (no pixel)",
        )
        assert_near(tf.query(f"{EXT}/x_frac"), 0.5, "intervened fraction")
        assert_eq(tf.query(f"{EXT}/has_crosshair"), True, "intervene lit the crosshair")
        assert has_crosshair(drawn), "the intervened crosshair is in the scene"
        assert header(drawn) is not None, "the intervened crosshair has a header"
        tf.intervene(f"{EXT}/x_frac", None)
        assert_eq(tf.query(f"{EXT}/x_frac"), None, "intervene Null cleared the crosshair")

        # --- the fraction contract: out of range and read-only are rejected ---
        try:
            tf.intervene(f"{EXT}/x_frac", 1.5)
            raise AssertionError("a fraction past 1.0 must be rejected")
        except Exception as exc:  # noqa: BLE001 — the RPC error is the assertion
            assert "1.5" not in str(tf.query(f"{EXT}/x_frac") or ""), "still clear"
            print(f"  ok: out-of-range fraction rejected ({type(exc).__name__})")
        try:
            tf.intervene(f"{EXT}/has_crosshair", True)
            raise AssertionError("the derived flag must be read-only")
        except Exception as exc:  # noqa: BLE001
            print(f"  ok: has_crosshair is read-only ({type(exc).__name__})")

        # --- recovery: a real hover after all that still works ---
        tf.hover(at=hover_at(0.5))
        back = wait_snap(
            tf,
            has_crosshair,
            source="paint",
            viewport=WIN,
            desc="a real hover recovers after the intervene round-trip",
        )
        assert has_crosshair(back), "the crosshair recovers on a real hover"
        assert_eq(tf.query(f"{EXT}/has_crosshair"), True, "recovered crosshair is live")


if __name__ == "__main__":
    sys.exit(run_demo("r1406_crosshair", body))
