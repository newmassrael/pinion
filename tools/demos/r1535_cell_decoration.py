#!/usr/bin/env python3
"""R1535 §5.27 §2 #1 §2 #7 — a cell is asked for its decoration role.

Qt's model answers `data(index, role)`: `DisplayRole` for the text,
`DecorationRole` for the mark drawn beside it, `EditRole`, `ToolTipRole`.
pinion's `GridModel` had no role dimension at all — `cell` and `header` each
returned a `String`, so the only thing a grid could ever say about a cell was
what it spelled. R1530 named that as the Model/View axis's largest remaining
gap; R1532 named the same thing from the other side ("the painter the
framework sells is text only; Qt's default also paints Decoration").

## Why this is a role and not another delegate

R1532 gave a **column** a painter (Qt `setItemDelegateForColumn`). That is
the right axis for "this column is a bar" — but a column's painter is
resolved once for every row it draws, so it cannot express a mark whose
value is a function of the row: a status colour, a layer colour, a severity.
A binding wanting one had to delegate the cell wholesale and then re-derive
the status from the text the model had already answered with.

A role is asked per **cell**, which is the axis the datum actually varies on.
That difference is what section (C) below measures, and it is the reason the
fixture's colour is keyed to the row rather than to the column: a per-column
implementation passes every other assertion here.

## The shape, and why it is this shape

Qt reaches every role through one `data(index, role)` because a C++ model
needs one virtual entry point, and pays for it with `QVariant` — an untyped
hole every caller unwraps. `GridModel` gives each role its own typed
accessor instead, so a role's answer type is exact and a model that cannot
answer one is unrepresentable rather than returning an invalid variant. That
is the shape R1530 already chose when it split Qt's `headerData` out as
`GridModel::header`.

`CellDecoration` is an enum with one arm (`Swatch(Color)`) because
`Qt::DecorationRole` is a *variant* (QColor | QPixmap | QIcon): naming the
role's type as a sum keeps the arms pinion cannot paint yet absent rather
than misrepresented.

The mark is a structured `Scene` node, not an opaque draw — §2 #1 — which is
why this demo can assert its colour and its geometry without a pixel.

## What this demo drives

`hello-virtual-table`, a 10,000-row virtualized grid that now exercises BOTH
seams at once: column `Load` is delegated (R1532, a gauge), column `Status`
is decorated (R1535, a colour keyed to the row's status). One grid, so the
difference between the two extension points is legible in one snapshot.

## Verification scope (>= 30 assertions, sections A-G)

  (A) The grid still is a grid — a new role in the cell path must not cost
      the a11y properties the cell path already had.
  (B) The decorated cell carries the model's mark, as scene data: a square
      of the declared size with the model's colour, ahead of the label.
  (C) The mark is a function of the ROW. This is what a per-column delegate
      cannot do, and the reason the role exists.
  (D) NEGATIVE CONTROL — an undecorated cell is exactly its label. No mark,
      and no gap spacer either: the decoration is additive, so a grid that
      answers `None` paints the pre-R1535 node.
  (E) The mark and the label describe ONE status — same status, same mark;
      different status, different mark. Asserted against the label rather
      than a colour literal, so the two roles cannot drift apart unnoticed.
  (F) It survives virtualization — after a scroll each painted cell carries
      ITS row's mark, not a leftover from the window that scrolled away.
  (G) The two seams compose — the delegated column is still delegated, and
      the decorated column did not become one.
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
NCOLS = 4
#: The decorated column's absolute index (`Status`).
STATUS_COL = 2
#: The delegated column's absolute index (`Load`, R1532).
LOAD_COL = 3
#: `TableStyle::decoration_px` — the swatch is this square.
DECORATION_PX = 10
#: How many statuses the fixture cycles through, mirrored here so the demo
#: derives the expected grouping itself rather than reading it back.
STATUS_KINDS = 3
#: The window the app boots with, so the paint snapshot is taken at the
#: extent the runtime laid out.
WIN = (400, 480)


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


def cell_node(all_nodes: list, row: int, col: int):
    want = f"{TABLE_TAG}#{row}_{col}"
    for n in all_nodes:
        if n.get("tag") == want:
            return n
    return None


def texts_under(node) -> list:
    out: list = []
    walk(node, out)
    return [n["content"] for n in out if n.get("type") == "Text" and "content" in n]


def mark_of(cell) -> tuple | None:
    """The cell's decoration swatch as an `(r, g, b, a)` tuple, or `None`.

    Identified by its **square declared size**, not merely by being an
    untagged empty container: the gap spacer beside it is one too, and a
    looser predicate reports the spacer's transparent fill as a mark
    whenever the swatch is missing — exactly the case (D) exists to detect.
    (Measured: the in-crate helper did precisely that until it was fixed.)
    """
    if cell is None:
        return None
    for ch in cell.get("children") or []:
        if ch.get("type") != "Container" or ch.get("tag") or ch.get("children"):
            continue
        rect = ch.get("rect") or {}
        if rect.get("w") != DECORATION_PX or rect.get("h") != DECORATION_PX:
            continue
        f = (ch.get("style") or {}).get("fill") or {}
        return (f.get("r"), f.get("g"), f.get("b"), f.get("a"))
    return None


def painted_rows(all_nodes: list) -> list:
    return sorted(
        int(str(n["tag"]).removeprefix(f"{TABLE_TAG}_row"))
        for n in tagged(all_nodes, f"{TABLE_TAG}_row")
    )


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        for _ in range(3):
            tf.tick(0.016)
        ns = nodes(tf)
        rows = painted_rows(ns)
        assert len(rows) > STATUS_KINDS, (
            f"premise: the body windowed more rows than there are statuses, "
            f"got {len(rows)} — otherwise (C) and (E) cannot see a cycle"
        )

        # ── (A) it is still a grid ───────────────────────────────────
        a11y = tf.request("scene/access").result
        assert a11y, "scene/access returned no result"
        flat: list = []
        walk(a11y, flat)
        roles = [n.get("role") for n in flat if isinstance(n, dict) and "role" in n]
        assert "grid" in roles, f"the paint root is still a grid, got {set(roles)}"
        assert_eq(roles.count("columnheader"), NCOLS, "one columnheader per column")
        assert_eq(
            roles.count("gridcell"),
            len(rows) * NCOLS,
            "one gridcell per column per windowed row — a decorated column is "
            "still a cell of the grid, not an opaque blob",
        )

        # ── (B) the decorated cell carries the model's mark ──────────
        for col in range(NCOLS):
            assert cell_node(ns, rows[0], col) is not None, (
                f"premise: column {col} of the first painted row is addressable"
            )
        first = cell_node(ns, rows[0], STATUS_COL)
        mark = mark_of(first)
        assert mark is not None, (
            f"the `Status` cell carries a {DECORATION_PX}x{DECORATION_PX} mark "
            f"as SCENE DATA — §2 #1 forbids an opaque draw, which is why this "
            f"assertion can exist at all"
        )
        assert mark[3] == 255, f"the mark is opaque ink, got alpha {mark[3]}"
        kids = first.get("children") or []
        assert_eq(len(kids), 3, "the decorated cell is mark + gap + label")
        assert_eq(kids[0].get("type"), "Container", "the mark comes first")
        assert_eq(kids[-1].get("type"), "Text", "and the label last")
        assert_eq(
            (kids[0].get("style") or {}).get("corner_radius"),
            2,
            "the mark is a softened square (Qt paints a QColor decoration as a "
            "filled rectangle), not a disc — the shape must not editorialise",
        )
        assert kids[0]["rect"]["x"] < kids[-1]["rect"]["x"], (
            "the mark is laid out BEFORE the label, as Qt's default delegate "
            "places a decoration"
        )
        assert kids[-1]["rect"]["x"] >= kids[0]["rect"]["x"] + DECORATION_PX, (
            "and the label clears it rather than overlapping"
        )

        # ── (C) the mark is a function of the ROW ────────────────────
        # THE reason this is a role and not a delegate: a column's painter is
        # resolved once for all its rows, so it cannot produce this.
        marks = {r: mark_of(cell_node(ns, r, STATUS_COL)) for r in rows}
        for r, m in marks.items():
            assert m is not None, f"row {r}'s status cell carries a mark"
        distinct = {m for m in marks.values()}
        assert len(distinct) == STATUS_KINDS, (
            f"one column shows {len(distinct)} distinct marks across "
            f"{len(rows)} rows — a per-column painter could only ever show 1"
        )
        assert marks[rows[0]] != marks[rows[1]], (
            "and adjacent rows differ, so the variation is per-row and not "
            "per-block"
        )

        # ── (D) NEGATIVE CONTROL: an undecorated cell is its label ───
        for col in (0, 1, LOAD_COL):
            for r in rows[:2]:
                n = cell_node(ns, r, col)
                assert n is not None, f"cell {r}_{col} present"
                assert_eq(
                    mark_of(n),
                    None,
                    f"column {col} answers the decoration role with nothing — "
                    f"a painter that marked every cell fails exactly here",
                )
        for r in rows[:2]:
            plain = cell_node(ns, r, 0)
            assert_eq(
                len(plain.get("children") or []),
                1,
                f"cell {r}_0 is exactly its label: no mark AND no gap spacer, "
                f"so an undecorated grid paints the pre-R1535 node",
            )

        # ── (E) the mark and the label describe one status ───────────
        labels = {r: texts_under(cell_node(ns, r, STATUS_COL))[0] for r in rows}
        by_label: dict[str, set] = {}
        for r in rows:
            by_label.setdefault(labels[r], set()).add(marks[r])
        assert_eq(
            len(by_label),
            STATUS_KINDS,
            f"the fixture really does cycle {STATUS_KINDS} statuses, got "
            f"{sorted(by_label)}",
        )
        for label, ms in by_label.items():
            assert_eq(len(ms), 1, f"every '{label}' row shows the same mark")
        all_marks = [next(iter(ms)) for ms in by_label.values()]
        assert_eq(
            len(set(all_marks)),
            STATUS_KINDS,
            "and different statuses show different marks — otherwise 'agrees "
            "with the label' is satisfied by one colour for everything",
        )

        # ── (F) it survives virtualization ───────────────────────────
        tf.wheel(path=TABLE_TAG, pixels=(0.0, 4000.0))
        tf.tick(0.016)
        ns2 = nodes(tf)
        rows2 = painted_rows(ns2)
        assert rows2 and rows2[0] > rows[0], (
            f"premise: the wheel moved the window, {rows[0]} -> {rows2[0]}"
        )
        assert len(rows2) < 40, (
            f"the body is still windowed after scrolling, got {len(rows2)} "
            f"rows of 10,000"
        )
        # The expectation for the NEW window is derived from the OLD one: the
        # status of a row is `row % STATUS_KINDS`, so the first window already
        # showed every status once and pins what each must look like. Built as
        # a total map with no fallback — an expected value that degrades to
        # "whatever was observed" is an assertion that cannot fail, which is
        # what this comparison was before it was rewritten.
        by_status = {r % STATUS_KINDS: (marks[r], labels[r]) for r in rows}
        assert_eq(
            len(by_status),
            STATUS_KINDS,
            "premise: the first window pinned every status's appearance",
        )
        for r in rows[:STATUS_KINDS]:
            assert_eq(
                by_status[r % STATUS_KINDS],
                (marks[r], labels[r]),
                f"premise: the map agrees with the window it came from at {r}",
            )
        for r in rows2[:4]:
            n = cell_node(ns2, r, STATUS_COL)
            assert n is not None, f"status cell {r} present after scrolling"
            want_mark, want_label = by_status[r % STATUS_KINDS]
            assert_eq(
                mark_of(n),
                want_mark,
                f"row {r}'s mark is ITS status's, not a leftover from the "
                f"window that scrolled away",
            )
            assert_eq(
                texts_under(n)[0],
                want_label,
                f"and row {r}'s label agrees with it after the scroll too",
            )

        # ── (G) the two seams compose ────────────────────────────────
        # The delegated column keeps its gauge, and the decorated one did not
        # become a delegated cell: one grid, two extension points, each doing
        # only its own job.
        def empty_boxes(node) -> int:
            out: list = []
            walk(node, out)
            return sum(
                1
                for n in out
                if n.get("type") == "Container"
                and not n.get("tag")
                and not n.get("children")
            )

        gauge = cell_node(ns2, rows2[0], LOAD_COL)
        assert gauge is not None, "the delegated cell is still addressable"
        assert empty_boxes(gauge) >= 2, (
            f"the delegated column still draws its track and fill, got "
            f"{empty_boxes(gauge)}"
        )
        assert_eq(
            mark_of(gauge),
            None,
            "and it carries no decoration — the two seams are independent",
        )
        status = cell_node(ns2, rows2[0], STATUS_COL)
        assert_eq(
            len(texts_under(status)),
            1,
            "the decorated cell is still one label plus its mark, not a "
            "delegated subtree",
        )


if __name__ == "__main__":
    run_demo("R1535 §5.27 — a cell is asked for its decoration role", body)
