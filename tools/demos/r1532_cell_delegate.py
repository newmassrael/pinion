#!/usr/bin/env python3
"""R1532 §5.27 §2 #1 §2 #7 — a column declares how its cells are painted.

`GridModel::cell` answers with a `String`, so until R1532 every column of
every pinion grid was a label. A size column could not be a bar, a
visibility column could not be a mark, a swatch column could not be a
swatch. That is the extension point of every Model/View framework — Qt's
`QStyledItemDelegate`, whose documented purpose is precisely "a column
that is not text" — and a DCC / IDE grid is mostly made of such columns.

The evidence that the gap was real and being paid for: `hello-property-grid`
already draws a gauge inside a cell (`ranged_slider_cell`), and it does so
by NOT using the grid's cell path at all — it builds its own row tree.
That is what a missing extension point looks like from the consumer side.

## The shape, and why it is this shape

`VirtualTableData::delegate` is Qt's `setItemDelegateForColumn`: a lookup
from column to painter, resolved **once per painted column** (a column's
delegate cannot vary by row, so resolving per cell would repeat one answer
once per row — the per-section discipline R1530 gave the header axis). A
column with no delegate takes the built-in text painter, reached through
the same call, so the default is not a second path that can drift.

A painter is given a `CellRender` and *returns* a `Scene`. It is handed no
painter object, because §2 #1 forbids an opaque paint callback — which is
what keeps a custom column as introspectable through `scene/snapshot` as a
text one, and is the whole reason this demo can assert what it asserts
without a single pixel.

## What this demo drives

`hello-virtual-table`, a 10,000-row virtualized grid, whose fourth column
`Load` is delegated to a gauge painter. The other three keep the built-in
painter, which is the half that discriminates: a delegate wired for one
column that captured all of them paints a plausible grid and fails only
against the columns it should not have touched.

## Verification scope (>= 30 assertions, sections A-F)

  (A) The grid still is a grid — `columnheader` per column, windowed
      `gridcell`s per row, the full dataset's `setsize`. A new seam in the
      cell path must not cost the properties the cell path already had.
  (B) The delegated column is REACHABLE — its cells carry the same
      composite tags text cells do, so pointer routing and every
      tag-addressed RPC still reach them.
  (C) The delegated column is DIFFERENT — its subtree carries the gauge
      geometry no text cell has, and the value it encodes tracks the row.
  (D) NEGATIVE CONTROL — the undelegated columns are unchanged. This is
      what a delegate claiming every column fails.
  (E) The model's own string is still painted, so the number a sighted
      user reads off the bar and the number `scene/snapshot` reports come
      from one source and cannot disagree.
  (F) It survives virtualization — scrolling re-paints delegated cells for
      the new window, with no leftovers from the old one.
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    run_demo,
)

EXAMPLE = "hello-virtual-table"
TABLE_TAG = "vtbl"
NCOLS = 5
#: The delegated column's absolute index (`Load`).
LOAD_COL = 4
#: `load_bar`'s track height — the gauge wrapper is this tall, which is how
#: this demo tells a bar from R1535's decoration mark (both are untagged
#: empty containers, and `TableStyle::decoration_px` is also 10).
BAR_H = 10
#: The example's synthetic datum for a row, mirrored here so the demo checks
#: the painted text against a value it derived independently rather than
#: against whatever the app happened to say.
def load_percent(row: int) -> int:
    return (row * 7) % 101


#: The window the app boots with, so the paint snapshot is taken at the
#: extent the runtime laid out.
WIN = (470, 480)


def snapshot(tf: RpcSubprocess) -> dict:
    """The **paint** scene, not the state scene.

    `hello-virtual-table`'s root is a `StubExternal`, so the default
    (`source="state"`) snapshot answers with one External node and the grid
    this demo is about is not in it ([[state-scene-vs-paint-scene-introspect]]).
    """
    res = tf.snapshot(source="paint", viewport=WIN)
    assert res, "paint snapshot returned no result"
    return res


def walk(node, out: list) -> None:
    """Every container/text node in document order."""
    if isinstance(node, dict):
        out.append(node)
        for v in node.values():
            walk(v, out)
    elif isinstance(node, list):
        for v in node:
            walk(v, out)


def nodes(tf: RpcSubprocess) -> list:
    out: list = []
    walk(snapshot(tf), out)
    return out


def tagged(all_nodes: list, prefix: str) -> list:
    return [n for n in all_nodes if str(n.get("tag") or "").startswith(prefix)]


def cell_tags(all_nodes: list, col: int) -> list:
    """Composite cell tags for one absolute column: `vtbl#<row>_<col>`."""
    out = []
    for n in tagged(all_nodes, f"{TABLE_TAG}#"):
        parts = str(n["tag"]).split("#", 1)[1].split("_")
        if len(parts) == 2 and parts[1] == str(col):
            out.append(n["tag"])
    return out


def texts_under(node) -> list:
    out: list = []
    walk(node, out)
    return [n["content"] for n in out if n.get("type") == "Text" and "content" in n]


def cell_node(all_nodes: list, row: int, col: int):
    want = f"{TABLE_TAG}#{row}_{col}"
    for n in all_nodes:
        if n.get("tag") == want:
            return n
    return None


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        for _ in range(3):
            tf.tick(0.016)
        ns = nodes(tf)

        # ── (A) it is still a grid ───────────────────────────────────
        a11y = tf.request("scene/access").result
        assert a11y, "scene/access returned no result"
        flat: list = []
        walk(a11y, flat)
        roles = [n.get("role") for n in flat if isinstance(n, dict) and "role" in n]
        assert "grid" in roles, f"the paint root is still a grid, got {set(roles)}"
        headers = roles.count("columnheader")
        assert_eq(headers, NCOLS, "one columnheader per column, including the delegated one")
        gridcells = roles.count("gridcell")
        rows = len(tagged(ns, f"{TABLE_TAG}_row"))
        assert rows > 1, f"premise: the body windowed more than one row, got {rows}"
        assert_eq(
            gridcells,
            rows * NCOLS,
            "one gridcell per column per windowed row — a delegated column is "
            "still a cell of the grid, not an opaque blob",
        )

        # ── (B) the delegated column is reachable ────────────────────
        for col in range(NCOLS):
            tags = cell_tags(ns, col)
            assert_eq(
                len(tags),
                rows,
                f"column {col} paints one tagged cell per windowed row",
            )
        delegated = cell_node(ns, 0, LOAD_COL)
        assert delegated is not None, (
            f"the delegated cell carries `{TABLE_TAG}#0_{LOAD_COL}`, the same "
            f"composite tag a text cell does — a painter that dropped it would "
            f"take the column out of pointer routing and out of every "
            f"tag-addressed RPC"
        )

        # ── (C) the delegated column is different ────────────────────
        # The gauge is a track+fill pair inside the BAR_H-tall wrapper
        # `load_bar` builds; a text cell has none. Compare the delegated
        # cell's subtree against a text one's rather than asserting a
        # constant, so the claim is a difference between columns and not a
        # fact about one.
        #
        # R1535 — identified through the WRAPPER, not as "an untagged empty
        # container". The decoration swatch is one of those too, and
        # `TableStyle::decoration_px` happens to equal `BAR_H`, so the older
        # predicate counted a decorated column's marks as bars and this
        # section's negative control started failing the moment a second
        # column grew a mark. A probe that names a node kind by a property
        # another kind also has is a probe that expires.
        def boxes(node) -> int:
            wrappers = [
                ch
                for ch in (node.get("children") or [])
                if ch.get("type") == "Container"
                and ch.get("children")
                and (ch.get("rect") or {}).get("h") == BAR_H
            ]
            return sum(
                1
                for w in wrappers
                for g in w["children"]
                if g.get("type") == "Container" and not g.get("children")
            )

        text_cell = cell_node(ns, 0, 0)
        assert text_cell is not None, "premise: column 0's cell is in the scene"
        assert_eq(boxes(text_cell), 0, "a text cell draws no gauge geometry")
        assert boxes(delegated) >= 2, (
            f"the delegated cell draws a track and a fill, got "
            f"{boxes(delegated)} — this is the column a text-only grid "
            f"cannot have, and the reason the seam exists"
        )

        # ── (D) NEGATIVE CONTROL: the other columns are unchanged ────
        for col in (0, 1, 2):
            for row in (0, 1):
                n = cell_node(ns, row, col)
                assert n is not None, f"cell {row}_{col} present"
                assert_eq(
                    boxes(n),
                    0,
                    f"column {col} is undelegated and paints as a plain label "
                    f"— a delegate that claimed every column fails exactly here",
                )
                assert len(texts_under(n)) == 1, (
                    f"cell {row}_{col} is one label and nothing else"
                )

        # ── (E) the painted number comes from the model ──────────────
        for row in (0, 1, 2):
            n = cell_node(ns, row, LOAD_COL)
            assert n is not None, f"delegated cell {row} present"
            got = texts_under(n)
            assert_eq(
                got,
                [f"{load_percent(row)}%"],
                f"row {row}'s bar is labelled with the model's own value — a "
                f"bar encodes in pixels, so a delegate that dropped the label "
                f"would make the column invisible to §2 #7 and to a screen "
                f"reader (the reason Qt's QProgressBar carries text())",
            )

        # ── (F) it survives virtualization ───────────────────────────
        first_rows = sorted(
            int(str(n["tag"]).removeprefix(f"{TABLE_TAG}_row")) for n in tagged(ns, f"{TABLE_TAG}_row")
        )
        tf.wheel(path=TABLE_TAG, pixels=(0.0, 4000.0))
        tf.tick(0.016)
        ns2 = nodes(tf)
        later_rows = sorted(
            int(str(n["tag"]).removeprefix(f"{TABLE_TAG}_row"))
            for n in tagged(ns2, f"{TABLE_TAG}_row")
        )
        assert later_rows and later_rows[0] > first_rows[0], (
            f"premise: the wheel moved the window, {first_rows[0]} -> "
            f"{later_rows[0]}"
        )
        # Not an equality: at offset 0 the overscan only extends downward, so
        # a mid-dataset window is legitimately a few rows larger. The claim is
        # that it is still a WINDOW — a delegated column did not turn the body
        # into an eager render.
        assert len(later_rows) < 40, (
            f"the body is still windowed after scrolling, got "
            f"{len(later_rows)} rows of 10,000"
        )
        assert_eq(
            len(cell_tags(ns2, LOAD_COL)),
            len(later_rows),
            "one delegated cell per row of the NEW window",
        )
        for row in later_rows[:3]:
            n = cell_node(ns2, row, LOAD_COL)
            assert n is not None, f"delegated cell {row} present after scrolling"
            assert boxes(n) >= 2, f"row {row} still draws its gauge"
            assert_eq(
                texts_under(n),
                [f"{load_percent(row)}%"],
                f"and row {row}'s label is ITS value, not a leftover from the "
                f"window that scrolled away",
            )


if __name__ == "__main__":
    run_demo("R1532 §5.27 — a column declares how its cells are painted", body)
