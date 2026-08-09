#!/usr/bin/env python3
"""R1446 §5.38 model-mapped chart — `hello-model-chart`.

A `LineChart` fed by `pinion_chart::ModelMapper` from a live `CellValue` block
(the toolkit charting module `Q*XYModelMapper` contract). WHICH field is plotted is picker
state, not code: an `x_field` radio group picks the independent variable and a
toggle per column picks the measures.

Proven as DATA through `scene/snapshot` (§2 #7, no pixels):

  - boot: the two seeded measures plot 8 points each, against the record
    ordinal; the legend names them in mapped order.
  - toggling a measure changes the series set without touching the data.
  - **the point of the round** — point an axis at the TEXT column and the chart
    plots NOTHING and the status names the reason (`Month: 8 text cells, not a
    measure`). The toolkit's `toReal()` would draw a flat line on zero here,
    indistinguishable from measured data.
  - an unreadable X is reported ONCE, not once per mapped series (the
    `ModelMapper::map` de-duplication) — asserted with TWO measures mapped, the
    only arrangement that can tell the two behaviours apart.
  - re-pointing x re-domains the axis (the ordinal starts at 0; the `Units`
    column starts at 30), so "the x field changed" is read off the axis rather
    than off the status string alone.
  - §2 #2 data input: `scene/intervene /records/external/value.R.C` writes a
    cell and the plotted series follows it, while a payload of the wrong type
    is REJECTED with its typed reason rather than coerced.

ZERO-FLAKE: every step is a deterministic `scene/click` / `scene/key` /
`scene/intervene`, and every wait is a `wait_snap` predicate on published data
— no wall-clock sleeps.

Run from the workspace root:
    cargo build -p hello-model-chart --release
    python3 tools/demos/r1446_model_chart.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    assert_rpc_error,
    find_by_tag,
    run_demo,
    wait_snap,
)

EXAMPLE = "hello-model-chart"
WIN = (940, 560)

# Mirrors of the binding's declarations.
COL_NAMES = ["Month", "Revenue", "Cost", "Units"]
COL_KINDS = ["text", "float", "float", "int"]
NROWS = 8
X_TAG = "x_field"
Y_TAGS = ["y_field_0", "y_field_1", "y_field_2", "y_field_3"]
STATUS_TAG = "status"
RECORDS = "/records/external"

# The seeded ledger (SSOT: `boot_cells` in the binding).
MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug"]
REVENUE = [120.0, 138.0, 131.0, 155.0, 149.0, 172.0, 168.0, 190.0]
UNITS = [30, 34, 33, 39, 37, 43, 42, 47]


# ── published-state readers ────────────────────────────────────────────────


def status_of(snap) -> str:
    node = find_by_tag(snap, STATUS_TAG)
    assert node is not None, "the status line is in the scene"
    return node["content"]


def series_vertices(snap, i: int) -> int:
    """Vertex count of mapped series `i`, or 0 when it draws no polyline."""
    node = find_by_tag(snap, f"chart.series.{i}")
    if node is None:
        return 0
    return sum(1 for c in node["commands"] if c["type"] in ("MoveTo", "LineTo"))


def legend_names(snap) -> list[str]:
    """The mapped series, in the order the mapper declared them."""
    out = []
    i = 0
    while (node := find_by_tag(snap, f"chart.legend.{i}.label")) is not None:
        out.append(node["content"])
        i += 1
    return out


def axis_ticks(snap, axis: str) -> list[float]:
    """Numeric tick-label values on `axis` ("x" / "y"). `format_si` renders a
    kilo tick as `1k`, so a bare float() would read a grown axis as empty."""
    out = []
    for k in range(12):
        node = find_by_tag(snap, f"chart.label.{axis}.{k}")
        if node is None:
            continue
        raw = node["content"].strip()
        mul = 1.0
        if raw.endswith("k"):
            raw, mul = raw[:-1], 1000.0
        try:
            out.append(float(raw) * mul)
        except ValueError:
            continue
    return out


def records_row(snap, r: int) -> str:
    node = find_by_tag(snap, f"records.row.{r}")
    assert node is not None, f"records.row.{r} is in the scene"
    return node["content"]


def snap_when(tf, pred, desc: str):
    return wait_snap(tf, pred, viewport=WIN, desc=desc)


def snap_with_status(tf, needle: str, desc: str):
    return snap_when(tf, lambda s: needle in status_of(s), desc)


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        # ── boot: two measures against the record ordinal ───────────────────
        snap = snap_with_status(tf, "16 points plotted", "boot: both measures map")
        assert_eq(
            status_of(snap),
            "x = record #  |  y = Revenue, Cost  |  16 points plotted",
            "boot status",
        )
        assert_eq(legend_names(snap), ["Revenue", "Cost"], "boot: mapped series")
        assert_eq(series_vertices(snap, 0), NROWS, "boot: Revenue has a point per record")
        assert_eq(series_vertices(snap, 1), NROWS, "boot: Cost has a point per record")
        assert_eq(series_vertices(snap, 2), 0, "boot: no third series")
        assert "not a measure" not in status_of(snap), "boot: a clean mapping is silent"

        # The chart is drawn from the readout the reader can see.
        assert records_row(snap, 0).startswith("Jan"), "boot: first record"
        assert records_row(snap, NROWS - 1).startswith("Aug"), "boot: last record"

        # An ordinal x domain starts at 0.
        assert_eq(min(axis_ticks(snap, "x")), 0.0, "boot: the ordinal x starts at 0")

        # ── the cell block's RPC surface (§2 #2) ────────────────────────────
        assert_eq(tf.query(f"{RECORDS}/row_count"), NROWS, "row_count")
        assert_eq(tf.query(f"{RECORDS}/col_count"), len(COL_NAMES), "col_count")
        for c, (name, kind) in enumerate(zip(COL_NAMES, COL_KINDS)):
            assert_eq(tf.query(f"{RECORDS}/col_name.{c}"), name, f"col_name.{c}")
            assert_eq(tf.query(f"{RECORDS}/col_kind.{c}"), kind, f"col_kind.{c}")
        assert_eq(tf.query(f"{RECORDS}/value.0.0"), "Jan", "value.0.0 reads the label")
        assert_eq(tf.query(f"{RECORDS}/value.0.1"), REVENUE[0], "value.0.1 reads a measure")
        assert_eq(tf.query(f"{RECORDS}/value.0.3"), UNITS[0], "value.0.3 reads an int")

        # ── toggle a measure off: the series set follows the picker ─────────
        tf.click(path=Y_TAGS[2])  # Cost
        snap = snap_with_status(tf, "y = Revenue  |", "Cost off")
        assert_eq(legend_names(snap), ["Revenue"], "Cost off: one series mapped")
        assert_eq(series_vertices(snap, 0), NROWS, "Cost off: Revenue survives")
        assert_eq(series_vertices(snap, 1), 0, "Cost off: no second polyline")
        assert "8 points plotted" in status_of(snap), "Cost off: half the points"

        # ── THE ROUND: a text column plots nothing, and says why ────────────
        tf.click(path=Y_TAGS[0])  # Month (Text)
        snap = snap_with_status(tf, "not a measure", "Month mapped as a measure")
        assert_eq(
            legend_names(snap), ["Month", "Revenue"], "declaration order is column order"
        )
        assert_eq(
            series_vertices(snap, 0),
            0,
            "the text column draws NOTHING — no zeros are invented (the toolkit draws a flat line here)",
        )
        assert_eq(series_vertices(snap, 1), NROWS, "the real measure is unaffected")
        assert "Month: 8 text cells, not a measure" in status_of(snap), (
            f"the status names column, count and kind: {status_of(snap)}"
        )
        assert "8 points plotted" in status_of(snap), "only Revenue's points plotted"

        # ── and back: the diagnosis is not sticky ───────────────────────────
        tf.click(path=Y_TAGS[0])
        snap = snap_when(
            tf, lambda s: "not a measure" not in status_of(s), "Month un-mapped"
        )
        assert_eq(legend_names(snap), ["Revenue"], "back to one measure")

        # ── re-point x: the axis re-domains, not just the label ─────────────
        tf.click(path=f"{X_TAG}#4")  # Units
        snap = snap_with_status(tf, "x = Units", "x re-pointed at Units")
        assert_eq(series_vertices(snap, 0), NROWS, "a numeric x keeps every record")
        assert "8 points plotted" in status_of(snap), "no record lost"
        assert min(axis_ticks(snap, "x")) >= 25.0, (
            "the x axis now spans the Units column (30..47), not the ordinal: "
            f"{axis_ticks(snap, 'x')}"
        )

        # ── an unreadable X drops every record — reported ONCE ──────────────
        tf.click(path=Y_TAGS[2])  # Cost back on: TWO series mapped
        snap = snap_with_status(tf, "y = Revenue, Cost", "two measures mapped again")
        tf.click(path=f"{X_TAG}#1")  # x = Month (Text)
        snap = snap_with_status(tf, "x = Month", "x pointed at the text column")
        assert_eq(series_vertices(snap, 0), 0, "no x -> no points (series 0)")
        assert_eq(series_vertices(snap, 1), 0, "no x -> no points (series 1)")
        assert "0 points plotted" in status_of(snap), "nothing plotted at all"
        assert_eq(
            status_of(snap).count("Month: 8 text cells, not a measure"),
            1,
            "the bad x column is reported once, not once per mapped series: "
            f"{status_of(snap)}",
        )

        # ── keyboard: the x group is ONE Tab stop with a roving descendant ──
        # `focus/next` is the AI's focus-traversal peer (an injected Tab reaches
        # the focused widget instead of traversing). The strip is the stop — the
        # chips are hit targets only — so exactly one step is needed from boot.
        focused = None
        for _ in range(6):
            focused = tf.request("focus/next").result.get("focused")
            if focused == X_TAG:
                break
        assert_eq(focused, X_TAG, "focus/next reaches the x group (one stop, not five)")

        tf.key(path=X_TAG, name="Home")
        snap = snap_with_status(tf, "x = record #", "Home selects the first x option")
        assert_eq(series_vertices(snap, 0), NROWS, "the ordinal x restores every point")
        tf.key(path=X_TAG, name="ArrowRight")
        snap = snap_with_status(tf, "x = Month", "ArrowRight steps to the next option")
        tf.key(path=X_TAG, name="End")
        snap = snap_with_status(tf, "x = Units", "End jumps to the last option")
        tf.key(path=X_TAG, name="Home")
        snap = snap_with_status(tf, "x = record #", "Home again")

        # ── §2 #2 data input: a written cell moves the plotted series ───────
        before = max(axis_ticks(snap, "y"))
        assert before < 400.0, f"the seeded measures top out in the hundreds: {before}"
        tf.intervene(f"{RECORDS}/value.0.1", 999.0)
        snap = snap_when(
            tf, lambda s: "999" in records_row(s, 0), "the written cell reaches the readout"
        )
        assert_eq(tf.query(f"{RECORDS}/value.0.1"), 999.0, "the write is readable back")
        after = max(axis_ticks(snap, "y"))
        assert after >= 900.0, f"the y axis re-scaled to the written value: {after}"
        assert_eq(series_vertices(snap, 0), NROWS, "the series kept every record")

        # ── a wrong-typed write is REJECTED, never coerced ──────────────────
        assert_rpc_error(
            lambda: tf.intervene(f"{RECORDS}/value.0.1", "1234"),
            data="InterveneTypeMismatch",
        )
        assert_rpc_error(
            lambda: tf.intervene(f"{RECORDS}/value.0.0", 5.0),
            data="InterveneTypeMismatch",
        )
        assert_rpc_error(
            lambda: tf.intervene(f"{RECORDS}/nope", 1.0),
            data="UnknownIntervenePath",
        )
        assert_eq(
            tf.query(f"{RECORDS}/value.0.1"),
            999.0,
            "a rejected write left the cell exactly as it was",
        )


if __name__ == "__main__":
    sys.exit(run_demo("R1446 model-mapped chart (hello-model-chart)", body))
