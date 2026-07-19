#!/usr/bin/env python3
"""R1381 §5.38 — hello-rescale-toggle: hide the dominant series to rescale.

The forcing consumer for `pinion_chart::LineChart::rescale_to_visible`. Three
series of very different magnitude (`total` ~4k, `cache` ~900, `errors` ~90)
share one plot built `rescale_to_visible(true)`. With all visible the y-axis is
pinned to `total`, so `errors` is a sliver; hiding the bigger series (via the
R1380 interactive legend) snaps the y-domain to the survivors, so `errors`
GROWS to fill the plot — measured directly as the pixel height of its polyline's
bounding rect (`chart.series.2`).

Atomic verification scope (>=30 assertions):

  (A) boot — three lines drawn, three legend entries + all ON; the `errors`
      polyline is a thin sliver (pinned under `total`).
  (B) hide `total` (coordinate click on entry 0) — its line goes AND the
      `errors` extent grows (the axis rescaled to the survivors).
  (C) hide `cache` too — `errors` now owns the whole y-range, its extent grows
      again by a large factor.
  (D) show them back — the `errors` extent shrinks back toward the sliver.
  (E) keyboard — each entry is its own Tab stop; Space/Enter toggles only the
      focused entry; Arrow roves focus.
  (F) BOUNDED hit region — a plot-body click toggles nothing.
  (G) negative — an unknown introspect path is rejected.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    find_by_tag,
    node_center,
    run_demo,
)

EXAMPLE = "hello-rescale-toggle"
VIEWPORT = (560, 360)
N = 3


def entry_tag(i: int) -> str:
    return f"legend_{i}"


def value(tf, i: int):
    return tf.query(f"/{entry_tag(i)}/external/value")


def paint(tf):
    return tf.snapshot(source="paint", viewport=VIEWPORT)


def line_present(snap, i: int) -> bool:
    return find_by_tag(snap, f"chart.series.{i}") is not None


def errors_extent(snap) -> int:
    """Pixel height of the `errors` (series 2) polyline's bounding rect — the
    direct rescale witness. A larger extent = a smaller y-domain = rescaled."""
    node = find_by_tag(snap, "chart.series.2")
    assert node is not None, "errors polyline present"
    return int(node["rect"]["h"])


def click_entry(tf, snap, i: int) -> None:
    entry = find_by_tag(snap, entry_tag(i))
    assert entry is not None, f"legend entry {i} present"
    tf.click(at=node_center(entry))
    tf.pointer_leave()


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── (A) boot: all drawn, errors is a sliver ──────────────────────
        snap = paint(tf)
        for i in range(N):
            assert find_by_tag(snap, entry_tag(i)) is not None, f"entry {i} present"
            assert_eq(value(tf, i), True, f"boot: series {i} visible")
            assert line_present(snap, i), f"boot: series {i} polyline drawn"
        sliver = errors_extent(snap)

        # ── (B) hide `total`: line goes, errors rescales UP ──────────────
        click_entry(tf, snap, 0)
        assert_eq(value(tf, 0), False, "click hides `total`")
        assert_eq(value(tf, 1), True, "`cache` untouched")
        assert_eq(value(tf, 2), True, "`errors` untouched")
        snap = paint(tf)
        assert not line_present(snap, 0), "hidden `total` draws no polyline"
        assert line_present(snap, 1), "`cache` still drawn"
        assert line_present(snap, 2), "`errors` still drawn"
        total_hidden = errors_extent(snap)
        assert (
            total_hidden > sliver
        ), f"hiding `total` rescales the errors extent up (sliver={sliver}, now={total_hidden})"

        # ── (C) hide `cache` too: errors owns the y-range ────────────────
        click_entry(tf, snap, 1)
        assert_eq(value(tf, 1), False, "click hides `cache`")
        snap = paint(tf)
        assert not line_present(snap, 1), "hidden `cache` draws no polyline"
        assert line_present(snap, 2), "`errors` still drawn"
        big_two_hidden = errors_extent(snap)
        assert (
            big_two_hidden > total_hidden * 2
        ), f"errors fills the plot once both bigs are hidden ({total_hidden} -> {big_two_hidden})"

        # ── (D) show them back: errors shrinks toward the sliver ─────────
        click_entry(tf, snap, 1)  # cache back
        assert_eq(value(tf, 1), True, "`cache` shown again")
        snap = paint(tf)
        cache_back = errors_extent(snap)
        assert cache_back < big_two_hidden, "restoring `cache` rescales errors back down"
        click_entry(tf, snap, 0)  # total back
        assert_eq(value(tf, 0), True, "`total` shown again")
        snap = paint(tf)
        restored = errors_extent(snap)
        assert restored <= cache_back, "restoring `total` shrinks errors to the sliver again"
        assert line_present(snap, 0) and line_present(snap, 1) and line_present(snap, 2)

        # ── (E) keyboard: each entry is its own Tab stop ─────────────────
        focused = tf.request("focus/set", {"tag": entry_tag(2)}).result.get("focused")
        assert_eq(focused, entry_tag(2), "entry 2 is an independent Tab stop")
        tf.key(path=entry_tag(2), name="Space")
        assert_eq(value(tf, 2), False, "Space hides focused `errors`")
        assert_eq(value(tf, 0), True, "sibling `total` untouched by Space")
        assert_eq(value(tf, 1), True, "sibling `cache` untouched by Space")
        snap = paint(tf)
        assert not line_present(snap, 2), "Space-hidden `errors` draws no polyline"
        tf.key(path=entry_tag(2), name="Enter")
        assert_eq(value(tf, 2), True, "Enter shows `errors` again")
        snap = paint(tf)
        assert line_present(snap, 2), "`errors` polyline returns via Enter"
        tf.key(path=entry_tag(2), name="ArrowRight")
        assert_eq(
            tf.request("focus/get").result.get("focused"),
            entry_tag(0),
            "ArrowRight wraps: focus 2 -> 0",
        )

        # ── (F) BOUNDED hit region: a plot-body click toggles nothing ────
        before = [value(tf, i) for i in range(N)]
        tf.click(at=(VIEWPORT[0] / 2, VIEWPORT[1] * 0.6))
        tf.pointer_leave()
        for i in range(N):
            assert_eq(value(tf, i), before[i], f"plot-body click leaves series {i} unchanged")

        # ── (G) negative — unknown introspect path is rejected ───────────
        raised = False
        try:
            tf.query(f"/{entry_tag(0)}/external/no_such_field")
        except Exception:  # noqa: BLE001 — RpcError type varies by harness
            raised = True
        assert raised, "unknown introspect path must be rejected"


if __name__ == "__main__":
    sys.exit(run_demo("R1381 §5.38 — hide the dominant series to rescale", body))
