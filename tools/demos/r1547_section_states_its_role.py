#!/usr/bin/env python3
"""R1547 §5.27 §5.40 §2 #7 — a header section is asked for its role.

## The gap

`GridModel` had four accessors. Three address a **cell** (`cell`,
`decoration`, `edit`) and one addresses a **section** (`header`) — and the
section could answer exactly one question: what the column is *called*. Qt
reaches both axes through one role enum (`data(index, role)` /
`headerData(section, orientation, role)`) and `QHeaderView::paintSection`
consumes `Qt::DecorationRole` into `QStyleOptionHeader::icon`, so a Qt grid can
put a key glyph, a type glyph, a filter funnel or a lock in a column header.
Nothing here could mark a column at all.

R1547 gives the section axis its role dimension, opening it with the role the
cell axis opened with in R1535: `GridModel::header_decoration`, answered with
the **same** `Decoration` type and drawn by the **same** painter. (The type was
`CellDecoration` until this round. A role is not axis-specific; two types would
be two contracts that must agree about what a mark is.)

## What Qt 6.11 cannot do, and why

  1. **The mark reaches assistive technology.** Qt's decoration is appearance;
     `QAccessibleTableHeaderCell::text(Name)` answers from `headerData(...,
     Qt::DisplayRole)` alone, so a Qt header whose distinguishing information
     IS its glyph announces only the column's name. Here the answer carries a
     `meaning` (R1536's rule, now on the section axis) and it joins the
     `columnheader`'s accessible name ahead of the label. Empty is the
     decorative answer — HTML `alt=""` — not a missing one.

  2. **The `columnheader` is named by what is PAINTED.** Qt derives a header's
     accessible name from the model on a path entirely independent of
     `paintSection`, so a view that elides, reformats or overrides the drawn
     label announces a string that is not on screen. Here the name comes from
     the paint scene, and R1547 removed the labels from the a11y builders
     outright — `windowed_grid_nodes*` now take a column **count** — so there
     is no second source left to disagree with the pixels. Measured on
     `hello-virtual-columns`: the introspection pass asked its model for 5
     labels a frame and now asks for **0**.

## What this demo drives

  * `hello-virtual-table` — the meaningful / decorative pair on the section
    axis. `Index` carries a mark meaning "Primary key" (the word "Index" does
    not say it); `Status` carries a legend swatch that restates its own label
    and is therefore silent.
  * `hello-virtual-columns` — 200 columns showing a handful, so "asked per
    painted section" is a claim with something at stake.

## Verification scope (>= 30 assertions, sections A-F)

  (A) The section role is painted, at its own address, on the marked columns
      and only those.
  (B) A section mark is not a cell mark — the two axes have separate address
      spaces and separate answers.
  (C) The meaningful arm reaches the AT tree; the decorative arm does not.
  (D) Every `columnheader` is named, and named by the PAINTED label — the
      builders no longer carry one.
  (E) The role is windowed: over 200 columns it is asked once per painted
      section, published per §2 #7 and cross-checked against the tree.
  (F) It survives a horizontal scroll — the new window's sections are marked
      and named for THEIR columns.
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
VT_HEADERS = ["Index", "Name", "Status", "Flag", "Load"]
KEY_COL = 0
STATUS_COL = 2
KEY_MEANING = "Primary key"
DECORATION_PX = 10

VC = "vcol"
VC_WIN = (560, 420)
VC_NCOLS = 200
MARKED_EVERY = 10
VC_HSCROLL = "vcol_hscroll"
VC_HDECO_STATUS = "vcol_hdstatus"


def header_deco_tag(root: str, col: int) -> str:
    """`GridTag::header_decoration` — mirrored, so the demo asks by the address
    the framework publishes rather than by a structural guess."""
    return f"{root}_hdeco{col}"


def header_tag(root: str, col: int) -> str:
    """`GridTag::col_header`."""
    return f"{root}_ch{col}"


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


def headers_of(a11y: list) -> dict:
    return {n["tag"]: n for n in a11y if n.get("role") == "columnheader"}


def painted_sections(all_nodes: list, root: str) -> list[int]:
    out = []
    for n in all_nodes:
        tag = str(n.get("tag") or "")
        rest = tag.removeprefix(f"{root}_ch")
        if rest != tag and rest.isdigit():
            out.append(int(rest))
    return sorted(out)


def marked_sections(all_nodes: list, root: str) -> list[int]:
    out = []
    for n in all_nodes:
        tag = str(n.get("tag") or "")
        rest = tag.removeprefix(f"{root}_hdeco")
        if rest != tag and rest.isdigit():
            out.append(int(rest))
    return sorted(out)


def readout_int(all_nodes: list, tag: str) -> int:
    node = by_tag(all_nodes, tag)
    assert node is not None, f"readout {tag!r} present in the paint scene"
    digits = [w for w in str(node.get("content") or "").split() if w.isdigit()]
    assert digits, f"readout {tag!r} states a number, got {node.get('content')!r}"
    return int(digits[0])


def narrow_grid() -> None:
    """(A)-(D) on `hello-virtual-table`: a five-column grid whose section axis
    answers both the meaningful and the decorative arm."""
    with RpcSubprocess("hello-virtual-table", boot_grace=1.5) as tf:
        for _ in range(3):
            tf.tick(0.016)
        ns = nodes(tf, VT_WIN)
        a11y = access(tf)
        headers = headers_of(a11y)

        # ── (A) the section role is painted, and only where answered ──
        assert_eq(
            len(headers),
            len(VT_HEADERS),
            "one columnheader per column",
        )
        assert_eq(
            marked_sections(ns, VT),
            [KEY_COL, STATUS_COL],
            "exactly the two sections whose role answered carry a mark — the "
            "negative half, without which 'marked' could mean 'marked "
            "unconditionally'",
        )
        for col in (KEY_COL, STATUS_COL):
            mark = by_tag(ns, header_deco_tag(VT, col))
            assert mark is not None, (
                f"section {col}'s mark answers to its own address; before "
                f"R1547 a header could not carry one at all"
            )
            rect = mark.get("rect") or {}
            assert_eq(rect.get("w"), DECORATION_PX, f"section {col} mark width")
            assert_eq(rect.get("h"), DECORATION_PX, f"section {col} mark height")
            fill = (mark.get("style") or {}).get("fill") or {}
            assert_eq(fill.get("a"), 255, f"section {col} mark is opaque ink")
        # Two marked sections, two distinct inks: the answer is per section.
        inks = [
            ((by_tag(ns, header_deco_tag(VT, c)) or {}).get("style") or {}).get("fill")
            for c in (KEY_COL, STATUS_COL)
        ]
        assert inks[0] != inks[1], (
            f"each section's mark is its OWN answer, not one repeated: {inks}"
        )

        # ── (B) a section mark is not a cell mark ────────────────────
        assert by_tag(ns, f"{VT}_deco0_{KEY_COL}") is None, (
            "no CELL of the key column is marked — the two axes answer "
            "separately, and R1547 gave the section its own address space so "
            "`_deco0_0` and `_hdeco0` cannot be confused"
        )
        assert header_deco_tag(VT, KEY_COL) != f"{VT}_deco0_{KEY_COL}", (
            "and the two addresses are distinct strings"
        )
        # The cell axis still answers its own role in the same grid.
        assert by_tag(ns, f"{VT}_deco0_{STATUS_COL}") is not None, (
            "premise: the cell axis is still decorated, so this grid really "
            "does answer a role on BOTH axes"
        )

        # ── (C) meaningful is announced, decorative is silent ────────
        assert_eq(
            headers[header_tag(VT, KEY_COL)].get("name"),
            f"{KEY_MEANING} {VT_HEADERS[KEY_COL]}",
            "the MEANINGFUL mark joins the header's accessible name ahead of "
            "the label — in Qt this glyph is silent, because "
            "QAccessibleTableHeaderCell names a section from its DisplayRole "
            "alone",
        )
        assert_eq(
            headers[header_tag(VT, STATUS_COL)].get("name"),
            VT_HEADERS[STATUS_COL],
            "and the DECORATIVE one is not announced: it restates the column's "
            "own label, so alt=\"\" is the correct markup",
        )
        assert by_tag(ns, header_deco_tag(VT, STATUS_COL)) is not None, (
            "decorative means unannounced, not absent — it is still painted"
        )

        # ── (D) every columnheader is named, by the PAINTED label ────
        unnamed = [t for t, n in headers.items() if n.get("name") is None]
        assert not unnamed, (
            f"every columnheader carries an accessible name; {unnamed} do not. "
            f"R1547 removed the labels from the a11y builders, so the paint is "
            f"now the ONLY source — if this is empty the derivation is not "
            f"reaching the header band"
        )
        for col, label in enumerate(VT_HEADERS):
            if col in (KEY_COL,):
                continue  # composed above
            assert_eq(
                headers[header_tag(VT, col)].get("name"),
                label,
                f"column {col} is named by its own painted label",
            )
        # And the names really do come from the pixels: the painted header
        # cell holds that text.
        for col, label in enumerate(VT_HEADERS):
            cell = by_tag(ns, header_tag(VT, col))
            assert cell is not None, f"column {col}'s header is painted"
            texts = []
            walk(cell, texts)
            contents = [str(t.get("content")) for t in texts if t.get("content")]
            assert label in contents, (
                f"column {col}'s painted header contains {label!r}, so the AT "
                f"name and the pixels are one fact: {contents}"
            )


def wide_grid() -> None:
    """(E)-(F) on `hello-virtual-columns`: 200 columns, a handful painted."""
    with RpcSubprocess("hello-virtual-columns", boot_grace=1.5) as tf:
        for _ in range(3):
            tf.tick(0.016)
        ns = nodes(tf, VC_WIN)
        painted = painted_sections(ns, VC)
        assert painted, "the wide grid paints some header cells"
        assert len(painted) * 10 < VC_NCOLS, (
            f"premise: the column axis is windowed, {len(painted)} of "
            f"{VC_NCOLS} painted"
        )

        # ── (E) the role is asked per painted section ────────────────
        asked = readout_int(ns, VC_HDECO_STATUS)
        assert_eq(
            asked,
            len(painted),
            "the section's mark is asked for exactly the sections painted — "
            "the label's R1530 property, now on the role beside it",
        )
        expected_marks = [c for c in painted if c % MARKED_EVERY == 0]
        assert_eq(
            marked_sections(ns, VC),
            expected_marks,
            "and only the sections whose role answered carry one",
        )

        a11y = access(tf)
        headers = headers_of(a11y)
        assert_eq(
            len(headers),
            len(painted),
            "the AT tree describes the sections the paint holds",
        )
        unnamed = [t for t, n in headers.items() if n.get("name") is None]
        assert not unnamed, (
            f"every windowed columnheader is named from the paint: {unnamed}"
        )
        for col in painted:
            want = f"C{col:03d}"
            if col % MARKED_EVERY == 0:
                want = f"Sampled {want}"
            assert_eq(
                headers[header_tag(VC, col)].get("name"),
                want,
                f"section {col} announces its own label, with its mark's "
                f"meaning ahead of it when it has one",
            )

        # ── (F) it survives a horizontal scroll ──────────────────────
        tf.scroll(VC_HSCROLL, to=(6000, 0))
        tf.tick(0.016)
        ns2 = nodes(tf, VC_WIN)
        painted2 = painted_sections(ns2, VC)
        assert painted2 and painted2[0] > painted[-1], (
            f"premise: the scroll moved the column window, {painted} -> "
            f"{painted2}"
        )
        assert_eq(
            readout_int(ns2, VC_HDECO_STATUS),
            len(painted2),
            "still asked once per painted section after the window moved",
        )
        assert_eq(
            marked_sections(ns2, VC),
            [c for c in painted2 if c % MARKED_EVERY == 0],
            "and the NEW window's marks are that window's, not leftovers",
        )
        headers2 = headers_of(access(tf))
        for col in painted2:
            want = f"C{col:03d}"
            if col % MARKED_EVERY == 0:
                want = f"Sampled {want}"
            assert_eq(
                headers2[header_tag(VC, col)].get("name"),
                want,
                f"section {col} is named for ITS column, not for one that "
                f"scrolled away",
            )


def eager_grid() -> None:
    """(G) The EAGER surface, on `hello-table`.

    Two things at once, both of which R1547 needed and neither of which the
    virtualized grid can show:

      * the eager `view_table` answers the section role too
        (`TableData::header_decoration`), so the tree does not hold two header
        contracts that disagree about whether it exists — the rule R1536
        established on the cell axis;
      * the **icon** arm on a header, drawn by the same lifted
        `decoration_node` as the swatch, from a `memory://` source.

    And it is the proof that the hole R1547 opened underneath itself is
    closed: `grid_table_nodes` stamped every `columnheader`'s name, which
    outranks the §5.40 derivation, so this meaning could not have been heard
    however correctly the header painted it.
    """
    with RpcSubprocess("hello-table", boot_grace=1.5) as tf:
        for _ in range(3):
            tf.tick(0.016)
        ns = nodes(tf, (560, 420))
        a11y = access(tf)
        headers = headers_of(a11y)
        assert headers, "the eager grid emits columnheaders"

        mark = by_tag(ns, header_deco_tag("table", 0))
        assert mark is not None, (
            "the eager surface paints the section role at the same address the "
            "virtualized one does"
        )
        assert_eq(mark.get("type"), "Image", "the header's mark is the ICON arm")
        assert str(mark.get("source") or "").startswith("memory://"), (
            f"drawn from a producer-registered buffer: {mark.get('source')!r}"
        )
        rect = mark.get("rect") or {}
        assert_eq(rect.get("w"), DECORATION_PX, "header mark width")
        assert_eq(rect.get("h"), DECORATION_PX, "header mark height")
        assert by_tag(ns, header_deco_tag("table", 1)) is None, (
            "and no other column is marked — the negative half"
        )

        assert_eq(
            headers[header_tag("table", 0)].get("name"),
            f"{KEY_MEANING} Widget",
            "the meaning joins the header's name ON THE EAGER PATH — until "
            "R1547 `grid_table_nodes` stamped 'Widget' here, and an explicit "
            "name silently outranks the derivation, so this was unhearable",
        )
        for col, label in enumerate(["Widget", "Round", "Status", "Role"]):
            if col == 0:
                continue
            assert_eq(
                headers[header_tag("table", col)].get("name"),
                label,
                f"column {col} is still named by its own painted label",
            )
        unnamed = [t for t, n in headers.items() if n.get("name") is None]
        assert not unnamed, f"every eager columnheader is named: {unnamed}"


def all_sections() -> None:
    """`run_demo` never returns (R1527 — it exits so a sweep cannot mistake a
    failing demo for a passing one), so the three apps this round touches are
    driven from ONE body rather than three calls."""
    narrow_grid()
    wide_grid()
    eager_grid()


if __name__ == "__main__":
    run_demo("R1547 §5.27 §5.40 — a header section is asked for its role", all_sections)
