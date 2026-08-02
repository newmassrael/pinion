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
                "dock-", "tree-", "tree-view", "column-", "cell-select",
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
        "judged_at": 1544,
        "completion": 92,
        "evidence_snapshot": {"example-name": 26, "round-axis": 2},
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
                "table", "grid-", "streaming-log", "tail-reveal", "live-data",
                "multi-select", "listbox", "flex-virtual",
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
        "judged_at": 1536,
        "completion": 87,
        "evidence_snapshot": {"example-name": 37, "round-axis": 6},
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
        #   * Press-and-hold auto-repeat (`QAbstractButton::setAutoRepeat`):
        #     holding a spin arrow or a scrollbar arrow steps ONCE here. No
        #     repeat timer exists anywhere in the tree (the `auto_repeat`
        #     hits are all about OS *key* repeat, a different thing). With
        #     mnemonics closed this is now the largest CROSS-CUTTING item —
        #     the other remaining ones are individual widget kinds.
        #   * Qt also has `wheelEvent` on `QComboBox` and `QTabBar`; R1533
        #     covered value arithmetic, not index arithmetic.
        #   * NEW at R1543 — the capability is universal but ADOPTION is
        #     three sites (menu titles, menu items, one buddy label). Every
        #     other catalog paint helper takes a plain `&str` label and calls
        #     `TextNode::styled`, so `&Save` on a button is inert until each
        #     helper routes through `TextNode::mnemonic_styled`. Deliberately
        #     not done blind: a helper whose label ALSO feeds a hand-passed
        #     a11y name has to resolve the markup there too, which R1543 hit
        #     once (`menu_item_nodes`) and did not audit for across the tree.
        #   * Absent widget kinds, in rough order of how much a pro tool
        #     misses them: `QGroupBox` (especially checkable — no titled
        #     group frame exists), `QDial`, a paged container
        #     (`QStackedWidget` / `QWizard`), `QKeySequenceEdit`,
        #     `QFontComboBox`, and the standard `QMessageBox` /
        #     `QInputDialog` canned dialogs.
        #
        # R1543 re-judgment, 84 -> 87, demanded by the tool (round ledger
        # 1 -> 2). +3 and not more: what closed is cross-cutting and closed
        # past Qt, but what remains is six absent widget kinds plus the
        # second cross-cutting interaction gap — more surface than the round
        # filled — and the round added a stated gap of its own (adoption).
        "judged_at": 1543,
        "completion": 87,
        "evidence_snapshot": {"example-name": 73, "round-axis": 2},
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
        "judged_at": 1534,
        "completion": 77,
        "evidence_snapshot": {"example-name": 24, "round-axis": 3},
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
        "judged_at": 1542,
        "completion": 75,
        "evidence_snapshot": {"example-name": 9, "round-axis": 2},
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
        "judged_at": 1538,
        "completion": 78,
        "evidence_snapshot": {"example-name": 5, "demo-body": 11, "round-axis": 6},
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
                "forge-counter",
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
        "judged_at": 1539,
        "completion": 42,
        "evidence_snapshot": {"example-name": 7, "round-axis": 1},
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
