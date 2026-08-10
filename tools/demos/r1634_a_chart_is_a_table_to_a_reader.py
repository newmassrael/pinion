#!/usr/bin/env python3
"""R1634 — a chart is a table to a reader who does not read pixels.

Every chart in this tree was **one node** to assistive technology: a single
`AccessNode` whose whole content was whatever string the inspect readout
happened to build. "The third series in April" was not addressable, not
navigable, and not announced. The strongest thing this framework draws was, to a
screen-reader user, a sentence.

The references have nothing here, measured: the toolkit's three charting
modules at 6.11 — its 2D charts, the newer graphs module replacing them, and the
3D data visualisation one — contain **no accessibility integration at all**
between them, and a chart view there reaches the platform as one scroll-area
object.
The DCC and the engine have no charting at all. So there was no floor to clear;
the shape had to be argued. It is a **table**, because that is what a chart's
data is (points down, series across), because WAI-ARIA has no chart role, and
because `table` / `row` / `columnheader` / `rowheader` / `cell` are roles every
screen reader already navigates.

What each check discriminates:

* **The chart is not one node any more.** Counting roles, not nodes: a `table`
  with a `row` per point and a `cell` per datum, not a `group` with a string.
* **★ The picture thins and the table does not.** R1633 draws eight names for
  thirty endpoints because thirty will not fit. This asserts the two numbers
  side by side — 8 painted, 30 announced — because a reader who is not reading
  pixels is not subject to a pixel constraint. That contrast IS the round.
* **A cell names its series with its value.** A number announced alone has no
  subject; a reader moving across a row must hear which series each belongs to.
* **A window presents what it drew and declares the whole extent.** Without the
  second half an AT would announce three months where there are twelve — the
  virtualized-list model applied to a chart's own window.
* **The cell points at its mark.** The bounds resolve to the bar, so a touch
  explorer or a magnifier lands on the thing that was drawn rather than on the
  chart.

Run from the workspace root:
    cargo build -p hello-category-axis --release
    python3 tools/demos/r1634_a_chart_is_a_table_to_a_reader.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    assert_eq,
    run_demo,
)

#: The bar chart's tag prefix in `hello-category-axis`.
BARS = "bars"

#: The external the axis mode hangs off.
DENSE = "/category_window/external/dense"

#: The window this example opens at.
WIN = (720, 560)


def access(tf: RpcSubprocess) -> list[dict]:
    return list(tf.request("scene/access", {"viewport": list(WIN)}).result["nodes"])


def of_role(nodes: list[dict], role: str) -> list[dict]:
    return [n for n in nodes if n.get("role") == role]


def painted_labels(tf: RpcSubprocess) -> int:
    rects = abs_rects_of(tf.snapshot(source="paint", viewport=list(WIN)))
    return sum(1 for t in rects if t.startswith(f"{BARS}.xlabel."))


def a_chart_is_a_table(tf: RpcSubprocess) -> None:
    nodes = access(tf)
    tables = of_role(nodes, "table")
    assert_eq(len(tables), 1, "the chart publishes exactly one table")
    table = tables[0]
    assert_eq(table["tag"], BARS, "and it IS the chart, at the chart's own tag")
    assert_eq(table["name"], "Revenue by category")
    assert_eq(table["column_count"], 2, "one series plus the row-header column")
    assert_eq(table["row_count"], 13, "twelve months plus the header row")

    assert_eq(len(of_role(nodes, "row")), 13, "a row per month, and the header")
    assert_eq(len(of_role(nodes, "rowheader")), 12, "each month is named")
    assert_eq(len(of_role(nodes, "cell")), 12, "each month has its datum")
    assert_eq(
        len(of_role(nodes, "columnheader")),
        2,
        "the axis's name and the series' name",
    )
    headers = sorted(n["name"] for n in of_role(nodes, "columnheader"))
    assert_eq(headers, ["Category", "Revenue by category"])
    print(f"[demo] months: 1 table, {len(of_role(nodes, 'row'))} rows, "
          f"{len(of_role(nodes, 'cell'))} cells — not one node with a sentence")


def a_cell_names_its_series_and_points_at_its_mark(tf: RpcSubprocess) -> None:
    nodes = access(tf)
    cells = of_role(nodes, "cell")
    for cell in cells:
        assert cell["name"].startswith("Revenue by category: "), (
            f"★ a number announced alone has no subject: {cell}"
        )
    values = [c["name"].split(": ")[1] for c in cells]
    assert_eq(values[0], "182", "the January revenue, as the chart formats it")

    # The cell resolves its bounds to the BAR it was drawn as, so a touch
    # explorer lands on the mark rather than on the chart.
    boxed = [c for c in cells if c.get("bounds")]
    assert_eq(len(boxed), len(cells), "every cell has a rectangle of its own")
    rects = abs_rects_of(tf.snapshot(source="paint", viewport=list(WIN)))
    first_bar = rects.get(f"{BARS}.bar.0")
    assert first_bar is not None, f"the bar is painted: {sorted(rects)[:8]}"
    bounds = boxed[0]["bounds"]
    assert_eq(
        (bounds["x"], bounds["w"]),
        (first_bar[0], first_bar[2]),
        "★ and the rectangle is the BAR's, not the chart's",
    )
    print(f"[demo] a cell is '{cells[0]['name']}' at the bar's own rect")


def the_picture_thins_and_the_table_does_not(tf: RpcSubprocess) -> None:
    slots = int(str(tf.invoke(DENSE, True)))
    tf.tick(0.016)
    painted = painted_labels(tf)
    nodes = access(tf)
    announced = len(of_role(nodes, "rowheader"))

    assert painted < slots, f"R1633 thinned the picture: {painted} of {slots}"
    assert_eq(
        announced,
        slots,
        "★★ THE ROUND: the canvas drew "
        f"{painted} names because {slots} will not fit, and the table announces "
        "all of them — a reader who is not reading pixels is not subject to a "
        "pixel constraint",
    )
    names = [n["name"] for n in of_role(nodes, "rowheader")]
    assert_eq(names[0], "/health")
    assert_eq(names[-1], "/version", "including the ones no label was drawn for")
    assert_eq(len(of_role(nodes, "cell")), slots, "and every one has its datum")
    print(f"[demo] ★ painted {painted} names, announced {announced} — the "
          "picture thins and the table does not")


def a_window_declares_what_it_is_a_window_onto(tf: RpcSubprocess) -> None:
    tf.invoke(DENSE, False)
    tf.tick(0.016)
    before = of_role(access(tf), "row")

    assert_eq(
        str(tf.invoke("/category_window/external/range", {"from": "Apr", "to": "Jun"})),
        "",
        "the range resolved",
    )
    tf.tick(0.016)
    nodes = access(tf)
    rows = of_role(nodes, "row")
    assert len(rows) < len(before), (
        f"the window is the bound: {len(rows)} < {len(before)}"
    )
    assert_eq(len(of_role(nodes, "rowheader")), 3, "April, May and June")

    table = of_role(nodes, "table")[0]
    assert_eq(
        table["row_count"],
        13,
        "★ and it still claims twelve months — an AT that announced three "
        "would be describing the window as the data",
    )
    windowed = of_role(nodes, "row")
    sized = [r.get("size_of_set") for r in windowed if r.get("size_of_set")]
    assert_eq(
        sorted(set(sized)),
        [12],
        "★ each present row says which of the WHOLE set it is",
    )
    print(f"[demo] windowed: {len(of_role(nodes, 'rowheader'))} rows present, "
          f"{table['row_count'] - 1} declared")


def body() -> None:
    with RpcSubprocess("hello-category-axis", boot_grace=1.5) as tf:
        for _ in range(3):
            tf.tick(0.016)
        a_chart_is_a_table(tf)
        a_cell_names_its_series_and_points_at_its_mark(tf)
        the_picture_thins_and_the_table_does_not(tf)
        a_window_declares_what_it_is_a_window_onto(tf)


if __name__ == "__main__":
    run_demo("R1634 — a chart is a table to a reader", body)
