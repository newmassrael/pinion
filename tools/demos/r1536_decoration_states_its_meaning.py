#!/usr/bin/env python3
"""R1536 §5.27 §5.40 §2 #7 — a decoration states what it means, and is addressable.

R1535 gave the grid a `DecorationRole`. This round takes it past the toolkit, and
fixes what looking closely found underneath.

## What the toolkit cannot do, and why

the toolkit's decoration role is **appearance**: a color, a pixmap, a icon.
What the mark *means* is a different role (`AccessibleTextRole`) that
abstract item view does not wire to the decoration, so
`text(Name)` returns the display string and the mark
contributes nothing. A status column that is only a colour is, to a toolkit
screen-reader user, a column of empty cells.

Here the two travel in one answer, so they cannot drift and anything that can
see the mark can also read it:

  CellDecoration::Swatch { color, meaning }

`meaning` empty is the **decorative** answer, not a missing one — HTML's
`alt=""`, correct when the mark restates text the cell already shows, because
announcing it would make a screen reader say the status twice. Non-empty joins
the cell's accessible name ahead of the label, as a browser does for
`<td><img alt="Overdue"> 3 days</td>`. The model states which, because the
model is the only thing that knows; a framework guessing would be wrong for
half its consumers.

## What this round found underneath (the reason it matters)

Wiring the meaning through and then *asking the running app* what the AT tree
said produced: **0 of 75 `gridcell`s named** — including the plain text ones.
Two stacked defects, both verified by probe, not by reading:

  1. `find_container_by_tag` (the name derivation's walker) stopped at any
     node that was not a `Container`. Every virtualized list / grid / tree
     paints inside a `ScrollNode`, so the derivation could never reach a
     single cell. It went unnoticed because the *bounds* walker does descend
     a scroll — so the AT tree was structurally right and pointed at the right
     pixels while announcing nothing.
  2. The grid painted each cell's label as `TextRole::Presentational`, which
     tells the derivation "skip past this to the real label". A data cell has
     no other label; that text is the only thing it says. Meanwhile
     `pinion_a11y::tree_view` documented the opposite contract in so many
     words: "gridcell name comes from the painted text, not the builder". The
     write path and the read path disagreed and the AT tree took paint's
     answer, which was silence.

After both: **75 of 75 named**. The toolkit names its cells; pinion did not. The toolkit is the
floor.

## What this demo drives

`hello-virtual-table`, which now carries every cell kind in one grid: plain
text (`Index`, `Name`), text + a **decorative** mark (`Status`), a
**mark-only** cell whose mark is its whole content (`Flag`), and a delegated
gauge (`Load`, R1532).

## Verification scope (>= 30 assertions, sections A-F)

  (A) Every painted cell is NAMED to an AT. The regression that hid for ~760
      rounds, asserted as a census rather than a sample.
  (B) The decorative arm — `Status` says its status once, not twice.
  (C) The meaningful arm — the mark-only `Flag` cell is named by its mark,
      and the unflagged cells in the same column are not.
  (D) The mark is ADDRESSABLE — `GridTag::cell_decoration` reaches it by name
      rather than by walking a cell's children by position.
  (E) The mark is still not a click target: addressable and hit-transparent
      are independent axes (R1535 conflated them).
  (F) It survives virtualization — after a scroll the new window's cells are
      named for THEIR rows.
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
STATUS_COL = 2
FLAG_COL = 3
LOAD_COL = 4
STATUS_KINDS = 3
DECORATION_PX = 10
WIN = (470, 480)


def decoration_tag(row: int, col: int) -> str:
    """`GridTag::cell_decoration` — mirrored, so the demo asks by the address
    the framework publishes rather than by a structural guess."""
    return f"{TABLE_TAG}_deco{row}_{col}"


def walk(node, out: list) -> None:
    if isinstance(node, dict):
        out.append(node)
        for v in node.values():
            walk(v, out)
    elif isinstance(node, list):
        for v in node:
            walk(v, out)


def nodes(tf: RpcSubprocess) -> list:
    res = tf.snapshot(source="paint", viewport=WIN)
    assert res, "paint snapshot returned no result"
    out: list = []
    walk(res, out)
    return out


def access(tf: RpcSubprocess) -> list:
    res = tf.request("scene/access").result
    assert res, "scene/access returned no result"
    out: list = []
    walk(res, out)
    return [n for n in out if isinstance(n, dict) and "role" in n]


def by_tag(all_nodes: list, tag: str):
    for n in all_nodes:
        if n.get("tag") == tag:
            return n
    return None


def cells_of(a11y: list) -> dict:
    return {n["tag"]: n for n in a11y if n.get("role") == "gridcell"}


def rows_painted(all_nodes: list) -> list:
    return sorted(
        int(str(n["tag"]).removeprefix(f"{TABLE_TAG}_row"))
        for n in all_nodes
        if str(n.get("tag") or "").startswith(f"{TABLE_TAG}_row")
    )


def is_flagged(row: int) -> bool:
    return row % 3 == 1


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        for _ in range(3):
            tf.tick(0.016)
        ns = nodes(tf)
        a11y = access(tf)
        cells = cells_of(a11y)
        rows = rows_painted(ns)
        assert len(rows) > STATUS_KINDS, (
            f"premise: more rows painted than there are statuses, got {len(rows)}"
        )

        # ── (A) every painted cell is named ──────────────────────────
        assert_eq(
            len(cells),
            len(rows) * NCOLS,
            "one gridcell per column per windowed row",
        )
        unnamed = [t for t, n in cells.items() if n.get("name") is None]
        assert not unnamed, (
            f"every painted cell carries an accessible name; {len(unnamed)} do "
            f"not: {unnamed[:5]} — this is the regression that hid behind a "
            f"structurally-correct AT tree, because bounds resolved while names "
            f"did not"
        )
        # And the names are the cells' OWN content, not a constant.
        for r in rows[:3]:
            assert_eq(
                cells[f"{TABLE_TAG}#{r}_0"].get("name"),
                f"{r:05d}",
                f"row {r}'s index cell is named by its own text",
            )
        assert_eq(
            cells[f"{TABLE_TAG}#{rows[0]}_{LOAD_COL}"].get("name"),
            f"{(rows[0] * 7) % 101}%",
            "and the DELEGATED column is named too — a bar encodes in pixels, "
            "so its label is the only thing a screen reader can read",
        )

        # ── (B) the decorative arm says the status once ──────────────
        for r in rows[:STATUS_KINDS]:
            status = cells[f"{TABLE_TAG}#{r}_{STATUS_COL}"].get("name")
            assert status, f"row {r}'s status cell is named, got {status!r}"
            assert_eq(
                status.split().__len__(),
                1,
                f"row {r}'s status is announced ONCE: its mark restates the "
                f"label, so the mark is decorative (alt=\"\") — got {status!r}",
            )
            # The mark is nonetheless painted: silent to AT, visible on screen.
            assert by_tag(ns, decoration_tag(r, STATUS_COL)) is not None, (
                f"row {r} still paints its status mark — decorative means "
                f"unannounced, not absent"
            )

        # ── (C) the meaningful arm names a mark-only cell ────────────
        flagged = [r for r in rows if is_flagged(r)]
        unflagged = [r for r in rows if not is_flagged(r)]
        assert len(flagged) > 1, f"premise: several flagged rows, got {flagged}"
        assert unflagged, "premise: some rows are unflagged"
        for r in flagged:
            assert_eq(
                cells[f"{TABLE_TAG}#{r}_{FLAG_COL}"].get("name"),
                "Flagged",
                f"row {r}'s mark-only cell is named by its MARK — its text is "
                f"empty, so without the meaning this cell is silent, which is "
                f"exactly what the same column built on Qt's DecorationRole is",
            )
        for r in unflagged[:3]:
            assert_eq(
                cells[f"{TABLE_TAG}#{r}_{FLAG_COL}"].get("name"),
                "",
                f"row {r} carries no flag, so its cell has nothing to say — the "
                f"negative half, without which 'named' could mean 'named "
                f"unconditionally'",
            )
            assert by_tag(ns, decoration_tag(r, FLAG_COL)) is None, (
                f"and row {r} paints no mark at all"
            )

        # ── (D) the mark is addressable ──────────────────────────────
        for r in rows[:3]:
            mark = by_tag(ns, decoration_tag(r, STATUS_COL))
            assert mark is not None, (
                f"cell ({r}, {STATUS_COL})'s mark answers to its own address — "
                f"before R1536 it could be found only by walking the cell's "
                f"children by position, a layout detail no client should know"
            )
            rect = mark.get("rect") or {}
            assert_eq(rect.get("w"), DECORATION_PX, f"row {r} mark width")
            assert_eq(rect.get("h"), DECORATION_PX, f"row {r} mark height")
            fill = (mark.get("style") or {}).get("fill") or {}
            assert_eq(fill.get("a"), 255, f"row {r} mark is opaque ink")
        assert by_tag(ns, decoration_tag(rows[0], 0)) is None, (
            "an undecorated column has no decoration node to address"
        )
        # Two rows of one column carry different ink (R1535's per-row axis),
        # read here through the addresses rather than by position.
        ink = [
            ((by_tag(ns, decoration_tag(r, STATUS_COL)) or {}).get("style") or {}).get("fill")
            for r in rows[:STATUS_KINDS]
        ]
        assert len({str(i) for i in ink}) == STATUS_KINDS, (
            f"one column, {STATUS_KINDS} distinct marks, addressed per cell: {ink}"
        )

        # ── (E) addressable is not clickable ─────────────────────────
        assert "#" not in decoration_tag(0, 0), (
            "the mark's address stays in the '_' presentational family, so it "
            "never enters the composite click-router namespace"
        )
        # The cell keeps its own composite tag, which is the click target.
        for r in rows[:2]:
            assert by_tag(ns, f"{TABLE_TAG}#{r}_{STATUS_COL}") is not None, (
                f"cell ({r}, {STATUS_COL}) is still the addressable click target"
            )

        # ── (F) it survives virtualization ───────────────────────────
        tf.wheel(path=TABLE_TAG, pixels=(0.0, 4000.0))
        tf.tick(0.016)
        ns2 = nodes(tf)
        cells2 = cells_of(access(tf))
        rows2 = rows_painted(ns2)
        assert rows2 and rows2[0] > rows[0], (
            f"premise: the wheel moved the window, {rows[0]} -> {rows2[0]}"
        )
        unnamed2 = [t for t, n in cells2.items() if n.get("name") is None]
        assert not unnamed2, f"still every cell named after scrolling: {unnamed2[:5]}"
        for r in rows2[:4]:
            assert_eq(
                cells2[f"{TABLE_TAG}#{r}_0"].get("name"),
                f"{r:05d}",
                f"row {r} is named for ITS row, not a leftover from the window "
                f"that scrolled away",
            )
            assert_eq(
                cells2[f"{TABLE_TAG}#{r}_{FLAG_COL}"].get("name"),
                "Flagged" if is_flagged(r) else "",
                f"and row {r}'s flag cell agrees with its own row",
            )


def icon_arm() -> None:
    """(G) The `QIcon` arm, driven on `hello-table` (the eager grid).

    Two things at once, both of which R1536 added: the decoration role reaches
    the EAGER `view_table` (until now only the virtualized grid answered it, so
    the tree held two cell-paint contracts that disagreed about whether the
    role exists), and the icon arm paints a `Scene::Image` from a `memory://`
    source rather than a filled box.
    """
    with RpcSubprocess("hello-table", boot_grace=1.5) as tf:
        for _ in range(3):
            tf.tick(0.016)
        res = tf.snapshot(source="paint", viewport=(560, 420))
        assert res, "paint snapshot returned no result"
        ns: list = []
        walk(res, ns)
        marks = [n for n in ns if str(n.get("tag") or "").startswith("table_deco")]
        assert marks, "the eager grid paints decorations at their own addresses"
        rows = len({str(n["tag"]).removeprefix("table_deco").split("_")[0] for n in marks})
        assert_eq(len(marks), rows, "one mark per row, in the one decorated column")
        for m in marks:
            assert_eq(m.get("type"), "Image", f"{m['tag']} is an image, not a box")
            assert str(m.get("source") or "").startswith("memory://"), (
                f"{m['tag']} draws a producer-registered buffer, not a file: "
                f"{m.get('source')!r}"
            )
            rect = m.get("rect") or {}
            assert_eq(rect.get("w"), DECORATION_PX, f"{m['tag']} width")
            assert_eq(rect.get("h"), DECORATION_PX, f"{m['tag']} height")
        # The icon restates its cell's label, so it is decorative: the cell's
        # name must not contain it twice.
        a11y = access(tf)
        cells = {n["tag"]: n for n in a11y if n.get("role") == "gridcell"}
        assert cells, "the eager grid still has gridcells"
        unnamed = [t for t, n in cells.items() if n.get("name") is None]
        assert not unnamed, f"every eager cell is named too: {unnamed[:5]}"
        status = cells["table#0_2"].get("name") or ""
        assert status.count("Done") == 1, (
            f"the status is announced ONCE — the icon restates the label, so it "
            f"is decorative (alt=\"\"), got {status!r}"
        )


def all_sections() -> None:
    """`run_demo` never returns (R1527 — it exits so a sweep cannot mistake a
    failing demo for a passing one), so the two apps this round touches are
    driven from ONE body rather than two calls."""
    body()
    icon_arm()


if __name__ == "__main__":
    run_demo("R1536 §5.27 §5.40 — a decoration states what it means", all_sections)
