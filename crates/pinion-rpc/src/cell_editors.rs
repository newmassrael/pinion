//! `scene/cell_editors` — which editor a grid cell of each datum kind opens
//! (R1555 §5.12 §5.27 §2 #2 §2 #7).
//!
//! # Against the toolkit 6.11
//!
//! The toolkit's equivalent is item editor factory, and three properties of it
//! make this question unanswerable there:
//!
//! - **`createEditor` constructs.** Its only accessor
//!   (`createEditor(Type, widget *parent)`) *instantiates a
//!   widget*, so "what editor would an `int` cell get" cannot be asked
//!   without building one and destroying it again. Here the mapping is
//!   [`CellKind::editor_form`], a pure function, so a driver reads it.
//! - **The registry cannot be enumerated.** item editor factory exposes
//!   `registerEditor` and `createEditor`; `creatorMap` is private and there is
//!   no "which types do you handle". A caller can only probe types it already
//!   thought of, and a `nullptr` answer does not distinguish "not registered"
//!   from "deliberately not editable".
//! - **The behaviour that follows is not stated anywhere.** Whether an editor's
//!   in-flight value is its text or its current index decides how a commit reads
//!   it back, and in the toolkit that lives in each delegate's `qobject_cast`. Every row
//!   here carries it (`buffer_is_text`), beside the keystroke gate that admits
//!   text into it in the first place.
//!
//! # Wire shape
//!
//! ```json
//! {
//!   "jsonrpc": "2.0",
//!   "id": 1,
//!   "result": {
//!     "editors": [
//!       { "kind": "bool",  "form": "toggle",
//!         "buffer_is_text": false, "inline_text": false,
//!         "accepts_keystrokes": false, "role": "checkbox" },
//!       { "kind": "int",   "form": "stepper",
//!         "buffer_is_text": true,  "inline_text": true,
//!         "accepts_keystrokes": true,  "role": "spinbutton" }
//!     ]
//!   }
//! }
//! ```
//!
//! Request — no parameters. The answer is a property of the framework rather
//! than of a window, which is why it takes none: the factory is the same for
//! every grid in the process, and a per-window answer would imply otherwise.
//!
//! ```json
//! { "jsonrpc": "2.0", "method": "scene/cell_editors", "id": 1 }
//! ```
//!
//! Rows are in [`CellKind::ALL`] order, one per kind, so the response is a
//! **census**: an agent that finds a kind missing has found a factory gap, not
//! an under-specified response.
//!
//! # What it does not answer
//!
//! Which cell is being edited *right now*. That is a property of a binding's
//! [`GridEditState`](pinion_core::widgets::grid_edit::GridEditState), not of the
//! factory.
//!
//! **R1571 correction.** This paragraph used to end "and it already reaches the
//! wire two ways — the painted editor is in `scene/snapshot` and the latch is in
//! the binding's `ExternalIntrospect` slots; publishing it a third time from
//! here would be one fact with three spellings that can disagree". That was true
//! of *one transient* editor and does not survive N of them. With
//! `openPersistentEditor`, an editor on a row outside the painted window is in
//! no snapshot at all — it paints nothing — and *which* open editor holds the
//! keyboard is a property of the set rather than of any painted node. Neither is
//! derivable from the paint, so [`crate::grid_editors`] is a **first** spelling,
//! not a third. It is still not published from *here*: this method answers about
//! the factory, which is a property of the framework, and that one answers about
//! one grid's state, which needs a tag.

use pinion_a11y::editor_role;
use pinion_core::CellKind;
use serde::Serialize;
use serde_json::Value;

use crate::RpcError;

/// What one [`CellKind`] opens, and the behaviour that follows from it.
#[derive(Debug, Clone, Serialize)]
pub struct CellEditorEntry {
    /// The datum kind (`CellKind::name`) — the toolkit's `Type` key.
    pub kind: &'static str,
    /// The editor form it opens (`EditorForm::name`) — what
    /// `createEditor` would have constructed.
    pub form: &'static str,
    /// Whether the in-flight value is read back out of a text buffer rather
    /// than held as a typed value. **The column the toolkit keeps inside each
    /// delegate's `qobject_cast`.**
    pub buffer_is_text: bool,
    /// Whether the cell's own box is a text field the keystroke gate feeds — so
    /// an agent knows whether typing at the cell is the way in.
    pub inline_text: bool,
    /// Whether a printable keystroke is admitted into this kind's editor at all
    /// (`CellKind::accepts_keystroke`). `false` for the two kinds that are
    /// gestured rather than typed.
    pub accepts_keystrokes: bool,
    /// The WAI-ARIA role the open editor is announced with. The toolkit
    /// reaches its role by accident of construction, so a toolkit bool cell
    /// announces as a **combo box** and a toolkit colour cell announces
    /// nothing at all.
    pub role: &'static str,
}

/// Response payload for `scene/cell_editors`.
#[derive(Debug, Clone, Serialize)]
pub struct CellEditorsOutcome {
    /// One row per [`CellKind`], in census order.
    pub editors: Vec<CellEditorEntry>,
}

/// Build the `scene/cell_editors` response.
///
/// # Errors
///
/// Only if the outcome fails to serialize, which for a `Vec` of static strings
/// and bools is unreachable in practice; it is surfaced rather than unwrapped so
/// an RPC handler never panics the shell.
pub fn handle_scene_cell_editors() -> Result<Value, RpcError> {
    let editors = CellKind::ALL
        .into_iter()
        .map(|kind| {
            let form = kind.editor_form();
            CellEditorEntry {
                kind: kind.name(),
                form: form.name(),
                buffer_is_text: form.buffer_is_text(),
                inline_text: form.inline_text(),
                // A single printable character is the probe the gate is built
                // for; the kinds that admit none are the gestured ones.
                accepts_keystrokes: kind.accepts_keystroke("1") || kind.accepts_keystroke("a"),
                role: editor_role(form).aria_name(),
            }
        })
        .collect();
    serde_json::to_value(CellEditorsOutcome { editors }).map_err(RpcError::internal_error)
}

#[cfg(test)]
mod tests {
    use super::handle_scene_cell_editors;
    use pinion_core::CellKind;

    #[test]
    fn every_kind_has_a_row_and_the_answer_is_a_census() {
        let value = handle_scene_cell_editors().expect("ok");
        let rows = value["editors"].as_array().expect("array");
        assert_eq!(
            rows.len(),
            CellKind::ALL.len(),
            "one row per kind — a missing row is a factory gap, not an \
             under-specified response"
        );
        for (row, kind) in rows.iter().zip(CellKind::ALL) {
            assert_eq!(row["kind"], kind.name(), "rows are in census order");
        }
    }

    #[test]
    fn the_two_gestured_kinds_say_they_take_no_typing() {
        let value = handle_scene_cell_editors().expect("ok");
        let rows = value["editors"].as_array().expect("array");
        let row = |kind: &str| {
            rows.iter()
                .find(|r| r["kind"] == kind)
                .expect("kind present")
                .clone()
        };
        for kind in ["bool", "choice"] {
            let r = row(kind);
            assert_eq!(r["accepts_keystrokes"], false, "{kind}");
            assert_eq!(
                r["buffer_is_text"], false,
                "{kind} — so a commit does not look for its value in a text \
                 buffer that would always be empty"
            );
        }
        // The forms, and the roles the toolkit gets wrong by construction.
        assert_eq!(row("bool")["form"], "toggle");
        assert_eq!(
            row("bool")["role"],
            "checkbox",
            "Qt's default factory hands a two-item combo box for a bool, so \
             that is what its AT announces"
        );
        assert_eq!(row("choice")["form"], "selector");
        assert_eq!(row("choice")["role"], "combobox");
    }

    #[test]
    fn a_colour_cell_is_editable_here_and_says_how() {
        // The toolkit's default factory has no color creator, so `createEditor` answers
        // nullptr and `edit` silently does nothing.
        let value = handle_scene_cell_editors().expect("ok");
        let rows = value["editors"].as_array().expect("array");
        let color = rows
            .iter()
            .find(|r| r["kind"] == "color")
            .expect("colour is in the census");
        assert_eq!(color["form"], "swatch");
        assert_eq!(
            color["buffer_is_text"], true,
            "its hex half is the in-flight buffer"
        );
        assert_eq!(
            color["inline_text"], false,
            "but the cell's own box is not a text field — a printable key at \
             the cell must not start a hex edit"
        );
        assert_eq!(color["role"], "textbox");
    }

    #[test]
    fn the_numeric_kinds_get_a_stepper() {
        let value = handle_scene_cell_editors().expect("ok");
        let rows = value["editors"].as_array().expect("array");
        for kind in ["int", "float"] {
            let r = rows.iter().find(|r| r["kind"] == kind).expect("present");
            assert_eq!(r["form"], "stepper", "{kind}");
            assert_eq!(r["role"], "spinbutton", "{kind}");
            assert_eq!(r["accepts_keystrokes"], true, "{kind}");
            assert_eq!(r["inline_text"], true, "{kind}");
        }
        let text = rows.iter().find(|r| r["kind"] == "text").expect("present");
        assert_eq!(text["form"], "field");
        assert_eq!(text["role"], "textbox");
    }
}
