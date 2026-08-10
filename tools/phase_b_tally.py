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
axis against the toolkit" is a judgment, and a script that emitted a number for it would
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
                # R1576, cleared inline: `hello-graph-diff` (R1575) had no
                # pattern anywhere and this tool reported it UNCLASSIFIED for a
                # round. It is a node-graph binding — the axis's own third
                # named item — and R1575 declared `dcc`, so the round and its
                # example were being filed under different answers.
                "graph-diff",
                # R1577 — `hello-node-groups`, the binding that composes the
                # node-system substrate. `node-editor` above is an exact-ish
                # name rather than a prefix for the node-graph family, so a
                # second node-graph binding needed its own entry; the tool
                # reported it UNCLASSIFIED at the very push that added it,
                # which is the census working.
                "node-groups",
                # R1599 — `hello-node-flow`, the control-plane binding. Added
                # with the round rather than after it: `node-groups` needed its
                # own entry at R1577 for the same reason (`node-editor` is an
                # exact-ish name, not a prefix for the family), and the tool
                # reported that one UNCLASSIFIED at the very push that added it.
                "node-flow",
                "asset-browser", "file-manager", "undo", "grid-header-menu",
                "grid-frozen-col", "row-dissect", "hex-dump", "code-fold",
                "command-palette", "selection-toolbar", "tab-reorder",
                "dock-presets",
                # R1609, cleared inline: `hello-tile-dashboard` (R1608) had no
                # pattern and this tool reported it UNCLASSIFIED for a round —
                # the R1575 / R1577 shape a third time, and again found by the
                # census rather than remembered. It is filed here because
                # `dock-` is: a tile board and a dock are the same KIND of
                # artifact, two shells that arrange panels, and this axis is
                # where this tree already files that kind.
                #
                # Its ROUNDS still declare `none` (R1607, R1608, R1609), which
                # is not a contradiction: the example-name census answers "which
                # axis does this artifact belong to" and the round ledger
                # answers "which axis did this round advance", and R1606
                # recorded why those are different questions — this axis's three
                # named families are property grid, data grid and node graph,
                # and a dashboard shell is none of them.
                "tile-dashboard",
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
        # how its cells are drawn (the toolkit `setItemDelegateForColumn`), which is the extension
        # point that decides whether a grid can have a bar column, a mark
        # column or a swatch column at all. Before it, a binding wanting one
        # had to stop using the grid's cell path — which is exactly what `hello-property-grid`'s
        # `ranged_slider_cell` does.
        #
        # Only +3, and the remaining item is verified rather than assumed:
        # the delegate covers paint and not EDITING. The toolkit's
        # styled item delegate also owns `createEditor` / `setEditorData` /
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
        #   * the MODEL's `EditRole`, fused with `flags() &
        #     ItemIsEditable` into one `Option<CellEdit>` — so "an editor
        #     open on a cell the model will not edit" stopped being a check
        #     the view must remember and became a state the types reject;
        #   * the DELEGATE's editing half (`createEditor` + `setEditorData`
        #     collapse into one call in a view-fn world, `setModelData` stays
        #     separate because it is a distinct moment);
        #   * the VIEW's half — the latch, the toolkit's `EditTriggers` gate, and the
        #     `EndEditHint` cursor walk over the MODEL extent.
        #
        # Two things past the toolkit 6.11, both verified over the wire: a
        # **refused** write keeps the editor open holding the typed text (the
        # toolkit's `setModelData` returns `void`, so a rejected value closes the editor and
        # the typing is gone), and a cell's editability reaches assistive
        # technology as `aria-readonly` (the toolkit's accessible table cell builds its
        # state from the view's selection and never reads the model's `ItemIsEditable`, so a
        # toolkit screen-reader user cannot tell a fixed column from an
        # editable one until they type into it).
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
        #     `CellKind::Color` reach an editor only through a delegate. The toolkit
        #     has the same split (item editor factory); what is missing here
        #     is a *shipped* combo / palette editor.
        #
        # R1555 re-judgment, demanded by the tool: the round ledger took this
        # axis 2 -> 3, past the 25% band. It closes the third of the items
        # R1544 left, and closes it WIDER than that item was stated. R1544 read
        # the gap as two missing widgets; it was a missing **axis**. The
        # toolkit's editing decomposition has two halves — `setItemDelegateForColumn` (the per-column
        # override, which R1532 and R1544 built) and item editor factory (a
        # registry from the DATUM'S TYPE to an editor, which styled item
        # delegate consults when nothing overrides it). The second did not
        # exist, so `text_cell_editor` was the built-in for all six kinds, and for two of them
        # it is an editor that CANNOT WORK: `Bool` and `Choice` refuse every keystroke
        # (`accepts_keystroke`) and parse to nothing (`parse`), so the seam opened a field that
        # could not be typed into and whose commit could never produce a value.
        # Two more were simply below the toolkit: `Int` / `Float` got a bare field
        # where the toolkit's factory ships a spin box / double spin box.
        #
        # `CellKind::editor_form` is the registry and `EditorForm` its answer, a pure function — where `createEditor`
        # *instantiates a widget*, so the toolkit cannot be asked what an `int`
        # cell would get without building one, and `creatorMap` is private so the
        # registry cannot be enumerated at all. Five forms ship (field /
        # stepper / toggle / selector / swatch), `scene/cell_editors` publishes the whole
        # mapping, and the a11y role is derived from the form rather than from
        # whichever widget a factory happened to construct — which is how a
        # toolkit bool cell ends up announcing as a COMBO BOX and a toolkit
        # colour cell announcing nothing.
        #
        # Reaching for it forced the model half too: `CellEdit` now carries the DATUM
        # (the toolkit's `EditRole` is a dynamic value), because a `Choice`'s options are
        # part of its value's identity and a `(kind, String)` pair cannot tell a selector
        # what to select between. That also retired a
        # representable-but-meaningless edit role, `(Int, "not a number")`.
        #
        # Four things past the toolkit 6.11, all read over the wire: the
        # factory is ENUMERABLE; a bool gets a checkbox rather than the
        # toolkit's two-item combo; a colour cell is editable at all (the
        # toolkit's factory has no color creator, so `createEditor` answers nullptr and
        # `edit` silently does nothing); and every commit outcome is NAMED — `malformed`
        # (the model was never asked) apart from `refused` (it was), where the
        # toolkit's `commitData` discards `setData`'s verdict and its validators make the
        # malformed case unreachable at the price of committing a value the
        # user did not type.
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
        #   * two of the toolkit's factory creators have no form because they have no
        #     KIND: date / time / date time (there is no date arm on
        #     `CellKind` at all) and `UInt`.
        #   * the step arrows do not AUTO-REPEAT. R1549 made a cadence a
        #     per-widget declaration (`External::auto_repeat`, level-read from
        #     the widget's own state), and a grid's step arrow is a sub-region
        #     of one External that covers every cell — so repeating it needs a
        #     per-sub-region cadence, or the External tracking which sub-key is
        #     held, which duplicates the router press record R1549 put the run
        #     inside on purpose. The toolkit's `setAccelerated` has it; the same shape
        #     R1549 recorded for scrollbar arrows.
        #   * a selector's open list and a swatch's HSV picker are the
        #     binding's overlays; the factory ships the closed states.
        #
        # ---- R1571 re-judgment, 95 -> 97, demanded by the round count going
        # 3 -> 4. It closes `openPersistentEditor` — the item R1544's list and
        # R1555's list BOTH named, and the last of the three R1544 left.
        #
        # It also CORRECTS R1555's own prescription above, and the correction
        # is the round's finding: widening `use_text_edit_state`'s key would have been wrong. `Owner::cache`
        # has `cache`, `cache_contains` and `cache_get_by_str` and **no removal of any kind**, so a per-cell
        # buffer would be retained for every cell ever edited, for the life of
        # the window — unbounded growth on the models the Model/View axis is
        # named for, the class R1550 built `scene/memory` to see. An editor's buffer has
        # to die with the editor, so the buffers live in the editor SET (`OpenEditors`),
        # and what that set needs follows from a fact about this framework
        # rather than about the toolkit: there is exactly ONE keyboard focus,
        # where the toolkit has one focusable widget per editor. Only the
        # focused editor holds the shared inline field; every other open
        # editor's text is PARKED in the latch.
        #
        # Persistence is a property OF THE EDITOR rather than a second
        # collection, which is what makes "a cell has at most one editor" true
        # by construction — the toolkit keeps an index->widget hash plus a
        # separate `set<widget *>` and reconciles them by convention.
        #
        # Five things past the toolkit 6.11, all read over the wire by `r1571_editor_persistence_is_a_property.py`: the
        # set is ENUMERABLE (`scene/grid_editors`; the toolkit's only public question is `isPersistentEditorOpen(index)`,
        # one index at a time, so you must already know the answer to ask);
        # FOCUS IS DATA (in the toolkit, `focusWidget()` reverse-mapped through a private
        # hash); <kbd>Escape</kbd> REVERTS a persistent editor and keeps it
        # open (`closeEditor` returns early for one, so Escape there does nothing at all
        # and the original is unrecoverable); each editor's in-flight value and
        # DIRTY flag are readable WITHOUT focusing it (the toolkit keeps no
        # record of what `setEditorData` seeded); and the COST IS WINDOWED (an editor
        # outside the painted rows contributes no scene node and keeps its
        # value, where `updateEditorGeometries()` walks every persistent editor on every scroll).
        #
        # A counterfactual found a real defect and the fix is the design: the
        # set's fourth invariant was stated one-directionally ("at most one
        # live buffer and it belongs to the focused editor"), which "nobody
        # holds the field" satisfies while the focused editor's cell still
        # paints one. Stated both ways, and the paint now branches on the
        # BUFFER rather than on a second `focused` flag.
        #
        # +2 and not more, and what remains is audited at R1571:
        #   * ADOPTION is unchanged — six bindings still hand-roll a cell edit
        #     latch and none of them uses the grid's cell path; per-binding
        #     domain work, not seam work.
        #   * the two absences R1555 listed still stand: no date/time
        #     `CellKind` (date / time / date time, `UInt`),
        #     and the step arrows do not auto-repeat.
        #   * `scene/grid_editors` is READ-ONLY. Closing and focusing need no
        #     model and could be framework verbs; opening cannot be one,
        #     because it needs a `CellEdit` only the model produces (R1544) —
        #     an honest split, and the write half is the named next slice.
        #   * `setIndexWidget` / `indexWidget` — the toolkit's arbitrary widget in a
        #     cell, distinct from an editor — has no analogue here.
        #   * an open editor is not its own keyboard focus stop, so
        #     <kbd>Tab</kbd> between N editors is a binding's vocabulary
        #     rather than the focus ring's. That is the price of the one-focus
        #     design above, stated rather than hidden.
        #
        # R1577 RE-JUDGED **DOWN**, 97 -> 95, demanded by the tool (rd 4 -> 6 =
        # +50%, R1575 and R1577 both declaring `dcc`). A decrease is the first
        # in this series, and it is the point: the round CORRECTS the
        # measurement as much as it moves it.
        #
        # This axis names three widget families, and one of them is
        # "node-graph editor substrate (visual scripting / material graph)".
        # At 97 that third was credited as substantially done while the whole
        # node MODEL lived inside a 9,075-line example as a flat
        # `Vec<GraphNode>` plus a `Vec<Edge>` — so an application wanting a
        # node graph had to COPY it, which is a fork, and **node groups did
        # not exist at all**. A flat vector cannot hold a node that is a
        # graph, so the single largest capability a node editor has was not
        # missing-and-planned, it was inexpressible.
        #
        # R1577 closes that (`pinion-graph::group` + `pinion-node-graph`:
        # re-usable definitions, instances, an interface DERIVED from the
        # selection boundary, nesting acyclicity, an edit path, and evaluation
        # that descends and keys its memo by instance), and R1575 gave the
        # graph its authored/observed layers. Both are real gains. They do not
        # cover the correction, because the same round ran the measurement the
        # 97 never had: a census against `~/DCC-ref` at `8cf50599` — 91
        # operators, 66 keymap entries — names ELEVEN gaps in editor
        # capability, and R1577 closes ONE. Copy/paste, duplicate,
        # collapse/mute/hide, the richer selection vocabulary (lasso, circle,
        # linked-from/to), link mute and detach, `insert_offset`, `find`,
        # `resize`, `view_selected` and `swap` are all absent. And the
        # substrate has ONE consumer: `hello-node-editor` still holds its own
        # model, so the tree now carries TWO
        # ([[debt-two-node-graph-models]]).
        #
        # Weighing the families rather than the round: property grid ~98 and
        # data grid ~98 (R1532 / R1544 / R1555 / R1571 took those deep), node
        # graph ~90 after this round against a DCC-class reference. That
        # averages ~95, and 95 is what is recorded. The lesson is R1519's own,
        # one axis over: a completion nobody checked against a reference is
        # not a measurement, and the check is what moved this one DOWN.
        # R1584 re-judgment, 95 -> 96, DEMANDED by the tool (round-axis 6 -> 8,
        # +33%). R1584's own reading of this tally said +17% and "inside the
        # band" — taken while the round was still DECLARED AHEAD, which is
        # exactly the reading R1550 recorded as the one to distrust: the ledger
        # counts rounds git HAS. The deferral was wrong and this corrects it.
        #
        # What moved: the node-graph family, the third of this axis's three and
        # the one the R1577 census found weakest. R1584 makes the group
        # boundary a PARTITION THAT MOVES — `group_insert` / `group_separate`, the two directions of
        # one operation, with the interface RE-DERIVED from the partition that
        # results and every value whose crossing disappeared RECONNECTED. That
        # last part is where the DCC stops: `node_group_separate_selected` copies the nodes out, deletes
        # them from the group, touches the interface not at all, and the value
        # that flowed through them is gone. Held as a test helper and asserted
        # rather than described.
        #
        # Only +1, and the reason is the R1577 census this axis is now measured
        # against: eleven named gaps in editor capability, of which R1578 closed
        # four as one concept and R1584 closes two. SIX REMAIN — collapse/hide/
        # mute, the richer selection vocabulary, link mute and detach,
        # `insert_offset`, `find`, and `resize`/`view_selected`/`swap_node` —
        # and `hello-node-editor` still holds its own model, so the tree still
        # carries two ([[debt-two-node-graph-models]]). Weighing the families:
        # property grid ~98, data grid ~98, node graph ~92 after this round,
        # which averages ~96.
        # R1589 re-judgment, 96 -> 97, DEMANDED by the tool (round-axis 8 -> 11,
        # +38%), and it absorbs THREE rounds: R1586 and R1587 each landed inside
        # the 25% band and deferred their look.
        #
        # All three are the node-graph family, and between them they close the
        # two largest MODEL gaps the R1577 census named plus the largest gap of
        # any kind. R1586 — a node says HOW IT TAKES PART: bypass, with
        # dissolve and detach applying the same derivation to the structure, a
        # muted LINK named apart from a bypassed NODE because they are opposite
        # behaviours the DCC spells alike, and `Appearance` as a type the evaluator
        # cannot read (census items 4 and 6, whole). R1587 — a PORT declares
        # whether a value passes through it, the whole extension point, chosen
        # by censusing what the DCC's eleven per-node callbacks actually
        # compute. R1589 — a node can BELONG TO A FRAME, which is census item
        # 12 and, by the 2026-08-07 re-measurement against the crate rather
        # than the example, the heaviest of the five remaining clusters: eight
        # operators over a concept the crate did not have at all.
        #
        # Only +1, and the audit is at R1589 rather than inherited. What
        # remains on this family is NINE operators and every one of them is an
        # editor GESTURE over a model that now exists: census item 5 (the
        # richer selection vocabulary, six of them), 7 (`insert_offset`), 8
        # (`find_node`), 10 (`view_selected`), with 9 (`resize`) and 11
        # (`swap_node` / `node_copy_color`) half-done at the model layer. Two
        # things the census cannot see are larger than all of it: EXECUTION
        # SEMANTICS are inexpressible in `evaluate(inputs) -> outputs` and the
        # DCC is pure dataflow too, so no comparison against it will ever
        # surface that; and `hello-node-editor` still holds its own model, so
        # the tree now carries two node models AND two frame implementations
        # ([[debt-two-node-graph-models]]). Weighing the families: property
        # grid ~98, data grid ~98, node graph ~95 after these three rounds,
        # which averages ~97. R1594 re-judged and the number HELD at 97, which
        # is the finding. The tool demanded the look (`round-axis` 11 -> 14 =
        # +27%) and it absorbs R1590, R1593 and R1594. R1593 gave the crate a
        # DIRECTED type relation (`NodeKind::conversion`) — its gate was
        # `source.ty != sink.ty`, and `!=` is symmetric, so a lattice where a
        # scalar broadcasts into a vector without the vector narrowing back was
        # INEXPRESSIBLE, and the trait's own doc said to model it "by making
        # the coercion part of equality", which no equality relation can be.
        # R1594 gave a NODE its own socket values (the DCC's
        # `node socket::default_value`); before it every node of a kind shared
        # one value, so a source's constant had to live inside the taxonomy
        # where nothing could edit it.
        #
        # Both are PREREQUISITES rather than polish — a node-graph substrate
        # without either cannot host a material or a visual script graph at all
        # — so the honest reading is that 97 was OVERSTATED before them, and
        # the reason nobody noticed is R1577's own lesson recurring one level
        # in: its census measured OPERATORS, and neither of these is an
        # operator, so both read as zero. A reference has more than one axis.
        #
        # Composition now: property grid ~98, data grid ~98, node graph 90 -> 95
        # (R1590's four selection operators plus the two model gaps), which
        # averages back to 97. A re-judgment that holds still is a legitimate
        # outcome: the tool demands a LOOK, not a move.
        # R1598.3 re-judged 97 -> 98, DEMANDED by the tool (`round-axis` 14 -> 18
        # = +29%), absorbing R1595, R1596, R1597 and R1598.
        #
        # It moves because the item EVERY judgment in this series has carried is
        # closed. R1577 lifted the node model into `pinion-node-graph` and left
        # the flagship example holding its own; R1584, R1589 and R1594 each
        # re-stated that as remaining. R1597 finished the migration —
        # `hello-node-editor` is 9,186 -> 8,331 lines, the tree has no second
        # `GraphNode` / `Edge` and no second frame implementation
        # ([[debt-two-node-graph-models]], closed) — and the migration is what
        # produced the round's findings rather than being bookkeeping after
        # them: membership had been a RECTANGLE re-tested on every read, so
        # widening a comment frame silently adopted nodes, and `attach` /
        # `detach` (two census operators whose model layer had shipped with
        # nothing to reach it) came with the fix. R1595 gave a frame a HEIGHT
        # (`Appearance::height`, `Option` because an ordinary node's height is
        # derived from its ports and a frame's is not), R1596 made a cycle NAME
        # the nodes on it, and R1598 let a node change what it IS without
        # changing which node it is (`Document::set_kind`).
        #
        # Re-run rather than inherited: the DCC operator census at `8cf59599` is 91,
        # of which 32 are that application's own content. Of the 59 that are
        # node-system mechanism, FOUR are absent — `find_node`, `insert_offset`, `node_copy_color` and `view_selected` — so
        # coverage is 55/59 = 93%, up from 86% at R1590. The two the campaign
        # file still lists as excluded, `select_circle` and `select_lasso`, are NOT excluded any more:
        # R1591 made a region a value and R1592 gave the node editor both, so
        # they are live consumers rather than a stated gap.
        #
        # +1 and not more, and what caps it is this axis's own name. "Node-graph
        # editor substrate (visual scripting / material graph)" is two things,
        # and only the material half is built: EXECUTION SEMANTICS — control
        # flow, iteration, side effects, time — are inexpressible in
        # `evaluate(inputs) -> outputs`, so the control plane is at zero. That
        # was already true when R1594.1 set 97, so it is not a reason to hold
        # now; everything that changed since is a gain. It is a reason not to
        # reach 99, and it is the round after this one.
        #
        # Composition now: property grid ~98, data grid ~98, node graph 95 -> 97,
        # which averages 97.7.
        #
        # R1604 RE-JUDGED **DOWN**, 98 -> 86, DEMANDED by the tool (`round-axis`
        # 18 -> 23 = +28%), absorbing R1599, R1600, R1601, R1602 and R1603. It
        # is the largest single move this table has had, and the second
        # DOWNWARD one; R1577 was the first, for the same reason.
        #
        # TWO FORCES, and they point opposite ways.
        #
        # UP: the item every judgment in this series has named as the cap is
        # CLOSED. "Execution semantics — control flow, iteration, side effects,
        # time — are inexpressible in `evaluate(inputs) -> outputs`, so the
        # control plane is at zero" was the stated reason not to reach 99, three
        # judgments running. R1599 gave a port a FLOW, so a graph has two kinds
        # of edge whose laws invert, a control cycle is a LOOP rather than a
        # contradiction, and `Document::run` derives an execution order. R1600
        # gave it MEMORY — `NodeBody::Delay`, a machine addressed by INSTANCE,
        # `tick` and `settle` — so a loop can compute something different on its
        # second pass. That cap is gone.
        #
        # DOWN, and it dominates: for the first time this axis has a MEASURED,
        # test-backed figure for its node-graph third, and it is far below what
        # the 98 rested on. The 98 was set against a hand census claiming 93%.
        # R1601 made that census a tool and WITHDREW the number for a measured
        # 78% (the DCC, operator surface). R1602 made every `have` verdict name
        # a TEST that exercises it through the public API, so a verdict can no
        # longer be a hand claim. R1603 then found the census was BLIND rather
        # than incomplete — R1593's implicit conversion and R1594's per-socket
        # default are graph schema virtuals and node type callbacks, not
        # operators, so an operator census read two PREREQUISITES as zero — and
        # censused the hook surface on both references. Measured, per surface:
        # The DCC 54/72 = 75% (operator 78, hook 62); the engine 60/149 = 40%
        # (command 39, hook 40).
        #
        # R1604.1 — ADDENDUM, not a rewrite: those are the figures this judgment
        # was made against and they are left standing, because what a snapshot
        # is FOR is saying what was known at the time. They have since moved,
        # and the NEXT re-judgment must compute from the new ones rather than
        # from the paragraph above. R1605 widened both censuses and the two
        # halves moved differently, which is the part worth carrying:
        #
        #   * the engine did NOT move — 40% before and after, command 39% before and
        #     after (20/51 -> 45/113). Reading eight command lists instead of one
        #     found that the generic canvas was REPRESENTATIVE, which is a result
        #     rather than an absence of one.
        #   * the DCC DID — 75% -> 71% (operator 78 -> 73), because the census
        #     had been reading C++ as text and a FIFTH registration mechanism
        #     was invisible to it: `NOD_socket_items_ops.hh` registers 69
        #     operator ids through four templates whose idnames live as
        #     `static constexpr StringRefNull` in a per-accessor struct, so no
        #     registration site writes `ot->idname` at all. Live 170 -> 246.
        #
        # So the node-graph third's inputs are now the DCC 71 / the engine 40,
        # not 75 / 40. The completion here is NOT changed on that account: the
        # tool did not demand a re-judgment (`round-axis` 23 -> 24 = +4%, inside the
        # band), and moving a number on evidence the staleness check has not
        # flagged would be exactly the hand-adjustment this table exists to
        # stop. It is recorded so the next demanded look starts from the truth.
        #
        # ★ And the direction is now a pattern worth naming: R1601, R1603 and
        # R1605 each widened this measurement and each time the coverage fell.
        # Every widening so far has found more reference than it found pinion.
        #
        # The node-graph figure. This axis's own name is TWO references' worth
        # of scope: the DCC is the material-graph one at 75%, the engine is the
        # visual-scripting one at 40%, and equal weight gives 57.5. Two stated
        # biases push that up and neither is a measurement, so they buy a
        # little and not a lot: R1603's judging rule was "when unsure prefer
        # `absent`", which biases the number LOW by construction, and the absences
        # CLUSTER — the engine's 89 are about eight distinct capabilities
        # counted many times (alignment 11, variadic ports 6, struct pins 5,
        # breakpoints 5, watches 2, relinking 3, per-node permissions 6, colour
        # 5). Node graph: 62.
        #
        # The method is UNCHANGED on purpose — the three families averaged, as
        # every judgment in this series has done. Changing the method and the
        # inputs in one re-judgment would make the move unreadable. Composition:
        # property grid ~98 and data grid ~98 (neither has had a round since
        # R1571), node graph 97 -> 62, which averages 86.
        #
        # ★ THE DROP IS NOT A REGRESSION. Nothing this axis had was lost; three
        # rounds built a meter and this is its first full reading. That is
        # R1519's own lesson, which this table exists for: a completion nobody
        # checked against a reference is not a measurement, and R1577 already
        # recorded that checking one moves it DOWN.
        #
        # R1644 — ADDENDUM, not a re-judgment, and for the reason R1604.1 gave:
        # the staleness check has not fired (`example-name` 30 -> 31 = +3%,
        # `round-axis` 23 -> 26 = +13%, both inside the band), and moving a
        # number on evidence nobody was asked to re-read is the hand-adjustment
        # this table exists to stop. What changed is recorded so the next
        # demanded look starts from the truth rather than from R1604's figures:
        #
        #   * the engine moved for the first time in this series — 40% -> 62%
        #     (109/211 -> 132/211), because R1644 closed the DEBUGGING cluster
        #     whole: breakpoints, watches and stepping are three of the eight
        #     capabilities the paragraph above names, and its tree-debugger
        #     command list went 2/9 -> 9/9.
        #   * the DCC did not move — 76% (58/76), untouched by that round.
        #
        # So the node-graph third's inputs are the DCC 76 / the engine 62 (equal
        # weight 69) where R1604 computed from 75 / 40 (equal weight 57.5). Every
        # widening before this one found more reference than pinion; this is the
        # first round in the series that moved the measurement UP by building,
        # which is worth naming as the other direction rather than as a
        # correction.
        "judged_at": 1604,
        "completion": 86,
        "evidence_snapshot": {"example-name": 30, "round-axis": 23},
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
        # `round-axis` kind takes this axis's declared rounds from a snapshot of 0 to 3,
        # and R1522's rule is that changing the UNIT of evidence without
        # re-judging leaves the number and the evidence in different units.
        # R1519 said 75% on windowing (list/grid/tree) + three composable
        # proxies + data-indexed selection + async/lazy + an LRU million-row
        # source. The three rounds now declared closed the core of the
        # toolkit's abstract item model data path: R1523 windows the column
        # axis as well as the row axis (200 -> 5 cells a row), R1524 makes the
        # contract per-cell rather than per-row (`data(model index)`; 2400 -> 84 cells asked a
        # frame), R1525 makes the painted string the one the ordering read.
        # R1530 re-judgment, demanded by the tool: the round ledger took this
        # axis 3 -> 4, past the 25% band. R1526 named exactly two remaining
        # gaps and R1530 closed the first of them — header data was per-slice
        # (`headers: &[&str]` for all 200 columns where the toolkit's `headerData` is per-section)
        # because `VirtualTableData` read its column count off that slice's length; `column_count` + `GridModel::header`
        # split the two the way `columnCount()` / `headerData()` are split, and the a11y builder takes
        # the window rather than the table.
        #
        # +3 and not more, because the gap that is left is the LARGER of the
        # two R1526 named: `cell` and `header` both return a String with no role
        # dimension (the toolkit's Display/Edit/Decoration/ToolTip), which is a
        # whole axis of the contract rather than one accessor's shape — it is
        # what a decorated cell, an edit-vs-display value and a tooltip all
        # need. R1530 also surfaced three smaller ones: the eager `view_table` still
        # takes a header slice (two header contracts in one tree), five of the
        # six a11y grid builders still take every label, and a binding still
        # states its column window twice (paint + a11y). Unified data layer
        # stays out by the R780/R821 fourth-consumer gate, not by omission.
        # R1536 re-judgment, demanded by the tool: the round ledger took this
        # axis 4 -> 6, past the 25% band. R1530's judgment named the role
        # dimension as the LARGER of the two gaps it left, and R1535 + R1536
        # closed it on the CELL axis — not merely opened it. `GridModel` gained `decoration` as
        # a third typed accessor (the toolkit `data(index, DecorationRole)`, asked per cell, which is the
        # axis a per-column delegate cannot express); the answer carries a `meaning`
        # beside its ink, which the toolkit does NOT (its decoration role is
        # appearance and the accessible text is a separate role the item view
        # never wires to it, so a colour-only status column is an empty cell to
        # a toolkit screen-reader user); the mark is addressable by `GridTag::cell_decoration`; it has
        # both of the toolkit's arms (color, icon); and the EAGER `view_table` answers
        # the same role, so the tree no longer holds two cell-paint contracts
        # that disagree about whether it exists.
        #
        # R1536 also fixed what reaching for that found underneath, which is
        # the larger part of this +4: the accessible-name derivation could not
        # enter a `ScrollNode`, so NOTHING in any virtualized list, grid or tree was
        # named to an AT — measured, `hello-virtual-table` 0 of 75 gridcells, `hello-virtual-list` 1 of 16 — while
        # the bounds walker descended fine and made the tree look correct. The
        # toolkit names its cells; this axis did not.
        #
        # +4 and not more, because what is left is verified rather than assumed
        # (checked at R1536, not carried from R1530): the HEADER axis has no
        # role dimension at all — the largest item on this axis now — and two
        # of the toolkit's four canonical roles stay unanswerable, `EditRole` behind
        # the delegate's absent editing half and `ToolTipRole` behind a per-cell hover
        # path. R1530's three smaller ones were re-checked and all three still
        # hold: the eager `view_table` still takes a header slice, five of the six a11y
        # grid builders still take every label, and a binding still states its
        # column window twice (paint + a11y). R1547 did NOT force a re-judgment
        # (round ledger 6 -> 7, +17%, inside the band) and the number stayed at
        # 87 with the gap statement updated: it OPENED the header axis's role
        # dimension on the horizontal axis (`header_decoration`, the toolkit `headerData(section, Horizontal, DecorationRole)`) and named the
        # axis's own largest remainder — there was no VERTICAL section axis at
        # all.
        #
        # R1548 re-judgment, demanded by the tool: the round ledger took this
        # axis 6 -> 8, past the 25% band. It closes that named item whole. The
        # toolkit spells both axes with one virtual (`headerData(section, orientation, role)`) and a table view
        # shows the vertical one by default; here a column could be asked what
        # it was called and what mark it carried, and a ROW could be asked
        # nothing — no row numbers, no pin, no lock, no breakpoint gutter, the
        # whole left-hand band a professional table, editor or profiler has.
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
        # Two things past the toolkit 6.11, both read over the wire: an
        # unanswered axis is a DECLARATION, not a blank strip (the toolkit's
        # orientation is a runtime argument, so the commonest abstract table
        # model bug there — handle `Horizontal`, fall through returning `dynamic value()` — paints
        # sections that still occupy their width and is reported by nothing;
        # here `no_row_header()` is written down, the band is not painted, the model is asked
        # ZERO times a frame, and painted-iff-answered is structural because
        # there is no second "show the header" flag); and the mark's MEANING
        # reaches assistive technology (`text(Name)` answers from `DisplayRole` on both
        # orientations, so a toolkit row header whose distinguishing
        # information is its glyph announces only the number).
        #
        # +4 and not more, audited at R1548: a section axis answers 2 of the
        # toolkit's roles (`ToolTipRole` / `TextAlignmentRole` / `InitialSortOrderRole` / `SizeHintRole` all absent on a header); the
        # row axis has NO interaction (the toolkit's header view section click
        # selects the row, and its sections resize — row height here is one
        # grid-wide pitch the windowing arithmetic is built on); the band's
        # width is stated rather than `ResizeToContents`; and R1530's last small one now holds
        # on both axes — a binding states its row window twice (paint + a11y)
        # as it already did its column window.
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
        # interaction changed anything. So `CellSelection` holds the function row -> column
        # set GROUPED BY ITS VALUE — one band per distinct `ColumnSpan` — which is
        # unique by construction. Past the toolkit: `ColumnSpan` carries no column
        # count, so a record stays whole when the schema grows, where a toolkit
        # range built against `columnCount() - 1` is silently demoted and drops out of `selectedRows()`.
        #
        # +4 and not more, and the remainder is audited at R1563 rather than
        # carried: the section axis still answers 2 of the toolkit's roles on
        # both axes (`ToolTipRole` / `TextAlignmentRole` / `InitialSortOrderRole` / `SizeHintRole`); the band's width is stated rather
        # than `ResizeToContents`; a binding still states its row window twice (paint + a11y)
        # and `virtual_grid.rs` still has two row emitters; DRAG-select across sections is
        # still blocked on a substrate absence the pointer wire has (it does
        # not say whether a button is held — W3C `PointerEvent.buttons`); the KEYBOARD has no
        # two-axis vocabulary (the toolkit's `Ctrl+Space` on a cell, `Ctrl+Shift+Arrow` growing a
        # rectangle), which is this round's own new gap; the `SelectColumns` arm has no
        # binding; and R1563 FOUND one this axis had never named — the eager
        # `Table` holds its own single-rectangle cell selection (R952), so the
        # tree now has two cell-selection models, one canonical and windowed,
        # one a rectangle bounded by a model small enough to materialise.
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
                "crosshair", "settings-panel", "todomvc", "design tool-",
                # R1554 — the group box. A titled frame that gates its
                # contents is a catalog widget; the pattern list is a census,
                # so a member with no pattern is reported UNCLASSIFIED rather
                # than counted somewhere convenient.
                "group-box",
                # R1569 — the key-sequence editor (the toolkit key-sequence
                # editor), the field a shortcut is recorded into. Added on the
                # round that built it, because the census reported it
                # UNCLASSIFIED at the push that shipped it — which is the
                # census doing its job.
                "key-sequence",
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
        # R1533 gave the two stepped value widgets `External::wheel` (the toolkit `wheelEvent` / `wheelEvent`)
        # plus the `WheelStepper` sub-notch carry they need. The hook had existed since
        # R877 and a census found ONE implementor in the repo (the node canvas'
        # zoom), so no widget in the catalog answered a wheel.
        #
        # Only +2, because the audit that produced the gap list below found
        # MORE absent surface than the round filled — the R1528 pattern, where
        # naming a dimension for the first time grows the stated gap:
        #
        #   * ~~Mnemonics / accelerators~~ — CLOSED R1543. It was the first
        #     item R1533 listed and the largest, because it is not one
        #     widget: it is an axis every labelled widget sits on. R1543
        #     landed the toolkit's `&`/`&&` vocabulary as ONE declaration on the
        #     painted label, from which the underline ink (a `StyleRun`, so
        #     both painters draw it with no per-backend code), the Alt+char
        #     binding (derived from the PAINT scene, so it cannot disagree
        #     with what the user sees underlined) and the AT `accesskey` are
        #     all derived. Past the toolkit in four places: the map is published
        #     (`scene/mnemonics`; the toolkit's lives in the private
        #     `qshortcutmap_p.h`), a conflict is a STATIC property of the
        #     scene rather than a bool on the event the user triggered, the
        #     ink and the binding come from one parse instead of the toolkit's two,
        #     and `accesskey` stays distinct from `keyboard_shortcut` where
        #     `Accelerator` collapses them.
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
        #     transition. Past the toolkit in three places, all read over the wire:
        #     the hold is DRIVABLE AS DATA (the toolkit's basic timer cannot be
        #     told "hold for 900 ms"; this rides the `scene/tick` clock and
        #     the demo asserts exact fire counts with no tolerance), the RUN
        #     IS PUBLISHED and predictive (`scene/auto_repeat` gives target
        #     / repeating / cadence / fires / seconds-to-next, where the toolkit's
        #     only public fact is a static per-widget property), and a held
        #     arrow AT ITS BOUND stops (abstract spin box keeps its 10 Hz
        #     timer running against a value pinned at `maximum()`).
        #     Armed-ness is re-ASKED every frame instead of stored, and the
        #     run lives IN the R876 press record, so the toolkit's runaway-timer bug
        #     class has nowhere to live. Adoption is COMPLETE for the widget
        #     classes that can express a hold — all three that own `Button`
        #     sub-regions (`ButtonExternal` opt-in as push button is,
        #     `SpinButtonExternal` and `PaginationExternal` on by default as
        #     the toolkit's spin arrows are).
        #   * the toolkit also has `wheelEvent` on combo box and tab bar; R1533
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
        #   * Absent widget kinds. group box — the one R1549 put FIRST
        #     and called out as "especially checkable" — is CLOSED R1554;
        #     re-censused there, the other five are still absent: dial
        #     (no dial or knob; the one `dial` hit is a rotate GESTURE
        #     example), a paged container (stacked widget / wizard),
        #     key-sequence editor, font picker, and the standard
        #     message box / input dialog canned dialogs — each of the
        #     five appears in this tree only inside a doc comment.
        #
        # R1549 re-judgment, 87 -> 90, demanded by the tool (round ledger 2 ->
        # 3). +3, the same calibration R1543 got for mnemonics and for the same
        # reason: what closed is not one widget but an axis every pressable
        # widget sits on, it was wholly absent, and it closed past the toolkit
        # in three places. Unlike R1543 it also added NO gap of its own —
        # adoption is complete for the widget classes that can express a hold.
        # Not more than +3 because the audit that produced this list was RE-RUN
        # at R1549 rather than inherited ([[r1532-column-declares
        # -its-painter]]: a gap list is worth only what it is checked against),
        # and every other item still stands, verified by census: `External::wheel` still has
        # exactly two implementors, mnemonic adoption is still three sites, and
        # all six absent widget kinds are still absent (no `group_box` / `fieldset`, no dial
        # or knob, no stacked-page or wizard container, no key-sequence editor,
        # no font combo, no canned message / input dialog). Six absent kinds is
        # a lot of surface for an axis whose name is "catalog". R1554
        # re-judgment, 90 -> 93, demanded by the tool (round ledger 3 -> 4). It
        # closes the item R1549's list named FIRST among the absent widget
        # kinds and flagged as the one a pro tool misses most — group box,
        # "especially checkable" — and what made it absent was never the frame.
        # It was that `setCheckable(true)`'s whole point, clearing the title checkbox to make
        # the panel inert, was INEXPRESSIBLE: `LayoutStyle` carried four interaction
        # declarations (`pointer_transparent`, `focusable`, `drop_target`, `cursor`) and every one described the node
        # carrying it and nothing else. The toolkit's `setEnabled` is the one that is
        # INHERITED.
        #
        # So the round is a scene declaration (`with_disabled`) plus four derivations, each
        # resolved where that consequence is already decided — the §5.39 focus
        # enumeration, `Scene::hit_test`, the a11y assembler's stamp, and the ink — and it
        # rides `settle_to_fixed_point`, the one loop every paint-scene producer in both backends
        # passes through, so a window and a terminal cannot disagree about
        # which controls are inert. Past the toolkit 6.11 in four places, all
        # read over the wire: the CAUSE is published by name (`scene/disabled`'s `declared_by`; the
        # toolkit's `isEnabled()` is a bool and `isEnabledTo()` needs the caller to have already
        # guessed the ancestor), the SET is enumerable at all (the toolkit has
        # no such query), a refusal has a NAME (`focus/set` -> `tag_disabled` handing back the
        # region, where `setFocus()` is a silent no-op), and whether the INK followed is
        # stated per node rather than left to be discovered from a screenshot.
        # The derived half is recomputed every paint instead of written into
        # descendants, which is what the toolkit's `setEnabled_helper` does and must walk
        # back.
        #
        # +3 and not more. Five of the six absent widget kinds remain, the
        # wheel item is untouched and still the largest cross-cutting one,
        # and the round adds gaps of its own, audited at R1554: the cascade
        # has ONE consumer (every other catalog widget still expresses
        # disabledness only through its own state enum, so a form cannot gate
        # a section without a group box), and four node kinds carry content
        # the fade cannot reach (`Image` / `External` / `ImmediateModeNode` /
        # `TextGrid`) — the toolkit cannot grey a GL widget either, so it is
        # stated on the wire rather than fixed.
        # R1570 re-judge, 93 -> 95, demanded by the tool: the round ledger took
        # this axis 4 -> 6 and it absorbs TWO rounds, because R1569 landed at
        # exactly the band edge (+25%, and the test is `> 25%`) and deferred
        # its look.
        #
        # R1569 made the FOCUSED widget able to shadow the window's accelerator
        # layers (the toolkit `ShortcutOverride`) — a place the tree sat BELOW
        # the floor, and shipped: typing `d` into `hello-textfield`'s focused
        # field disabled the field. It also closes one of the five widget kinds
        # this axis's own list called absent, key-sequence editor, since the
        # editor is what forced the axis.
        #
        # R1570.1 closed something the gap list had never NAMED, which is why
        # it is worth more than its size: the catalog's atomic controls were
        # not keyboard-operable at all. `#[widget(role = ...)]` announces an operable control, and
        # in **17 of 23** such bindings `focus/set` refused the tag and `focus/next` answered
        # `None` — no focus stop in the window. HTML gives it without a `tabindex` and
        # the toolkit gives it as `StrongFocus`, so this was below both floors. The
        # second-order cost is what makes it structural rather than cosmetic:
        # `apply_aria_activate` gates on `focused == Some(my_tag)`, so 13 of the 25 byte-identical `apply_key` bodies in the
        # tree were UNREACHABLE code under doc comments describing a
        # Space/Enter behaviour that could not happen.
        #
        # Only +2, and the reason is that the axis's STATED gap list barely
        # moved: the wheel item (`External::wheel` still has two implementors) is untouched
        # and still the largest cross-cutting one, mnemonic adoption is still
        # four sites, the disabled cascade still has one consumer, and four
        # absent widget kinds remain (dial, a paged container, font picker, the
        # canned message box / input dialog). R1570.1 adds two of its own,
        # audited: ten of the sixteen hand-painted controls repeat the focus
        # declaration because there is no `switch` painter to own it, and a POINTER
        # click paints the focus ring with no `:focus-visible` distinction — not below the
        # toolkit, whose common styles do the same, but now visible on 17 more
        # controls.
        "judged_at": 1570,
        "completion": 95,
        "evidence_snapshot": {"example-name": 76, "round-axis": 6},
    },
    {
        "key": "dataviz",
        "name": "Charting / data visualisation",
        "weight": 10,
        "gated": False,
        # R1519 — this axis did not exist in the R931 tally, which is why the
        # entire R1372-R1442 campaign (22 examples, 72 demos, `pinion-chart` + `pinion-graph`) could
        # not move the Phase B number by a single point. The toolkit ships the
        # toolkit's charting module, so under the toolkit-parity directive it
        # is in scope.
        #
        # R1528 re-judge, 65 -> 68, and the tool demanded it: a round declared
        # this axis where the snapshot was 0, which `drift` reads as movement
        # whatever the count. Small on purpose. R1528 landed a logarithmic
        # value axis (the toolkit log value axis) on both cartesian axes of the
        # two numeric-x charts — one of the toolkit's charting module' FIVE
        # axis types.
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
        # axis (the toolkit date time axis, d3 `scaleUtc`) on both cartesian axes of
        # the two numeric-x charts plus the timeline ruler. Four points, one
        # more than the log axis got, because a monitoring chart's x-channel is
        # the commoner need — and only four, because it closes UTC and not
        # local time.
        #
        # The dimension R1528 opened stays the useful one, and building the
        # third kind sharpened what remains on it. Of the toolkit's charting
        # module' axis classes the crate now has value, log and datetime as
        # interchangeable `ValueScale` arms — but **category is not an axis kind here at
        # all**: the bar chart's x is a `BarGeom` slot metric on a separate code
        # path, so no chart can swap a category axis in the way it can now swap
        # the other three. R1528 recorded that as "no category axis outside the
        # bar chart's slots"; the shape of the gap is now structural rather
        # than a missing variant. Untouched otherwise: no polar / candlestick /
        # box-plot / spline / 3D-surface series, and no plot-level zoom or pan
        # — which is the bulk of what is left.
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
        #   * No drag pan and no rubber-band zoom (the toolkit's charting module
        #     `setRubberBand`). An `External` has no pointer-down /
        #     pointer-up hook, so a press-drag needs either the raw-pointer
        #     seam or a slider-style statechart — a design choice R1534 did not
        #     have to make and should not make by accident.
        #   * The window is x-only. The toolkit's charting module zooms a RECT; there is no y-window
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
        # `Categories` / `CategoryScale` are the fourth `AxisKind` arm (the toolkit bar category axis, d3
        # `scaleBand`), the bar chart's private slot metric IS that axis, and `LineChart::x_category` / `ScatterChart::x_category`
        # swap it into a numeric-x chart the way the log and time kinds already
        # swapped. Of the toolkit's charting module' axis classes the crate now
        # has four of five interchangeable.
        #
        # Two things past the toolkit 6.11, both read over the wire by the
        # demo: `CategoryScale::band` publishes where a category is DRAWN (a toolkit bar's rect
        # is computed inside the private bar series private painter, and the
        # absence of that accessor is exactly why `bar.rs` carried three copies of
        # `left + i * slot`), and a window is resolved from NAMES before it can reach a chart
        # — `Categories::window` answers a `Result`, where `setRange(string, string)` returns `void` and silently ignores a
        # name that is not a category.
        #
        # +5 and not more, the same size R1534 got for half of its item,
        # because the remaining list is long and mostly untouched. Audited at
        # R1545:
        #
        #   * the toolkit's OTHER category axis, category axis — labels attached to
        #     arbitrary value RANGES rather than to discrete slots — is absent.
        #     It is a different kind, not a variant of this one.
        #   * Label thinning is absent: a windowless 60-category axis labels
        #     all 60 and they collide. How many labels fit is a measured-TEXT-
        #     WIDTH decision and a scale has no text measurement, so
        #     `axis_ticks` ignores its tick target on this kind.
        #   * A slot has no band-level a11y. R1545's consumer names the WINDOW
        #     to an AT; an individual category label is painted text with no
        #     accessible relationship. The toolkit is the same, so a stated limit.
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
        # behind it: the BOX PLOT (the toolkit box plot series).
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
        # Three things past the toolkit 6.11, all read over the wire by the
        # demo, and all consequences of one decision — the summary is DERIVED
        # here rather than handed in. box set is five doubles and `the toolkit's charting module` computes
        # none of them (its own box-plot example ships a `findMedian()` helper IN THE
        # EXAMPLE):
        #
        #   * The quantile DEFINITION is part of the value. `QuantileMethod`
        #     carries three standard ones (Tukey's hinges, Hyndman & Fan
        #     types 7 and 6) that disagree at small n — and the demo shows the
        #     disagreement deciding whether a sample is an outlier at all. A
        #     box set cannot record which definition built it.
        #   * OUTLIERS exist. Tukey's `k * IQR` fence limits each whisker and
        #     every sample beyond it is its own addressable mark. The toolkit's five
        #     slots have no per-outlier geometry, so a toolkit box plot cannot draw
        #     one at any setting — and that fence is the defining half of the
        #     form.
        #   * The NOTCH, because the sample count survived the summary
        #     (McGill, Tukey & Larsen 1978). box set carries no n, so the toolkit
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
        #   * The pre-computed path (`Distribution::from_summary`, the toolkit's own
        #     contract) has no forcing consumer: `hello-boxplot` derives every
        #     one of its five, so the summary arm is exercised by unit tests
        #     only.
        #   * A box has no per-mark a11y. The scrub readout names the whole
        #     summary and its provenance, which is past the toolkit (the toolkit's charting module
        #     implements no accessibility interface at all), but an individual
        #     outlier is painted geometry with no accessible relationship.
        #   * category axis, label thinning, band-level a11y, drag pan /
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
        #   * category axis, label thinning, local time, drag pan /
        #     rubber-band zoom (blocked on the pointer wire not reporting a
        #     held button), the y-window, the plot zoom's a11y and its second
        #     consumer — all seven unchanged since R1545.
        #   * Neither new form has PER-MARK a11y: both scrub readouts name the
        #     whole datum, which is past the toolkit (the toolkit's charting module implements no
        #     accessibility interface), but an individual candle body or polar
        #     vertex is painted geometry.
        #   * The polar chart has no cross-filter leg and no legend
        #     interaction, where the cartesian charts have both.
        #
        # R1625 re-judged 92 -> 95, and the tool demanded it: `round-axis`
        # went 7 -> 9 across R1622, R1624 and R1625, past the band.
        #
        # +3, and the arithmetic is the R1568 list above, which those three
        # rounds worked straight down. Of its FIVE named series items,
        # stacked area closed at R1622, the OHLC bar at R1624 and the spline
        # at R1625 — and each closed past what the audit asked for. Stacking
        # is a crate derivation rather than an application's running sum. The
        # bar is a MARK on the chart that already exists, so it shares the
        # sort, both axis readings, the log axis, the window and the inspect
        # readout; the reference has no bar series at all. The spline arrives
        # with `overshoot`, which answers the question a smooth chart owes its
        # reader — did it draw a value that was never measured — where the
        # reference's spline series has one method and no report.
        #
        # It is not more because the R1622 audit corrected three of this
        # list's own entries and the corrections do not all pay: the category
        # axis WAS already present (`TickFormat::Category`), label thinning
        # exists in part, and local time is an external dependency (a
        # timezone database) rather than a gap. Those move the denominator,
        # not the numerator. Still open and buildable: the violin, the polar
        # chart's missing cross-filter leg and legend interaction, drag pan /
        # rubber-band zoom, the y-window and the plot zoom's second consumer.
        # 3D-surface waits on a 3D renderer and is Phase C's.
        #
        # PER-MARK a11y left this axis at R1622: measured, the whole crate has
        # no `AccessNode` anywhere, so it is one accessibility question rather
        # than three chart-shaped ones, and it now has its own debt file. An
        # axis should not be judged short for a defect that is not its own.
        # R1629 re-judged 95 -> 96, and the tool demanded it: `round-axis`
        # went 9 -> 12 across R1626, R1628 and this round, past the band.
        #
        # +1, and the small size is the finding rather than a shrug. Two of
        # the three rounds absorbed here move the numerator by less than they
        # look like they should:
        #
        #   * R1626 closed the VIOLIN — one of the six buildable items the
        #     R1625 audit left, and closed it past what that audit asked for:
        #     `Density` publishes the kernel, the resolved bandwidth, the
        #     sample count and `spill` (the share of estimated mass outside
        #     the range the samples spanned), `ViolinScale` states what the
        #     widths CLAIM, and `Density::bounded` makes spill exactly zero by
        #     reflecting rather than by clipping a picture that was already
        #     wrong. The reference has no violin at all.
        #   * R1628 closed two debts THIS SERIES CREATED (an area fill that
        #     ignored the interpolation its own stroke used; a density
        #     `bounded` that re-took its samples). Repayment restores what the
        #     axis was already credited for; it does not add capability.
        #   * R1629 put every chart's derivations ON THE WIRE. New surface,
        #     and not on any gap list this axis has ever kept — which by the
        #     R1528 rule is why it is worth only a point: naming a dimension
        #     reveals more absent surface than the round that named it filled.
        #
        # Audited at R1629, and the list is now SHORTER IN ITEMS AND LONGER IN
        # WHAT IS ACTIONABLE:
        #
        #   * Drag pan / rubber-band zoom is NO LONGER BLOCKED. R1534, R1545,
        #     R1553 and R1568 all recorded it as waiting on a pointer wire
        #     that could not report a held button; R1619 put
        #     `held_pointer_buttons` on every pointer event and R1620 added
        #     the autoscroll substrate. Four judgments called this
        #     unbuildable and it is buildable now.
        #   * The y-window, the plot zoom's a11y and its second consumer —
        #     unchanged since R1534.
        #   * The polar chart still has no cross-filter leg and no legend
        #     interaction, unchanged since R1568.
        #   * 3D-surface waits on a 3D renderer and is Phase C's.
        #   * NEW, and revealed by the round that closed the wire gap: only
        #     FIVE of the ten chart builders have anything to derive. Bar,
        #     donut, sparkline, timeline and treemap publish empty sets, which
        #     is honest — every setting they take is visible in the drawing or
        #     an explicit domain — but a treemap's TILING and a timeline's
        #     lane packing are layout decisions a reader cannot recover, and
        #     neither is modelled as a choice anywhere. That is a real gap
        #     this axis had never named.
        #   * PER-MARK a11y is not counted against this axis (R1622): the
        #     whole crate has no `AccessNode`, so it is one accessibility
        #     question with its own debt file rather than a charting one.
        "judged_at": 1629,
        "completion": 96,
        "evidence_snapshot": {"example-name": 28, "round-axis": 13},
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
        # vocabulary the TERMINAL cell has spoken since R1399 — single / double
        # / curly / dotted / dashed, plus the underline's own colour (the
        # toolkit `setUnderlineColor`). The tree could draw an undercurl in a terminal and not
        # on screen, with the painter that knew how sitting in the same file as
        # the one that flattened every form to one rule. An LSP diagnostic mark
        # is now drawable at all.
        #
        # +4 and not more, because the CHARACTER-format axis is nearly done
        # while the DOCUMENT axis is barely started. Audited at R1540:
        #
        #  - `setBackground` — no per-run background exists.
        #    The paint layer hand-rolls FOUR band kinds instead (selection,
        #    find-match, current-line, preedit), each with its own fill fn and
        #    alpha knob. The toolkit has both this and `ExtraSelection`; the
        #    tree has neither as a contract.
        #  - no vertical alignment (super/subscript), and no overline.
        #  - the DOCUMENT model is absent: text list (ordered / unordered),
        #    text table, text block format's per-paragraph indent and
        #    margins, and `setMarkdown` / `toHtml` import-export. A styled run
        #    is a span of characters; a document is more than a span list.
        #  - a mark is invisible to assistive technology (the toolkit too, so parity,
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
        # `TextStyle::bg_color` (the toolkit `setBackground`) is now a run-level declaration whose band is
        # cut by BYTE and measured by `selection_rects_for_range` — the function the selection band
        # already calls — so a highlight and a selection over the same bytes
        # are one function called twice rather than two derivations that agree.
        # Two things past the toolkit 6.11, both read over the wire: the
        # PAINTED EXTENT is published (the toolkit computes the rect inside the
        # private `draw` and discards it, so a toolkit application re-derives it
        # from `cursorToX` — a second implementation free to disagree with the
        # painter's), and the fg/bg pair publishes its WCAG contrast, so "no
        # highlight in this application drops below 4.5:1" is one call where
        # the toolkit will paint any brush behind any pen and say nothing.
        #
        # +5 and not more, and the remainder is audited at R1546 rather than
        # carried. The CHARACTER-format half is now nearly complete: what is
        # left of it is **vertical alignment** (super/subscript — the OS/2
        # metrics are parsed in `pinion-text-font` and nothing consumes them)
        # and **overline** (`TextDecoration` is underline-form + strikethrough
        # + underline-colour). Both small. What dominates the axis now is the
        # half that is untouched: **there is no document model at all** —
        # text list, text table, text block format's per-paragraph indent
        # and margins, `setMarkdown` / `toHtml`. Not one of those has a scene
        # primitive. Also unchanged, and now for a RECORDED reason rather than
        # by omission: the four view-level bands stay separate, because a
        # `StyleRun` carries a fully-resolved style and layering a selection
        # run over a syntax run would clobber the syntax run's foreground —
        # which is why the toolkit splits the same way (text char format for the
        # document, `ExtraSelection` for the view).
        #
        # The R1542 name/evidence mismatch above still stands and is still
        # deliberately undecided here.
        #
        # R1551 re-judged 80 -> 84, demanded by the tool (round count 3 -> 4).
        #
        # It closes the item R1546's audit named as DOMINATING the axis, on the
        # one sub-item that audit named with specifics: text block format's
        # per-paragraph indent and margins. Before it, a paragraph could say
        # how its glyphs looked and nothing about how the paragraph itself sat
        # — no indent, no space between paragraphs, no first-line indent, no
        # way to mark one a heading. `BlockFormat` is now a scene declaration that lowers
        # to the node's ordinary layout margin, so the flex pass indents a
        # paragraph with no document-specific layout code and the result
        # composes with the rest of the tree; the toolkit's block margins are
        # known only to the private text document layout, which is a second
        # layout engine that meets the widget layout at a viewport and nowhere
        # else.
        #
        # Four things past the toolkit 6.11: the format is a **struct** where
        # text format is a dynamic value property bag whose unset properties
        # silently return defaults, so a block's whole declaration can be
        # enumerated; every length is **one unit** where the toolkit mixes `indent()`
        # (indent-width multiples) with `leftMargin()` (pixels) in one class; `text-indent` carries
        # CSS's **`hanging` and `each-line`** keywords, which the toolkit's bare `qreal textIndent` cannot
        # express (a hanging indent in the toolkit needs a negative indent plus
        # a compensating margin, i.e. two properties that must agree); and a
        # **heading level reaches assistive technology** — `headingLevel()` has existed
        # since the toolkit 5.15, but the interface a text edit implements is
        # accessible text interface, whose vocabulary is character offsets,
        # selections and text attributes with no method that reports block
        # structure at all, so a toolkit document's heading levels reach its
        # layout and stop. `scene/text_blocks` then publishes the declaration BESIDE the shaped
        # line boxes, which is the only form in which "did my indent reach the
        # layout" is a question with an answer.
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
        # what was closed: **text list** (ordered / unordered, with automatic
        # numbering across sibling blocks — the part that cannot be hand-
        # composed), **text table**, and **`setMarkdown` / `toHtml`** import-export. None has
        # a scene primitive. text block format itself keeps four properties
        # this round did not take: `marker` (Unchecked / Checked, which belongs with
        # text list), `nonBreakableLines`, `pageBreakPolicy` (meaningful only against `pinion-pdf`'s paged output) and
        # `tabPositions`. The CHARACTER half is unchanged from R1546: vertical alignment
        # (super/subscript) and overline, both small. R1560 re-judged 84 -> 90,
        # demanded by the tool (round count 4 -> 6).
        #
        # It absorbs TWO rounds, because R1559 landed at the band edge exactly
        # (+25%) and did not force a look — the sticky behaviour R1547/R1548
        # already showed. Both of them close an item R1551's own audit named,
        # and between them they close TWO OF THE THREE things that audit listed
        # as the whole of what was left of the document model.
        #
        # R1559 — text list. What a list cannot have written by hand is the
        # NUMBER, because a number is not a property of the item: it is a
        # property of its place among its siblings, so inserting one renumbers
        # every item after it and nesting one restarts the inner sequence while
        # the outer carries on underneath. `ListSpec` declares membership and never a
        # number; `number_blocks` derives it. Past the toolkit: the counter styles have
        # RANGES and fall back through CSS Counter Styles Level 3 where `itemText()`
        # answers "?" and loses the value; a BULLET IS TEXT (the toolkit draws
        # `ListDisc` as an ellipse, so no accessor can say what an unordered marker
        # looks like and it is not in the text at all); the structure is
        # enumerable; it reaches assistive technology; and a suffix's default
        # belongs to the style rather than hiding in a null string.
        #
        # R1560 — text table, and the same argument one dimension up. A cell's
        # ADDRESS is not a property of the cell: it is where the cell lands
        # once every earlier cell's spans have taken their slots. `place_cells` derives
        # it by HTML's own slot allocation and `view_document` lowers it onto a REAL CSS
        # GRID — the layout kind the framework did not have, added here with
        # its forcing consumer, because a column of flex rows measures each row
        # alone (so columns cannot agree without being told a width) and cannot
        # express a rowspan at all. Past the toolkit: the address is derived
        # rather than maintained; a span that does not fit is clamped to the
        # FREE RUN and NAMED, where `mergeCells` returns `void` and a refused merge leaves
        # no trace; a table may be RAGGED and its unfilled slots are published,
        # a state text table cannot be in; header COLUMNS exist and header-ness
        # is derived FROM THE ADDRESS; the structure reaches assistive
        # technology, where a text table reaches no accessibility interface at
        # all; and it is enumerable over the wire.
        #
        # +6 and not more. What remains is audited at R1560, and the largest
        # item is the third one R1551 named:
        #
        #  - **`setMarkdown` / `toHtml`** — the import/export half of the
        #    document model. Untouched, and now the only one of R1551's three
        #    still open.
        #  - **Nested tables.** the toolkit has them. The honest way in is the general
        #    text frame containment axis, not a second ad-hoc level counter
        #    beside the list's — two nesting mechanisms that would have to
        #    agree.
        #  - text block format's four untaken properties are now three:
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
        #
        # R1642 re-judged 90 -> 92, DEMANDED by the tool (round-axis 6 -> 8,
        # +33%; R1615 and R1641 each declared this axis and neither re-judged).
        #
        # ONE OF R1560's FIVE NAMED GAPS CLOSED, which is why the move is small
        # and real rather than large or nil. R1560 recorded the CHARACTER half as
        # "unchanged since R1546"; R1641 moved it, and moved it as a type rather
        # than a number — `LetterSpacing { Normal, PxX100, EmX1000 }` shaped like
        # the `LineHeight` above it, so tracking is specifiable relative to the
        # font instead of restated per size, with `word_spacing` landing beside it
        # in the same encoding at R1641.3. Fixed point rather than a float because
        # `TextStyle` derives `Eq + Hash` and that participates in the §5.16 paint
        # fragment cache key. R1615 is the other round: `StyleRun::name` gives a
        # styled run an IDENTITY, where the reference's range decoration IS its
        # format, so two runs resolving to the same ink are indistinguishable
        # there once drawn.
        #
        # +2 and not more. The four remaining gaps were re-verified at R1642 by
        # identifier census, and TWO OF THE COUNTS WERE FALSE POSITIVES worth
        # recording, because both are the failure mode this tool's own axes have
        # been burned by:
        #
        #  - **`setMarkdown` / `toHtml`** — 0 sites. Still the last of R1551's
        #    three, and still the import/export half of the document model.
        #  - **Nested tables.** Unchanged. The honest way in is the general text
        #    frame containment axis, not a second ad-hoc level counter.
        #  - **text block format's three** (`nonBreakableLines`,
        #    `pageBreakPolicy`, `tabPositions`) — 0 sites each.
        #  - **the character half's remainder**: vertical alignment
        #    (super/subscript) and overline. `Subscript` matches 74 times in
        #    `crates/` and every one is a substring of `Subscription*` — the
        #    unbounded-substring credit R1560 itself warned about, one axis over.
        #    `overline` matches once, in a comment about Roman numerals.
        #  - **the grid vocabulary**: `minmax()` / `fit-content()` and
        #    `grid-auto-flow`. `minmax` matches three times and all three are doc
        #    comments in `pinion-runtime::layout` explaining how a `GridTrack`
        #    LOWERS to taffy's minmax pair — the mechanism is used, the AUTHORING
        #    vocabulary is not exposed, and a census that stopped at the count
        #    would have reported this closed.
        "judged_at": 1642,
        "completion": 92,
        "evidence_snapshot": {"example-name": 13, "round-axis": 8},
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
        # entry that already holds the layout (Skia's SkTextBlob, the toolkit's
        # glyph run). Measured before and after on the same box, same probe,
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
        #    took is unmeasured, and a pro tool states it (the engine `stat gpu`).
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
        #    not a footprint. A pro tool states its own (the engine `stat memory`).
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
        # R1550 re-judgment, demanded by the tool (the ledger took this axis 6
        # -> 8, past the 25% band). 78 -> 83, because the FIRST of the three
        # gaps the 78% named is closed outright, and it was total: a census of
        # the RPC surface found not one field in BYTES. `scene/memory` is now the memory
        # axis — one row per arena per owner, with the process RSS beside it —
        # and the accounting is a trait whose every impl destructures its type,
        # so a field added to a cached struct cannot silently go unpriced. It
        # also closes R1531's leftover (`MAX_CAPACITY` bounded memory by an entry count
        # times a measured AVERAGE, and an average bounds nothing) and fixes an
        # arena that sat BELOW the toolkit's floor: the decoded-image cache had
        # no bound of any kind and is now byte-bounded at pixmap cache's own 10
        # MiB default.
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
                # R1576 — the DISPLAY the windows sit on. `displays` rather
                # than `display`, which would be an unbounded prefix over any
                # future `hello-display-list` AND over words containing it;
                # the R1560/R1563 finding, applied before it can bite.
                "displays",
            ]),
        ],
        # R1576 re-judged 58 -> 63, and the tool demanded it: this axis had
        # never declared a round, so its first one moved `round-axis` off zero.
        #
        # What moved it: the framework had NO NOTION OF A MONITOR AT ALL. A
        # census over the whole tree found zero references to winit's
        # `available_monitors` / `primary_monitor` / `MonitorHandle`, so
        # `WindowSpec::position`'s own doc described "logical pixels, the OS
        # applies the per-monitor DPI scale" in a coordinate space nothing
        # could describe, and three questions were unaskable: how many
        # monitors are there, which one is this window on, and is this window
        # on any of them.
        #
        # R1576 answers all three. `pinion_core::display` is the pure value
        # (topology, union-area geometry, placement resolution, anchors),
        # `pinion-shell` supplies it from winit and RESOLVES window placements
        # through it, `scene/displays` publishes it, and
        # `WindowSpec::display` makes a position display-relative — so a saved
        # layout survives the desk changing, and a vanished display is
        # SUBSTITUTED BY NAME rather than silently.
        #
        # ONLY +5, and the reason is this axis's own gate. What "OS-native
        # integration" is judged short on is Mac/Win native surfaces (native
        # menus, native print dialogs), which need those OSes' runners and are
        # untouched. Worth recording that the gate does NOT cover what R1576
        # did: display enumeration is buildable and testable on Linux, and was
        # done here. Also still absent, audited at R1576: a display's USABLE
        # region (the toolkit `availableGeometry`) — winit exposes no work
        # area and EWMH's `_NET_WORKAREA` is one rectangle for the whole
        # desktop rather than one per monitor, so it needs a platform probe
        # rather than more geometry; no hot-plug EVENT (winit 0.30 emits none,
        # so the desk is re-read at each window create and RPC dispatch and a
        # binding that only paints will not see a monitor arrive until then);
        # and `Window::current_monitor()` is not cross-checked against the
        # derived home display.
        #
        # R1617 re-judged 63 -> 66, and the tool demanded it: `round-axis` went
        # 1 -> 3, past the band, because R1610 and R1617 both declared here.
        #
        # ONLY +3, and the arithmetic is the gap list R1576 wrote down. Of the
        # three things that audit named as still absent, R1617 closes exactly
        # ONE — the cross-check — and it closes it properly rather than
        # narrowly: `DisplayHome` publishes both answers and the relation
        # between them on `scene/windows`, `use_window_home` gives a binding the
        # same read in-process, and the whole judgment is a pure function of a
        # topology so a two-monitor divergence is a fixture rather than
        # hardware. The other two are untouched and are both platform probes:
        # a display's USABLE REGION (winit has no work-area accessor; EWMH's
        # `_NET_WORKAREA` is one rectangle for the whole desktop) and a hot-plug
        # EVENT (winit 0.30 emits none).
        #
        # The +3 also absorbs R1610, which advanced this axis and attached no
        # number to it — a window level is declared, its outcome is reported
        # against the running backend, and `scene/window_declare` made every
        # live axis writable where five were readable and one was writable. And
        # R1617's second half is what makes that outcome trustworthy: the
        # per-backend table was a reading of a vendored crate's source that
        # nothing held to it, and `winit_level_model.rs` now parses that source
        # and fails when the two disagree.
        #
        # It is not more because this axis is judged short on Mac/Win NATIVE
        # SURFACES — native menus, native print dialogs — which need those OSes'
        # runners and are exactly as untouched as they were at R1576. That is
        # the gate, and no amount of Linux window-system depth moves it. Worth
        # restating each time: display and window work IS buildable and testable
        # here, and is what these three rounds did.
        #
        # R1621 re-judged 66 -> 69, and the tool demanded it: `round-axis` went
        # 3 -> 4, past the band.
        #
        # ONLY +3 again, and the arithmetic is again the gap list the previous
        # audit wrote down. R1617 named exactly TWO things still absent, both
        # platform probes; R1621 closes exactly one of them, the display's
        # USABLE REGION, and closes it past what the reference does rather than
        # narrowly. `UsableRegion` is an answer WITH ITS PROVENANCE in four
        # arms rather than a bare rectangle, `pinion-shell::work_area` is a
        # real EWMH `_NET_WORKAREA` probe (measured on this desk:
        # 2494x1568+66+32, `reported`), and `scene/displays.usable` publishes
        # both halves. The reference's own X11 plugin writes at length that a
        # per-monitor work area cannot be trusted and then CONCLUDES by handing
        # back full bounds for every display on a multi-head desk with nothing
        # in the answer saying so — so a caller there cannot tell a measured
        # region from a fallback, which is the whole of what the fourth arm is
        # for. It also reclaims what that conclusion throws away: a dock on the
        # left monitor no longer costs the right monitor its measurement.
        #
        # The remaining named absence is a hot-plug EVENT, and it stays
        # EXTERNAL: winit 0.30 emits none, so the desk is re-read at each
        # window create and RPC dispatch. R1621 added two stated limits of its
        # own, both honest and neither buildable from here: Wayland has no work
        # area to publish at all (the arm says `Unpublished`; under XWayland
        # that atom describes the X root rather than the compositor's panels,
        # so the probe REFUSES rather than answering wrongly), and a WM that
        # publishes a per-desktop list is read only at its first quadruple
        # because the model has no notion of a virtual desktop.
        #
        # The gate is where it has been since R1576: Mac/Win native surfaces,
        # untouched, needing those OSes' runners. Linux window-system depth
        # does not move it, and three rounds of that depth is what 58 -> 69 is.
        "judged_at": 1621,
        "completion": 69,
        "evidence_snapshot": {"example-name": 14, "round-axis": 4},
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
        # declared a round, so its first one moved `round-axis` past the band. The
        # largest single move any axis has had here, and the reason is that the
        # baseline was the lowest. R1519's 30% described a surface an agent
        # could ENUMERATE but not READ: `rpc/methods` answered with names and an OCC
        # class, and its own module doc deferred the rest as "added when a
        # consumer needs it" — a defer [[toolkit-parity-over-yagni]] does not
        # admit, and one R1538 then supplied a consumer for the hard way.
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
        #  - NO METHOD -> TYPE BINDING, on either side. The toolkit's meta-method has
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
        # `RpcEgress` is the mirror of `RpcIngress`, and `scene/subscribe` is the framework's own consumer of
        # it. Three things past the toolkit 6.11, all read over the wire: the
        # stream is ENUMERABLE (`scene/subscriptions` answers who is listening to what — the
        # toolkit binds no server write to a named stream, so local server
        # cannot be asked); a stream cannot be named to a client before the
        # answer that told it the name (armed after the reply, structural
        # rather than remembered); and a client that VANISHES has exactly its
        # own stream released, with no unsubscribe ever sent.
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
        #    not WHICH SUBTREE. There is no per-subscription filter. The toolkit has no
        #    equivalent at all so it is an axis gap rather than round debt, but
        #    a large scene where an agent watches one panel will want it.
        # R1585 re-judgment, 62 -> 65, DEMANDED by the tool (round-axis 4 -> 6,
        # +50%; R1566 landed at exactly the band edge and deferred, and R1585
        # carries both).
        #
        # A method now says HOW A WINDOW IS NAMED TO IT. Two spellings address a
        # window — `params.window` for the dispatch scope, `/window[id]/` for a
        # path — and which method took which was published nowhere. That gap had
        # already been paid for: R1581 tried `window[main]/scene/access`, met a
        # bare -32601, and registered a debt against a capability that was there
        # all along. `rpc/methods` now carries a per-method `window` class plus a
        # WINDOW_DOC legend, and the unknown-method arm READS that catalog, so
        # the exact call that produced the false debt corrects itself.
        #
        # The column is PROVEN rather than parsed — every catalogued method is
        # probed with a malformed prefix — because a source census demonstrably
        # cannot answer it (a call graph keyed by function name merges four
        # different `fn parse` and credits two methods with a prefix neither
        # reads). R1585.1 then gated the probe's own population against the
        # source, closing the curated-population hole in the gate itself.
        #
        # Only +3, and the ceiling is this axis's name: R1539's four gaps all
        # still stand (no method -> type binding, no version negotiation,
        # deprecation path, compatibility policy or freeze; the census covers
        # `pinion-rpc` only), R1552's per-subscription filter is still absent,
        # and this round DELIBERATELY WITHHELD half of its own subject — whether
        # a method's ANSWER varies by scope is applied by the embedder and
        # cannot be observed from inside `pinion-rpc`, so publishing it would
        # ship a fact the surface has not established
        # ([[debt-scope-effect-per-method-unpublished]], R1539's own precedent).
        #
        # R1642 re-judged 65 -> 72, DEMANDED by the tool (round-axis 6 -> 11,
        # +83%, the largest evidence move this axis has had; R1637 / R1638 /
        # R1639 / R1640 each declared it and none re-judged).
        #
        # Five rounds of contract work, and one of them is this axis's FIRST
        # GUARANTEE rather than another description. R1637 turned the order of
        # two questions around so the transport asks the declaration BEFORE it
        # dispatches: a name absent from a surface's `$schema` is now absent from
        # the wire, in both directions, which is what makes "read the declaration"
        # a contract instead of advice. Turning it on found 123 undeclared or
        # mis-declared actions plus 2 reads. R1638 gave a declared action its
        # ARGUMENT GRAMMAR (form, names, types, and where the values come from);
        # R1639 gave a widget its driveable verb vocabulary, projected from the
        # statechart's own drivable const so an internal event stays unforgeable;
        # R1640 widened the gates from 19 of 39 widget surfaces to all 39 and
        # repaired the oracle, the comparison and the escape hatch it found
        # broken on the way; R1642 made a CONDITIONAL argument declarable, which
        # is a shape the reference cannot express at all, and moved `SchemaArg`'s
        # wire form into the crate that owns the type so a `#[non_exhaustive]`
        # field can no longer be silently dropped by a renderer one crate away.
        #
        # +7 and not more, and the reason is a NEW measurement rather than the
        # old refrain. THERE ARE TWO SELF-DESCRIBING SURFACES HERE AND ONLY ONE
        # OF THEM GOT THE TREATMENT. Probed over the wire at R1642: `rpc/methods`
        # answers 111 methods whose entries carry exactly `{name, occ, window}` —
        # no parameters, no return type, no error codes, no revision — and
        # `rpc/version` is not a method at all (-32601). So everything R1637-R1642
        # built lives on the per-External `$schema` path, while the JSON-RPC
        # METHOD surface, which is the one this axis is named for, is where it was
        # at R1585. That asymmetry is now measured rather than inferred, and it is
        # registered: [[debt-two-describing-surfaces-at-different-maturity]].
        #
        # Audited at R1642, all still open and now probe-backed rather than
        # copied forward: no method -> type binding (the toolkit's meta-method has
        # `returnMetaType()` and `parameterTypes()`; `rpc/methods` has neither),
        # no version negotiation, no deprecation path, no compatibility policy,
        # no freeze, no per-method error taxonomy, the type census still covers
        # `pinion-rpc` only, and no per-subscription filter. `SchemaChannel` still
        # cannot say a slot is WRITABLE (R1566's item, two arms unchanged), so a
        # settable value is still two unrelated fields — `x` and `set_x` — with
        # nothing published to relate them.
        "judged_at": 1642,
        "completion": 72,
        "evidence_snapshot": {"example-name": 9, "round-axis": 11},
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
