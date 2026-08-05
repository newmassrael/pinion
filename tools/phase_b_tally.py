#!/usr/bin/env python3
"""Phase B progress tally — the evidence is counted, the judgment is dated.

Why this exists (R1519). `CLAUDE.md` carried "Phase B ~56%" from a hand tally
made at **R931**. By R1518 — 587 rounds later — the tree had gone from 20 crates
/ 115 examples / 228 demos to 27 / 197 / 474, and the number had not moved once.
The percentage was not wrong so much as UNDATED: nothing said what evidence it
was judged against, so nothing could notice it no longer described the tree. A
progress figure that cannot go stale visibly is not a measurement, it is a
slogan.

**This tool does not compute the percentage.** "How complete is the DCC widget
axis against Qt" is a judgment, and a script that emitted a number for it would
be inventing precision (the workspace's own rule against fake metrics). What a
script CAN do is:

  1. count the evidence each axis rests on, mechanically and repeatably;
  2. hold the judgment NEXT TO the evidence it was made against, with the round
     it was made in; and
  3. shout when today's evidence has drifted far enough from that snapshot that
     the judgment should be re-made.

So the number stays human, and its staleness becomes mechanical.

It also reports what it CANNOT classify. An example matching no axis is not
silently dropped — it is listed, because a body of work with no axis is exactly
how the R1372-R1442 dataviz campaign became invisible to this tally.

**Evidence kinds (R1522).** R1519 counted one artifact — example directory
names — for every axis, and for six of the eight that is exactly right: a new
widget is a new example. For the performance axis it is structurally wrong, and
two consecutive rounds proved it. R1520 (scroll paint cache, 1360us -> 42us) and
R1521 (shape cache, 27.4ms -> 1.59ms) each closed the very gap this axis's
judgment named — "no measured large-scene hot-path opt" — and each moved its
evidence by ZERO, because an optimisation creates no example. Measured while
fixing it:

  * the 476 demos were counted in the report header and used for nothing;
  * the perf axis's four patterns were the names of the four examples that
    existed at R1519 — one pattern per match — so the axis could grow only if a
    future round happened to have been named in advance;
  * demo *names* do not rescue it. 63% of them match no axis at all (29% after
    normalising `_` to `-`, which the patterns need because they are written in
    example orthography), and the perf patterns still miss R1520/R1521 because
    they name example features rather than a category;
  * demo *bodies* do. What an optimisation leaves behind is a demo asserting on
    a cost counter, and that set contains R1520 and R1521 while excluding the
    six rounds that read `frame_timings` to verify focus, window identity or
    hover.

So an axis declares its evidence as (kind, patterns) pairs. The count is only
ever compared with that axis's OWN snapshot — never between axes — so axes may
legitimately count different artifacts; what the count must do is MOVE when work
lands on the axis, and that is the property the perf axis lacked.

**Rounds, declared (R1526).** R1522 fixed the perf axis and left the mechanism
underneath it intact, so the same blindness came back on the next axis three
rounds later: R1523, R1524 and R1525 each closed a named gap in the Model/View
contract and each moved that axis's evidence by +0%. The cause is not which
artifacts an axis counts — it is that evidence is a COUNT, so only work that
CREATES an artifact can register. Depth work modifies what already exists; those
three rounds edited `hello-grid-*` examples Model/View already owned. perf's
`demo-body` probe worked only because an optimisation happens to leave a new
demo behind.

The unit that moves whether work creates or modifies is the project's own unit
of work: the round (1 commit = 1 round). R1522 said the demo-to-axis question is
"frequently unanswerable" — unanswerable by INFERENCE. The round knows, so the
round declares it in `docs/phase-b-rounds.tsv` and this counts declarations. Git
— not the ledger — enumerates which rounds exist, so a round that forgot to
declare is reported rather than silently absent, which is the direction R1522's
asymmetry says must never be silent.

**Snapshots are per kind (R1526).** R1522 summed an axis's kinds into one count
(perf reported `11` for 4 examples + 7 demos), which dilutes exactly the signal
each kind was added to carry: one depth round against 37 examples is +2.7%, so
nine of them would be needed to reach a 25% threshold that a single count of
rounds crosses at once. Each kind is now snapshotted and drifted separately and
an axis is STALE if ANY of its kinds has drifted.

Usage:
    python3 tools/phase_b_tally.py            # report
    python3 tools/phase_b_tally.py --selftest # check the tool's own logic
"""

from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

#: Evidence drift, as a fraction of the snapshot count, past which an axis's
#: judgment is called STALE. 25% is not a principled constant — it is "a quarter
#: more evidence than you judged against is enough to look again". The point is
#: that SOME threshold fires; the R931 tally drifted +71% on examples and never
#: fired anything, because there was no threshold at all.
STALE_AT = 0.25

#: How an artifact is counted, and whether every artifact of that kind is
#: expected to belong somewhere.
#:
#: A *census* kind must account for all of its artifacts: an unmatched one is a
#: finding, and listing it is how a body of work with no axis surfaces at all.
#: A *probe* kind is consulted only by the axes that declare it, so an unmatched
#: artifact is not a signal and listing them would bury the report. Demos are a
#: probe and not a census on measurement, not preference: a demo is named for
#: the round it served and EVERY round has one whatever axis it advanced, so
#: "which axis owns this demo" is frequently unanswerable — 29% of demo names
#: match no axis even after separator normalisation.
#: A census kind therefore costs one hand-written pattern per new artifact, and
#: R1545 / R1553 / R1554 / R1555 each paid it. That cost is the mechanism
#: working, not friction to engineer away: which axis an example belongs to is a
#: *judgment* — `hello-cell-editors` is arguably DCC (an editor delegate) or
#: Model/View (a grid's data path), and R1555 chose DCC — and the one derivation
#: available is worse than asking. Attributing an example to the axis of the
#: round that first added it (`git log --diff-filter=A`) is right when the round
#: created the example for its own axis and SILENTLY wrong otherwise: a perf
#: round that leaves a probe example behind, or a round that declared `none`,
#: would file evidence under an axis that did not advance. A loud UNCLASSIFIED
#: line is the better failure, for the same reason R1522 made this kind a census
#: in the first place.
CENSUS, PROBE = "census", "probe"

KINDS = {
    "example-name": CENSUS,  # examples/<name>/ — matched against the name
    "demo-body": PROBE,  # tools/demos/<name>.py — matched against the source
    "round-axis": CENSUS,  # a round — matched against the axis it declared
}

#: Where a round declares the axis it advanced, and the round below which no
#: declaration is possible. The floor is the round this tally was born in:
#: assigning axes to the 1500 rounds before it would be inventing history rather
#: than recording it, and a bound that is stated can be argued with.
ROUND_LEDGER = ROOT / "docs" / "phase-b-rounds.tsv"
LEDGER_FLOOR = 1519

#: The declaration a round makes when it advanced no Phase B axis (a process,
#: tooling or audit round). It needs a reason in the note column for the same
#: reason NOT_PHASE_B entries do.
NO_AXIS = "none"

#: `<type>(<scope>): R<NNNN> <subject>` — the subject grammar `commit-msg`
#: enforces. Anchored and first-match-only so a subject that CITES another round
#: ("R1200 revisit R744") declares one round, not two.
ROUND_SUBJECT = re.compile(r"^[a-z]+(?:\([^)]*\))?: R(\d+)")

#: The axes, their weights in Phase B, the evidence each rests on, and the
#: judgment last recorded for each.
#:
#: `evidence` is a list of (kind, patterns). Patterns are substrings; the FIRST
#: axis whose pattern matches owns the artifact, so list order is the tie-break
#: for work that touches two axes (e.g. `hello-grid-sort` is Model/View before
#: it is catalog). `gated` marks an axis that cannot be advanced from this
#: machine — it is excluded from the "buildable" subtotal so that subtotal is a
#: target that can actually be reached.
AXES = [
    {
        "key": "dcc",
        "name": "Advanced DCC / IDE widgets",
        "weight": 20,
        "gated": False,
        "evidence": [
            ("example-name", [
                "property-grid", "data-grid", "node-editor", "inspector",
                "dock-", "tree-", "tree-view", "cell-select",
                # R1563 — `column-reorder` / `column-visibility` rather than
                # `column-`: this axis owns the two header-MANIPULATION
                # bindings, and the unbounded prefix took `hello-column-select`
                # with them, which is a Model/View selection grid. The R1560
                # finding again, one axis over — an unbounded substring credits
                # an axis with work that is not its own, and it does it
                # silently, because the round that adds the example sees only a
                # total go up.
                "column-reorder", "column-visibility",
                "cell-editor",
                "asset-browser", "file-manager", "undo", "grid-header-menu",
                "grid-frozen-col", "row-dissect", "hex-dump", "code-fold",
                "command-palette", "selection-toolbar", "tab-reorder",
                "dock-presets",
            ]),
        ],
        # R1532 re-judgment, demanded by this axis's round count going 0 -> 1
        # — the first round to declare `dcc` since the ledger existed.
        #
        # It corrects the judgment as well as moving it, which is the third
        # time a demanded re-judgment has done that (R1528 is the lineage).
        # R1519 said 85% and named three remaining items: node *evaluation*,
        # advanced delegates, and per-element modified-reset. **Two of the
        # three were already closed when it was written** — node evaluation
        # landed R1255-R1264 and the modified indicator + reset arrow landed
        # R958 — so the stated gap had been describing work already done for
        # ~250 rounds.
        #
        # R1532 closes the PAINT half of the third: a column can now declare
        # how its cells are drawn (Qt `setItemDelegateForColumn`), which is
        # the extension point that decides whether a grid can have a bar
        # column, a mark column or a swatch column at all. Before it, a
        # binding wanting one had to stop using the grid's cell path — which
        # is exactly what `hello-property-grid`'s `ranged_slider_cell` does.
        #
        # Only +3, and the remaining item is verified rather than assumed:
        # the delegate covers paint and not EDITING. Qt's
        # `QStyledItemDelegate` also owns `createEditor` / `setEditorData` /
        # `setModelData`, and every editable grid here still hand-rolls its
        # own edit latch. The seam also has one consumer — six example
        # bindings still build cell subtrees outside the grid's cell path.
        #
        # Deliberately NOT counted as remaining: comment frames and marquee
        # box-select in the node editor, which an audit this round found
        # already present (R1227). A gap list is worth only what it is
        # checked against.
        #
        # ---- R1544 re-judgment, 88 -> 92, demanded by the round count going
        # 1 -> 2. It closes the item R1532 itself named as the largest one
        # left, and closes it WHOLE rather than by half:
        #
        #   * the MODEL's `Qt::EditRole`, fused with `flags() &
        #     Qt::ItemIsEditable` into one `Option<CellEdit>` — so "an editor
        #     open on a cell the model will not edit" stopped being a check
        #     the view must remember and became a state the types reject;
        #   * the DELEGATE's editing half (`createEditor` + `setEditorData`
        #     collapse into one call in a view-fn world, `setModelData` stays
        #     separate because it is a distinct moment);
        #   * the VIEW's half — the latch, Qt's `EditTriggers` gate, and the
        #     `EndEditHint` cursor walk over the MODEL extent.
        #
        # Two things past Qt 6.11, both verified over the wire: a **refused**
        # write keeps the editor open holding the typed text (Qt's
        # `setModelData` returns `void`, so a rejected value closes the editor
        # and the typing is gone), and a cell's editability reaches assistive
        # technology as `aria-readonly` (Qt's `QAccessibleTableCell` builds its
        # state from the view's selection and never reads the model's
        # `ItemIsEditable`, so a Qt screen-reader user cannot tell a fixed
        # column from an editable one until they type into it).
        #
        # +4 and not more, because what remains is real and audited at R1544:
        #   * ADOPTION is one binding. Six still hand-roll a cell edit latch
        #     (`hello-data-grid`, `hello-property-grid`, `hello-inspector`,
        #     `hello-node-editor`, plus two rename editors) and NONE of them
        #     uses the grid's cell path at all — two do not use the grid
        #     painter. Migrating them is per-binding domain work, not seam
        #     work, which is why it is remainder rather than a carry.
        #   * `openPersistentEditor` (N simultaneously open editors) is
        #     absent: it needs N independent text-edit states, and
        #     `use_text_edit_state` is keyed by `&'static str`.
        #   * the built-in editor is a text field, so `CellKind::Choice` and
        #     `CellKind::Color` reach an editor only through a delegate. Qt
        #     has the same split (`QItemEditorFactory`); what is missing here
        #     is a *shipped* combo / palette editor.
        #
        # R1555 re-judgment, demanded by the tool: the round ledger took this
        # axis 2 -> 3, past the 25% band. It closes the third of the items
        # R1544 left, and closes it WIDER than that item was stated. R1544 read
        # the gap as two missing widgets; it was a missing **axis**. Qt's
        # editing decomposition has two halves — `setItemDelegateForColumn`
        # (the per-column override, which R1532 and R1544 built) and
        # `QItemEditorFactory` (a registry from the DATUM'S TYPE to an editor,
        # which `QStyledItemDelegate` consults when nothing overrides it). The
        # second did not exist, so `text_cell_editor` was the built-in for all
        # six kinds, and for two of them it is an editor that CANNOT WORK:
        # `Bool` and `Choice` refuse every keystroke (`accepts_keystroke`) and
        # parse to nothing (`parse`), so the seam opened a field that could not
        # be typed into and whose commit could never produce a value. Two more
        # were simply below Qt: `Int` / `Float` got a bare field where Qt's
        # factory ships a `QSpinBox` / `QDoubleSpinBox`.
        #
        # `CellKind::editor_form` is the registry and `EditorForm` its answer,
        # a pure function — where `createEditor` *instantiates a QWidget*, so
        # Qt cannot be asked what an `int` cell would get without building one,
        # and `creatorMap` is private so the registry cannot be enumerated at
        # all. Five forms ship (field / stepper / toggle / selector / swatch),
        # `scene/cell_editors` publishes the whole mapping, and the a11y role
        # is derived from the form rather than from whichever widget a factory
        # happened to construct — which is how a Qt bool cell ends up
        # announcing as a COMBO BOX and a Qt colour cell announcing nothing.
        #
        # Reaching for it forced the model half too: `CellEdit` now carries the
        # DATUM (Qt's `EditRole` is a `QVariant`), because a `Choice`'s options
        # are part of its value's identity and a `(kind, String)` pair cannot
        # tell a selector what to select between. That also retired a
        # representable-but-meaningless edit role, `(Int, "not a number")`.
        #
        # Four things past Qt 6.11, all read over the wire: the factory is
        # ENUMERABLE; a bool gets a checkbox rather than Qt's two-item combo;
        # a colour cell is editable at all (Qt's factory has no `QColor`
        # creator, so `createEditor` answers nullptr and
        # `QAbstractItemView::edit` silently does nothing); and every commit
        # outcome is NAMED — `malformed` (the model was never asked) apart from
        # `refused` (it was), where Qt's `commitData` discards `setData`'s
        # verdict and its validators make the malformed case unreachable at the
        # price of committing a value the user did not type.
        #
        # +3 and not more, and the remainder is audited at R1555 rather than
        # inherited:
        #   * ADOPTION is now two bindings (`hello-grid-nav` +
        #     `hello-cell-editors`) against the same six hand-rollers. R1544's
        #     reading stands: migrating them is per-binding domain work.
        #   * `openPersistentEditor` is still absent — and R1544's stated
        #     BLOCKER was wrong. `Owner::cache` has taken
        #     `impl Into<Cow<'static, str>>` since R685.C, precisely so runtime
        #     ids allocate without `Box::leak`, so a runtime-keyed text-edit
        #     state was always possible; only `use_text_edit_state`'s own
        #     signature is `&'static str`. What the feature actually needs is
        #     that signature widened, the open latch becoming a map, and the
        #     binding threading a `TextFieldState` per open editor.
        #   * two of Qt's factory creators have no form because they have no
        #     KIND: `QDate` / `QTime` / `QDateTime` (there is no date arm on
        #     `CellKind` at all) and `QMetaType::UInt`.
        #   * the step arrows do not AUTO-REPEAT. R1549 made a cadence a
        #     per-widget declaration (`External::auto_repeat`, level-read from
        #     the widget's own state), and a grid's step arrow is a sub-region
        #     of one External that covers every cell — so repeating it needs a
        #     per-sub-region cadence, or the External tracking which sub-key is
        #     held, which duplicates the router press record R1549 put the run
        #     inside on purpose. Qt's `setAccelerated` has it; the same shape
        #     R1549 recorded for scrollbar arrows.
        #   * a selector's open list and a swatch's HSV picker are the
        #     binding's overlays; the factory ships the closed states.
        "judged_at": 1555,
        "completion": 95,
        "evidence_snapshot": {"example-name": 27, "round-axis": 3},
    },
    {
        "key": "modelview",
        "name": "Model/View at scale",
        "weight": 16,
        "gated": False,
        "evidence": [
            ("example-name", [
                "virtual-", "lazy-", "million-row", "paged-stream",
                "async-data", "measured-list", "variable-list", "grouped-",
                # `-table` rather than `table`: R1560 found `hello-combobox-editable`
                # counted here, because "editable" CONTAINS "table". A substring
                # census with no word boundary credits an axis with work that is
                # not its own, silently and forever — the axis had been carrying
                # a combobox as Model/View evidence since the pattern was
                # written. The leading hyphen is the boundary the names already
                # have.
                "-table", "grid-", "streaming-log", "tail-reveal", "live-data",
                "multi-select", "listbox", "flex-virtual", "column-select",
            ]),
        ],
        # R1526 re-judgment, forced by this round's own change: introducing the
        # `round-axis` kind takes this axis's declared rounds from a snapshot of
        # 0 to 3, and R1522's rule is that changing the UNIT of evidence without
        # re-judging leaves the number and the evidence in different units.
        # R1519 said 75% on windowing (list/grid/tree) + three composable
        # proxies + data-indexed selection + async/lazy + an LRU million-row
        # source. The three rounds now declared closed the core of Qt's
        # QAbstractItemModel data path: R1523 windows the column axis as well as
        # the row axis (200 -> 5 cells a row), R1524 makes the contract per-cell
        # rather than per-row (`data(QModelIndex)`; 2400 -> 84 cells asked a
        # frame), R1525 makes the painted string the one the ordering read.
        # R1530 re-judgment, demanded by the tool: the round ledger took this
        # axis 3 -> 4, past the 25% band. R1526 named exactly two remaining
        # gaps and R1530 closed the first of them — header data was per-slice
        # (`headers: &[&str]` for all 200 columns where Qt's `headerData` is
        # per-section) because `VirtualTableData` read its column count off
        # that slice's length; `column_count` + `GridModel::header` split the
        # two the way `columnCount()` / `headerData()` are split, and the a11y
        # builder takes the window rather than the table.
        #
        # +3 and not more, because the gap that is left is the LARGER of the
        # two R1526 named: `cell` and `header` both return a String with no
        # role dimension (Qt's Display/Edit/Decoration/ToolTip), which is a
        # whole axis of the contract rather than one accessor's shape — it is
        # what a decorated cell, an edit-vs-display value and a tooltip all
        # need. R1530 also surfaced three smaller ones: the eager `view_table`
        # still takes a header slice (two header contracts in one tree), five
        # of the six a11y grid builders still take every label, and a binding
        # still states its column window twice (paint + a11y).
        # Unified data layer stays out by the R780/R821 fourth-consumer gate,
        # not by omission.
        # R1536 re-judgment, demanded by the tool: the round ledger took this
        # axis 4 -> 6, past the 25% band. R1530's judgment named the role
        # dimension as the LARGER of the two gaps it left, and R1535 + R1536
        # closed it on the CELL axis — not merely opened it. `GridModel` gained
        # `decoration` as a third typed accessor (Qt `data(index,
        # Qt::DecorationRole)`, asked per cell, which is the axis a per-column
        # delegate cannot express); the answer carries a `meaning` beside its
        # ink, which Qt does NOT (its decoration role is appearance and the
        # accessible text is a separate role the item view never wires to it,
        # so a colour-only status column is an empty cell to a Qt screen-reader
        # user); the mark is addressable by `GridTag::cell_decoration`; it has
        # both of Qt's arms (QColor, QIcon); and the EAGER `view_table` answers
        # the same role, so the tree no longer holds two cell-paint contracts
        # that disagree about whether it exists.
        #
        # R1536 also fixed what reaching for that found underneath, which is
        # the larger part of this +4: the accessible-name derivation could not
        # enter a `ScrollNode`, so NOTHING in any virtualized list, grid or
        # tree was named to an AT — measured, `hello-virtual-table` 0 of 75
        # gridcells, `hello-virtual-list` 1 of 16 — while the bounds walker
        # descended fine and made the tree look correct. Qt names its cells;
        # this axis did not.
        #
        # +4 and not more, because what is left is verified rather than
        # assumed (checked at R1536, not carried from R1530): the HEADER axis
        # has no role dimension at all — the largest item on this axis now —
        # and two of Qt's four canonical roles stay unanswerable, `EditRole`
        # behind the delegate's absent editing half and `ToolTipRole` behind a
        # per-cell hover path. R1530's three smaller ones were re-checked and
        # all three still hold: the eager `view_table` still takes a header
        # slice, five of the six a11y grid builders still take every label, and
        # a binding still states its column window twice (paint + a11y).
        # R1547 did NOT force a re-judgment (round ledger 6 -> 7, +17%, inside
        # the band) and the number stayed at 87 with the gap statement updated:
        # it OPENED the header axis's role dimension on the horizontal axis
        # (`header_decoration`, Qt `headerData(section, Qt::Horizontal,
        # Qt::DecorationRole)`) and named the axis's own largest remainder —
        # there was no VERTICAL section axis at all.
        #
        # R1548 re-judgment, demanded by the tool: the round ledger took this
        # axis 6 -> 8, past the 25% band. It closes that named item whole. Qt
        # spells both axes with one virtual (`headerData(section, orientation,
        # role)`) and a `QTableView` shows the vertical one by default; here a
        # column could be asked what it was called and what mark it carried,
        # and a ROW could be asked nothing — no row numbers, no pin, no lock,
        # no breakpoint gutter, the whole left-hand band a professional table,
        # editor or profiler has.
        #
        # It lands as a TYPE, not a second pair of accessors: `HeaderAxis<L,
        # D>` holds the two roles a section answers, `GridModel::columns` and
        # `GridModel::rows` are both one, so the axes answer the same role set
        # by construction and one lifted painter (`section_content`) draws
        # either. It reaches all three surfaces — unsplit grid, frozen split,
        # eager `view_table` — from one composition point, and the a11y half is
        # a PASS (`attach_row_headers`, the R1544 `mark_grid_editability`
        # shape) rather than a seventh builder variant, so it lands on every
        # topology including the permuted one.
        #
        # Two things past Qt 6.11, both read over the wire: an unanswered axis
        # is a DECLARATION, not a blank strip (Qt's orientation is a runtime
        # argument, so the commonest `QAbstractTableModel` bug there — handle
        # `Qt::Horizontal`, fall through returning `QVariant()` — paints
        # sections that still occupy their width and is reported by nothing;
        # here `no_row_header()` is written down, the band is not painted, the
        # model is asked ZERO times a frame, and painted-iff-answered is
        # structural because there is no second "show the header" flag); and
        # the mark's MEANING reaches assistive technology
        # (`QAccessibleTableHeaderCell::text(Name)` answers from
        # `Qt::DisplayRole` on both orientations, so a Qt row header whose
        # distinguishing information is its glyph announces only the number).
        #
        # +4 and not more, audited at R1548: a section axis answers 2 of Qt's
        # roles (`ToolTipRole` / `TextAlignmentRole` / `InitialSortOrderRole` /
        # `SizeHintRole` all absent on a header); the row axis has NO
        # interaction (Qt's `QHeaderView` section click selects the row, and
        # its sections resize — row height here is one grid-wide pitch the
        # windowing arithmetic is built on); the band's width is stated rather
        # than `ResizeToContents`; and R1530's last small one now holds on both
        # axes — a binding states its row window twice (paint + a11y) as it
        # already did its column window.
        #
        # R1563 re-judgment, DEMANDED by the tool: the round ledger takes this
        # axis 8 -> 11 (+37.5%), past the band, and it absorbs THREE rounds —
        # R1561 and R1562 each landed at or inside the edge and deferred their
        # look (R1562 at exactly +25%, and the test is `> 25%`).
        #
        # Between them they close the item this axis's own gap statement named
        # twice running, and named as the largest one left: THE SELECTION HAD
        # ONE AXIS. R1561 made it a set of runs rather than of rows (a
        # `Ctrl+A` over 10 000 rows answered `query("selection")` with 58 890
        # bytes in 10.9 ms for a fact whose statement is eleven); R1562 made
        # the vertical band's section press select the row through it, by the
        # derivation that a section ANSWERS WITH ITS ROW; and R1563 gave the
        # model the column axis those two kept arriving at the edge of — a
        # column header selects the column through it, a cell press selects a
        # cell, and `Shift` grows a rectangle.
        #
        # The shape is the round's argument rather than a detail: a set of
        # cells has no unique minimal decomposition into rectangles (a cross is
        # two rectangles two ways, both minimal), and this framework's
        # selection is CANONICAL because that is what lets it report whether an
        # interaction changed anything. So `CellSelection` holds the function
        # row -> column set GROUPED BY ITS VALUE — one band per distinct
        # `ColumnSpan` — which is unique by construction. Past Qt: `ColumnSpan`
        # carries no column count, so a record stays whole when the schema
        # grows, where a Qt range built against `columnCount() - 1` is silently
        # demoted and drops out of `selectedRows()`.
        #
        # +4 and not more, and the remainder is audited at R1563 rather than
        # carried: the section axis still answers 2 of Qt's roles on both axes
        # (`ToolTipRole` / `TextAlignmentRole` / `InitialSortOrderRole` /
        # `SizeHintRole`); the band's width is stated rather than
        # `ResizeToContents`; a binding still states its row window twice
        # (paint + a11y) and `virtual_grid.rs` still has two row emitters;
        # DRAG-select across sections is still blocked on a substrate absence
        # the pointer wire has (it does not say whether a button is held —
        # W3C `PointerEvent.buttons`); the KEYBOARD has no two-axis vocabulary
        # (Qt's `Ctrl+Space` on a cell, `Ctrl+Shift+Arrow` growing a
        # rectangle), which is this round's own new gap; the `SelectColumns`
        # arm has no binding; and R1563 FOUND one this axis had never named —
        # the eager `Table` holds its own single-rectangle cell selection
        # (R952), so the tree now has two cell-selection models, one canonical
        # and windowed, one a rectangle bounded by a model small enough to
        # materialise.
        "judged_at": 1563,
        "completion": 95,
        "evidence_snapshot": {"example-name": 37, "round-axis": 11},
    },
    {
        "key": "catalog",
        "name": "Common widget catalog + interaction",
        "weight": 16,
        "gated": False,
        "evidence": [
            ("example-name", [
                "button", "checkbox", "radio", "toggle", "slider",
                "spinbutton", "number-input", "combobox", "tabs", "toolbar",
                "menu", "dialog", "tooltip", "popover", "accordion",
                "disclosure", "drawer", "snackbar", "badge", "fab", "rating",
                "chip", "card", "stepper", "nav-rail", "pagination",
                "breadcrumb", "segmented", "progress", "status-bar",
                "datepicker", "color-picker", "contextmenu", "hyperlink",
                "theme", "gradient", "path", "timeline", "transport",
                "scrubber", "image", "commands", "dnd", "range-slider",
                "popup", "gesture", "pinch-zoom", "smart-zoom", "raw-pointer",
                "crosshair", "settings-panel", "todomvc", "figma-",
                # R1554 — the group box. A titled frame that gates its
                # contents is a catalog widget; the pattern list is a census,
                # so a member with no pattern is reported UNCLASSIFIED rather
                # than counted somewhere convenient.
                "group-box",
            ]),
        ],
        # R1533 re-judgment, 82 -> 84, demanded by the tool: the round ledger
        # took this axis 0 -> 1, which `drift` reads as movement whatever the
        # count. What the re-judgment mostly buys is the JUDGMENT ITSELF —
        # this axis reached the top of the leverage order carrying a bare
        # number with NO recorded rationale and NO stated remaining gap, while
        # every other axis held its gap list right here. R1528 and R1532 each
        # found a stated gap describing finished work; an axis with nothing
        # stated cannot even be caught that way.
        #
        # R1533 gave the two stepped value widgets `External::wheel` (Qt
        # `QAbstractSlider::wheelEvent` / `QAbstractSpinBox::wheelEvent`) plus
        # the `WheelStepper` sub-notch carry they need. The hook had existed
        # since R877 and a census found ONE implementor in the repo (the node
        # canvas' zoom), so no widget in the catalog answered a wheel.
        #
        # Only +2, because the audit that produced the gap list below found
        # MORE absent surface than the round filled — the R1528 pattern, where
        # naming a dimension for the first time grows the stated gap:
        #
        #   * ~~Mnemonics / accelerators~~ — CLOSED R1543. It was the first
        #     item R1533 listed and the largest, because it is not one
        #     widget: it is an axis every labelled widget sits on. R1543
        #     landed Qt's `&`/`&&` vocabulary as ONE declaration on the
        #     painted label, from which the underline ink (a `StyleRun`, so
        #     both painters draw it with no per-backend code), the Alt+char
        #     binding (derived from the PAINT scene, so it cannot disagree
        #     with what the user sees underlined) and the AT `accesskey` are
        #     all derived. Past Qt in four places: the map is published
        #     (`scene/mnemonics`; Qt's lives in the private
        #     `qshortcutmap_p.h`), a conflict is a STATIC property of the
        #     scene rather than a bool on the event the user triggered, the
        #     ink and the binding come from one parse instead of Qt's two,
        #     and `accesskey` stays distinct from `keyboard_shortcut` where
        #     `QAccessible::Accelerator` collapses them.
        #   * ~~Press-and-hold auto-repeat~~ — CLOSED R1549. It was the
        #     item R1543 named FIRST and called the largest cross-cutting
        #     one left, and it was 100% absent: holding a spin arrow
        #     stepped ONCE, and the tree contained no repeat timer at all
        #     (the one `auto_repeat` hit was OS *key*-repeat SUPPRESSION,
        #     R1071 — a different thing). R1549 made the cadence a
        #     DECLARATION the widget answers (`External::auto_repeat`) and
        #     gave the router the clock; a fire re-dispatches the widget's
        #     own `PointerUp`+`PointerDown` arc, so a repeat is a click by
        #     the same derivation and no widget needed a new SCXML
        #     transition. Past Qt in three places, all read over the wire:
        #     the hold is DRIVABLE AS DATA (Qt's `QBasicTimer` cannot be
        #     told "hold for 900 ms"; this rides the `scene/tick` clock and
        #     the demo asserts exact fire counts with no tolerance), the RUN
        #     IS PUBLISHED and predictive (`scene/auto_repeat` gives target
        #     / repeating / cadence / fires / seconds-to-next, where Qt's
        #     only public fact is a static per-widget property), and a held
        #     arrow AT ITS BOUND stops (`QAbstractSpinBox` keeps its 10 Hz
        #     timer running against a value pinned at `maximum()`).
        #     Armed-ness is re-ASKED every frame instead of stored, and the
        #     run lives IN the R876 press record, so Qt's runaway-timer bug
        #     class has nowhere to live. Adoption is COMPLETE for the widget
        #     classes that can express a hold — all three that own `Button`
        #     sub-regions (`ButtonExternal` opt-in as `QPushButton` is,
        #     `SpinButtonExternal` and `PaginationExternal` on by default as
        #     Qt's spin arrows are).
        #   * Qt also has `wheelEvent` on `QComboBox` and `QTabBar`; R1533
        #     covered value arithmetic, not index arithmetic. Re-checked at
        #     R1554: `External::wheel` still has exactly the two CATALOG
        #     implementors R1533 added (`slider`, `spin_button`; two more
        #     live in bindings' own Externals), so this is still the largest
        #     cross-cutting item left.
        #   * NEW at R1543 — the capability is universal but ADOPTION is
        #     FOUR sites (menu titles, menu items, one buddy label, and
        #     R1554's group-box legend — a new widget adopting it is how
        #     this number moves, one helper at a time). Every
        #     other catalog paint helper takes a plain `&str` label and calls
        #     `TextNode::styled`, so `&Save` on a button is inert until each
        #     helper routes through `TextNode::mnemonic_styled`. Deliberately
        #     not done blind: a helper whose label ALSO feeds a hand-passed
        #     a11y name has to resolve the markup there too, which R1543 hit
        #     once (`menu_item_nodes`) and did not audit for across the tree.
        #   * Absent widget kinds. `QGroupBox` — the one R1549 put FIRST
        #     and called out as "especially checkable" — is CLOSED R1554;
        #     re-censused there, the other five are still absent: `QDial`
        #     (no dial or knob; the one `dial` hit is a rotate GESTURE
        #     example), a paged container (`QStackedWidget` / `QWizard`),
        #     `QKeySequenceEdit`, `QFontComboBox`, and the standard
        #     `QMessageBox` / `QInputDialog` canned dialogs — each of the
        #     five appears in this tree only inside a doc comment.
        #
        # R1549 re-judgment, 87 -> 90, demanded by the tool (round ledger
        # 2 -> 3). +3, the same calibration R1543 got for mnemonics and for
        # the same reason: what closed is not one widget but an axis every
        # pressable widget sits on, it was wholly absent, and it closed past
        # Qt in three places. Unlike R1543 it also added NO gap of its own —
        # adoption is complete for the widget classes that can express a
        # hold. Not more than +3 because the audit that produced this list
        # was RE-RUN at R1549 rather than inherited ([[r1532-column-declares
        # -its-painter]]: a gap list is worth only what it is checked
        # against), and every other item still stands, verified by census:
        # `External::wheel` still has exactly two implementors, mnemonic
        # adoption is still three sites, and all six absent widget kinds are
        # still absent (no `group_box` / `fieldset`, no dial or knob, no
        # stacked-page or wizard container, no `QKeySequenceEdit`, no font
        # combo, no canned message / input dialog). Six absent kinds is a
        # lot of surface for an axis whose name is "catalog".
        # R1554 re-judgment, 90 -> 93, demanded by the tool (round ledger
        # 3 -> 4). It closes the item R1549's list named FIRST among the
        # absent widget kinds and flagged as the one a pro tool misses most —
        # `QGroupBox`, "especially checkable" — and what made it absent was
        # never the frame. It was that `setCheckable(true)`'s whole point,
        # clearing the title checkbox to make the panel inert, was
        # INEXPRESSIBLE: `LayoutStyle` carried four interaction declarations
        # (`pointer_transparent`, `focusable`, `drop_target`, `cursor`) and
        # every one described the node carrying it and nothing else. Qt's
        # `QWidget::setEnabled` is the one that is INHERITED.
        #
        # So the round is a scene declaration (`with_disabled`) plus four
        # derivations, each resolved where that consequence is already
        # decided — the §5.39 focus enumeration, `Scene::hit_test`, the a11y
        # assembler's stamp, and the ink — and it rides
        # `settle_to_fixed_point`, the one loop every paint-scene producer in
        # both backends passes through, so a window and a terminal cannot
        # disagree about which controls are inert. Past Qt 6.11 in four
        # places, all read over the wire: the CAUSE is published by name
        # (`scene/disabled`'s `declared_by`; Qt's `isEnabled()` is a bool and
        # `isEnabledTo()` needs the caller to have already guessed the
        # ancestor), the SET is enumerable at all (Qt has no such query), a
        # refusal has a NAME (`focus/set` -> `tag_disabled` handing back the
        # region, where `QWidget::setFocus()` is a silent no-op), and whether
        # the INK followed is stated per node rather than left to be
        # discovered from a screenshot. The derived half is recomputed every
        # paint instead of written into descendants, which is what Qt's
        # `setEnabled_helper` does and must walk back.
        #
        # +3 and not more. Five of the six absent widget kinds remain, the
        # wheel item is untouched and still the largest cross-cutting one,
        # and the round adds gaps of its own, audited at R1554: the cascade
        # has ONE consumer (every other catalog widget still expresses
        # disabledness only through its own state enum, so a form cannot gate
        # a section without a group box), and four node kinds carry content
        # the fade cannot reach (`Image` / `External` / `ImmediateModeNode` /
        # `TextGrid`) — Qt cannot grey a `QOpenGLWidget` either, so it is
        # stated on the wire rather than fixed.
        "judged_at": 1554,
        "completion": 93,
        "evidence_snapshot": {"example-name": 74, "round-axis": 4},
    },
    {
        "key": "dataviz",
        "name": "Charting / data visualisation",
        "weight": 10,
        "gated": False,
        # R1519 — this axis did not exist in the R931 tally, which is why the
        # entire R1372-R1442 campaign (22 examples, 72 demos, `pinion-chart` +
        # `pinion-graph`) could not move the Phase B number by a single point.
        # Qt ships QtCharts, so under the qt-parity directive it is in scope.
        #
        # R1528 re-judge, 65 -> 68, and the tool demanded it: a round declared
        # this axis where the snapshot was 0, which `drift` reads as movement
        # whatever the count. Small on purpose. R1528 landed a logarithmic
        # value axis (Qt `QLogValueAxis`) on both cartesian axes of the two
        # numeric-x charts — one of QtCharts' FIVE axis types.
        #
        # What the re-judgment mostly bought is a correction to the judgment
        # itself: R1519 named the remaining gap as series types (polar,
        # candlestick, 3D surface) and interaction depth, and did not mention
        # AXIS types at all. Naming that dimension reveals more absent surface
        # than R1528 filled — there is still no datetime axis, which every
        # monitoring chart in the world has and whose absence makes x a bare
        # number today. So the axis moves three points and its stated gap
        # grows.
        #
        # R1529 re-judge, 68 -> 72, demanded by the same mechanism (a second
        # declared round doubles a snapshot of 1). This closes the gap the
        # R1528 re-judgment had just named as the largest one: the datetime
        # axis (Qt `QDateTimeAxis`, d3 `scaleUtc`) on both cartesian axes of
        # the two numeric-x charts plus the timeline ruler. Four points, one
        # more than the log axis got, because a monitoring chart's x-channel
        # is the commoner need — and only four, because it closes UTC and not
        # local time.
        #
        # The dimension R1528 opened stays the useful one, and building the
        # third kind sharpened what remains on it. Of QtCharts' axis classes
        # the crate now has value, log and datetime as interchangeable
        # `ValueScale` arms — but **category is not an axis kind here at
        # all**: the bar chart's x is a `BarGeom` slot metric on a separate
        # code path, so no chart can swap a category axis in the way it can
        # now swap the other three. R1528 recorded that as "no category axis
        # outside the bar chart's slots"; the shape of the gap is now
        # structural rather than a missing variant. Untouched otherwise: no
        # polar / candlestick / box-plot / spline / 3D-surface series, and no
        # plot-level zoom or pan — which is the bulk of what is left.
        "evidence": [
            ("example-name", [
                "chart", "scatter", "heatmap", "treemap", "donut", "histogram",
                "legend", "brush", "elevation", "market-map", "stat-tiles",
                "topology", "series-toggle", "rescale-toggle", "autoscale-y",
                "cross-filter", "live-data", "deviation-grid",
                # R1545 — an axis-KIND consumer need not be named "chart":
                # `hello-category-axis` plots a bar chart and a line chart
                # from one axis, and the census flagged it UNCLASSIFIED
                # because every pattern here named a chart TYPE.
                "axis",
                # R1553 — `hello-boxplot`, flagged UNCLASSIFIED by the same
                # census for the same reason the last one was: a chart type
                # this axis did not yet name.
                "boxplot",
                # R1568 — `hello-polar`. The FOURTH consecutive arrival this
                # census could not name, and the first that is not a series
                # type at all: a polar plot is a coordinate SYSTEM, which the
                # note below did not anticipate when it called the lag a
                # property of naming chart types.
                "polar",
                # R1567 — `hello-candlestick`, the THIRD consecutive series
                # type this census flagged UNCLASSIFIED on arrival. That is
                # the pattern list's shape, not three oversights: naming chart
                # TYPES means every new one is invisible until someone adds
                # it, so the census is a lagging indicator by construction.
                # Left as a list rather than fixed, because the alternative —
                # matching any example that depends on `pinion-chart` — would
                # credit this axis for a consumer that merely embeds a chart
                # (a dashboard, a dock pane), which is the false-positive the
                # census exists to avoid.
                "candlestick",
            ]),
        ],
        # R1534 re-judgment, 72 -> 77, demanded by the tool (the round ledger
        # took this axis 2 -> 3). The largest single move in this series so far,
        # and deliberately so: R1529's stated gap named plot-level zoom and pan
        # as "the bulk of what is left", and R1534 closed half of that item —
        # direct manipulation of the x-window now exists (`PlotWindow`, a wheel
        # vocabulary on the plot area, `plot_area` made public so an overlay
        # covers the axis rather than the chart rect).
        #
        # HALF, and the audit that produced this list is what keeps it to +5:
        #
        #   * No drag pan and no rubber-band zoom (QtCharts
        #     `QChartView::setRubberBand`). An `External` has no pointer-down /
        #     pointer-up hook, so a press-drag needs either the raw-pointer
        #     seam or a slider-style statechart — a design choice R1534 did not
        #     have to make and should not make by accident.
        #   * The window is x-only. QtCharts zooms a RECT; there is no y-window
        #     and no diagonal drag-select. (`hello-autoscale-y` fits y TO the
        #     x-window, which is a different thing.)
        #   * The window is invisible to a screen reader — the status line is a
        #     text node like the caption, where the brush at least carries a
        #     range-slider role.
        #   * One consumer. The four brush consumers were not given plot zoom,
        #     so the two ways of windowing one axis never appear on one plot,
        #     which is where the interesting question lives (does a wheel move
        #     the strip's thumbs?).
        #
        # Unchanged from R1529: local time needs a tzdb; **category is not an
        # axis kind here at all** (the bar chart's x is a `BarGeom` slot metric
        # on a separate code path); no polar / candlestick / box-plot / spline /
        # 3D-surface series.
        #
        # R1545 re-judgment, 77 -> 82, demanded by the tool (the round ledger
        # took this axis 3 -> 4). It closes the item the last TWO re-judgments
        # both named, and closes it whole: **category is an axis kind now**.
        # `Categories` / `CategoryScale` are the fourth `AxisKind` arm (Qt
        # `QBarCategoryAxis`, d3 `scaleBand`), the bar chart's private slot
        # metric IS that axis, and `LineChart::x_category` /
        # `ScatterChart::x_category` swap it into a numeric-x chart the way
        # the log and time kinds already swapped. Of QtCharts' axis classes
        # the crate now has four of five interchangeable.
        #
        # Two things past Qt 6.11, both read over the wire by the demo:
        # `CategoryScale::band` publishes where a category is DRAWN (a Qt bar's
        # rect is computed inside the private `QBarSeriesPrivate` painter, and
        # the absence of that accessor is exactly why `bar.rs` carried three
        # copies of `left + i * slot`), and a window is resolved from NAMES
        # before it can reach a chart — `Categories::window` answers a
        # `Result`, where `setRange(QString, QString)` returns `void` and
        # silently ignores a name that is not a category.
        #
        # +5 and not more, the same size R1534 got for half of its item,
        # because the remaining list is long and mostly untouched. Audited at
        # R1545:
        #
        #   * Qt's OTHER category axis, `QCategoryAxis` — labels attached to
        #     arbitrary value RANGES rather than to discrete slots — is absent.
        #     It is a different kind, not a variant of this one.
        #   * Label thinning is absent: a windowless 60-category axis labels
        #     all 60 and they collide. How many labels fit is a measured-TEXT-
        #     WIDTH decision and a scale has no text measurement, so
        #     `axis_ticks` ignores its tick target on this kind.
        #   * A slot has no band-level a11y. R1545's consumer names the WINDOW
        #     to an AT; an individual category label is painted text with no
        #     accessible relationship. Qt is the same, so a stated limit.
        #   * Still open from R1534, all four: no drag pan / rubber-band zoom
        #     (an `External` has no pointer-down hook), no y-window, the plot
        #     zoom is invisible to a screen reader, one consumer.
        #   * Still open from R1529: local time needs a tzdb.
        #   * Still open from R1519: no polar / candlestick / box-plot /
        #     spline / 3D-surface series — a whole dimension, untouched.
        #
        # R1553 re-judgment, 82 -> 87, demanded by the tool (the round ledger
        # took this axis 4 -> 5). It opens the dimension the line above calls
        # untouched, and opens it at the member with the most statistics
        # behind it: the BOX PLOT (Qt `QBoxPlotSeries`).
        #
        # What earns +5 rather than the +2 a bare renderer would: this is the
        # crate's first datum that is NOT A POINT. Every value it could plot
        # resolved to one position; a `Distribution` occupies a span of the
        # value axis and carries interior landmarks, so one datum emits a box,
        # a median, two whiskers, two caps and a mark per outlier. That datum
        # is the substrate the next member of the dimension (candlestick) is
        # a different reading of, so the dimension is now open rather than
        # one item shorter.
        #
        # Three things past Qt 6.11, all read over the wire by the demo, and
        # all consequences of one decision — the summary is DERIVED here
        # rather than handed in. `QBoxSet` is five doubles and `QtCharts`
        # computes none of them (its own box-plot example ships a
        # `findMedian()` helper IN THE EXAMPLE):
        #
        #   * The quantile DEFINITION is part of the value. `QuantileMethod`
        #     carries three standard ones (Tukey's hinges, Hyndman & Fan
        #     types 7 and 6) that disagree at small n — and the demo shows the
        #     disagreement deciding whether a sample is an outlier at all. A
        #     `QBoxSet` cannot record which definition built it.
        #   * OUTLIERS exist. Tukey's `k * IQR` fence limits each whisker and
        #     every sample beyond it is its own addressable mark. Qt's five
        #     slots have no per-outlier geometry, so a Qt box plot cannot draw
        #     one at any setting — and that fence is the defining half of the
        #     form.
        #   * The NOTCH, because the sample count survived the summary
        #     (McGill, Tukey & Larsen 1978). `QBoxSet` carries no n, so Qt
        #     could not offer it even as a paint option — and a distribution
        #     handed in pre-computed keeps its plain box in the same chart,
        #     which is the visible difference between a box a reader can apply
        #     the test to and one they cannot.
        #
        # +5 and not more. Audited at R1553, and the R1545 list was RE-RUN
        # rather than inherited — every one of its items still stands:
        #
        #   * FOUR of the five series types remain: polar, candlestick,
        #     spline, 3D-surface. Candlestick is the cheapest of them now (the
        #     same interval geometry over open / high / low / close) and is
        #     deliberately not built here, being the second consumer that
        #     would decide whether the interval mark lifts.
        #   * The pre-computed path (`Distribution::from_summary`, Qt's own
        #     contract) has no forcing consumer: `hello-boxplot` derives every
        #     one of its five, so the summary arm is exercised by unit tests
        #     only.
        #   * A box has no per-mark a11y. The scrub readout names the whole
        #     summary and its provenance, which is past Qt (QtCharts
        #     implements no accessibility interface at all), but an individual
        #     outlier is painted geometry with no accessible relationship.
        #   * `QCategoryAxis`, label thinning, band-level a11y, drag pan /
        #     rubber-band zoom, the y-window, the plot zoom's a11y, the second
        #     zoom consumer, local time — all eight still open, unchanged.
        # R1568 re-judged, 87 -> 92, DEMANDED by the tool (the round ledger
        # takes this axis 5 -> 7, past the band). It absorbs TWO rounds,
        # because R1567 landed inside the band and deferred its look.
        #
        # R1567 took the CANDLESTICK, and corrected a claim while closing it:
        # R1553 recorded that a candlestick would be the box plot's SECOND
        # CONSUMER, "the same interval geometry", and building one showed that
        # wrong. A `Distribution`'s five landmarks are totally ordered by
        # construction; a `Candle` has four and only THREE order relations
        # among them, with nothing at all between `open` and `close` — and
        # that absence IS the datum, because which of the two is larger is
        # what the form exists to show. Two sessions with the same four
        # numbers, the same extent and the same box mean opposite things.
        #
        # R1568 took POLAR, which is not a series type at all but the crate's
        # first non-cartesian COORDINATE SYSTEM. `ValueScale` had been the
        # unexamined assumption under every chart here — a value maps to a
        # pixel on one line, and four axis kinds fit inside that because each
        # is still a map onto one line. An angular axis is not: it is
        # PERIODIC, so 0 and 360 are one place, and everything the round adds
        # falls out of that one fact (a value outside the period is placed
        # rather than dropped, a series closes on itself by derivation, and
        # the tick at the period's end is the tick at its start).
        #
        # +5 for the two, and the audit that holds it there:
        #
        #   * THREE series types remain — spline, 3D-surface, and the OHLC
        #     bar, which is the Western reading of R1567's own datum and is
        #     now the cheapest thing on this axis. 3D-surface needs a 3D
        #     renderer and is Phase C's, not this axis's.
        #   * `QCategoryAxis`, label thinning, local time, drag pan /
        #     rubber-band zoom (blocked on the pointer wire not reporting a
        #     held button), the y-window, the plot zoom's a11y and its second
        #     consumer — all seven unchanged since R1545.
        #   * Neither new form has PER-MARK a11y: both scrub readouts name the
        #     whole datum, which is past Qt (QtCharts implements no
        #     accessibility interface), but an individual candle body or polar
        #     vertex is painted geometry.
        #   * The polar chart has no cross-filter leg and no legend
        #     interaction, where the cartesian charts have both.
        "judged_at": 1568,
        "completion": 92,
        "evidence_snapshot": {"example-name": 28, "round-axis": 7},
    },
    {
        "key": "text",
        "name": "Rich-text editing / selection",
        "weight": 9,
        "gated": False,
        "evidence": [
            ("example-name", [
                "textfield", "textarea", "richtext", "find-replace",
                "syntax-highlight", "textgrid", "completer", "app-font",
            ]),
        ],
        # R1540 re-judged 70 -> 74, demanded by the tool: this axis had never
        # declared a round, so its first one moved `round-axis` past the band.
        #
        # It CORRECTS the judgment as well as moving it. R1519's stated gap was
        # "full styled-run text-editing depth / code-folding still partial",
        # and the folding half had been finished for ~600 rounds: R933 derives
        # fold regions from the live buffer, R933.1 shifts them on edit, R955
        # adds the keyboard, and two demos cover it. A gap list is worth only
        # what it has been checked against (R1532).
        #
        # What R1540 added: the GUI text run adopted the SGR 4:x underline
        # vocabulary the TERMINAL cell has spoken since R1399 — single /
        # double / curly / dotted / dashed, plus the underline's own colour
        # (Qt `setUnderlineColor`). The tree could draw an undercurl in a
        # terminal and not on screen, with the painter that knew how sitting
        # in the same file as the one that flattened every form to one rule.
        # An LSP diagnostic mark is now drawable at all.
        #
        # +4 and not more, because the CHARACTER-format axis is nearly done
        # while the DOCUMENT axis is barely started. Audited at R1540:
        #
        #  - `QTextCharFormat::setBackground` — no per-run background exists.
        #    The paint layer hand-rolls FOUR band kinds instead (selection,
        #    find-match, current-line, preedit), each with its own fill fn and
        #    alpha knob. Qt has both this and `QTextEdit::ExtraSelection`; the
        #    tree has neither as a contract.
        #  - no vertical alignment (super/subscript), and no overline.
        #  - the DOCUMENT model is absent: `QTextList` (ordered / unordered),
        #    `QTextTable`, `QTextBlockFormat`'s per-paragraph indent and
        #    margins, and `setMarkdown` / `toHtml` import-export. A styled run
        #    is a span of characters; a document is more than a span list.
        #  - a mark is invisible to assistive technology (Qt too, so parity,
        #    but it is what a red squiggle most needs).
        #
        # R1542 re-judged 74 -> 75, demanded by the tool (`round-axis` 1 -> 2).
        # It moves ONE point, and the reason it moves so little is the finding:
        # this axis is named "Rich-text editing / selection" and its evidence
        # counts `textgrid`, `app-font` and `completer`, none of which is
        # rich-text EDITING. R1542 landed squarely in that mismatch — it fixed
        # a `TextGridNode` contract (a terminal cell grid), so it advanced the
        # axis's evidence by 100% while touching none of the four gaps above.
        #
        # What it closed is real and belongs here, in the terminal-grid half:
        # `rect` was meaning both the PAINT EXTENT and the WINSIZE, which agree
        # only while the layout is what sizes the producer. A multiplexer whose
        # daemon tiles in cells while a client lays out in pixels can satisfy
        # neither reading, so every snapshot reported a permanent divergence
        # from `buffer_cols` — the signal pinion's own docs define as a resize
        # in flight or a producer bug. `with_winsize` splits the two facts and
        # `winsize_source` names which authority sized the grid, because the
        # authority is not recoverable from the values.
        #
        # The mismatch itself is the thing to carry forward. An axis whose name
        # and whose evidence disagree cannot be re-judged coherently: a round
        # that doubles its round count while leaving its stated gaps untouched
        # is not a 100% move, and a scorer who only reads the number would take
        # it for one. Either the terminal grid earns its own axis (it has 8
        # demos and a cross-backend paint contract) or this one is renamed for
        # what it counts. Deliberately NOT decided here — an axis-set change is
        # the R1522/R1526 class of round, and doing it as a side effect of a
        # feature round is how the last two got made by accident.
        #
        # R1546 re-judged 75 -> 80, demanded by the tool (round count 2 -> 3).
        #
        # It closes the item R1540's audit named FIRST and largest: a text run
        # had no background. `TextStyle` carried a foreground and nothing else,
        # while `TermCell` beside it had carried `fg` AND `bg` since the §5.41
        # grid arm — so one document was paintable with a highlight in a
        # terminal and not as text, the same shape R1540 itself found in the
        # underline. And the absent extension point showed up as workaround
        # code, exactly as R1532 predicts: the paint layer hand-rolls FOUR band
        # kinds (selection / find-match / current-line / IME-preedit), each an
        # absolute-positioned box with its own fill fn, under a comment
        # conceding all four bodies were byte-identical.
        #
        # `TextStyle::bg_color` (Qt `QTextCharFormat::setBackground`) is now a
        # run-level declaration whose band is cut by BYTE and measured by
        # `selection_rects_for_range` — the function the selection band already
        # calls — so a highlight and a selection over the same bytes are one
        # function called twice rather than two derivations that agree. Two
        # things past Qt 6.11, both read over the wire: the PAINTED EXTENT is
        # published (Qt computes the rect inside the private
        # `QTextLayout::draw` and discards it, so a Qt application re-derives
        # it from `cursorToX` — a second implementation free to disagree with
        # the painter's), and the fg/bg pair publishes its WCAG contrast, so
        # "no highlight in this application drops below 4.5:1" is one call
        # where Qt will paint any brush behind any pen and say nothing.
        #
        # +5 and not more, and the remainder is audited at R1546 rather than
        # carried. The CHARACTER-format half is now nearly complete: what is
        # left of it is **vertical alignment** (super/subscript — the OS/2
        # metrics are parsed in `pinion-text-font` and nothing consumes them)
        # and **overline** (`TextDecoration` is underline-form + strikethrough
        # + underline-colour). Both small. What dominates the axis now is the
        # half that is untouched: **there is no document model at all** —
        # `QTextList`, `QTextTable`, `QTextBlockFormat`'s per-paragraph indent
        # and margins, `setMarkdown` / `toHtml`. Not one of those has a scene
        # primitive. Also unchanged, and now for a RECORDED reason rather than
        # by omission: the four view-level bands stay separate, because a
        # `StyleRun` carries a fully-resolved style and layering a selection
        # run over a syntax run would clobber the syntax run's foreground —
        # which is why Qt splits the same way (`QTextCharFormat` for the
        # document, `QTextEdit::ExtraSelection` for the view).
        #
        # The R1542 name/evidence mismatch above still stands and is still
        # deliberately undecided here.
        #
        # R1551 re-judged 80 -> 84, demanded by the tool (round count 3 -> 4).
        #
        # It closes the item R1546's audit named as DOMINATING the axis, on the
        # one sub-item that audit named with specifics: `QTextBlockFormat`'s
        # per-paragraph indent and margins. Before it, a paragraph could say how
        # its glyphs looked and nothing about how the paragraph itself sat — no
        # indent, no space between paragraphs, no first-line indent, no way to
        # mark one a heading. `BlockFormat` is now a scene declaration that
        # lowers to the node's ordinary layout margin, so the flex pass indents
        # a paragraph with no document-specific layout code and the result
        # composes with the rest of the tree; Qt's block margins are known only
        # to the private `QTextDocumentLayout`, which is a second layout engine
        # that meets the widget layout at a viewport and nowhere else.
        #
        # Four things past Qt 6.11: the format is a **struct** where
        # `QTextFormat` is a `QVariant` property bag whose unset properties
        # silently return defaults, so a block's whole declaration can be
        # enumerated; every length is **one unit** where Qt mixes `indent()`
        # (indent-width multiples) with `leftMargin()` (pixels) in one class;
        # `text-indent` carries CSS's **`hanging` and `each-line`** keywords,
        # which Qt's bare `qreal textIndent` cannot express (a hanging indent
        # in Qt needs a negative indent plus a compensating margin, i.e. two
        # properties that must agree); and a **heading level reaches assistive
        # technology** — `QTextBlockFormat::headingLevel()` has existed since
        # Qt 5.15, but the interface a `QTextEdit` implements is
        # `QAccessibleTextInterface`, whose vocabulary is character offsets,
        # selections and text attributes with no method that reports block
        # structure at all, so a Qt document's heading levels reach its layout
        # and stop. `scene/text_blocks` then publishes the declaration BESIDE
        # the shaped line boxes, which is the only form in which "did my indent
        # reach the layout" is a question with an answer.
        #
        # It also closed a §2 #6 gap this axis had carried unnamed since R1344:
        # `TextStyle::text_align` never reached the cell backend at all. The
        # terminal now places every line by the same rule the pixel backend
        # does — indent and alignment together, because both answer "which
        # column does this line begin at" and splitting them would be two
        # derivations of one CSS rule.
        #
        # +4 and not more, and the remainder is audited at R1551. The document
        # model is OPENED, not complete, and what is left of it is larger than
        # what was closed: **`QTextList`** (ordered / unordered, with automatic
        # numbering across sibling blocks — the part that cannot be hand-
        # composed), **`QTextTable`**, and **`setMarkdown` / `toHtml`**
        # import-export. None has a scene primitive. `QTextBlockFormat` itself
        # keeps four properties this round did not take: `marker`
        # (Unchecked / Checked, which belongs with `QTextList`),
        # `nonBreakableLines`, `pageBreakPolicy` (meaningful only against
        # `pinion-pdf`'s paged output) and `tabPositions`. The CHARACTER half is
        # unchanged from R1546: vertical alignment (super/subscript) and
        # overline, both small.
        # R1560 re-judged 84 -> 90, demanded by the tool (round count 4 -> 6).
        #
        # It absorbs TWO rounds, because R1559 landed at the band edge exactly
        # (+25%) and did not force a look — the sticky behaviour R1547/R1548
        # already showed. Both of them close an item R1551's own audit named,
        # and between them they close TWO OF THE THREE things that audit listed
        # as the whole of what was left of the document model.
        #
        # R1559 — `QTextList`. What a list cannot have written by hand is the
        # NUMBER, because a number is not a property of the item: it is a
        # property of its place among its siblings, so inserting one renumbers
        # every item after it and nesting one restarts the inner sequence while
        # the outer carries on underneath. `ListSpec` declares membership and
        # never a number; `number_blocks` derives it. Past Qt: the counter
        # styles have RANGES and fall back through CSS Counter Styles Level 3
        # where `itemText()` answers "?" and loses the value; a BULLET IS TEXT
        # (Qt draws `ListDisc` as an ellipse, so no accessor can say what an
        # unordered marker looks like and it is not in the text at all); the
        # structure is enumerable; it reaches assistive technology; and a
        # suffix's default belongs to the style rather than hiding in a null
        # `QString`.
        #
        # R1560 — `QTextTable`, and the same argument one dimension up. A
        # cell's ADDRESS is not a property of the cell: it is where the cell
        # lands once every earlier cell's spans have taken their slots.
        # `place_cells` derives it by HTML's own slot allocation and
        # `view_document` lowers it onto a REAL CSS GRID — the layout kind the
        # framework did not have, added here with its forcing consumer, because
        # a column of flex rows measures each row alone (so columns cannot
        # agree without being told a width) and cannot express a rowspan at
        # all. Past Qt: the address is derived rather than maintained; a span
        # that does not fit is clamped to the FREE RUN and NAMED, where
        # `mergeCells` returns `void` and a refused merge leaves no trace; a
        # table may be RAGGED and its unfilled slots are published, a state
        # `QTextTable` cannot be in; header COLUMNS exist and header-ness is
        # derived FROM THE ADDRESS; the structure reaches assistive technology,
        # where a `QTextTable` reaches no accessibility interface at all; and
        # it is enumerable over the wire.
        #
        # +6 and not more. What remains is audited at R1560, and the largest
        # item is the third one R1551 named:
        #
        #  - **`setMarkdown` / `toHtml`** — the import/export half of the
        #    document model. Untouched, and now the only one of R1551's three
        #    still open.
        #  - **Nested tables.** Qt has them. The honest way in is the general
        #    `QTextFrame` containment axis, not a second ad-hoc level counter
        #    beside the list's — two nesting mechanisms that would have to
        #    agree.
        #  - `QTextBlockFormat`'s four untaken properties are now three:
        #    R1559 landed the list `marker` belongs with, leaving
        #    `nonBreakableLines`, `pageBreakPolicy` and `tabPositions`.
        #  - the CHARACTER half is unchanged since R1546: vertical alignment
        #    (super/subscript) and overline, both small.
        #  - the grid vocabulary R1560 added stops short of `minmax()` /
        #    `fit-content()` and `grid-auto-flow` (every cell is placed
        #    explicitly, by design, so auto-flow never runs).
        #
        # The R1542 name/evidence mismatch above STILL stands and is still
        # deliberately undecided here — and it has now grown, because
        # `hello-richtext-cells` and `hello-richtext-list` are rich-text
        # DOCUMENT work while `textgrid` / `app-font` / `completer` are not.
        "judged_at": 1560,
        "completion": 90,
        "evidence_snapshot": {"example-name": 13, "round-axis": 6},
    },
    {
        "key": "perf",
        "name": "Pro-tool performance",
        "weight": 9,
        "gated": False,
        # R1522 — this axis is the reason `demo-body` exists. Its example
        # patterns name the four infrastructure demos (profiler, immediate-mode
        # canvas, replay), which is evidence of *tooling*; its completion is
        # gated on *optimisations*, which produce no example. So the axis's own
        # bottleneck was invisible to its own evidence, and two rounds of
        # exactly that work registered as +0%.
        #
        # The body patterns are cost-counter names. A demo that asserts on one
        # is what a landed optimisation leaves behind — deterministic counters
        # rather than wall-clock, so the guard is not flaky. Deliberately NOT
        # included: bare `frame_timings` / `render_us`, which any round may read
        # (measured: six do so to verify focus, window identity or hover, none
        # of them perf work).
        "evidence": [
            ("example-name", [
                "frame-profiler", "immediate-mode-canvas", "immediate-intent",
                "replay",
                # R1538 — the large-scene end-to-end harness. The first example
                # this axis has gained whose subject is a MEASUREMENT rather
                # than a facility: a binding whose model grows four orders of
                # magnitude at runtime, so the node census can be asserted
                # scale-invariant. The census flagged it UNCLASSIFIED on the
                # round that added it, which is the whole point of a census.
                "scene-scale",
            ]),
            ("demo-body", [
                "cache_stats", "paint_cache", "frame_budget", "fixed_timestep",
                # R1537 — the GPU frame clock. Same rule as the four above and
                # not the excluded pair: `gpu_us` is a cost counter that only
                # this axis's work produces, where `frame_timings` is a wire a
                # focus or window-identity round reads in passing. Measured at
                # R1537: 1 of 490 demos mentions it, and it did not exist
                # before the round that added it.
                "gpu_us",
                # R1550 — the memory axis. `total_bytes` is the per-arena
                # census's own total, and the counter a round that SHRINKS a
                # cache asserts against; measured at R1550, 0 of 499 demos
                # mentioned it before the round that added it. Deliberately not
                # `bytes`, which appears in `font/parse`'s input array and in
                # every screenshot demo's pixel arithmetic.
                "total_bytes",
                # R1556 — the draw census. Same rule as the three above:
                # `path_segments` is the frame's geometric cost, and a round
                # that reduces per-frame drawing asserts against it. Measured
                # at R1556, 1 of 509 demos mentions it and that one is the
                # round that added the counter. Deliberately not `scene_nodes`,
                # which is a count of the TREE — the very proxy this axis had
                # to stop treating as a cost.
                "path_segments",
            ]),
        ],
        # R1527 re-judgment, forced by this axis's round count going 2 -> 3.
        # R1519 said 50% on "measurement infra mature, measured hot-path opt
        # 0"; R1522 said 60% when that 0 became 2 (R1520 scroll paint encode
        # 1360us -> 42us, R1521 shape cache 27.4ms -> 1.59ms at 1200 leaves).
        # R1527 is the third, and the first whose cost was being paid on
        # ordinary interaction frames rather than a specific gesture: the
        # fragment cache's sweep evicted every fragment a hit had replayed, so
        # one keystroke in a data grid re-encoded the whole visible tree
        # (hello-grid-nav 1 hit/83 misses -> 20/10; 1200 rows, one row changed,
        # 17.1ms -> 1.4ms).
        #
        # It also measured what R1522 could only name. The "2.6us per text
        # node" is now decomposed on a warm cache: vello encode 54%, parley
        # glyph-run walk 37%, shape-cache lookup 9% — which killed this
        # round's own first hypothesis (the lookup allocates an owned key per
        # call, and is not the dominant term).
        #
        # R1531 re-judgment, demanded by this axis's round count going 3 -> 4.
        # R1527 named three things absent at 65%, and R1531 closes the third
        # of them OUTRIGHT rather than partially: the per-leaf paint cost is
        # no longer merely attributed. The parley walk that positions a
        # shaped layout's glyphs — 37% of a warm-cache frame, and the half of
        # it that is pinion's own code — now runs once per shaped layout
        # instead of once per paint, because the draw list is cached in the
        # entry that already holds the layout (Skia's SkTextBlob, Qt's
        # QGlyphRun). Measured before and after on the same box, same probe,
        # same steady state: 1,200 text leaves 1,489us -> 480us a frame, 3.1x.
        # It is the fourth measured optimisation on this axis and the first
        # whose saving lands on EVERY re-encoding frame rather than on a
        # gesture (scroll, R1520) or a capacity cliff (R1521).
        #
        # Only +4, because what remains is larger than what was closed and is
        # the axis's own NAME — "60fps with large scenes; profiling":
        #
        #  - no GPU-timestamp render time. `render_us` is CPU submit cost
        #    with the vsync block split out (R1361.1); what the GPU actually
        #    took is unmeasured, and a pro tool states it (Unreal `stat gpu`).
        #  - no large-scene 60fps end-to-end measurement. Every number this
        #    axis holds is a component measured in isolation.
        #
        # And R1531 leaves one of its own: the draw lists are held per cache
        # entry at ~12 bytes a glyph, so MAX_CAPACITY's stated ~26 MB is now
        # an understatement by an amount nobody has measured.
        #
        # R1538 re-judgment, demanded by this axis's round count going 4 -> 6.
        # BOTH gaps R1531 named at 69% are now closed, which is the largest
        # single move this axis has had:
        #
        #  - R1537 closed the GPU-timestamp half. It had been recorded as an
        #    UPSTREAM blocker and was not one: `vello::Renderer::new` takes a
        #    `&Device` the caller owns, so pinion owns it (`pinion-gpu`), asks
        #    for TIMESTAMP_QUERY, and publishes `gpu_us`.
        #  - R1538 closed the large-scene end-to-end half. Not by timing a big
        #    binding — a wall-clock threshold reads the host, so it either
        #    flakes or proves nothing — but by noticing what the claim IS.
        #    "60fps at scale" is a complexity claim (per-frame work is bounded
        #    by what is visible, not by the model), and a count can state it.
        #    `scene/frame_timings` carries scene/layout/encode node censuses;
        #    `hello-scene-scale` grows its model 1e2 -> 1e6 at runtime and the
        #    painted tree does not move, with an EAGER arm as the negative
        #    control that proves the guard can fail.
        #
        # +9 and not more, because naming those closed surfaced a dimension
        # this axis had never named at all — audited at R1538 rather than
        # assumed:
        #
        #  - NO MEMORY MEASUREMENT ANYWHERE. Census of the 70-method RPC
        #    surface: not one reports bytes. `cache_stats.entries` and
        #    `text_cache_stats.capacity` are counts of things, and a count is
        #    not a footprint. A pro tool states its own (Unreal `stat memory`).
        #    This is also where R1531's leftover lives: MAX_CAPACITY's ~26 MB
        #    is an unmeasured claim, and nothing can measure it.
        #  - the census counts NODES, not their cost. A Container and a
        #    4,000-glyph Text leaf are both 1, so a scene that grew heavier
        #    without growing wider is invisible to R1538's guard.
        #  - present latency is still unmeasured, and is genuinely EXTERNAL:
        #    the GPU span covers rasterize + blit, and what the compositor
        #    does after `present()` needs an extension wgpu does not expose.
        #
        # R1541 landed on this axis WITHOUT moving the number (the tool did not
        # demand a re-judgment: 6 -> 7 rounds is +17%, inside the band). Logged
        # here so the next re-judgment has it, because it is a dimension none
        # of the statements above cover — every one of them measures the RENDER
        # path, and this one is the CONTROL plane. `pinion-rpc-transport`'s
        # accept loop slept a fixed 50 ms per `WouldBlock`, so a fresh
        # connection waited that long to be *accepted*: measured by the sprag
        # consumer at 99.5% of a CLI invocation's wall time, reproduced here at
        # a 50,141 us median and fixed to 36 us by waiting on `poll(2)` over
        # the listener plus an out-of-band wake channel. The guard is this
        # axis's own shape — a deterministic counter, `accept_wakeups`, not a
        # wall clock. Note what the docstring's premises show about how such a
        # defect survives: both were TRUE when written, and a consumer's
        # architecture changed underneath them.
        #
        # R1550 re-judgment, demanded by the tool (the ledger took this axis
        # 6 -> 8, past the 25% band). 78 -> 83, because the FIRST of the three
        # gaps the 78% named is closed outright, and it was total: a census of
        # the RPC surface found not one field in BYTES. `scene/memory` is now
        # the memory axis — one row per arena per owner, with the process RSS
        # beside it — and the accounting is a trait whose every impl
        # destructures its type, so a field added to a cached struct cannot
        # silently go unpriced. It also closes R1531's leftover (`MAX_CAPACITY`
        # bounded memory by an entry count times a measured AVERAGE, and an
        # average bounds nothing) and fixes an arena that sat BELOW Qt's floor:
        # the decoded-image cache had no bound of any kind and is now
        # byte-bounded at `QPixmapCache`'s own 10 MiB default.
        #
        # +5 and not more. Two of the three gaps stand — the node census counts
        # nodes rather than their cost, and present latency needs a wgpu
        # extension that does not exist — and this axis's completion has
        # always been gated on OPTIMISATIONS as much as on measurement: R1550
        # made nothing faster. What it did is make the resource visible, which
        # is the precondition for the round that shrinks it.
        #
        # R1558 re-judgment, demanded by the tool (the ledger took this axis
        # 8 -> 11, past the band). 83 -> 90, and it absorbs THREE rounds at
        # once: R1556 and R1557 each closed a named gap and each landed inside
        # the band, so neither forced a look. Taken together they are the
        # profiler:
        #
        #  - R1556 closed the SECOND of the three gaps the 83% named — "the
        #    census counts nodes, not their cost". `last.draw` counts what was
        #    DRAWN in the units a 2D vector renderer is charged in (draw
        #    commands / paths / path segments / clip layers / glyph runs /
        #    glyphs), read off the submitted scene, so a replayed subtree
        #    counts like an encoded one.
        #  - R1557 attributed that census PER SUBTREE, as a difference of
        #    censuses across each node's walk, with `own = total - children`
        #    an arithmetic identity and the whole tree a partition.
        #  - R1558 scoped the MEASUREMENT to an address, which is what turns a
        #    profiler from a thing that exists into a thing that is used: a
        #    drill-down costs less at each step instead of re-encoding the
        #    window three times. It rests on a property of the encoder — a
        #    subtree's draw work is independent of its context — asserted
        #    against a real encode and again on the wire.
        #
        # Reaching for the address also found the vocabulary bound to the
        # wrong window registry (`path::resolve` judges a prefix against the
        # SCE topology; live windows are the shell's `WindowSpec` slots), so
        # R1557's own rows were unreadable on any multi-window binding.
        #
        # +7 and not more, and the remainder is audited at R1558 rather than
        # carried: PRESENT LATENCY is still external (wgpu exposes no
        # presentation-timestamp extension), the footprint is what the
        # allocator was ASKED for rather than what is resident, per-node
        # REPLAY status is absent by construction (the profile re-encodes into
        # a cold cache so that it can decompose), a profile row's address
        # still has no GENERAL reader, and R1550's two modelled arenas
        # (`hash_table_bytes` / `lru_table_bytes`) are still unpinned against
        # the crates whose layouts they model.
        "judged_at": 1558,
        "completion": 90,
        "evidence_snapshot": {"example-name": 5, "demo-body": 15, "round-axis": 11},
    },
    {
        "key": "osnative",
        "name": "OS-native integration",
        "weight": 11,
        "gated": True,  # Mac/Win surfaces need those OSes' runners
        "evidence": [
            ("example-name", [
                "file-dialog", "file-open-dialog", "file-save-dialog",
                "file-browser", "filedrop", "print", "pdf-export", "tray",
                "window-", "multi-window", "no-primary", "modal-handoff",
                "modal-refocus",
            ]),
        ],
        "judged_at": 1519,
        "completion": 58,
        "evidence_snapshot": {"example-name": 13, "round-axis": 0},
    },
    {
        "key": "api",
        "name": "§7 API stabilisation",
        "weight": 9,
        "gated": True,  # deliberately parked: freeze a mature surface, not a churning one
        "evidence": [
            ("example-name", [
                "ai-introspect", "answer-origin", "encoded-answer",
                "endpoint-identity", "viewport-question", "conn-lifecycle",
                "forge-counter", "subscribe",
                # R1564 — a refused invoke states why. The wire's error channel
                # is this axis's subject as much as its answer channel is: PR-82
                # measured a consumer guessing at causes because `error.data`
                # published a variant name where the surface had a sentence.
                "refused-invoke",
            ]),
        ],
        # R1539 re-judged 30 -> 42, demanded by the tool: this axis had never
        # declared a round, so its first one moved `round-axis` past the band.
        # The largest single move any axis has had here, and the reason is that
        # the baseline was the lowest. R1519's 30% described a surface an agent
        # could ENUMERATE but not READ: `rpc/methods` answered with names and an
        # OCC class, and its own module doc deferred the rest as "added when a
        # consumer needs it" — a defer [[qt-parity-over-yagni]] does not admit,
        # and one R1538 then supplied a consumer for the hard way.
        #
        # What R1539 added is the whole missing half of a describable API:
        #
        #  - `rpc/schema` publishes a census of all 82 serialized types — key
        #    sets, JSON types, absence, nullability, and `$ref` nesting — so an
        #    agent discovers the SHAPE of what it will be answered with.
        #  - a source-parse gate proves that census true of the Rust types, so
        #    a silent breaking change to any response is now impossible. That
        #    is the core of a stabilisation story: not the freeze itself, but a
        #    machine that can tell you the surface moved.
        #
        # +12 and not more. The axis is named for STABILISATION, and the
        # describability half is what moved; every guarantee half is untouched.
        # Audited at R1539 rather than assumed:
        #
        #  - NO METHOD -> TYPE BINDING, on either side. Qt's `QMetaMethod` has
        #    `returnMetaType()` AND `parameterTypes()`; pinion has neither, so
        #    the vocabulary is discoverable and its use is not. Withheld rather
        #    than shipped partial: 28 `*Outcome` types against 91 methods, so
        #    the column would read `null` for most of the surface and an agent
        #    reads a null return type as "answers with nothing".
        #  - no version negotiation, no deprecation path, no compatibility
        #    policy, and no freeze — the four things "stabilisation" names.
        #  - no per-method error taxonomy. `RpcError` is censused as a shape;
        #    which codes a given method can answer with is undeclared.
        #  - the census covers `pinion-rpc` only. `scene/snapshot` and
        #    `scene/access` answer with trees built in `pinion-core` and
        #    `pinion-a11y`, which the gate's source parse does not reach.
        #
        # R1552 re-judged 42 -> 50, demanded by the tool: the round ledger took
        # this axis 1 -> 2. It closes a gap none of R1539's four named, because
        # it is a gap in a DIRECTION rather than in a description: the protocol
        # had no server-initiated path at all. One frame carried one `FnOnce`
        # reply, so one request could produce at most one response and a
        # subscription was inexpressible at any price — which is what
        # PINION-PR83 reported, and what `waiter.rs` had already recorded as a
        # property ("no server-push, streaming, or subscription") without
        # anyone reading it as an absence to close.
        #
        # `RpcEgress` is the mirror of `RpcIngress`, and `scene/subscribe`
        # is the framework's own consumer of it. Three things past Qt 6.11,
        # all read over the wire: the stream is ENUMERABLE (`scene/subscriptions`
        # answers who is listening to what — Qt binds no server write to a named
        # stream, so `QLocalServer` cannot be asked); a stream cannot be named
        # to a client before the answer that told it the name (armed after the
        # reply, structural rather than remembered); and a client that VANISHES
        # has exactly its own stream released, with no unsubscribe ever sent.
        #
        # +8 and not more, because the axis is named for STABILISATION and this
        # made the surface BIGGER. Audited at R1552, all four of R1539's gaps
        # re-checked and all four still open:
        #
        #  - no method -> type binding, no version negotiation, no deprecation
        #    path, no compatibility policy, no freeze, no per-method error
        #    taxonomy, and the census still covers `pinion-rpc` only. R1552
        #    added `SubscribeOutcome` / `UnsubscribeOutcome` / `SubscriptionView`
        #    / `SubscriptionsOutcome` to that census the day it landed, which is
        #    the R1539 gate working — but it moved none of the guarantees.
        #  - NEW, and this round's own: a subscriber is told the scene advanced,
        #    not WHICH SUBTREE. There is no per-subscription filter. Qt has no
        #    equivalent at all so it is an axis gap rather than round debt, but
        #    a large scene where an agent watches one panel will want it.
        "judged_at": 1565,
        "completion": 62,
                # R1565 re-judged 55 -> 62, DEMANDED by the tool (ledger 3 -> 4). It
        # closes BOTH items R1564's own audit left open.
        #
        #  - The WRITE channel had no reason at all, so `ReadOnly` /
        #    `OutOfRange` were exactly as opaque as `Rejected` had been. Only
        #    `OutOfRange` gains a payload, and that asymmetry is the design: it
        #    is the one arm whose meaning its variant does not determine.
        #  - The code R1564 allocated was discoverable only by reading pinion's
        #    source. `rpc/errors` completes the discovery triple — R1089 names,
        #    R1539 shapes, R1565 codes — and publishes `data_is_prose`, the
        #    single fact that tells a client whether `error.data` may be MATCHED
        #    or only shown. A source-scan test proves every code this crate
        #    emits is in the catalogue.
        #
        # +7 and not more. This is still not stabilisation: the four things that
        # word names are untouched, and the round again made the surface bigger
        # and moved a wire contract. What it did was finish making the error
        # channel DESCRIBABLE, which is a prerequisite for freezing it.
        #
        # Audited at R1565, unchanged from R1539/R1552: no method->type binding;
        # no version negotiation, deprecation path, compatibility policy or
        # freeze; the type census covers `pinion-rpc` only; no per-subscription
        # filter. New this round: `rpc/errors` is a hand-kept catalogue whose
        # completeness gate scans for `RpcError::new` literals, so a code
        # reaching the wire by some other construction would escape it.
        #
        # R1564 re-judged 50 -> 55, DEMANDED by the tool: the ledger took this
        # axis 2 -> 3 and the round-axis snapshot moved +50%, past the band.
        #
        # It closes the ERROR half of a describable API — R1539 opened the
        # answer half and R1552 the direction. `InvokeError::Rejected` was
        # payload-free, so a producer that knew exactly why it was refusing had
        # nowhere to say it, and the wire published `"InvokeRejected"`: the
        # transport's classification, not the fact the surface observed. The
        # cost was MEASURED by the consumer rather than argued — six of sprag's
        # fifteen reachable CLI failure paths print an `or`-joined guess at
        # causes their own daemon had already told apart (PINION-PR82).
        #
        # Only +5, and the ceiling is the axis's own NAME. Two things hold it
        # down, both stated rather than waved at:
        #
        #  - This round made the surface BIGGER and changed a WIRE CONTRACT: a
        #    refusal moved from -32602 to -32005. That is the opposite of
        #    stabilisation in the short run, and it is a prerequisite for it in
        #    the long run — a category error cannot be frozen, and "the
        #    parameters were invalid" is the wrong statement about a call whose
        #    parameters were fine.
        #  - Of R1539's four remaining gaps it touches exactly one, and touches
        #    it partially: "no per-method error taxonomy" is now a per-CLASS
        #    one (the framework's finding vs the surface's refusal, told apart
        #    by code), which is what a consumer needs to branch. Per METHOD is
        #    still absent.
        #
        # Audited at R1564, and what remains is larger than what was closed:
        # `InterveneError` carries no reason at all, so `ReadOnly` /
        # `OutOfRange` are exactly as opaque as `Rejected` was — the write-state
        # channel is the next slice ([[wire-form-read-write-symmetry]]); no
        # version negotiation, deprecation path, compatibility policy or freeze
        # (the four things "stabilisation" names, untouched since R1519); no
        # method->type binding; the census still covers `pinion-rpc` only; no
        # per-subscription filter (R1552's own); and the -32000..-32099 space is
        # now three codes deep with no published map, so a client discovers
        # `ACTION_REFUSED` by reading pinion's source rather than by asking.
        "evidence_snapshot": {"example-name": 9, "round-axis": 4},
    },
]


#: Examples that are NOT Phase B evidence, and why. Listing them with a reason
#: is the difference between "excluded" and "invisible" — the dataviz campaign
#: was invisible for 587 rounds precisely because nothing named it.
NOT_PHASE_B = {
    "hello-audio": "Phase C — audio substrate",
    "hello-audio-device": "Phase C — audio substrate",
    "hello-audio-rt": "Phase C — audio substrate",
    "hello-narrative-walk": "cross-repo VN consumer axis (sprag)",
    "hello-place-map": "cross-repo VN consumer axis (sprag)",
    "hello-transcript": "cross-repo VN consumer axis (sprag)",
    "hello-vn-tide": "cross-repo VN consumer axis (sprag)",
}

#: Demos whose body matches an axis pattern for a reason that is not evidence.
#: Same idiom as NOT_PHASE_B, same reason: a documented exclusion can be argued
#: with, a silent one cannot. A body proxy will always admit some of these — the
#: alternative is a name proxy, which admits nothing and sees nothing.
NOT_EVIDENCE = {
    "r889_window_known_gate.py": (
        "exercises cache_stats only to prove it rejects a bogus window "
        "(window-identity round, not a cost measurement)"
    ),
    "r1539_wire_states_its_shape.py": (
        "calls cache_stats / text_cache_stats to check the SHAPE of their "
        "answers against the published census (§7 API round); it asserts on "
        "no counter's value, so it is not a cost measurement"
    ),
    "r1546_run_background.py": (
        "asserts text_cache_stats.background_builds holds still across repeated "
        "reads — a CORRECTNESS invariant (a derivation that is a pure function "
        "of the layout runs once), not a cost that was reduced. The probe reads "
        "a counter assertion as an optimisation's residue, which is right for "
        "the rounds it was built for and wrong here: R1546 advanced rich-text "
        "and made nothing faster"
    ),
}


def examples() -> list[str]:
    return sorted(
        p.name for p in (ROOT / "examples").iterdir() if (p / "Cargo.toml").is_file()
    )


def demos() -> list[str]:
    return sorted(p.name for p in (ROOT / "tools" / "demos").glob("*.py"))


def rounds_in(subjects: str) -> list[int]:
    """Round numbers named by commit subjects, newest first. Pure in `subjects`.

    Deduplicated: a round occasionally lands as `R1481` and `R1481.1`, and it is
    one round either way.
    """
    seen: dict[int, None] = {}
    for line in subjects.splitlines():
        m = ROUND_SUBJECT.match(line)
        if m:
            seen.setdefault(int(m.group(1)), None)
    return list(seen)


def parse_ledger(text: str) -> dict[int, tuple[str, str]]:
    """`docs/phase-b-rounds.tsv` as round -> (axis key or NO_AXIS, note).

    Pure in `text`, and strict: a malformed row raises rather than being skipped.
    A ledger that silently drops rows is a ledger that reports fewer declared
    rounds than were declared, which is the false negative this file exists to
    make impossible.
    """
    keys = {a["key"] for a in AXES} | {NO_AXIS}
    rows: dict[int, tuple[str, str]] = {}
    for lineno, raw in enumerate(text.splitlines(), 1):
        if not raw.strip() or raw.lstrip().startswith("#"):
            continue
        parts = raw.split("\t")
        if len(parts) != 3:
            raise ValueError(
                f"{ROUND_LEDGER.name}:{lineno}: expected 3 tab-separated fields, "
                f"got {len(parts)}: {raw!r}"
            )
        rnd, axis, note = (p.strip() for p in parts)
        if not rnd.isdigit():
            raise ValueError(f"{ROUND_LEDGER.name}:{lineno}: round {rnd!r} is not a number")
        if axis not in keys:
            raise ValueError(
                f"{ROUND_LEDGER.name}:{lineno}: unknown axis {axis!r} "
                f"(known: {', '.join(sorted(keys))})"
            )
        if not note:
            raise ValueError(
                f"{ROUND_LEDGER.name}:{lineno}: round {rnd} declares no reason; "
                f"an exclusion that cannot be argued with is not documented"
            )
        if int(rnd) <= LEDGER_FLOOR:
            raise ValueError(
                f"{ROUND_LEDGER.name}:{lineno}: round {rnd} is at or below the "
                f"floor R{LEDGER_FLOOR}; those rounds predate this tally"
            )
        rows[int(rnd)] = (axis, note)
    return rows


def ledger() -> dict[int, tuple[str, str]]:
    return parse_ledger(ROUND_LEDGER.read_text(encoding="utf-8"))


def git_rounds() -> list[int]:
    """Rounds git knows about, above the floor. Git is the authority on which
    rounds EXIST — if the ledger were also the census, a round that forgot to
    declare would be invisible instead of reported."""
    out = subprocess.run(
        ["git", "log", "--format=%s", "--no-merges"],
        cwd=ROOT,
        capture_output=True,
        text=True,
        check=True,
    ).stdout
    return [r for r in rounds_in(out) if r > LEDGER_FLOOR]


#: `demo-body` reads ~475 files, and the report consults each universe more than
#: once. Cached because the tree does not change mid-run, and because this runs
#: on every push: uncached it measured 3.0s, which is a cost a reporter has no
#: business charging.
_UNIVERSE: dict[str, dict[str, str]] = {}


def universe(kind: str) -> dict[str, str]:
    """Artifacts of `kind`, as name -> the text patterns are matched against."""
    if kind in _UNIVERSE:
        return _UNIVERSE[kind]
    if kind == "example-name":
        got = {n: n for n in examples() if n not in NOT_PHASE_B}
    elif kind == "demo-body":
        got = {
            n: (ROOT / "tools" / "demos" / n).read_text(encoding="utf-8")
            for n in demos()
            if n not in NOT_EVIDENCE
        }
    elif kind == "round-axis":
        # Keyed by the round git has, valued by what the ledger declared for it.
        # A round with no row gets "" — it matches no axis and so is REPORTED by
        # the census, which is what makes a forgotten declaration visible.
        # A round declaring NO_AXIS leaves the universe entirely, the same way
        # NOT_PHASE_B examples do and for the same reason.
        declared = ledger()
        got = {}
        for rnd in sorted(git_rounds()):
            axis, _note = declared.get(rnd, ("", ""))
            if axis == NO_AXIS:
                continue
            got[f"R{rnd}"] = axis
    else:
        raise KeyError(f"unknown evidence kind: {kind}")
    _UNIVERSE[kind] = got
    return got


def patterns_for(axis: dict, kind: str) -> list[str]:
    if kind == "round-axis":
        # Structural rather than declared: an axis counts the rounds that named
        # it, and its key IS the pattern. Derived here once instead of copied
        # into all eight axes so that an axis unable to register depth work
        # cannot be written — the eight copies would each be forgettable, and
        # forgetting is the direction that goes silent.
        return [axis["key"]]
    return [p for k, pats in axis["evidence"] if k == kind for p in pats]


def assign(kind: str, items: dict[str, str]) -> tuple[dict[str, list[str]], list[str]]:
    """Assign each artifact to the first axis whose pattern its text contains.

    Pure in `items` so the tool's own logic can be tested without the tree.
    """
    owned: dict[str, list[str]] = {a["key"]: [] for a in AXES}
    unmatched: list[str] = []
    for name, text in sorted(items.items()):
        for axis in AXES:
            pats = patterns_for(axis, kind)
            if pats and any(pat in text for pat in pats):
                owned[axis["key"]].append(name)
                break
        else:
            unmatched.append(name)
    return owned, unmatched


def evidence() -> tuple[dict[str, dict[str, list[str]]], dict[str, list[str]]]:
    """Per-axis, PER-KIND evidence names, and the unmatched artifacts of each
    census kind.

    Kept per kind rather than summed (R1526). R1522 reported perf as a single
    `11` for 4 examples plus 7 demos, which buries a kind that moves inside a
    kind that does not: against 37 examples one declared round is +2.7%, so nine
    depth rounds would be needed to cross a threshold that the round count on
    its own crosses at once.
    """
    counts: dict[str, dict[str, list[str]]] = {
        a["key"]: {k: [] for k in KINDS} for a in AXES
    }
    unmatched: dict[str, list[str]] = {}
    for kind, coverage in KINDS.items():
        owned, missed = assign(kind, universe(kind))
        for key, names in owned.items():
            counts[key][kind] = names
        if coverage == CENSUS:
            unmatched[kind] = missed
    return counts, unmatched


def kinds_of(axis: dict) -> list[str]:
    """The kinds this axis actually draws on, in KINDS order."""
    return [k for k in KINDS if patterns_for(axis, k)]


def drift(now: int, snapshot: int | None) -> tuple[bool, str]:
    if snapshot is None:
        return True, "no snapshot — never judged against counted evidence"
    if snapshot == 0:
        return (now > 0), f"{snapshot} -> {now}"
    delta = (now - snapshot) / snapshot
    return abs(delta) > STALE_AT, f"{snapshot} -> {now} ({delta:+.0%})"


#: Short names for the evidence columns. The counts are not comparable between
#: kinds any more than between axes, so every number printed says what it is of.
SHORT = {"example-name": "ex", "demo-body": "dm", "round-axis": "rd"}


def report() -> int:
    counts, unmatched = evidence()
    total_w = sum(a["weight"] for a in AXES)
    weighted = 0.0
    buildable_w = 0
    buildable_weighted = 0.0
    stale: list[str] = []

    declared = ledger()
    rounds = git_rounds()
    print(
        f"Phase B tally — {len(examples())} examples, {len(demos())} demos, "
        f"{len(rounds)} rounds since the R{LEDGER_FLOOR} floor\n"
    )
    print(f"{'axis':38s} {'w':>3s} {'done':>5s}   evidence per kind (judged -> now)")
    print("-" * 100)
    for axis in AXES:
        done = axis["completion"]
        snapshot = axis["evidence_snapshot"]
        cells = []
        for kind in kinds_of(axis):
            n = len(counts[axis["key"]][kind])
            is_stale, how = drift(n, snapshot.get(kind))
            if is_stale and axis["key"] not in stale:
                stale.append(axis["key"])
            cells.append(f"{SHORT[kind]} {how}")
        if done is not None:
            weighted += axis["weight"] * done / 100
            if not axis["gated"]:
                buildable_w += axis["weight"]
                buildable_weighted += axis["weight"] * done / 100
        gate = " [gated]" if axis["gated"] else ""
        shown = f"{done}%" if done is not None else "  ?"
        print(
            f"{axis['name'][:36] + gate:38s} {axis['weight']:3d} {shown:>5s}   "
            f"{' | '.join(cells)}"
        )
    print("-" * 100)
    print(f"{'weighted (all axes)':38s} {total_w:3d} {weighted:4.0f}%")
    if buildable_w:
        print(
            f"{'weighted (buildable only)':38s} {buildable_w:3d} "
            f"{buildable_weighted / buildable_w * 100:4.0f}%"
        )

    # Leverage = weight x remaining. The answer to "what next" derived from the
    # evidence rather than from whichever axis was written first in a list — the
    # value order in CLAUDE.md predates two re-tallies and is not re-derived when
    # completions move.
    lev = sorted(
        (
            (a["weight"] * (100 - a["completion"]), a["name"])
            for a in AXES
            if not a["gated"] and a["completion"] is not None
        ),
        reverse=True,
    )
    if lev:
        print("\nLEVERAGE (buildable only, weight x remaining) — highest first:")
        for score, name in lev:
            print(f"  {score:5d}  {name}")

    for kind, missed in unmatched.items():
        if not missed:
            continue
        if kind == "round-axis":
            # A round is not "unclassifiable" — it is undeclared, and the fix is
            # a line rather than a judgment, so it gets its own wording and its
            # own grep prefix in the push hook.
            print(
                f"\nUNDECLARED — {len(missed)} round(s) have no row in "
                f"{ROUND_LEDGER.name}. Add one (axis key, or `{NO_AXIS}` with a "
                f"reason); until then their work registers on no axis:"
            )
        else:
            print(
                f"\nUNCLASSIFIED — {len(missed)} {kind} artifact(s) belong to no "
                f"axis. Work with no axis is work this tally cannot see:"
            )
        for name in missed:
            print(f"  {name}")

    # A row for a round git has never seen is the round in progress: the ledger
    # line is written with the change and the commit does not exist until it is
    # made. Saying so is cheaper than leaving the reader to wonder, and a typo'd
    # round number looks exactly the same and wants the same look.
    ahead = sorted(r for r in declared if r not in set(rounds))
    if ahead:
        print(
            f"\nDECLARED AHEAD — {len(ahead)} ledger row(s) name a round with no "
            f"commit yet (the round in progress, or a mistyped number):"
        )
        for rnd in ahead:
            print(f"  R{rnd:<5d} {declared[rnd][0]}: {declared[rnd][1]}")

    # A probe's reach has to be visible, or "no axis looked" is indistinguishable
    # from "nothing was there" — which is the failure this tool keeps finding.
    for kind, coverage in KINDS.items():
        if coverage != PROBE:
            continue
        of_kind = set(universe(kind))
        total = len(of_kind)
        drawn = sum(len(of_kind.intersection(counts[a["key"]][kind])) for a in AXES)
        readers = [a["name"] for a in AXES if patterns_for(a, kind)]
        print(
            f"\nPROBE — {kind}: {drawn} of {total} counted, read only by "
            f"{', '.join(readers) if readers else '(no axis)'}. Unmatched "
            f"artifacts of a probe kind are not a finding."
        )

    print(f"\nEXCLUDED — {len(NOT_PHASE_B)} example(s) are not Phase B evidence:")
    for name, why in sorted(NOT_PHASE_B.items()):
        print(f"  {name:24s} {why}")
    if NOT_EVIDENCE:
        print(f"\nEXCLUDED — {len(NOT_EVIDENCE)} demo(s) match a pattern spuriously:")
        for name, why in sorted(NOT_EVIDENCE.items()):
            print(f"  {name:32s} {why}")
    no_axis = sorted(r for r, (a, _) in declared.items() if a == NO_AXIS)
    if no_axis:
        print(
            f"\nEXCLUDED — {len(no_axis)} round(s) declare no Phase B axis, with "
            f"the reason each gave:"
        )
        for rnd in no_axis:
            print(f"  R{rnd:<5d} {declared[rnd][1]}")

    if stale:
        print(
            f"\nSTALE — {len(stale)} axis judgment(s) rest on evidence that has "
            f"since moved more than {STALE_AT:.0%}: {', '.join(stale)}"
        )
        print("Re-judge them and update `judged_at` / `evidence_snapshot`.")
        return 1
    return 0


def selftest() -> int:
    """The tool's own logic, checked. A staleness detector that cannot report
    staleness is the very failure this round exists to fix."""
    fails = []

    def check(cond: bool, what: str) -> None:
        if not cond:
            fails.append(what)

    # drift()
    check(drift(100, 100) == (False, "100 -> 100 (+0%)"), "no drift is not stale")
    check(drift(126, 100)[0], "26% growth is stale")
    check(not drift(120, 100)[0], "20% growth is not stale")
    check(drift(70, 100)[0], "30% shrink is stale (evidence can be deleted)")
    check(drift(5, None)[0], "an unjudged axis is stale")
    check(drift(1, 0)[0], "first evidence against a zero snapshot is stale")

    # assign(): first-match-wins, and nothing is silently dropped
    names = ["hello-grid-sort", "hello-button", "hello-nothing-here"]
    owned, un = assign("example-name", {n: n for n in names})
    check(
        "hello-grid-sort" in owned["dcc"] or "hello-grid-sort" in owned["modelview"],
        "a grid example lands on a data axis",
    )
    check("hello-button" in owned["catalog"], "a button lands on the catalog axis")
    check(un == ["hello-nothing-here"], "an unmatched name is REPORTED, not dropped")
    total = sum(len(v) for v in owned.values()) + len(un)
    check(total == 3, "every input is accounted for exactly once")

    # R1522 — the property whose absence made this round necessary: an axis's
    # evidence must register work of the shape that axis actually receives. The
    # perf axis receives hot-path optimisations, which create no example, so
    # these two names are counted through `demo-body` or not at all. Under the
    # R1519 tool (example names only) this check FAILS.
    perf = next(a for a in AXES if a["key"] == "perf")
    counts, _ = evidence()
    for landed in ("r1520_scrolled_paint_cache.py", "r1521_shape_cache_working_set.py"):
        check(
            landed in counts["perf"]["demo-body"],
            f"the perf axis counts {landed} (a measured hot-path optimisation)",
        )
    check(
        any(k == "demo-body" for k, _ in perf["evidence"]),
        "the perf axis draws on demo bodies, not only example names",
    )

    # R1526 — the property whose absence made THIS round necessary, stated so
    # that it holds for work that has not happened yet. R1522's check above
    # names two demos that already exist, which guards a regression but can
    # never register anything new; this one is about the shape of the evidence,
    # not about any round. A count of artifacts moves only when an artifact is
    # CREATED, so an axis all of whose kinds are artifact counts is blind to a
    # round that improves what it already owns.
    unchanged = {"hello-grid-sort": "hello-grid-sort"}
    before, _ = assign("example-name", unchanged)
    after, _ = assign("example-name", unchanged)  # the round edited its contents
    check(
        len(before["modelview"]) == len(after["modelview"]),
        "an artifact count cannot see a round that only modified an artifact",
    )
    depth, _ = assign("round-axis", {"R9001": "modelview"})
    check(
        depth["modelview"] == ["R9001"],
        "a declared round registers on its axis, having created no artifact",
    )
    for a in AXES:
        check(
            any(k != "round-axis" for k in kinds_of(a)) and "round-axis" in kinds_of(a),
            f"{a['key']} can register both new artifacts and depth work",
        )

    # round-axis matches an axis key against a declaration, and `assign` matches
    # by substring — so the two agree only while no key is contained in another.
    # Asserted rather than assumed: adding an axis keyed `text-grid` would
    # silently hand every `text` round to it.
    keys = [a["key"] for a in AXES]
    check(
        not any(x != y and x in y for x in keys for y in keys),
        "no axis key contains another, so a declaration matches exactly one axis",
    )

    # per-kind drift: a kind that has not moved must not mask one that has.
    # Summed (the R1522 shape) these are 40 against 36, +11%, and silent.
    check(
        drift(37, 36)[0] is False and drift(3, 0)[0] is True,
        "a moved kind is stale even beside a kind of its own axis that has not",
    )

    # the ledger is strict: every way of writing a row wrong is a raise, because
    # a skipped row is a declared round counted as undeclared (or worse, a round
    # counted for an axis that a typo named).
    def raises(text: str, why: str) -> None:
        try:
            parse_ledger(text)
        except ValueError:
            return
        fails.append(f"ledger accepts {why}")

    check(
        parse_ledger("# note\n\n1600\tperf\twhy\n") == {1600: ("perf", "why")},
        "a ledger row parses to its axis and reason, comments and blanks ignored",
    )
    raises("1600\tperf\n", "a row with a missing field")
    raises("1600\tperfomance\twhy\n", "a row naming an axis that does not exist")
    raises(f"{LEDGER_FLOOR}\tperf\twhy\n", "a row at or below the declarable floor")
    raises("1600\tnone\t\n", "an exclusion with no reason")
    check(
        parse_ledger("1600\tnone\ta process round\n")[1600][0] == NO_AXIS,
        f"`{NO_AXIS}` is a legal declaration when it carries a reason",
    )

    # git, not the ledger, is the census of which rounds exist — so the subject
    # parse has to match what `commit-msg` enforces and nothing else.
    check(
        rounds_in("feat(core): R1524 the data grid asks for the cells\n") == [1524],
        "a conforming subject declares its round",
    )
    check(
        rounds_in("fix(rpc): R1481 revisit the R744 windowing decision\n") == [1481],
        "a subject citing another round still declares only its own",
    )
    check(
        rounds_in("fix(a): R1481 one\nfix(b): R1481.1 two\n") == [1481],
        "a round that landed as two commits is one round",
    )
    check(
        rounds_in("Merge branch 'main'\nchore: no round here\n") == [],
        "a subject with no round tag declares nothing",
    )

    # demo-body matches the SOURCE, not the name — else it is a name proxy with
    # extra steps, and the six frame_timings readers would slip back in.
    body_owned, _ = assign(
        "demo-body",
        {
            "named_frame_budget_only.py": "nothing a cost counter would say\n",
            "r9999_unrelated_name.py": "resp = tf.cache_stats()\n",
        },
    )
    check(
        "named_frame_budget_only.py" not in body_owned["perf"],
        "a perf-sounding demo NAME with no counter in its body is not evidence",
    )
    check(
        "r9999_unrelated_name.py" in body_owned["perf"],
        "a counter in the BODY is evidence whatever the demo is named",
    )

    # every kind an axis declares must be a known kind with a coverage rule,
    # and every axis needs at least one census source or it is invisible to the
    # census that reports work belonging to no axis
    for a in AXES:
        for kind, _ in a["evidence"]:
            check(kind in KINDS, f"{a['key']} declares unknown kind {kind}")
        check(
            any(KINDS.get(k) == CENSUS for k, _ in a["evidence"]),
            f"{a['key']} has no census source",
        )
    check(
        CENSUS in KINDS.values() and PROBE in KINDS.values(),
        "both coverage rules are in use, else the distinction is decoration",
    )

    # weights are a whole
    check(sum(a["weight"] for a in AXES) == 100, "axis weights sum to 100")
    check(
        any(a["gated"] for a in AXES) and any(not a["gated"] for a in AXES),
        "both gated and buildable axes exist, else the subtotal is meaningless",
    )

    # leverage: a low-completion axis must outrank a high-completion one of the
    # same weight, else the ordering says nothing
    a = {"weight": 10, "completion": 20, "gated": False}
    b = {"weight": 10, "completion": 90, "gated": False}
    check(
        a["weight"] * (100 - a["completion"]) > b["weight"] * (100 - b["completion"]),
        "leverage ranks the less-complete axis higher at equal weight",
    )

    for f in fails:
        print(f"SELFTEST FAIL: {f}")
    print(f"selftest: {'PASS' if not fails else 'FAIL'} ({len(fails)} failure(s))")
    return 1 if fails else 0


if __name__ == "__main__":
    if "--selftest" in sys.argv:
        sys.exit(selftest())
    sys.exit(report())
