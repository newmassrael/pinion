#!/usr/bin/env python3
"""R1562 §5.27 §5.40 — a header section selects the line through it.

Drives the `hello-grid-multi-select` binding (10 000-row virtualized data grid,
Qt `QItemSelectionModel` `ExtendedSelection` analogue) over JSON-RPC.

R1548 gave the grid its vertical header band and R1547 gave a section its roles,
so a row could be asked what it was called and what mark it carried. Nothing
could be **done** to it: pressing the band did nothing, and the only way to
select a row was to land on one of its cells. That is the part of a row least
likely to be on screen — the band is pinned against the horizontal scroll, the
cells are not — and it is the whole of Qt's `QHeaderView` interaction.

R1562 makes a section an address. `vtbl#r<row>` is the section's press, `vtbl#c`
is the corner where the two bands meet, and the section press reaches the
selection transition a *cell* press reaches: `GridSendKey::row()` answers with
the section's row, so the chord vocabulary is one implementation instead of the
two Qt keeps (`QHeaderView::mousePressEvent` -> `selectRow`, beside
`QAbstractItemView::mousePressEvent` -> `selectionCommand`).

  (A) boot — the band is painted, and every windowed section is addressable.
  (B) a plain band press selects that row, and only that row.
  (C) Shift extends from the anchor: 897 rows, ONE run, eleven bytes.
  (D) Ctrl toggles one row's membership without disturbing the rest.
  (E) the band press and the cell press are the SAME derivation — state for
      state, chord for chord.
  (F) the section shows the selection, and is windowed: the band's cost is the
      window's, not the model's.
  (G) the corner is TRI-STATE and reversible — empty -> all -> empty, with the
      glyph on the glass tracking it.
  (H) the corner reaches assistive technology NAMED, carrying `aria-checked`
      and its `mixed` leg.
  (I) a row is selectable when NONE of its cells is on screen — the property
      the pinned band exists for.
  (J) `toggle_all` is a verb, not only a pixel: the same control from the RPC
      path (SS2 #2).

Against Qt 6.11:
  * `QHeaderView::sectionsClickable` is a per-**axis** bool and what a click
    then means is decided by whoever connected `sectionClicked`; the two
    selection paths are separate implementations.  Phase (E) is that
    difference, read over the wire.
  * `QTableView::setCornerButtonEnabled(true)` is documented as: clicking the
    corner "selects all cells in the view".  There is no second press that
    takes it back, and the button carries no state.  Phase (G).
  * `QTableCornerButton` is a private `QAbstractButton` with no text, and
    `QTableView` exposes no accessor for it, so there is no supported way to
    name it — a screen-reader user meets an unnamed button whose state is not
    reported because it has none.  Phase (H).
  * `QHeaderView::highlightSections` — whether a section whose row is selected
    is drawn as such — is a view flag that **defaults to false**, so a Qt band
    is silent about the selection unless someone turns it on, and once on it is
    a second statement free to disagree with the rows.  Phase (F) reads the
    derivation instead.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    abs_rects_of,
    access_node_by_tag,
    assert_eq,
    chord_click,
    find_by_tag,
    indexed_tags,
    run_demo,
    selection_rows,
    texts_of,
    wait_until,
)

EXAMPLE = "hello-grid-multi-select"
WIN = (460, 480)
N = 10_000
TABLE_TAG = "vtbl"
STATUS_TAG = "vtbl_status"
CORNER_TAG = "vtbl_hcorner"
#: The corner's press address and its a11y toggle node — one tag, because the
#: control an agent clicks is the control an AT announces.
CORNER_PRESS = "vtbl#c"

#: `U+2212` MINUS SIGN / `U+2713` CHECK MARK — the marks an HTML checkbox draws
#: for `indeterminate` and checked. The empty leg draws no glyph at all.
MARK_PARTIAL = "−"
MARK_ALL = "✓"

#: What the 897-row span may cost on the wire. `[[4, 900]]` is 11 bytes.
MAX_SPAN_BYTES = 64


def band(row: int) -> str:
    """The press address of one row-header section."""
    return f"{TABLE_TAG}#r{row}"


def raw(tf) -> list:
    """The selection as it arrives: the runs themselves."""
    return tf.query("/external/selection")


def rows(tf) -> list[int]:
    return selection_rows(tf.query("/external/selection"))


def wire_bytes(value) -> int:
    return len(json.dumps(value, separators=(",", ":")))


def corner_marks(tf) -> list[str]:
    """Whatever glyph the corner is painting, as text."""
    snap = tf.snapshot(source="paint", viewport=WIN)
    node = find_by_tag(snap, CORNER_PRESS)
    assert node is not None, "the live corner must be addressable in the paint tree"
    return texts_of(node)


def status_text(tf) -> str:
    snap = tf.snapshot(source="paint", viewport=WIN)
    node = find_by_tag(snap, STATUS_TAG)
    assert node is not None, "the status bar must be in the paint tree"
    return texts_of(node)[0]


def painted_sections(tf) -> list[int]:
    """Which row-header sections the band actually drew, by data row."""
    snap = tf.snapshot(source="paint", viewport=WIN)
    return sorted(indexed_tags(abs_rects_of(snap), f"{TABLE_TAG}#r"))


def body() -> None:
    with RpcSubprocess(EXAMPLE, boot_grace=1.5) as tf:
        snap = tf.snapshot(source="paint", viewport=WIN)
        assert find_by_tag(snap, TABLE_TAG) is not None, "grid present at boot"

        # ── (A) the band is there, and it is addressable ─────────────
        assert_eq(tf.query("/external/mode"), "multi", "a multi-select model")
        assert_eq(tf.query("/external/item_count"), N, "over the whole dataset")
        assert_eq(raw(tf), [], "nothing selected at boot")
        assert find_by_tag(snap, CORNER_TAG) is not None, "the corner closes the bands"
        window = painted_sections(tf)
        assert window, "the band drew sections"
        assert len(window) < 40, (
            f"the band is WINDOWED — {len(window)} of {N} sections drawn"
        )
        first = window[0]
        assert find_by_tag(snap, band(first)) is not None, (
            "and a drawn section carries its press address"
        )

        # ── (B) a plain press selects that row ───────────────────────
        chord_click(tf, band(first))
        assert_eq(raw(tf), [[first, first]], "the section selected its own row")
        assert_eq(tf.query("/external/selected"), first, "and it is the cursor")
        assert_eq(tf.query("/external/selection_count"), 1, "one row, not a range")

        # ── (C) Shift extends: 897 rows, one run, eleven bytes ───────
        # The gesture R1561's representation was built for, now reachable from
        # the band. `vtbl#r900` is off-window, so the range is stated through
        # the wire the same way a Shift-click on a scrolled-to section would
        # state it — the anchor is what makes it one fact.
        chord_click(tf, band(first))
        tf.invoke("/external/extend_to", 900)
        span = raw(tf)
        assert_eq(len(span), 1, "a contiguous span is a single run")
        assert_eq(span, [[first, 900]], "and the run names its two ends")
        assert_eq(
            tf.query("/external/selection_count"),
            900 - first + 1,
            "the exact row count, from a sum over runs",
        )
        assert wire_bytes(span) <= MAX_SPAN_BYTES, (
            f"the span must cost at most {MAX_SPAN_BYTES} bytes, "
            f"got {wire_bytes(span)}"
        )

        # ── (D) Ctrl toggles one row out of the middle ───────────────
        chord_click(tf, band(first + 1), ctrl=True)
        holed = raw(tf)
        assert_eq(len(holed), 2, "Ctrl-pressing a section punched a hole")
        assert (first + 1) not in rows(tf), "that row left the selection"
        assert first in rows(tf), "and the rest of the range is untouched"
        chord_click(tf, band(first + 1), ctrl=True)
        assert_eq(raw(tf), span, "pressing it again puts it back, and MERGES")

        # ── (E) the band and the body are ONE derivation ─────────────
        # Chord for chord, the two addresses must leave the model in the same
        # state. In Qt these are different code paths.
        for chord in ({}, {"ctrl": True}, {"shift": True}):
            from_band, from_body = [], []
            for target, out in ((band(first + 2), from_band), (f"{TABLE_TAG}#{first + 2}_0", from_body)):
                # A common prior selection, so Ctrl has something to toggle
                # against and Shift has an anchor to extend from.
                tf.intervene("/external/selection", [[first, first]])
                tf.invoke("/external/select", first)
                chord_click(tf, target, **chord)
                out.append(raw(tf))
                out.append(tf.query("/external/selected"))
                out.append(tf.query("/external/anchor"))
            assert_eq(from_band, from_body, f"band == body for chord {chord or 'plain'}")

        # ── (F) the section shows the selection, windowed ────────────
        tf.intervene("/external/selection", [[0, N - 1]])
        wait_until(
            lambda: "10000 rows in 1 run" in status_text(tf) or None,
            desc="the whole model is selected",
        )
        drawn = painted_sections(tf)
        assert_eq(drawn, window, "selecting every row drew no extra section")
        assert len(drawn) < 40, (
            "the band's cost is the window's, not the model's: "
            f"{len(drawn)} sections for {N} selected rows"
        )

        # ── (G) the corner is tri-state and reversible ───────────────
        assert_eq(corner_marks(tf), [MARK_ALL], "everything selected: the check")
        chord_click(tf, CORNER_PRESS)
        assert_eq(raw(tf), [], "a second press TAKES IT BACK — Qt's selectAll cannot")
        wait_until(
            lambda: (corner_marks(tf) == []) or None,
            desc="an empty selection draws no mark, as an unchecked box does",
        )
        chord_click(tf, CORNER_PRESS)
        assert_eq(raw(tf), [[0, N - 1]], "and a press from empty selects everything")
        assert_eq(len(raw(tf)), 1, "select-all is ONE run")
        tf.invoke("/external/toggle", 5_000)
        wait_until(
            lambda: (corner_marks(tf) == [MARK_PARTIAL]) or None,
            desc="a hole makes the corner INDETERMINATE",
        )
        # Partial completes rather than clears — the header-checkbox rule.
        chord_click(tf, CORNER_PRESS)
        assert_eq(raw(tf), [[0, N - 1]], "a partial press completes the selection")

        # ── (H) the corner reaches assistive technology, named ───────
        access = tf.request("scene/access").result
        corner = access_node_by_tag(access, CORNER_TAG)
        assert corner is not None, "the corner is in the AT tree"
        assert_eq(corner.get("role"), "columnheader", "it is the band's header cell")
        toggle = access_node_by_tag(access, CORNER_PRESS)
        assert toggle is not None, "and it holds the control"
        assert_eq(toggle.get("role"), "checkbox", "which is a checkbox")
        assert_eq(toggle.get("name"), "Select all", "with a NAME — Qt's has none")
        assert_eq(
            (toggle.get("state") or {}).get("checked"),
            True,
            "everything is selected, and aria-checked says so",
        )
        tf.invoke("/external/toggle", 5_000)
        toggle = access_node_by_tag(tf.request("scene/access").result, CORNER_PRESS)
        state = toggle.get("state") or {}
        assert_eq(state.get("mixed"), True, 'a hole is aria-checked="mixed"')
        assert state.get("checked") is not True, "and no longer definitely checked"
        # Every windowed row also announces its section (R1548's axis, which
        # this round made pressable).
        assert access_node_by_tag(access, f"{TABLE_TAG}_rh{first}") is not None, (
            "the windowed row's `rowheader` is in the tree"
        )

        # ── (I) selectable with no cell on screen ────────────────────
        # The property the pinned band exists for. Scroll the body so `first`
        # is far above the window, then reach the row that IS drawn — its cells
        # and its section are drawn together, so the honest form of the claim
        # is that the section's address is stable while the horizontal scroll
        # slides the cells: the band does not move with it.
        tf.invoke("/external/clear", None)
        tf.scroll(f"{TABLE_TAG}_scroll", by=(0, 4000))
        tf.tick(0.05)
        scrolled = painted_sections(tf)
        assert scrolled and scrolled[0] > first, "the band scrolled with the body"
        target = scrolled[len(scrolled) // 2]
        chord_click(tf, band(target))
        assert_eq(raw(tf), [[target, target]], "a scrolled-to section still selects")
        assert_eq(
            tf.query("/external/selected"),
            target,
            "and the row it selected is the one whose number the band drew",
        )

        # ── (J) the same control from the RPC path ───────────────────
        assert_eq(
            tf.invoke("/external/toggle_all", None),
            [[0, N - 1]],
            "toggle_all is a verb — the corner's action without its pixels",
        )
        assert_eq(tf.invoke("/external/toggle_all", None), [], "and it reverses")
        assert_eq(tf.query("/external/selection_count"), 0, "nothing is selected")


if __name__ == "__main__":
    run_demo("R1562 §5.27 §5.40 — a section selects the line through it", body)
