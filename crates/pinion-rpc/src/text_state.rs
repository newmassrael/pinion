//! `scene/text_state` + `scene/set_text` RPC method dispatch — R603 / R610 §5.22 + §5.7.
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
//! R610 §5.22 adds the mutation pair `scene/set_text` whose
//! response is the same [`TextStateOutcome`] shape — wire symmetry
//! for the read/modify/write loop. The setter inherits the
//! substrate's "text swap drops selection + preedit + clamps caret"
//! contract; the post-state echo surfaces all three side effects.
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
use pinion_core::widgets::text_edit::TextEditState;
use serde::Serialize;

use crate::substrate_introspect::{lookup, SubstrateIntrospectError};

/// Typed errors the [`text_state`] dispatcher can return. R607
/// §5.7 §5.22 aliased to [`SubstrateIntrospectError`]. See the
/// `scroll_state` companion for the lift rationale.
pub type TextStateError = SubstrateIntrospectError;

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
    /// Echoes the request's `params.tag`. Owned `String` (post-R605
    /// `Box::leak` elimination).
    pub tag: String,
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
    fn from_state(tag: &str, state: &TextEditState) -> Self {
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
            tag: tag.to_owned(),
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
    tag: &str,
) -> Result<TextStateOutcome, TextStateError> {
    lookup::<TextEditState, _, _>(runtime_owner, tag, TextStateOutcome::from_state)
}

// ────────────────────────────────────────────────────────────────────
// scene/set_text — mutate-side (R610)
// ────────────────────────────────────────────────────────────────────

/// Typed request payload for [`set_text`]. Carries the new text and
/// the cache tag the mutation applies to.
///
/// Tag is **required** — per-field tagged ([`scroll_state`](crate::scroll_state)
/// pattern; no canonical default).
///
/// The substrate's
/// [`TextEditState::set_text`](pinion_core::widgets::text_edit::TextEditState::set_text)
/// (a) drops any active selection (the byte offsets would no longer
/// make sense against the new text), (b) drops any IME preedit (same
/// rationale), and (c) clamps the caret to the new text's length plus
/// snaps it to the nearest preceding `char` boundary if it would
/// land mid-codepoint. AI agents observe all three side effects
/// through the returned [`TextStateOutcome`].
///
/// R618 §5.22 — `text` borrows from the request's JSON payload via
/// the `'a` lifetime, matching the borrowed-tag shape every other
/// R608+ setter uses. Pre-R618 the field was an owned `String`,
/// forcing the dispatcher to `.to_owned()` the wire `&str` and the
/// pure fn to `.clone()` the String again before handing it to the
/// substrate's `set_text(String)` — two allocations per RPC call.
/// Post-R618 the dispatcher passes the borrow directly and the
/// pure fn does the single allocation the substrate API requires.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetTextParams<'a> {
    /// Cache tag the
    /// [`use_text_edit_state`](pinion_core::widgets::text_edit::use_text_edit_state)
    /// lookup resolves against. Required (no default).
    pub tag: &'a str,
    /// New committed text. Replaces the bound state's `text` field
    /// wholesale. The substrate's
    /// [`TextEditState::set_text`](pinion_core::widgets::text_edit::TextEditState::set_text)
    /// takes an owned `String`; the conversion happens inside the
    /// pure [`set_text`] fn's lookup closure so the wire-layer
    /// borrow is preserved at the dispatch boundary.
    pub text: &'a str,
}

/// Mutate the bound [`TextEditState`]'s `text` under `params.tag`
/// and return the post-mutation [`TextStateOutcome`].
///
/// # Side effects
///
/// Calls [`TextEditState::set_text`] which writes the text +
/// caret + selection + preedit signals inside a single
/// [`batch`](pinion_core::reactive::batch). Subscribers re-run at
/// most once per call. The dispatcher bumps
/// [`SceneRevision`](pinion_core::SceneRevision) after this call
/// returns `Ok` so any in-flight preview's `base_revision` detects
/// the concurrent mutation at apply time.
///
/// # Errors
///
/// - [`TextStateError::RuntimeOwnerUnavailable`] — no substrate
///   owner attached on the dispatch context.
/// - [`TextStateError::NotBound`] — owner has no text-edit state
///   cached under `params.tag`.
pub fn set_text(
    runtime_owner: Option<&Owner>,
    params: &SetTextParams<'_>,
) -> Result<TextStateOutcome, TextStateError> {
    // Same `lookup` reuse rationale as R609 (scroll_state) — see
    // [`crate::scroll_state::set_scroll_offset`] for the deferred
    // `mutate_substrate` lift discussion.
    lookup::<TextEditState, _, _>(runtime_owner, params.tag, |tag, state| {
        // The substrate's set_text takes an owned String; the
        // single `to_owned` happens inside this closure (1 alloc
        // per call). Pre-R618 the wire layer did the to_owned at
        // the dispatch boundary and the pure fn did another clone
        // here (2 alloc per call); R618 consolidates.
        state.set_text(params.text.to_owned());
        TextStateOutcome::from_state(tag, state)
    })
}

// ────────────────────────────────────────────────────────────────────
// scene/set_selection — mutate-side (R611)
// ────────────────────────────────────────────────────────────────────

/// Typed request payload for [`set_selection`]. Carries the
/// `(anchor, focus)` byte-offset pair and the cache tag the mutation
/// applies to.
///
/// `anchor` and `focus` follow the W3C `Selection` semantics:
/// `anchor` is the user-pinned end of the selection (where Shift+Arrow
/// extension started), `focus` is the moving end (where the caret
/// lands). When `anchor == focus` the selection collapses and the
/// substrate clears the `selection_anchor` signal — a caret-only
/// state.
///
/// The substrate's
/// [`TextEditState::set_selection`](pinion_core::widgets::text_edit::TextEditState::set_selection)
/// (a) snaps both offsets to the nearest `char` boundary, (b) clamps
/// both to `[0, text.len()]`, (c) writes `caret_pos` to the snapped
/// `focus`, and (d) collapses to `selection_anchor = None` if the
/// snapped values coincide. AI agents observe all four behaviours
/// through the returned [`TextStateOutcome`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetSelectionParams<'a> {
    /// Cache tag the
    /// [`use_text_edit_state`](pinion_core::widgets::text_edit::use_text_edit_state)
    /// lookup resolves against. Required (no default).
    pub tag: &'a str,
    /// User-pinned end of the selection (byte offset). Snapped to
    /// the nearest `char` boundary by the substrate.
    pub anchor: usize,
    /// Moving end of the selection (byte offset) — equal to the
    /// caret position post-call. Snapped to the nearest `char`
    /// boundary by the substrate.
    pub focus: usize,
}

/// Mutate the bound [`TextEditState`]'s selection under `params.tag`
/// and return the post-mutation [`TextStateOutcome`].
///
/// # Side effects
///
/// Calls [`TextEditState::set_selection`] which writes both
/// `caret_pos` and `selection_anchor` signals inside a single
/// [`batch`](pinion_core::reactive::batch). Subscribers re-run at
/// most once per call. The dispatcher bumps
/// [`SceneRevision`](pinion_core::SceneRevision) after this call
/// returns `Ok`.
///
/// # Errors
///
/// - [`TextStateError::RuntimeOwnerUnavailable`] — no substrate
///   owner attached on the dispatch context.
/// - [`TextStateError::NotBound`] — owner has no text-edit state
///   cached under `params.tag`.
pub fn set_selection(
    runtime_owner: Option<&Owner>,
    params: &SetSelectionParams<'_>,
) -> Result<TextStateOutcome, TextStateError> {
    lookup::<TextEditState, _, _>(runtime_owner, params.tag, |tag, state| {
        state.set_selection(params.anchor, params.focus);
        TextStateOutcome::from_state(tag, state)
    })
}

// ────────────────────────────────────────────────────────────────────
// scene/set_caret — mutate-side (R612)
// ────────────────────────────────────────────────────────────────────

/// Typed request payload for [`set_caret`]. Carries the target caret
/// byte offset and the cache tag the mutation applies to.
///
/// The substrate's
/// [`TextEditState::set_caret`](pinion_core::widgets::text_edit::TextEditState::set_caret)
/// (a) clamps `pos` to `[0, text.len()]`, (b) snaps to the nearest
/// preceding `char` boundary if `pos` would land mid-codepoint, and
/// (c) drops any active selection per the W3C `selectionchange`
/// canonical (any caret reposition that is not a Shift-modified
/// extension collapses to caret-only). AI agents observe all three
/// behaviours through the returned [`TextStateOutcome`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetCaretParams<'a> {
    /// Cache tag the
    /// [`use_text_edit_state`](pinion_core::widgets::text_edit::use_text_edit_state)
    /// lookup resolves against. Required (no default).
    pub tag: &'a str,
    /// Target caret byte offset. Clamped + snapped by the substrate.
    pub pos: usize,
}

/// Mutate the bound [`TextEditState`]'s caret under `params.tag` and
/// return the post-mutation [`TextStateOutcome`].
///
/// # Side effects
///
/// Calls [`TextEditState::set_caret`] which writes `caret_pos` and
/// (if a selection was active) clears `selection_anchor` inside a
/// single [`batch`](pinion_core::reactive::batch). Subscribers re-run
/// at most once per call. The dispatcher bumps
/// [`SceneRevision`](pinion_core::SceneRevision) after this call
/// returns `Ok`.
///
/// # Errors
///
/// - [`TextStateError::RuntimeOwnerUnavailable`] — no substrate
///   owner attached on the dispatch context.
/// - [`TextStateError::NotBound`] — owner has no text-edit state
///   cached under `params.tag`.
pub fn set_caret(
    runtime_owner: Option<&Owner>,
    params: &SetCaretParams<'_>,
) -> Result<TextStateOutcome, TextStateError> {
    lookup::<TextEditState, _, _>(runtime_owner, params.tag, |tag, state| {
        state.set_caret(params.pos);
        TextStateOutcome::from_state(tag, state)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::widgets::text_edit::use_text_edit_state;

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

    // ─────────────────────────────────────────────────────────────────
    // R610 §5.22 — set_text setter
    // ─────────────────────────────────────────────────────────────────

    fn set_text_params<'a>(tag: &'a str, text: &'a str) -> SetTextParams<'a> {
        SetTextParams { tag, text }
    }

    #[test]
    fn r610_set_text_missing_runtime_owner_errors() {
        let err = set_text(None, &set_text_params("field", "Hello")).unwrap_err();
        assert_eq!(err, TextStateError::RuntimeOwnerUnavailable);
    }

    #[test]
    fn r610_set_text_unbound_tag_errors_with_tag_echoed() {
        let owner = Owner::new();
        let err = set_text(Some(&owner), &set_text_params("ghost", "Hello")).unwrap_err();
        assert_eq!(
            err,
            TextStateError::NotBound {
                tag: "ghost".into(),
            },
        );
    }

    #[test]
    fn r610_set_text_writes_text_and_returns_post_state() {
        let owner = Owner::new();
        let state = bind_state(&owner, "field");
        let outcome = set_text(Some(&owner), &set_text_params("field", "Hello, world!"))
            .expect("happy path");
        assert_eq!(outcome.tag, "field");
        assert_eq!(outcome.text, "Hello, world!");
        // Pre-existing caret 0 stays 0 (within bounds for new text).
        assert_eq!(outcome.caret, 0);
        // Substrate immediately reflects the mutation.
        assert_eq!(state.text(), "Hello, world!");
    }

    #[test]
    fn r610_set_text_drops_active_selection_per_substrate_contract() {
        // Substrate set_text invariant: selection is dropped because
        // the byte offsets reference the pre-mutation text and become
        // meaningless after the swap. AI agents see selection=None
        // in the post-state.
        let owner = Owner::new();
        let state = bind_state(&owner, "field");
        state.set_text("Old text here".to_owned());
        state.set_selection(0, 5);
        assert!(state.selection_range().is_some(), "precondition");
        let outcome = set_text(Some(&owner), &set_text_params("field", "Brand new"))
            .expect("happy path");
        assert!(!outcome.has_selection);
        assert!(outcome.selection.is_none());
        assert!(state.selection_range().is_none());
    }

    #[test]
    fn r610_set_text_drops_ime_preedit_per_substrate_contract() {
        // Same rationale as selection — IME preedit byte offsets
        // become meaningless against the new text.
        let owner = Owner::new();
        let state = bind_state(&owner, "field");
        state.preedit_start();
        state.preedit_update("한");
        assert!(state.is_composing(), "precondition");
        let outcome = set_text(Some(&owner), &set_text_params("field", "fresh"))
            .expect("happy path");
        assert!(!outcome.is_composing);
        assert!(outcome.preedit.is_none());
    }

    #[test]
    fn r610_set_text_clamps_caret_to_new_text_length() {
        // Substrate set_text invariant: caret is clamped to
        // `[0, new_text.len()]`. Place caret past the new shorter
        // text and verify the outcome reflects the clamp.
        let owner = Owner::new();
        let state = bind_state(&owner, "field");
        state.set_text("A long original string".to_owned());
        state.set_caret(15);
        assert_eq!(state.caret(), 15, "precondition");
        let outcome = set_text(Some(&owner), &set_text_params("field", "short"))
            .expect("happy path");
        assert_eq!(outcome.text, "short");
        // Caret clamped to text.len() = 5.
        assert_eq!(outcome.caret, 5);
    }

    #[test]
    fn r610_set_text_accepts_empty_string() {
        let owner = Owner::new();
        let state = bind_state(&owner, "field");
        state.set_text("Some content".to_owned());
        state.set_caret(4);
        let outcome = set_text(Some(&owner), &set_text_params("field", "")).expect("happy path");
        assert_eq!(outcome.text, "");
        assert_eq!(outcome.caret, 0, "caret clamped to text.len() = 0");
        assert_eq!(state.text(), "");
    }

    #[test]
    fn r610_set_text_accepts_unicode_text_and_preserves_char_boundary() {
        // Multi-byte UTF-8 input — substrate snaps caret to nearest
        // `char` boundary. Pre-set caret to a byte offset that would
        // land mid-codepoint after the swap; outcome must show a
        // valid boundary.
        let owner = Owner::new();
        let state = bind_state(&owner, "field");
        // Initial text long enough to seed a non-zero caret.
        state.set_text("0123456789".to_owned());
        state.set_caret(10);
        let outcome = set_text(Some(&owner), &set_text_params("field", "안녕")).expect("happy path");
        assert_eq!(outcome.text, "안녕");
        // "안녕" = 6 bytes (3 + 3). Caret clamped to byte 6 (end of
        // text); ends are always char boundaries.
        assert_eq!(outcome.caret, 6);
    }

    #[test]
    fn r610_set_text_does_not_insert_a_new_cache_slot() {
        let owner = Owner::new();
        let _ = set_text(Some(&owner), &set_text_params("phantom", "x")).unwrap_err();
        assert!(!owner.cache_contains::<TextEditState>("phantom"));
    }

    #[test]
    fn r610_set_text_outcome_serializes_to_full_text_state_shape() {
        let owner = Owner::new();
        let _state = bind_state(&owner, "field");
        let outcome = set_text(Some(&owner), &set_text_params("field", "Hello"))
            .expect("happy path");
        let json = serde_json::to_value(&outcome).unwrap();
        assert_eq!(json["tag"], "field");
        assert_eq!(json["text"], "Hello");
        assert_eq!(json["caret"], 0);
        assert_eq!(json["has_selection"], false);
        assert_eq!(json["selection"], serde_json::Value::Null);
        assert_eq!(json["is_composing"], false);
        assert_eq!(json["preedit"], serde_json::Value::Null);
    }

    // ─────────────────────────────────────────────────────────────────
    // R611 §5.22 — set_selection setter
    // ─────────────────────────────────────────────────────────────────

    fn sel_params(tag: &str, anchor: usize, focus: usize) -> SetSelectionParams<'_> {
        SetSelectionParams { tag, anchor, focus }
    }

    #[test]
    fn r611_set_selection_missing_runtime_owner_errors() {
        let err = set_selection(None, &sel_params("field", 0, 3)).unwrap_err();
        assert_eq!(err, TextStateError::RuntimeOwnerUnavailable);
    }

    #[test]
    fn r611_set_selection_unbound_tag_errors_with_tag_echoed() {
        let owner = Owner::new();
        let err = set_selection(Some(&owner), &sel_params("ghost", 0, 3)).unwrap_err();
        assert_eq!(
            err,
            TextStateError::NotBound {
                tag: "ghost".into(),
            },
        );
    }

    #[test]
    fn r611_set_selection_writes_range_and_returns_post_state() {
        let owner = Owner::new();
        let state = bind_state(&owner, "field");
        state.set_text("Hello world".to_owned());
        let outcome = set_selection(Some(&owner), &sel_params("field", 0, 5)).unwrap();
        assert_eq!(outcome.tag, "field");
        assert!(outcome.has_selection);
        let sel = outcome.selection.expect("selection present");
        assert_eq!(sel.start, 0);
        assert_eq!(sel.end, 5);
        assert_eq!(sel.anchor, 0);
        // Caret = focus per W3C Selection contract.
        assert_eq!(outcome.caret, 5);
        // Substrate state mirrors the wire response.
        assert_eq!(state.caret(), 5);
    }

    #[test]
    fn r611_set_selection_anchor_after_focus_records_anchor_at_end() {
        // anchor=5, focus=0 → user selected right-to-left. start=0,
        // end=5, but anchor remains pinned at 5 (the user's drag
        // start). Pinned per W3C Selection semantics.
        let owner = Owner::new();
        let state = bind_state(&owner, "field");
        state.set_text("Hello world".to_owned());
        let outcome = set_selection(Some(&owner), &sel_params("field", 5, 0)).unwrap();
        let sel = outcome.selection.expect("selection present");
        assert_eq!(sel.start, 0);
        assert_eq!(sel.end, 5);
        assert_eq!(sel.anchor, 5);
        assert_eq!(outcome.caret, 0, "caret lands at focus");
        assert_eq!(state.caret(), 0);
    }

    #[test]
    fn r611_set_selection_collapsed_anchor_equals_focus_clears_selection() {
        // anchor == focus → substrate collapses to caret-only state.
        let owner = Owner::new();
        let state = bind_state(&owner, "field");
        state.set_text("Hello world".to_owned());
        let outcome = set_selection(Some(&owner), &sel_params("field", 3, 3)).unwrap();
        assert!(!outcome.has_selection);
        assert!(outcome.selection.is_none());
        assert_eq!(outcome.caret, 3);
        assert!(state.selection_range().is_none());
    }

    #[test]
    fn r611_set_selection_clamps_offsets_to_text_length() {
        // anchor=0, focus=999 → substrate clamps to text.len() = 5.
        let owner = Owner::new();
        let state = bind_state(&owner, "field");
        state.set_text("Hello".to_owned());
        let outcome = set_selection(Some(&owner), &sel_params("field", 0, 999)).unwrap();
        let sel = outcome.selection.expect("selection present");
        assert_eq!(sel.start, 0);
        assert_eq!(sel.end, 5);
        assert_eq!(outcome.caret, 5);
    }

    #[test]
    fn r611_set_selection_snaps_to_char_boundary_for_multibyte_text() {
        // "안녕" = 6 bytes, with char boundaries at 0 / 3 / 6.
        // anchor=1 (mid-codepoint) snaps to 0; focus=4 (mid) snaps
        // to 3. The wire payload sees the post-snap values.
        let owner = Owner::new();
        let state = bind_state(&owner, "field");
        state.set_text("안녕".to_owned());
        let outcome = set_selection(Some(&owner), &sel_params("field", 1, 4)).unwrap();
        let sel = outcome.selection.expect("selection present");
        assert_eq!(sel.start, 0);
        assert_eq!(sel.end, 3);
        assert_eq!(outcome.caret, 3);
    }

    #[test]
    fn r611_set_selection_does_not_insert_a_new_cache_slot() {
        let owner = Owner::new();
        let _ = set_selection(Some(&owner), &sel_params("phantom", 0, 1)).unwrap_err();
        assert!(!owner.cache_contains::<TextEditState>("phantom"));
    }

    #[test]
    fn r611_set_selection_outcome_serializes_to_full_text_state_shape() {
        let owner = Owner::new();
        let state = bind_state(&owner, "field");
        state.set_text("Hello".to_owned());
        let outcome = set_selection(Some(&owner), &sel_params("field", 0, 5)).unwrap();
        let json = serde_json::to_value(&outcome).unwrap();
        assert_eq!(json["tag"], "field");
        assert_eq!(json["caret"], 5);
        assert_eq!(json["has_selection"], true);
        assert_eq!(json["selection"]["start"], 0);
        assert_eq!(json["selection"]["end"], 5);
        assert_eq!(json["selection"]["anchor"], 0);
    }

    // ─────────────────────────────────────────────────────────────────
    // R612 §5.22 — set_caret setter
    // ─────────────────────────────────────────────────────────────────

    fn caret_params(tag: &str, pos: usize) -> SetCaretParams<'_> {
        SetCaretParams { tag, pos }
    }

    #[test]
    fn r612_set_caret_missing_runtime_owner_errors() {
        let err = set_caret(None, &caret_params("field", 3)).unwrap_err();
        assert_eq!(err, TextStateError::RuntimeOwnerUnavailable);
    }

    #[test]
    fn r612_set_caret_unbound_tag_errors_with_tag_echoed() {
        let owner = Owner::new();
        let err = set_caret(Some(&owner), &caret_params("ghost", 3)).unwrap_err();
        assert_eq!(
            err,
            TextStateError::NotBound {
                tag: "ghost".into(),
            },
        );
    }

    #[test]
    fn r612_set_caret_writes_pos_and_returns_post_state() {
        let owner = Owner::new();
        let state = bind_state(&owner, "field");
        state.set_text("Hello world".to_owned());
        let outcome = set_caret(Some(&owner), &caret_params("field", 5)).unwrap();
        assert_eq!(outcome.tag, "field");
        assert_eq!(outcome.caret, 5);
        assert!(!outcome.has_selection);
        assert!(outcome.selection.is_none());
        assert_eq!(state.caret(), 5);
    }

    #[test]
    fn r612_set_caret_clamps_to_text_length() {
        let owner = Owner::new();
        let state = bind_state(&owner, "field");
        state.set_text("Hello".to_owned());
        let outcome = set_caret(Some(&owner), &caret_params("field", 999)).unwrap();
        assert_eq!(outcome.caret, 5, "caret clamped to text.len() = 5");
    }

    #[test]
    fn r612_set_caret_snaps_to_char_boundary_for_multibyte_text() {
        // "안녕" = 6 bytes, char boundaries at 0 / 3 / 6. Pos = 1
        // lands mid-codepoint; substrate snaps to preceding boundary.
        let owner = Owner::new();
        let state = bind_state(&owner, "field");
        state.set_text("안녕".to_owned());
        let outcome = set_caret(Some(&owner), &caret_params("field", 1)).unwrap();
        assert_eq!(outcome.caret, 0, "mid-codepoint snaps to preceding boundary");
        let outcome = set_caret(Some(&owner), &caret_params("field", 4)).unwrap();
        assert_eq!(outcome.caret, 3);
    }

    #[test]
    fn r612_set_caret_drops_active_selection_per_w3c_canonical() {
        // W3C: any explicit caret reposition that is not a Shift-
        // modified extension collapses any active selection.
        let owner = Owner::new();
        let state = bind_state(&owner, "field");
        state.set_text("Hello world".to_owned());
        state.set_selection(0, 5);
        assert!(state.selection_range().is_some(), "precondition");
        let outcome = set_caret(Some(&owner), &caret_params("field", 7)).unwrap();
        assert!(!outcome.has_selection);
        assert!(outcome.selection.is_none());
        assert_eq!(outcome.caret, 7);
        assert!(state.selection_range().is_none());
    }

    #[test]
    fn r612_set_caret_does_not_insert_a_new_cache_slot() {
        let owner = Owner::new();
        let _ = set_caret(Some(&owner), &caret_params("phantom", 0)).unwrap_err();
        assert!(!owner.cache_contains::<TextEditState>("phantom"));
    }

    #[test]
    fn r612_set_caret_outcome_serializes_to_full_text_state_shape() {
        let owner = Owner::new();
        let state = bind_state(&owner, "field");
        state.set_text("Hello".to_owned());
        let outcome = set_caret(Some(&owner), &caret_params("field", 3)).unwrap();
        let json = serde_json::to_value(&outcome).unwrap();
        assert_eq!(json["tag"], "field");
        assert_eq!(json["caret"], 3);
        assert_eq!(json["has_selection"], false);
        assert_eq!(json["selection"], serde_json::Value::Null);
    }
}
