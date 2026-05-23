//! `scene/text_state` RPC method dispatch — R603 §5.22 + §5.7.
//!
//! Fourth reactive-substrate introspection method (after
//! [`crate::theme`] R598/R599, [`crate::animation_state`] R600,
//! [`crate::scroll_state`] R602). Projects the
//! [`TextEditState`](pinion_core::widgets::text_edit::TextEditState)
//! cached on the substrate's root [`Owner`] under the supplied
//! `tag` so AI agents can verify typed text, caret position,
//! selection range, and IME composition state without scraping
//! pixels from `scene/snapshot`.
//!
//! ## Wire shape
//!
//! ```json
//! {
//!   "jsonrpc": "2.0",
//!   "id": 1,
//!   "result": {
//!     "tag": "username",
//!     "text": "Hello",
//!     "caret": 5,
//!     "has_selection": false,
//!     "selection": null,
//!     "is_composing": false,
//!     "preedit": null
//!   }
//! }
//! ```
//!
//! With an active selection and IME composition:
//!
//! ```json
//! {
//!   "tag": "chat_input",
//!   "text": "안녕",
//!   "caret": 6,
//!   "has_selection": true,
//!   "selection": { "start": 0, "end": 3, "anchor": 0 },
//!   "is_composing": true,
//!   "preedit": "하"
//! }
//! ```
//!
//! Request:
//!
//! ```json
//! {
//!   "jsonrpc": "2.0",
//!   "method": "scene/text_state",
//!   "params": { "tag": "username" },
//!   "id": 1
//! }
//! ```
//!
//! `params.tag` is **required** — text-edit states are per-field
//! tagged (`"username"`, `"chat_input"`), no canonical default.
//!
//! ## Why both `has_selection` and `selection`
//!
//! `has_selection` is the textbook AI-first predicate the agent asks
//! ("does the user have something selected right now?"); `selection`
//! is the detail when the answer is yes. Surfacing the predicate at
//! the root mirrors W3C `Selection.isCollapsed` / `Range.collapsed`
//! and keeps the common "did the user select anything?" query a
//! single field lookup instead of a null check.
//!
//! Same rationale applies to `is_composing` / `preedit` — W3C
//! `KeyboardEvent.isComposing` is the AT-canonical predicate; the
//! preedit string itself is the detail.
//!
//! ## Anchor semantics
//!
//! `selection.anchor` is the byte offset where the user-driven
//! selection extension started (Shift+Arrow / Shift+Home /
//! Shift+End / mouse drag). When `anchor == start`, the focus
//! (caret) is at `end`; when `anchor == end`, the focus is at
//! `start`. AI agents that re-create the user's selection direction
//! for further shift-arrow extension read `anchor` to disambiguate.

use pinion_core::reactive::Owner;
use pinion_core::widgets::text_edit::{use_text_edit_state, TextEditState};
use serde::Serialize;

/// Typed errors the [`text_state`] dispatcher can return. Every
/// variant maps onto a JSON-RPC `-32602 Invalid params` at the
/// dispatch layer with the variant name in `error.data`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextStateError {
    /// No [`runtime_owner`](crate::DispatchContext) on the dispatch
    /// context.
    RuntimeOwnerUnavailable,
    /// `params.tag` missing — required because text-edit states are
    /// per-field tagged with no canonical default.
    TagRequired,
    /// Owner has no [`TextEditState`] cached under `tag` yet.
    NotBound { tag: String },
}

/// Selection projection. `start <= end`, both `char` boundaries.
/// `anchor` is one of `start` or `end` (whichever the user pinned).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct TextSelectionView {
    /// Lower byte offset of the selection range.
    pub start: usize,
    /// Upper byte offset of the selection range; same as `start`
    /// would mean an empty (collapsed) selection — collapsed
    /// selections surface as `selection: null` at the outcome root,
    /// so `start < end` for every `Some(_)` projection.
    pub end: usize,
    /// Byte offset where the selection extension started — equals
    /// either `start` or `end` (the user-pinned end of the range).
    pub anchor: usize,
}

/// Snapshot of the bound [`TextEditState`]'s observable surface.
/// Mirrors the W3C `HTMLInputElement` + `Selection` +
/// `CompositionEvent` reading contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TextStateOutcome {
    /// Echoes the request's `params.tag`.
    pub tag: &'static str,
    /// Committed text (no IME preedit spliced in — see
    /// [`Self::preedit`]).
    pub text: String,
    /// Caret byte offset inside [`Self::text`]. `0..=text.len()`,
    /// always a `char` boundary.
    pub caret: usize,
    /// Whether a non-collapsed selection is currently active —
    /// mirror of W3C `!Selection.isCollapsed`. Surface-level
    /// predicate so AI agents do not have to null-check
    /// [`Self::selection`] for the common "anything selected?"
    /// query.
    pub has_selection: bool,
    /// Selection range detail (`None` when no selection — i.e.
    /// [`Self::has_selection`] is `false`).
    pub selection: Option<TextSelectionView>,
    /// Whether an IME composition is currently active — mirror of
    /// W3C `KeyboardEvent.isComposing`. The preedit string may be
    /// empty during the transient compositionstart-before-update
    /// window while this is still `true`.
    pub is_composing: bool,
    /// Preedit string when [`Self::is_composing`] is `true` (`None`
    /// otherwise). Same shape as W3C `CompositionEvent.data`.
    pub preedit: Option<String>,
}

impl TextStateOutcome {
    fn from_state(tag: &'static str, state: &TextEditState) -> Self {
        let text = state.text();
        let caret = state.caret();
        let (selection, has_selection) = match state.selection_range() {
            Some((start, end)) => {
                let anchor = state.selection_anchor().unwrap_or(start);
                (
                    Some(TextSelectionView {
                        start,
                        end,
                        anchor,
                    }),
                    true,
                )
            }
            None => (None, false),
        };
        let is_composing = state.is_composing();
        let preedit = state.preedit();
        Self {
            tag,
            text,
            caret,
            has_selection,
            selection,
            is_composing,
            preedit,
        }
    }
}

/// Snapshot the [`TextEditState`] cached at `tag` on `runtime_owner`.
///
/// # Errors
///
/// - [`TextStateError::RuntimeOwnerUnavailable`] — no substrate
///   owner attached.
/// - [`TextStateError::NotBound`] — owner has no text-edit state
///   cached under `tag`.
///
/// # Side effects
///
/// None. The [`Owner::cache_contains`] gate routes the no-slot case
/// to [`TextStateError::NotBound`]; the `Owner::run` wrap only
/// activates [`Owner::current`] for the [`use_text_edit_state`]
/// hook; no reactive computation is established.
pub fn text_state(
    runtime_owner: Option<&Owner>,
    tag: &'static str,
) -> Result<TextStateOutcome, TextStateError> {
    let Some(owner) = runtime_owner else {
        return Err(TextStateError::RuntimeOwnerUnavailable);
    };
    if !owner.cache_contains::<TextEditState>(tag) {
        return Err(TextStateError::NotBound {
            tag: tag.to_owned(),
        });
    }
    let state: std::rc::Rc<TextEditState> = owner.run(|| use_text_edit_state(tag));
    Ok(TextStateOutcome::from_state(tag, &state))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bind_state(owner: &Owner, tag: &'static str) -> std::rc::Rc<TextEditState> {
        owner.run(|| use_text_edit_state(tag))
    }

    // ─────────────────────────────────────────────────────────────────
    // Failure modes
    // ─────────────────────────────────────────────────────────────────

    #[test]
    fn r603_missing_runtime_owner_errors() {
        let err = text_state(None, "field").unwrap_err();
        assert_eq!(err, TextStateError::RuntimeOwnerUnavailable);
    }

    #[test]
    fn r603_unbound_tag_errors_with_tag_echoed() {
        let owner = Owner::new();
        let err = text_state(Some(&owner), "ghost").unwrap_err();
        assert_eq!(
            err,
            TextStateError::NotBound {
                tag: "ghost".into(),
            },
        );
    }

    // ─────────────────────────────────────────────────────────────────
    // Happy path
    // ─────────────────────────────────────────────────────────────────

    #[test]
    fn r603_fresh_state_reports_empty_text_zero_caret_no_selection() {
        let owner = Owner::new();
        let _state = bind_state(&owner, "field");
        let outcome = text_state(Some(&owner), "field").unwrap();
        assert_eq!(outcome.tag, "field");
        assert_eq!(outcome.text, "");
        assert_eq!(outcome.caret, 0);
        assert!(!outcome.has_selection);
        assert!(outcome.selection.is_none());
        assert!(!outcome.is_composing);
        assert!(outcome.preedit.is_none());
    }

    #[test]
    fn r603_text_and_caret_round_trip_through_projection() {
        let owner = Owner::new();
        let state = bind_state(&owner, "field");
        state.set_text("Hello".to_owned());
        state.set_caret(5);
        let outcome = text_state(Some(&owner), "field").unwrap();
        assert_eq!(outcome.text, "Hello");
        assert_eq!(outcome.caret, 5);
    }

    #[test]
    fn r603_active_selection_surfaces_start_end_anchor() {
        let owner = Owner::new();
        let state = bind_state(&owner, "field");
        state.set_text("Hello world".to_owned());
        state.set_selection(0, 5);
        let outcome = text_state(Some(&owner), "field").unwrap();
        assert!(outcome.has_selection);
        let sel = outcome.selection.expect("selection present");
        assert_eq!(sel.start, 0);
        assert_eq!(sel.end, 5);
        assert_eq!(sel.anchor, 0, "anchor was the first arg to set_selection");
    }

    #[test]
    fn r603_collapsed_selection_serializes_as_none() {
        let owner = Owner::new();
        let state = bind_state(&owner, "field");
        state.set_text("Hello".to_owned());
        state.set_selection(2, 2);
        let outcome = text_state(Some(&owner), "field").unwrap();
        assert!(!outcome.has_selection);
        assert!(outcome.selection.is_none());
    }

    #[test]
    fn r603_ime_composition_surfaces_preedit_and_is_composing() {
        let owner = Owner::new();
        let state = bind_state(&owner, "field");
        state.preedit_start();
        state.preedit_update("하");
        let outcome = text_state(Some(&owner), "field").unwrap();
        assert!(outcome.is_composing);
        assert_eq!(outcome.preedit.as_deref(), Some("하"));
    }

    #[test]
    fn r603_composition_cancel_clears_preedit_and_predicate() {
        let owner = Owner::new();
        let state = bind_state(&owner, "field");
        state.preedit_start();
        state.preedit_update("한");
        state.preedit_cancel();
        let outcome = text_state(Some(&owner), "field").unwrap();
        assert!(!outcome.is_composing);
        assert!(outcome.preedit.is_none());
    }

    #[test]
    fn r603_anchor_after_selection_extension_reports_pinned_end() {
        // Set selection then extend via shift-arrow-style mutation —
        // anchor stays pinned at the original drag start.
        let owner = Owner::new();
        let state = bind_state(&owner, "field");
        state.set_text("0123456789".to_owned());
        state.set_selection(3, 5);
        state.select_right();
        let outcome = text_state(Some(&owner), "field").unwrap();
        let sel = outcome.selection.expect("selection present");
        assert_eq!(sel.anchor, 3, "anchor preserved across select_right");
        assert!(sel.end > 5, "focus moved past 5 via select_right");
    }

    // ─────────────────────────────────────────────────────────────────
    // Side-effect contract
    // ─────────────────────────────────────────────────────────────────

    #[test]
    fn r603_call_does_not_insert_a_new_cache_slot() {
        let owner = Owner::new();
        let _ = text_state(Some(&owner), "phantom").unwrap_err();
        assert!(!owner.cache_contains::<TextEditState>("phantom"));
    }

    #[test]
    fn r603_call_is_idempotent_two_consecutive_snapshots_match() {
        let owner = Owner::new();
        let state = bind_state(&owner, "field");
        state.set_text("test".to_owned());
        state.set_caret(2);
        let a = text_state(Some(&owner), "field").unwrap();
        let b = text_state(Some(&owner), "field").unwrap();
        assert_eq!(a, b);
    }

    // ─────────────────────────────────────────────────────────────────
    // JSON serialization shape
    // ─────────────────────────────────────────────────────────────────

    #[test]
    fn r603_outcome_serializes_to_expected_keys() {
        let owner = Owner::new();
        let state = bind_state(&owner, "field");
        state.set_text("Hello".to_owned());
        // set_selection(anchor, focus) leaves caret at focus per the
        // W3C Selection canonical contract — focus = caret = 3.
        state.set_selection(0, 3);
        let outcome = text_state(Some(&owner), "field").unwrap();
        let json = serde_json::to_value(&outcome).unwrap();
        assert_eq!(json["tag"], "field");
        assert_eq!(json["text"], "Hello");
        assert_eq!(json["caret"], 3);
        assert_eq!(json["has_selection"], true);
        assert_eq!(json["selection"]["start"], 0);
        assert_eq!(json["selection"]["end"], 3);
        assert_eq!(json["selection"]["anchor"], 0);
        assert_eq!(json["is_composing"], false);
        assert_eq!(json["preedit"], serde_json::Value::Null);
    }

    #[test]
    fn r603_outcome_collapsed_selection_serializes_selection_null() {
        let owner = Owner::new();
        let _state = bind_state(&owner, "field");
        let outcome = text_state(Some(&owner), "field").unwrap();
        let json = serde_json::to_value(&outcome).unwrap();
        assert_eq!(json["selection"], serde_json::Value::Null);
        assert_eq!(json["has_selection"], false);
    }
}
