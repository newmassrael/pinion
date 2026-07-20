#!/usr/bin/env python3
"""R1397 §5.16 §5.7 — the y-axis AUTO-FITS the brushed x-window, over RPC.

R1381 gave the chart a `rescale_to_visible`: hide a series and the axes snap to
the ones left. R1397 adds the orthogonal leg — `LineChart::rescale_y_to_x_window`:
when the x-domain is brushed narrower than the data, the y-axis re-fits to just
the points INSIDE that window. Pairing it with a brush that `with_x_domain`-zooms
the chart is the canonical "auto-scale Y to the visible X range" of a monitoring
chart (an oscilloscope zoom): zoom the time axis, and the value axis follows.

`examples/hello-autoscale-y` charts one signal — a large startup transient
(x=2, y=5000) over a long steady state of small ripples (y around 60). Seen
whole, the transient owns the y-axis and the ripples are a flat line pinned to
the bottom. The brush is the PRIMARY `RangeSliderExternal` (RPC `/external/low`
/ `/external/high`); dragging it past the transient re-domains x AND fits the
y-axis to the window, so the ripples expand to fill the plot.

The proof is geometry-as-data (§2 #7), driven by an AI agent (§2 #2), no OCR:

  (A) boot (full span) — the y-axis reaches a kilo magnitude (top tick 5k), and
      the rightmost sample sits near the bottom of the plot (ripples flattened).
  (B) brush past the transient (x >= ~8) — the y-tick labels fall from thousands
      to tens (no 'k' label survives), the rightmost sample LIFTS far up the
      plot, and the polyline's vertical band widens to fill it.
  (C) a mid window keeps the fit (tens, still zoomed on x).
  (D) reset to the full span — the transient owns the y-axis again.

Run from the workspace root:
    cargo build -p hello-autoscale-y --release
    python3 tools/demos/r1397_autoscale_y.py

>= 30 assertions.
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

VIEWPORT = (760, 460)
# The brush is the PRIMARY external (empirically `/external/...`, the r739
# single-external shape — a sibling brush would be `/<tag>/external/...`).
BRUSH = "/external"


def paint(tf):
    return tf.snapshot(source="paint", viewport=VIEWPORT)


def _tick_value(text: str) -> float | None:
    """Parse a chart y-tick label ('0', '2k', '1.5M') into its magnitude."""
    raw = (text or "").strip()
    mul = 1.0
    for suffix, factor in (("k", 1_000.0), ("M", 1_000_000.0), ("G", 1_000_000_000.0)):
        if raw.endswith(suffix):
            raw, mul = raw[: -len(suffix)], factor
            break
    try:
        return abs(float(raw.strip())) * mul
    except ValueError:
        return None


def y_tick_values(snap) -> list[float]:
    """Every numeric y-tick label magnitude currently on the chart."""
    out: list[float] = []
    for k in range(12):
        node = find_by_tag(snap, f"chart.label.y.{k}")
        if node is None:
            continue
        v = _tick_value(node.get("content"))
        if v is not None:
            out.append(v)
    return out


def max_y_tick(snap) -> float:
    vals = y_tick_values(snap)
    assert vals, "the chart has numeric y-tick labels"
    return max(vals)


def _series_path(snap) -> dict:
    node = find_by_tag(snap, "chart.series.0")
    assert node is not None, "the signal polyline is present"
    return node


def series_vertices(snap) -> list[tuple[float, float]]:
    """The signal polyline's vertices in WINDOW px (rect origin + command)."""
    p = _series_path(snap)
    ox, oy = p["rect"]["x"], p["rect"]["y"]
    out: list[tuple[float, float]] = []
    for c in p["commands"]:
        if c["type"] in ("MoveTo", "LineTo"):
            out.append((ox + c["point"]["x"], oy + c["point"]["y"]))
    return out


def last_vertex_wy(snap) -> float:
    """Window-y of the rightmost sample (x=40) — present in every window, so
    the same data point can be compared across two y-domains."""
    return series_vertices(snap)[-1][1]


def y_band(snap) -> float:
    ys = [wy for _, wy in series_vertices(snap)]
    return max(ys) - min(ys)


def x_span(snap) -> float:
    xs = [wx for wx, _ in series_vertices(snap)]
    return max(xs) - min(xs)


def set_brush(tf, low: float, high: float) -> None:
    # Set high first, then low (the r738 order — a range never crosses itself).
    tf.intervene(f"{BRUSH}/high", high)
    tf.intervene(f"{BRUSH}/low", low)
    wait_until(
        lambda: abs(tf.query(f"{BRUSH}/low") - low) < 0.02
        and abs(tf.query(f"{BRUSH}/high") - high) < 0.02,
        desc=f"brush -> [{low}, {high}]",
    )


def body() -> None:
    with RpcSubprocess("hello-autoscale-y", boot_grace=1.5) as tf:
        # ── (A) boot — full span, the transient owns the y-axis ──────────────
        snap = paint(tf)
        assert find_by_tag(snap, "chart") is not None, "the chart root"
        assert find_by_tag(snap, "signal_brush") is not None, "the brush strip"
        assert abs(tf.query(f"{BRUSH}/low") - 0.0) < 0.02, "boot brush low = 0"
        assert abs(tf.query(f"{BRUSH}/high") - 1.0) < 0.02, "boot brush high = 1"

        boot_top = max_y_tick(snap)
        assert boot_top >= 1000.0, f"boot y-axis reaches a kilo magnitude, got {boot_top}"
        boot_ticks = y_tick_values(snap)
        assert len(boot_ticks) >= 5, f"a full y-axis of ticks, got {boot_ticks}"
        assert 5000.0 in boot_ticks, f"the top tick is the transient (5k), got {boot_ticks}"
        assert 0.0 in boot_ticks, f"the baseline tick is 0, got {boot_ticks}"
        assert any(
            (find_by_tag(snap, f"chart.label.y.{k}") or {}).get("content", "").endswith("k")
            for k in range(12)
        ), "boot y-axis carries a 'k' (kilo) tick label"
        # The full domain plots every sample: MoveTo + 40 LineTo = 41 vertices.
        boot_verts = series_vertices(snap)
        assert_eq(len(boot_verts), 41, "the full domain plots every sample")
        # The transient sample (x=2, y=5000) reaches the very top of the plot.
        plot_top = _series_path(snap)["rect"]["y"]
        assert min(wy for _, wy in boot_verts) < plot_top + 12.0, (
            "the transient spikes to the top of the plot"
        )

        boot_last = last_vertex_wy(snap)
        boot_band = y_band(snap)
        boot_bottom = _series_path(snap)["rect"]["y"] + _series_path(snap)["rect"]["h"]
        # The rightmost ripple sits in the lower quarter of the plot (flattened).
        assert boot_last > boot_bottom - 0.25 * _series_path(snap)["rect"]["h"], (
            f"boot: the ripple sample sits near the bottom ({boot_last} vs bottom {boot_bottom})"
        )
        # The transient's polyline spans nearly the whole plot (spike at top,
        # ripples at the bottom), so there is no vertical room for ripple detail.
        assert boot_band > 200.0, (
            f"boot: the transient owns the plot height, leaving the ripples no "
            f"room (band {boot_band:.1f})"
        )

        # ── (B) brush past the transient (x >= ~8) — the y-axis fits ─────────
        set_brush(tf, 0.2, 1.0)
        snap = paint(tf)
        fit_top = max_y_tick(snap)
        assert fit_top < 200.0, f"brushed y-axis fits the ripples (tens), got {fit_top}"
        assert not any(
            (find_by_tag(snap, f"chart.label.y.{k}") or {}).get("content", "").endswith("k")
            for k in range(12)
        ), "no 'k' tick label survives the fit"
        # The axis shrank by a large factor — the whole point.
        assert boot_top / fit_top > 10.0, (
            f"the y-axis shrank by >10x (from {boot_top} to {fit_top})"
        )

        fit_last = last_vertex_wy(snap)
        assert fit_last < boot_last - 30.0, (
            f"the x=40 sample lifts up the plot under the fitted axis "
            f"(boot {boot_last} -> fit {fit_last})"
        )
        # The transient is x-clipped out of the window, so the polyline plots
        # fewer samples than the full domain did.
        assert len(series_vertices(snap)) < 41, (
            "the off-window transient is clipped out of the brushed polyline"
        )
        # Now the transient is gone from the window, the ripples alone span a
        # wide vertical band — the detail the fit exists to reveal. (Boot's band
        # was wider only because the off-axis SPIKE stretched the polyline; the
        # max_y_tick drop + the x=40 lift are what prove the ripples resolved.)
        fit_band = y_band(snap)
        assert fit_band > 80.0, (
            f"the fitted ripples span a wide vertical band of the plot, got {fit_band:.1f}"
        )
        # x is still zoomed to fill the plot width.
        assert x_span(snap) > 0.8 * _series_path(snap)["rect"]["w"], (
            "the brushed series still spans the plot width (x-zoom intact)"
        )

        # ── (C) a mid window keeps the fit ───────────────────────────────────
        set_brush(tf, 0.4, 0.7)
        snap = paint(tf)
        assert abs(tf.query(f"{BRUSH}/low") - 0.4) < 0.02, "mid brush low round-trips"
        assert abs(tf.query(f"{BRUSH}/high") - 0.7) < 0.02, "mid brush high round-trips"
        mid_top = max_y_tick(snap)
        assert mid_top < 200.0, f"mid window still fits the ripples, got {mid_top}"
        assert x_span(snap) > 0.7 * _series_path(snap)["rect"]["w"], (
            "the mid window is still x-zoomed to fill the plot"
        )

        # ── (D) a narrow tail window — still fitted, not collapsed ───────────
        set_brush(tf, 0.6, 0.95)
        snap = paint(tf)
        tail_top = max_y_tick(snap)
        assert tail_top < 200.0, f"tail window fits the ripples, got {tail_top}"
        assert y_band(snap) > 20.0, "the tail window's ripples still have vertical extent"

        # ── (E) reset to the full span — the transient owns the axis again ───
        set_brush(tf, 0.0, 1.0)
        snap = paint(tf)
        reset_top = max_y_tick(snap)
        assert_eq(reset_top, boot_top, "reset y-axis returns to the boot magnitude")
        assert reset_top >= 1000.0, "reset y-axis reaches the transient again"
        reset_last = last_vertex_wy(snap)
        assert abs(reset_last - boot_last) < 3.0, (
            f"the ripple sample drops back to the bottom (boot {boot_last} -> reset {reset_last})"
        )


if __name__ == "__main__":
    sys.exit(run_demo("R1397 y-axis auto-fits the brushed x-window", body))
