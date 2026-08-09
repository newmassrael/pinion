//! `scene/grid_editors` — which cells of a grid have an editor open, and which
//! of them holds the keyboard (R1571 §5.12 §5.27 §2 #7).
//!
//! # The gap this closes
//!
//! R1571 gave the grid the toolkit's `openPersistentEditor`, so N editors can be open at once.
//! Two of the facts that follow are unreachable from anything else the wire
//! already carries:
//!
//! - An editor on a row **outside the painted window** is in no snapshot,
//!   because it paints nothing. It is still open and still holds the user's
//!   in-flight value.
//! - Which of the open editors has the **keyboard** is a property of the set,
//!   not of any one painted node.
//!
//! R1555's `scene/cell_editors` module doc declined to publish "which cell is
//! being edited right now" on the grounds that the painted editor is already in
//! `scene/snapshot` and the latch is already in the binding's introspect slots
//! — one fact with three spellings. That reasoning was right for one transient
//! editor and does not survive N: neither of the two facts above is derivable
//! from the paint, so this is a *first* spelling rather than a third.
//!
//! # Against the toolkit 6.11
//!
//! `persistent` is a private `set<widget *>` and `indexEditorHash` is private beside it. The only public question
//! is `isPersistentEditorOpen(index)` — **one index at a time**, answering a bool. So a toolkit view
//! cannot be asked what it has open: you must already know the answer in order
//! to ask the question, which makes "save every open editor" and "is anything
//! unsaved" both unanswerable through the public API.
//!
//! Four things here are past that floor:
//!
//! - **The set is enumerable**, in one call, in canonical cell order.
//! - **Focus is data.** In the toolkit, "which editor has the keyboard" means
//!   `focusWidget()` reverse-mapped through that private hash.
//! - **Each editor's in-flight value is readable without focusing it**, and
//!   `dirty` says whether it differs from what the editor opened with —
//!   abstract item view keeps no record of what `setEditorData` seeded, so
//!   there is nothing there to compare against.
//! - **A malformed buffer is named** rather than shown as an absent value:
//!   `value` is `null` exactly when `malformed` is `true`, which R1555's
//!   `CommitOutcome::Malformed` is the commit-time half of.
//!
//! # Wire shape
//!
//! Request — `tag` is the [`use_grid_edit`](pinion_core::widgets::grid_edit::use_grid_edit)
//! cache key, required for the reason `scene/scroll_state`'s is: every grid owns
//! its own state under a distinct key, so there is no canonical default.
//!
//! ```json
//! { "jsonrpc": "2.0", "method": "scene/grid_editors",
//!   "params": { "tag": "cells_edit" }, "id": 1 }
//! ```
//!
//! ```json
//! {
//!   "jsonrpc": "2.0",
//!   "id": 1,
//!   "result": {
//!     "tag": "cells_edit",
//!     "count": 2,
//!     "field_tag": "cells_editor",
//!     "focused": { "row": 4, "col": 2 },
//!     "editors": [
//!       { "row": 1, "col": 1, "persistence": "persistent", "kind": "text",
//!         "form": "field", "focused": false, "dirty": true,
//!         "malformed": false, "value": "alpha", "seed": "Name 1" },
//!       { "row": 4, "col": 2, "persistence": "transient", "kind": "int",
//!         "form": "stepper", "focused": true, "dirty": false,
//!         "malformed": false, "value": "68", "seed": "68" }
//!     ]
//!   }
//! }
//! ```
//!
//! # What it does not answer, and why
//!
//! It is **read-only**, and the missing verb is `openPersistentEditor`. That is not an omission:
//! opening needs a [`CellEdit`](pinion_core::CellEdit), which only the *model*
//! produces and which it produces `None` for on a cell it will not edit (R1544) —
//! so a framework method that opened an editor would have to invent an edit
//! role the model never answered with, which is the exact state the type
//! system here refuses to represent. The toolkit has the same division and
//! pays for it the other way: `openPersistentEditor` is a view method that reaches the delegate
//! *without* consulting `flags() & ItemIsEditable`, so it opens a live editor on a read-only cell and
//! drops every write the user makes into it in silence.
//!
//! Closing and focusing need no model, and are reachable through the binding's
//! own gestures; a framework verb for them is the named next slice.

use pinion_core::reactive::Owner;
use pinion_core::widgets::grid_edit::{EditState, GridEditState, OpenEditor};
use serde::Serialize;

use crate::substrate_introspect::{SubstrateIntrospectError, lookup};

/// The failure of [`grid_editors`], shared with every other substrate read.
pub type GridEditorsError = SubstrateIntrospectError;

/// A cell address on the wire — the two fields of
/// [`CellIndex`](pinion_core::CellIndex), which is also how every grid tag
/// spells one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct GridEditorCell {
    /// The **data** row index, already resolved through any sort permutation.
    pub row: usize,
    /// The **absolute** column index, whatever the column window.
    pub col: usize,
}

/// One open editor.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GridEditorEntry {
    /// The data row its cell is on.
    pub row: usize,
    /// The absolute column its cell is in.
    pub col: usize,
    /// `"transient"` or `"persistent"` ([`EditorPersistence`](pinion_core::widgets::grid_edit::EditorPersistence)):
    /// whether a commit and an <kbd>Escape</kbd> close it. The toolkit's
    /// private persistence set, published.
    pub persistence: &'static str,
    /// The datum kind the cell holds (`CellKind::name`).
    pub kind: &'static str,
    /// The editor form it opened (`EditorForm::name`) — what
    /// `createEditor` would have constructed.
    pub form: &'static str,
    /// Whether this editor holds the keyboard, and so the shared inline field
    /// named by [`GridEditorsOutcome::field_tag`]. Exactly one entry can be
    /// `true`, and it is the one [`GridEditorsOutcome::focused`] names.
    pub focused: bool,
    /// Whether the in-flight value differs from what the editor opened with.
    /// A malformed buffer counts as dirty — the user typed something that is
    /// not the seed, which is what a "discard your changes?" prompt is for.
    pub dirty: bool,
    /// Whether the buffer does not currently hold a value of the cell's kind —
    /// a half-typed number, a malformed `#RRGGBB`. The model is never asked to
    /// store one of these, and [`Self::value`] is `null` exactly here.
    pub malformed: bool,
    /// The in-flight value's display form, or `null` when [`Self::malformed`].
    pub value: Option<String>,
    /// What the editor opened with, so a client can compute the difference
    /// [`Self::dirty`] reports rather than trusting it.
    pub seed: String,
}

/// The answer: the whole editor set of one grid.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GridEditorsOutcome {
    /// The `use_grid_edit` cache key the request named, echoed back.
    pub tag: String,
    /// The shared inline field's `use_text_edit_state` key — the buffer the
    /// focused text-buffered editor is typing into, so a client can drive it
    /// through the same `scene/*` text methods a standalone field takes.
    pub field_tag: &'static str,
    /// How many editors are open. abstract item view has no accessor for
    /// this at all.
    pub count: usize,
    /// The cell whose editor holds the keyboard, or `null`.
    pub focused: Option<GridEditorCell>,
    /// Every open editor, in cell order (row-major) — a canonical order, so
    /// two equal sets always serialize identically.
    pub editors: Vec<GridEditorEntry>,
}

/// Snapshot the [`GridEditState`] cached at `tag` on `runtime_owner`.
///
/// # Errors
///
/// - [`GridEditorsError::RuntimeOwnerUnavailable`] — no substrate owner on the
///   dispatch context.
/// - [`GridEditorsError::NotBound`] — the owner has no grid edit state under
///   `tag`.
///
/// # Side effects
///
/// None: [`Owner::cache_get_by_str`](pinion_core::reactive::Owner::cache_get_by_str)
/// never creates a slot on miss, and every accessor read here is a signal
/// *read* outside any reactive scope.
pub fn grid_editors(
    runtime_owner: Option<&Owner>,
    tag: &str,
) -> Result<GridEditorsOutcome, GridEditorsError> {
    lookup::<GridEditState, _, _>(runtime_owner, tag, GridEditorsOutcome::from_state)
}

impl GridEditorsOutcome {
    fn from_state(tag: &str, state: &GridEditState) -> Self {
        let editors = state.editors();
        Self {
            tag: tag.to_owned(),
            field_tag: state.field_tag(),
            count: editors.len(),
            focused: editors.focused_index().map(|at| GridEditorCell {
                row: at.row,
                col: at.col,
            }),
            editors: editors
                .iter()
                .map(|editor| entry(state, editor, editors.focused_index()))
                .collect(),
        }
    }
}

/// One entry, derived from the editor and the state that holds its buffer.
///
/// `value` and `malformed` come from the same
/// [`EditState`] rather than from
/// two reads, so the wire cannot report a value beside `"malformed": true`.
fn entry(
    state: &GridEditState,
    editor: &OpenEditor,
    focused: Option<pinion_core::CellIndex>,
) -> GridEditorEntry {
    let (malformed, value) = match state.state_at(editor.index) {
        EditState::Value(value) => (false, Some(value.display())),
        // `Closed` is unreachable for a member of the set this iterates, and
        // reads the same way on the wire as a malformed buffer would: no value.
        EditState::Malformed | EditState::Closed => (true, None),
    };
    GridEditorEntry {
        row: editor.index.row,
        col: editor.index.col,
        persistence: editor.persistence().wire_token(),
        kind: editor.kind().name(),
        form: editor.form().name(),
        focused: focused == Some(editor.index),
        dirty: state.is_dirty_at(editor.index),
        malformed,
        value,
        seed: editor.seed().display(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::widgets::grid_edit::use_grid_edit;
    use pinion_core::{CellEdit, CellIndex, CellValue};

    #[test]
    fn r1571_an_unbound_tag_is_named_rather_than_answered_with_an_empty_set() {
        let owner = Owner::new();
        assert_eq!(
            grid_editors(None, "cells_edit").unwrap_err(),
            GridEditorsError::RuntimeOwnerUnavailable
        );
        assert_eq!(
            grid_editors(Some(&owner), "cells_edit").unwrap_err(),
            GridEditorsError::NotBound {
                tag: "cells_edit".to_string()
            },
            "an empty set and a grid that does not exist are different answers"
        );
    }

    #[test]
    fn r1571_the_wire_enumerates_the_whole_set_with_one_focus() {
        let owner = Owner::new();
        owner.run(|| {
            let state = use_grid_edit("g.edit", "g.field");
            state.open_persistent(CellIndex::new(4, 2), &CellEdit::from(CellValue::Int(68)));
            state.open_persistent(
                CellIndex::new(1, 1),
                &CellEdit::from(CellValue::Text("Name 1".into())),
            );
            pinion_core::widgets::text_edit::use_text_edit_state("g.field")
                .set_text("alpha".to_string());
        });

        let out = grid_editors(Some(&owner), "g.edit").expect("bound");
        assert_eq!(out.tag, "g.edit");
        assert_eq!(out.field_tag, "g.field");
        assert_eq!(out.count, 2);
        assert_eq!(out.focused, Some(GridEditorCell { row: 1, col: 1 }));
        assert_eq!(
            out.editors
                .iter()
                .map(|e| (e.row, e.col))
                .collect::<Vec<_>>(),
            vec![(1, 1), (4, 2)],
            "canonical cell order, so two equal sets serialize identically"
        );
        assert_eq!(out.editors.iter().filter(|e| e.focused).count(), 1);

        let unfocused = &out.editors[1];
        assert_eq!(unfocused.persistence, "persistent");
        assert_eq!(unfocused.kind, "int");
        assert_eq!(unfocused.form, "stepper");
        assert!(!unfocused.dirty, "parked at its seed");
        assert_eq!(unfocused.value.as_deref(), Some("68"));
        assert_eq!(unfocused.seed, "68");

        let focused = &out.editors[0];
        assert!(focused.focused && focused.dirty);
        assert_eq!(
            focused.value.as_deref(),
            Some("alpha"),
            "the live buffer, read without focusing anything"
        );
        assert_eq!(focused.seed, "Name 1");
    }

    #[test]
    fn r1571_a_malformed_buffer_carries_no_value() {
        let owner = Owner::new();
        owner.run(|| {
            let state = use_grid_edit("m.edit", "m.field");
            state.open_persistent(CellIndex::new(0, 0), &CellEdit::from(CellValue::Int(7)));
            pinion_core::widgets::text_edit::use_text_edit_state("m.field")
                .set_text("12a".to_string());
        });
        let out = grid_editors(Some(&owner), "m.edit").expect("bound");
        let entry = &out.editors[0];
        assert!(entry.malformed);
        assert_eq!(
            entry.value, None,
            "`value` is null exactly when `malformed` — one EditState read, so \
             the wire cannot report both"
        );
        assert!(entry.dirty, "a malformed buffer is unsaved work");
    }
}
