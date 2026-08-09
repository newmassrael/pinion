#!/usr/bin/env python3
"""R1548 §5.27 §5.40 §2 #7 — a row is asked for its header.

## The gap

the toolkit spells both section axes with one virtual —
`headerData(int section, Orientation orientation, int role)` — and a
table view shows the vertical one by default. pinion had **no vertical axis
at all**: `GridModel` could be asked what a *column* was called and what mark
it carried, and a row could be asked nothing. A grid could not number its rows,
could not pin one, could not put a lock or a breakpoint or a diff marker beside
one — the whole left-hand gutter every professional table, editor and profiler
has.

R1548 adds it, and adds it as a **type**. `HeaderAxis<L, D>` holds the two roles
a section answers; `GridModel::columns` and `GridModel::rows` are both one, so
the two axes answer the same role set by construction and one painter
(`section_content`) draws either.

## What the toolkit 6.11 cannot do, and why

  1. **An unanswered axis is a declaration, not a blank strip.** the toolkit's
     orientation is a *runtime argument*, so the commonest
     abstract table model bug in existence — override `headerData`, handle
     `Horizontal`, fall off the end returning `dynamic value()` — paints a
     vertical header of empty sections that still occupy their width, and
     nothing reports it: not the model, not the view, not the accessibility
     tree. A blank strip is indistinguishable from rows that genuinely have no
     names. Here the axis is a field: `no_row_header()` is written down, the
     band is not painted, and the model is asked **zero** times a frame. The
     pair cannot even be split — the band is painted if and only if the axis is
     answered, because there is no second "show the header" flag to disagree
     with the first.

  2. **The mark's meaning reaches assistive technology.** the toolkit's
     `text(Name)` answers from `headerData(...,
     DisplayRole)` on *both* orientations, so a toolkit row header whose
     distinguishing information IS its glyph announces only the row's number.
     Here the answer carries a `meaning` and it joins the `rowheader`'s
     accessible name — and the name itself is derived from the **painted**
     band, so it cannot disagree with what is on screen.

## What this demo drives

  * `hello-virtual-table` — the vertical axis answering **both** roles: row
    numbers, and a mark on the pinned rows meaning something no cell says.
  * `hello-virtual-columns` — 10,000 rows showing a dozen, so "asked per
    painted row" is a claim with something at stake, published per §2 #7.
  * `hello-grid-sort` — the axis under a **sort**: the number is the row's
    identity, not its position, and the a11y pass composes with the permuted
    topology it was never told about.
  * `hello-table` — the EAGER surface answering the same axis, so the tree
    does not hold two contracts (the R1547.1 lesson).
  * `hello-grid-nav` — the negative control: a grid that declines the axis
    paints no band at all.

## Verification scope (>= 30 assertions, sections A-F)

  (A) The band is painted, one section per painted row, with the corner that
      aligns the two axes.
  (B) Both roles reach the AT tree: the meaningful mark is announced ahead of
      the row's number, the decorative case is named by the painted label, and
      every `rowheader` is named from the paint.
  (C) The axis is windowed: over 10,000 rows it is asked once per painted row,
      published, and it tracks a scroll.
  (D) A row header names its ROW, not its screen position: it survives a sort.
  (E) The eager surface answers the same axis, at the same addresses.
  (F) The negative control: an unanswered axis paints nothing.
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

VT = "vtbl"
VT_WIN = (470, 480)
VT_SCROLL = "vtbl_scroll"
VT_N = 10_000
PIN_EVERY = 25
PIN_MEANING = "Pinned"
DECORATION_PX = 10
# `TableStyle::row_header_width` / `header_height` / `row_height`.
BAND_W = 56
HEADER_H = 40
ROW_H = 36

VC = "vcol"
VC_WIN = (560, 420)
VC_SCROLL = "vcol_scroll"
VC_N = 10_000
VC_RH_STATUS = "vcol_rhstatus"

GS = "vtbl"
GS_WIN = (400, 480)
GS_SORT = "vsort"

NAV = "vtbl"
NAV_WIN = (400, 480)


def row_header_tag(root: str, row: int) -> str:
    """`GridTag::row_header` — mirrored, so the demo asks by the address the
    framework publishes rather than by a structural guess."""
    return f"{root}_rh{row}"


def row_mark_tag(root: str, row: int) -> str:
    """`GridTag::row_header_decoration`."""
    return f"{root}_rhdeco{row}"


def corner_tag(root: str) -> str:
    """`GridTag::header_corner`."""
    return f"{root}_hcorner"


def walk(node, out: list) -> None:
    if isinstance(node, dict):
        out.append(node)
        for v in node.values():
            walk(v, out)
    elif isinstance(node, list):
        for v in node:
            walk(v, out)


def nodes(tf: RpcSubprocess, viewport) -> list:
    res = tf.snapshot(source="paint", viewport=viewport)
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


def row_headers_of(a11y: list) -> dict:
    return {n["tag"]: n for n in a11y if n.get("role") == "rowheader"}


def indices_with_prefix(all_nodes: list, prefix: str) -> list[int]:
    out = []
    for n in all_nodes:
        tag = str(n.get("tag") or "")
        rest = tag.removeprefix(prefix)
        if rest != tag and rest.isdigit():
            out.append(int(rest))
    return sorted(out)


def painted_rows(all_nodes: list, root: str) -> list[int]:
    return indices_with_prefix(all_nodes, f"{root}_row")


def painted_sections(all_nodes: list, root: str) -> list[int]:
    return indices_with_prefix(all_nodes, f"{root}_rh")


def marked_sections(all_nodes: list, root: str) -> list[int]:
    return indices_with_prefix(all_nodes, f"{root}_rhdeco")


def readout_int(all_nodes: list, tag: str) -> int:
    node = by_tag(all_nodes, tag)
    assert node is not None, f"readout {tag!r} present in the paint scene"
    digits = [w for w in str(node.get("content") or "").split() if w.isdigit()]
    assert digits, f"readout {tag!r} states a number, got {node.get('content')!r}"
    return int(digits[0])


def texts_under(all_nodes: list, tag: str) -> list[str]:
    node = by_tag(all_nodes, tag)
    assert node is not None, f"{tag!r} is painted"
    found: list = []
    walk(node, found)
    return [str(t.get("content")) for t in found if t.get("content")]


def both_roles() -> None:
    """(A)-(B) on `hello-virtual-table`: the vertical axis answering both of
    its roles, with the marked and the unmarked case in one window."""
    with RpcSubprocess("hello-virtual-table", boot_grace=1.5) as tf:
        for _ in range(3):
            tf.tick(0.016)
        ns = nodes(tf, VT_WIN)
        rows = painted_rows(ns, VT)
        assert rows, "the grid paints some data rows"

        # ── (A) the band, its sections, and the corner ───────────────
        assert_eq(
            painted_sections(ns, VT),
            rows,
            "one header section per painted row, and no others — the band is "
            "windowed by the row window, exactly as the strips beside it are",
        )
        assert len(rows) * 100 < VT_N, (
            f"premise: the row axis is windowed, {len(rows)} of {VT_N} painted"
        )
        corner = by_tag(ns, corner_tag(VT))
        assert corner is not None, (
            "the corner cell — where the two section axes meet — is painted, "
            "so the two bands' first rows line up"
        )
        # The band's geometry: `TableStyle::row_header_width` wide, and the
        # corner as tall as the COLUMN band so the two axes' first cells line
        # up. Both are absolute claims, unlike an x-offset — the sections live
        # inside a scroll, so their rects are content-relative.
        assert_eq(
            (corner.get("rect") or {}).get("w"),
            BAND_W,
            "the corner is the band's width",
        )
        assert_eq(
            (corner.get("rect") or {}).get("h"),
            HEADER_H,
            "and the column band's height, so the two bands' first cells align",
        )
        for row in rows[:3]:
            rect = (by_tag(ns, row_header_tag(VT, row)) or {}).get("rect") or {}
            assert_eq(rect.get("w"), BAND_W, f"row {row}'s section is band-wide")
            assert_eq(rect.get("h"), ROW_H, f"and one row pitch tall")

        # Every painted section states its row's number (the toolkit's own
        # default `headerData` answer, here a written decision rather than a base-class
        # default nobody overrode).
        for row in rows[:4]:
            drawn = texts_under(ns, row_header_tag(VT, row))
            assert str(row + 1) in drawn, (
                f"row {row}'s header paints its 1-based number: {drawn}"
            )

        # ── (B) both roles reach the AT tree ─────────────────────────
        pinned = [r for r in rows if r % PIN_EVERY == 0]
        assert_eq(
            marked_sections(ns, VT),
            pinned,
            "exactly the rows whose decoration role answered carry a mark — "
            "the negative half, without which 'marked' could mean 'marked "
            "unconditionally'",
        )
        for row in pinned:
            mark = by_tag(ns, row_mark_tag(VT, row))
            assert mark is not None, (
                f"row {row}'s mark answers to its OWN address; before R1548 a "
                f"row could not carry one at all"
            )
            rect = mark.get("rect") or {}
            assert_eq(rect.get("w"), DECORATION_PX, f"row {row} mark width")
            assert_eq(rect.get("h"), DECORATION_PX, f"row {row} mark height")
            assert row_mark_tag(VT, row) != f"{VT}_hdeco{row}", (
                "and the two axes' mark addresses are different strings, which "
                "is why each needed its own prefix: at row 0 BOTH exist here"
            )
        # Concretely, at section 0 the two axes' marks coexist and are distinct
        # nodes — the collision `GridTag::row_header_decoration` avoids.
        assert by_tag(ns, f"{VT}_hdeco0") is not None, (
            "premise: column 0 carries a header mark (R1547)"
        )
        assert by_tag(ns, row_mark_tag(VT, 0)) is not None, (
            "and row 0 carries one too, at its own address"
        )

        a11y = access(tf)
        headers = row_headers_of(a11y)
        assert_eq(
            len(headers),
            len(rows),
            "the AT tree describes the sections the paint holds",
        )
        unnamed = [t for t, n in headers.items() if n.get("name") is None]
        assert not unnamed, (
            f"every rowheader carries an accessible name; {unnamed} do not. "
            f"The a11y pass stamps none, so the paint is the ONLY source — if "
            f"this fails the derivation is not reaching the band"
        )
        for row in rows:
            want = str(row + 1)
            if row in pinned:
                want = f"{PIN_MEANING} {want}"
            assert_eq(
                headers[row_header_tag(VT, row)].get("name"),
                want,
                f"row {row} announces its painted number, with the mark's "
                f"MEANING ahead of it when it has one — in Qt that glyph is "
                f"silent, because QAccessibleTableHeaderCell names a section "
                f"from its DisplayRole alone on both orientations",
            )
        assert pinned, "premise: the window holds at least one pinned row"

        # The rowheader leads its row, which is the reading order an AT walks.
        for row in rows[:3]:
            row_node = next(
                (n for n in a11y if n.get("tag") == f"{VT}_row{row}"), None
            )
            assert row_node is not None, f"row {row} is in the AT tree"
            children = row_node.get("children") or []
            assert children and children[0] == row_header_tag(VT, row), (
                f"row {row}'s FIRST child is its header cell: {children[:2]}"
            )


def windowed() -> None:
    """(C) on `hello-virtual-columns`: the axis is asked once per painted row,
    over 10,000 of them, and it tracks a scroll."""
    with RpcSubprocess("hello-virtual-columns", boot_grace=1.5) as tf:
        for _ in range(3):
            tf.tick(0.016)
        ns = nodes(tf, VC_WIN)
        rows = painted_rows(ns, VC)
        assert rows, "the wide grid paints some rows"
        assert len(rows) * 100 < VC_N, (
            f"premise: the row axis is windowed, {len(rows)} of {VC_N}"
        )
        assert_eq(
            readout_int(ns, VC_RH_STATUS),
            len(rows),
            "the vertical axis is asked exactly once per painted row — an "
            "EQUALITY, not a bound: 'asks for what it paints' is what the "
            "contract says, and a 10,000-row grid that asked 10,000 times "
            "would satisfy any bound",
        )
        assert_eq(
            painted_sections(ns, VC),
            rows,
            "and the sections it painted are the rows it painted",
        )

        # It tracks a scroll: the NEW window's sections are that window's.
        tf.scroll(VC_SCROLL, to=(0, 4000))
        tf.tick(0.016)
        ns2 = nodes(tf, VC_WIN)
        rows2 = painted_rows(ns2, VC)
        assert rows2 and rows2[0] > rows[-1], (
            f"premise: the scroll moved the row window, {rows[:2]}... -> "
            f"{rows2[:2]}..."
        )
        assert_eq(
            readout_int(ns2, VC_RH_STATUS),
            len(rows2),
            "still asked once per painted row after the window moved",
        )
        assert_eq(
            painted_sections(ns2, VC),
            rows2,
            "and the new window's sections are ITS rows, not leftovers",
        )
        headers2 = row_headers_of(access(tf))
        assert_eq(
            len(headers2), len(rows2), "the AT tree follows the window too"
        )
        for row in rows2[:3]:
            assert_eq(
                headers2[row_header_tag(VC, row)].get("name"),
                f"R{row:04d}",
                f"row {row} announces the label its model answered with",
            )


def under_a_sort() -> None:
    """(D) on `hello-grid-sort`: the number is the row's identity."""
    with RpcSubprocess("hello-grid-sort", boot_grace=1.5) as tf:
        for _ in range(3):
            tf.tick(0.016)
        ns = nodes(tf, GS_WIN)
        rows = painted_rows(ns, GS)
        assert rows, "the sortable grid paints rows"
        assert_eq(
            painted_sections(ns, GS),
            rows,
            "a header section per painted row",
        )
        before = {r: texts_under(ns, row_header_tag(GS, r))[0] for r in rows}
        for row, label in before.items():
            assert_eq(
                label,
                str(row + 1),
                f"unsorted, row {row} is numbered {row + 1}",
            )

        # Sort by a column: the VIEW is permuted.
        tf.click(path=f"{GS_SORT}#h1")
        tf.tick(0.016)
        ns2 = nodes(tf, GS_WIN)
        rows2 = painted_rows(ns2, GS)
        assert rows2, "rows survive the sort"
        assert rows2 != rows or sorted(rows2) != rows2, (
            f"premise: the sort changed which rows are on screen or their "
            f"order ({rows} -> {rows2})"
        )
        after = {r: texts_under(ns2, row_header_tag(GS, r))[0] for r in rows2}
        for row, label in after.items():
            assert_eq(
                label,
                str(row + 1),
                f"and after the sort row {row} is STILL numbered {row + 1} — "
                f"the vertical axis is asked with the row's data index, so a "
                f"number travels with its row rather than renumbering the "
                f"viewport",
            )
        headers = row_headers_of(access(tf))
        assert_eq(
            len(headers),
            len(rows2),
            "the a11y pass composed with the PERMUTED topology it was never "
            "told about — a builder flag would have reached one of six",
        )
        for row in rows2[:3]:
            assert_eq(
                headers[row_header_tag(GS, row)].get("name"),
                str(row + 1),
                f"row {row}'s AT name is its painted number",
            )


def eager_surface() -> None:
    """(E) on `hello-table`: the eager `view_table` answers the same axis.

    R1547.1's lesson, paid for on the horizontal axis: a role one grid surface
    answers and its sibling does not is two contracts in one tree, and the
    consumer that lands on the silent one reads the whole axis as absent.
    """
    with RpcSubprocess("hello-table", boot_grace=1.5) as tf:
        for _ in range(3):
            tf.tick(0.016)
        ns = nodes(tf, (560, 420))
        rows = painted_rows(ns, "table")
        assert rows, "the eager table paints rows"
        assert_eq(
            painted_sections(ns, "table"),
            rows,
            "the EAGER surface paints a header section per row, at the same "
            "addresses the virtualized one uses",
        )
        assert by_tag(ns, corner_tag("table")) is not None, "and the corner"

        pinned = 5
        mark = by_tag(ns, row_mark_tag("table", pinned))
        assert mark is not None, "the pinned row's mark is painted"
        assert_eq(mark.get("type"), "Image", "here it is the ICON arm")
        assert str(mark.get("source") or "").startswith("memory://"), (
            f"drawn from a producer-registered buffer: {mark.get('source')!r}"
        )
        assert_eq(
            marked_sections(ns, "table"),
            [pinned],
            "and no other row is marked — the negative half",
        )

        headers = row_headers_of(access(tf))
        assert_eq(len(headers), len(rows), "one rowheader per row in the AT tree")
        assert_eq(
            headers[row_header_tag("table", pinned)].get("name"),
            f"{PIN_MEANING} {pinned + 1}",
            "the meaning joins the row header's name ON THE EAGER PATH too",
        )
        for row in rows:
            if row == pinned:
                continue
            assert_eq(
                headers[row_header_tag("table", row)].get("name"),
                str(row + 1),
                f"row {row} is named by its own painted number",
            )


def declined() -> None:
    """(F) The negative control, on `hello-grid-nav`.

    **This is the assertion Qt cannot make.** A Qt model that does not answer
    `Qt::Vertical` still gets a header view: `QTableView` paints blank sections
    that occupy their width, and no observation distinguishes that from rows
    with genuinely empty names. Here declining the axis is `no_row_header()`,
    and what it produces is *nothing* — no band, no corner, no section, and no
    ask.
    """
    with RpcSubprocess("hello-grid-nav", boot_grace=1.5) as tf:
        for _ in range(3):
            tf.tick(0.016)
        ns = nodes(tf, NAV_WIN)
        rows = painted_rows(ns, NAV)
        assert rows, "premise: this grid paints rows, it just declines the axis"
        assert_eq(
            painted_sections(ns, NAV), [], "no header section for any row"
        )
        assert_eq(marked_sections(ns, NAV), [], "no row mark")
        assert by_tag(ns, corner_tag(NAV)) is None, (
            "and no corner — a declined axis leaves no trace in the scene, "
            "where Qt would leave an empty strip nothing can report on"
        )
        a11y = access(tf)
        assert_eq(
            row_headers_of(a11y), {}, "and no `rowheader` in the AT tree"
        )
        row_node = next(
            (n for n in a11y if n.get("tag") == f"{NAV}_row{rows[0]}"), None
        )
        assert row_node is not None, "its rows are still described"
        children = row_node.get("children") or []
        assert children and not children[0].startswith(f"{NAV}_rh"), (
            f"and a row still leads with its first gridcell: {children[:2]}"
        )


def all_sections() -> None:
    """`run_demo` never returns (R1527 — it exits so a sweep cannot mistake a
    failing demo for a passing one), so the five apps this round touches are
    driven from ONE body rather than five calls."""
    both_roles()
    windowed()
    under_a_sort()
    eager_surface()
    declined()


if __name__ == "__main__":
    run_demo("R1548 §5.27 §5.40 — a row is asked for its header", all_sections)
