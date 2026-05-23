//! `scene/scroll_state` + `scene/set_scroll_offset` RPC method dispatch — R602 / R609 §5.45 + §5.7.
//!
//! Projects the [`ScrollState`] cached on the substrate's root
//! [`Owner`] under the supplied `tag` so AI agents can verify scroll
//! position without resorting to [`scene/snapshot`] pixel diffs.
//! R609 §5.45 adds the mutation pair `scene/set_scroll_offset` whose
//! response is the same [`ScrollStateOutcome`] shape — wire symmetry
//! for the read/modify/write loop.
//!
//! Third reactive-substrate introspection method, after
//! [`crate::theme`] (R598/R599) and
//! [`crate::animation_state`] (R600). Same shape: read-only,
//! [`Owner::cache_contains`] gate, no signal subscription, no new
//! cache slot inserted on miss.
//!
//! ## Wire shape
//!
//! ```json
//! {
//!   "jsonrpc": "2.0",
//!   "id": 1,
//!   "result": {
//!     "tag": "history_list",
//!     "offset": { "x": 0, "y": 240 },
//!     "max":    { "x": 0, "y": 480 },
//!     "edges": {
//!       "at_top":    false,
//!       "at_bottom": false,
//!       "at_left":   true,
//!       "at_right":  true
//!     }
//!   }
//! }
//! ```
//!
//! Request:
//!
//! ```json
//! {
//!   "jsonrpc": "2.0",
//!   "method": "scene/scroll_state",
//!   "params": { "tag": "history_list" },
//!   "id": 1
//! }
//! ```
//!
//! `params.tag` is **required** — unlike `scene/theme_tokens` there
//! is no canonical default tag for scroll states (every scrollable
//! widget uses its own tag — `"history_list"`, `"chat_log"`, …).
//!
//! ## Derived edge predicates
//!
//! `at_top` / `at_bottom` / `at_left` / `at_right` are the most
//! common scroll-state queries an AI agent asks ("is the list
//! scrolled to bottom yet?"). Computing them server-side is the
//! textbook canonical AI-first surface — clients should not have to
//! re-derive the same predicate from `offset` + `max` on every poll.
//!
//! When `max.y == 0` (content fits in viewport, no scrollable range)
//! both `at_top` and `at_bottom` evaluate `true` — the W3C / CSS
//! convention treats a non-scrolling element as trivially at both
//! ends. The `max.x == 0` case is symmetric.

use pinion_core::reactive::Owner;
use pinion_core::widgets::scroll::ScrollState;
use serde::Serialize;

use crate::substrate_introspect::{lookup, SubstrateIntrospectError};

/// Typed errors the [`scroll_state`] dispatcher can return. R607
/// §5.7 §5.22 aliased to [`SubstrateIntrospectError`] because
/// scroll-state, text-state, and caret-state introspection all
/// share the same three failure modes (no runtime owner, missing
/// tag, unbound cache slot). The alias keeps call-site naming
/// distinct so a future divergence (e.g. a scroll-specific
/// `InvalidAxis` error) can replace the alias with a dedicated
/// enum without rippling through the unrelated modules.
pub type ScrollStateError = SubstrateIntrospectError;

/// Per-axis pair (offset and max). Two `i32`s — the
/// [`ScrollState`] backing carries `i32` natively, no widening at
/// the RPC boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ScrollAxisPair {
    /// Horizontal value — `0..=max_x` for offsets, `>= 0` for the
    /// max bound.
    pub x: i32,
    /// Vertical value; semantics symmetric with [`Self::x`].
    pub y: i32,
}

/// Server-side-derived edge predicates. Grouping the four booleans
/// into a dedicated sub-struct keeps the
/// [`ScrollStateOutcome`] root flat and produces cleaner JSON: edge
/// predicates live under a single `edges` key rather than mixing
/// with positional `offset` / `max` data.
#[allow(
    clippy::struct_excessive_bools,
    reason = "four edge predicates (at_top / at_bottom / at_left / \
              at_right) is the canonical W3C / CSS scroll-state \
              surface — collapsing them into a bitflag or enum \
              would erase the per-axis intent every AI client \
              cares about. The struct already sub-groups them \
              under `edges` to keep the outcome root flat; a state \
              machine is the wrong abstraction because these are \
              independent axis predicates, not a single mode."
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ScrollEdges {
    /// `true` when `offset.y == 0`. Trivially `true` when `max.y == 0`.
    pub at_top: bool,
    /// `true` when `offset.y == max.y`. Trivially `true` when `max.y == 0`.
    pub at_bottom: bool,
    /// `true` when `offset.x == 0`. Trivially `true` when `max.x == 0`.
    pub at_left: bool,
    /// `true` when `offset.x == max.x`. Trivially `true` when `max.x == 0`.
    pub at_right: bool,
}

/// Snapshot of the bound [`ScrollState`]'s observable surface plus
/// the four server-side-derived edge predicates (grouped under
/// [`Self::edges`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScrollStateOutcome {
    /// Echoes the request's `params.tag` so the response
    /// self-identifies. Owned `String` (post-R605 `Box::leak`
    /// elimination) so the outcome holds the request's tag without
    /// requiring a `&'static` storage path.
    pub tag: String,
    /// Current `(offset_x, offset_y)`.
    pub offset: ScrollAxisPair,
    /// Current `(max_x, max_y)` bound — the upper clamp on `offset`.
    pub max: ScrollAxisPair,
    /// Per-axis "at this edge" predicates derived from `offset` / `max`.
    pub edges: ScrollEdges,
}

impl ScrollStateOutcome {
    fn from_state(tag: &str, state: &ScrollState) -> Self {
        let (ox, oy) = state.offset();
        let (mx, my) = state.max();
        Self {
            tag: tag.to_owned(),
            offset: ScrollAxisPair { x: ox, y: oy },
            max: ScrollAxisPair { x: mx, y: my },
            edges: ScrollEdges {
                at_top: oy == 0,
                at_bottom: oy == my,
                at_left: ox == 0,
                at_right: ox == mx,
            },
        }
    }
}

/// Snapshot the [`ScrollState`] cached at `tag` on `runtime_owner`.
///
/// `tag` accepts any `&str` lifetime — R605 §5.22 lifted the
/// substrate to expose `Owner::cache_get_by_str` so the JSON-RPC
/// path no longer leaks the tag into `&'static str` storage.
///
/// # Errors
///
/// - [`ScrollStateError::RuntimeOwnerUnavailable`] — no substrate
///   owner attached on the dispatch context.
/// - [`ScrollStateError::NotBound`] — owner has no scroll state
///   cached under `tag`.
///
/// # Side effects
///
/// None. The [`Owner::cache_get_by_str`] walk never creates a slot
/// on miss — failed lookups surface as
/// [`ScrollStateError::NotBound`] without polluting the cache.
pub fn scroll_state(
    runtime_owner: Option<&Owner>,
    tag: &str,
) -> Result<ScrollStateOutcome, ScrollStateError> {
    lookup::<ScrollState, _, _>(runtime_owner, tag, ScrollStateOutcome::from_state)
}

// ────────────────────────────────────────────────────────────────────
// scene/set_scroll_offset — mutate-side (R609)
// ────────────────────────────────────────────────────────────────────

/// Typed request payload for [`set_scroll_offset`]. Carries the
/// requested `(x, y)` offset and the cache tag the mutation applies
/// to.
///
/// Unlike [`scene/theme_tokens`](crate::theme::theme_tokens) the tag
/// is **required** — every scrollable widget owns its own
/// [`ScrollState`] under a distinct key, so there is no canonical
/// default (no equivalent of [`crate::theme::DEFAULT_THEME_TAG`]).
///
/// The substrate's [`ScrollState::scroll_to`] clamps the requested
/// `(x, y)` against `[0, max]` so an out-of-range request lands on
/// the nearest valid offset rather than rejecting; AI agents read
/// the post-clamp value back through the returned
/// [`ScrollStateOutcome`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetScrollOffsetParams<'a> {
    /// Cache tag the [`use_scroll_state`](pinion_core::widgets::scroll::use_scroll_state)
    /// lookup resolves against. Required (no default).
    pub tag: &'a str,
    /// Target horizontal offset, clamped to `[0, max_x]` by
    /// [`ScrollState::scroll_to`].
    pub x: i32,
    /// Target vertical offset, clamped to `[0, max_y]` by
    /// [`ScrollState::scroll_to`].
    pub y: i32,
}

/// Mutate the bound [`ScrollState`]'s `(offset_x, offset_y)` under
/// `params.tag` and return the post-mutation
/// [`ScrollStateOutcome`] (same shape [`scroll_state`] read returns).
///
/// # Side effects
///
/// Calls [`ScrollState::scroll_to`] which writes both offset
/// [`Signal`](pinion_core::reactive::Signal)s inside a single
/// [`batch`](pinion_core::reactive::batch). Subscribers re-run at
/// most once per call even when both axes shift. The dispatcher
/// bumps [`SceneRevision`](pinion_core::SceneRevision) after this
/// call returns `Ok` so any in-flight preview's `base_revision`
/// detects the concurrent mutation at apply time.
///
/// # Why the outcome is the full read-side shape
///
/// The substrate clamps the request against `[0, max]`, so the
/// post-state offset may differ from `params`. Echoing back the full
/// [`ScrollStateOutcome`] (offset / max / edges) is the textbook
/// canonical AI-first shape: the agent sees the clamped offset, the
/// derived edge predicates, and the max bound in one round-trip
/// instead of needing a follow-up [`scroll_state`] call.
///
/// # Errors
///
/// - [`ScrollStateError::RuntimeOwnerUnavailable`] — no substrate
///   owner attached on the dispatch context.
/// - [`ScrollStateError::NotBound`] — owner has no scroll state
///   cached under `params.tag`.
pub fn set_scroll_offset(
    runtime_owner: Option<&Owner>,
    params: &SetScrollOffsetParams<'_>,
) -> Result<ScrollStateOutcome, ScrollStateError> {
    // R609 reuses the R607 `lookup` helper because the substrate
    // gate shape (`RuntimeOwnerUnavailable` + `NotBound`) is the
    // same as the read path's. `ScrollState`'s mutators take `&self`
    // (interior mutability via `Signal`), so the closure can write
    // through the borrowed reference without violating the helper's
    // signature. A dedicated `mutate_substrate` helper is deferred
    // per [[abstraction-needs-second-consumer]] until R610-R612
    // surface 2-3 more write-side write-then-read sites.
    lookup::<ScrollState, _, _>(runtime_owner, params.tag, |tag, state| {
        state.scroll_to(params.x, params.y);
        ScrollStateOutcome::from_state(tag, state)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::widgets::scroll::use_scroll_state;

    fn bind_state(owner: &Owner, tag: &'static str) -> std::rc::Rc<ScrollState> {
        owner.run(|| use_scroll_state(tag))
    }

    // ─────────────────────────────────────────────────────────────────
    // Failure modes
    // ─────────────────────────────────────────────────────────────────

    #[test]
    fn r602_missing_runtime_owner_errors() {
        let err = scroll_state(None, "ghost").unwrap_err();
        assert_eq!(err, ScrollStateError::RuntimeOwnerUnavailable);
    }

    #[test]
    fn r602_unbound_tag_errors_with_tag_echoed() {
        let owner = Owner::new();
        let err = scroll_state(Some(&owner), "ghost").unwrap_err();
        assert_eq!(
            err,
            ScrollStateError::NotBound {
                tag: "ghost".into(),
            },
        );
    }

    // ─────────────────────────────────────────────────────────────────
    // Happy path: shape projection
    // ─────────────────────────────────────────────────────────────────

    #[test]
    fn r602_initial_state_zero_offset_zero_max_reports_all_edges_true() {
        // ScrollState::new defaults to offset=(0,0) max=(0,0). Every
        // edge predicate is trivially true under the W3C "non-
        // scrolling content is at both ends" convention.
        let owner = Owner::new();
        let _state = bind_state(&owner, "list");
        let outcome = scroll_state(Some(&owner), "list").unwrap();
        assert_eq!(outcome.tag, "list");
        assert_eq!(outcome.offset, ScrollAxisPair { x: 0, y: 0 });
        assert_eq!(outcome.max, ScrollAxisPair { x: 0, y: 0 });
        assert!(outcome.edges.at_top);
        assert!(outcome.edges.at_bottom);
        assert!(outcome.edges.at_left);
        assert!(outcome.edges.at_right);
    }

    #[test]
    fn r602_mid_scroll_reports_neither_top_nor_bottom() {
        let owner = Owner::new();
        let state = bind_state(&owner, "list");
        state.set_max(0, 480);
        state.scroll_to(0, 240);
        let outcome = scroll_state(Some(&owner), "list").unwrap();
        assert_eq!(outcome.offset, ScrollAxisPair { x: 0, y: 240 });
        assert_eq!(outcome.max, ScrollAxisPair { x: 0, y: 480 });
        assert!(!outcome.edges.at_top);
        assert!(!outcome.edges.at_bottom);
    }

    #[test]
    fn r602_at_bottom_reports_true_when_offset_equals_max() {
        let owner = Owner::new();
        let state = bind_state(&owner, "list");
        state.set_max(0, 480);
        state.scroll_to(0, 480);
        let outcome = scroll_state(Some(&owner), "list").unwrap();
        assert!(!outcome.edges.at_top);
        assert!(outcome.edges.at_bottom);
    }

    #[test]
    fn r602_at_top_reports_true_when_offset_zero_with_room_to_scroll() {
        let owner = Owner::new();
        let state = bind_state(&owner, "list");
        state.set_max(0, 480);
        // offset is still (0, 0) after set_max — clamped to bound.
        let outcome = scroll_state(Some(&owner), "list").unwrap();
        assert!(outcome.edges.at_top);
        assert!(!outcome.edges.at_bottom);
    }

    #[test]
    fn r602_horizontal_axis_independent_of_vertical() {
        // Set up a 2-axis scrolling state, position mid-x but at top.
        let owner = Owner::new();
        let state = bind_state(&owner, "list");
        state.set_max(600, 480);
        state.scroll_to(300, 0);
        let outcome = scroll_state(Some(&owner), "list").unwrap();
        assert_eq!(outcome.offset, ScrollAxisPair { x: 300, y: 0 });
        assert_eq!(outcome.max, ScrollAxisPair { x: 600, y: 480 });
        assert!(outcome.edges.at_top);
        assert!(!outcome.edges.at_left);
        assert!(!outcome.edges.at_right);
    }

    // ─────────────────────────────────────────────────────────────────
    // Side-effect contract
    // ─────────────────────────────────────────────────────────────────

    #[test]
    fn r602_call_does_not_insert_a_new_cache_slot() {
        let owner = Owner::new();
        let _ = scroll_state(Some(&owner), "phantom").unwrap_err();
        // The Owner::cache_contains gate must not have promoted the
        // failed lookup into a bound slot.
        assert!(!owner.cache_contains::<ScrollState>("phantom"));
    }

    #[test]
    fn r602_call_is_idempotent_two_consecutive_snapshots_match() {
        let owner = Owner::new();
        let state = bind_state(&owner, "list");
        state.set_max(0, 100);
        state.scroll_to(0, 50);
        let a = scroll_state(Some(&owner), "list").unwrap();
        let b = scroll_state(Some(&owner), "list").unwrap();
        assert_eq!(a, b);
    }

    // ─────────────────────────────────────────────────────────────────
    // JSON serialization shape (wire pin)
    // ─────────────────────────────────────────────────────────────────

    #[test]
    fn r602_outcome_serializes_to_expected_keys() {
        let owner = Owner::new();
        let state = bind_state(&owner, "list");
        state.set_max(0, 480);
        state.scroll_to(0, 240);
        let outcome = scroll_state(Some(&owner), "list").unwrap();
        let json = serde_json::to_value(outcome).unwrap();
        assert_eq!(json["tag"], "list");
        assert_eq!(json["offset"]["x"], 0);
        assert_eq!(json["offset"]["y"], 240);
        assert_eq!(json["max"]["x"], 0);
        assert_eq!(json["max"]["y"], 480);
        assert_eq!(json["edges"]["at_top"], false);
        assert_eq!(json["edges"]["at_bottom"], false);
        assert_eq!(json["edges"]["at_left"], true);
        assert_eq!(json["edges"]["at_right"], true);
    }

    // ─────────────────────────────────────────────────────────────────
    // R609 §5.45 — set_scroll_offset setter
    // ─────────────────────────────────────────────────────────────────

    fn params_at(tag: &str, x: i32, y: i32) -> SetScrollOffsetParams<'_> {
        SetScrollOffsetParams { tag, x, y }
    }

    #[test]
    fn r609_set_scroll_offset_missing_runtime_owner_errors() {
        let err = set_scroll_offset(None, &params_at("list", 0, 100)).unwrap_err();
        assert_eq!(err, ScrollStateError::RuntimeOwnerUnavailable);
    }

    #[test]
    fn r609_set_scroll_offset_unbound_tag_errors_with_tag_echoed() {
        let owner = Owner::new();
        let err = set_scroll_offset(Some(&owner), &params_at("ghost", 0, 100)).unwrap_err();
        assert_eq!(
            err,
            ScrollStateError::NotBound {
                tag: "ghost".into(),
            },
        );
    }

    #[test]
    fn r609_set_scroll_offset_writes_offset_and_returns_post_state() {
        let owner = Owner::new();
        let state = bind_state(&owner, "list");
        state.set_max(0, 480);
        let outcome = set_scroll_offset(Some(&owner), &params_at("list", 0, 240)).unwrap();
        assert_eq!(outcome.tag, "list");
        assert_eq!(outcome.offset, ScrollAxisPair { x: 0, y: 240 });
        assert_eq!(outcome.max, ScrollAxisPair { x: 0, y: 480 });
        assert!(!outcome.edges.at_top);
        assert!(!outcome.edges.at_bottom);
        // The provider's own state reflects the mutation immediately.
        assert_eq!(state.offset(), (0, 240));
    }

    #[test]
    fn r609_set_scroll_offset_clamps_overshoot_to_max() {
        // Request y=999 against max=480 → ScrollState clamps to 480
        // and the outcome echoes the post-clamp value (480), not the
        // request. Pinned so AI agents can rely on the outcome being
        // the real post-state.
        let owner = Owner::new();
        let state = bind_state(&owner, "list");
        state.set_max(0, 480);
        let outcome = set_scroll_offset(Some(&owner), &params_at("list", 0, 999)).unwrap();
        assert_eq!(outcome.offset, ScrollAxisPair { x: 0, y: 480 });
        assert!(outcome.edges.at_bottom);
        assert_eq!(state.offset(), (0, 480));
    }

    #[test]
    fn r609_set_scroll_offset_clamps_negative_to_zero() {
        let owner = Owner::new();
        let state = bind_state(&owner, "list");
        state.set_max(0, 480);
        state.scroll_to(0, 240);
        let outcome = set_scroll_offset(Some(&owner), &params_at("list", 0, -100)).unwrap();
        assert_eq!(outcome.offset, ScrollAxisPair { x: 0, y: 0 });
        assert!(outcome.edges.at_top);
    }

    #[test]
    fn r609_set_scroll_offset_supports_both_axes() {
        let owner = Owner::new();
        let state = bind_state(&owner, "list");
        state.set_max(600, 480);
        let outcome = set_scroll_offset(Some(&owner), &params_at("list", 300, 240)).unwrap();
        assert_eq!(outcome.offset, ScrollAxisPair { x: 300, y: 240 });
        assert!(!outcome.edges.at_left);
        assert!(!outcome.edges.at_right);
        assert!(!outcome.edges.at_top);
        assert!(!outcome.edges.at_bottom);
        assert_eq!(state.offset(), (300, 240));
    }

    #[test]
    fn r609_set_scroll_offset_does_not_insert_a_new_cache_slot() {
        let owner = Owner::new();
        let _ = set_scroll_offset(Some(&owner), &params_at("phantom", 0, 50)).unwrap_err();
        assert!(!owner.cache_contains::<ScrollState>("phantom"));
    }

    #[test]
    fn r609_set_scroll_offset_is_idempotent_when_same_target() {
        let owner = Owner::new();
        let state = bind_state(&owner, "list");
        state.set_max(0, 480);
        let a = set_scroll_offset(Some(&owner), &params_at("list", 0, 240)).unwrap();
        let b = set_scroll_offset(Some(&owner), &params_at("list", 0, 240)).unwrap();
        assert_eq!(a, b);
        assert_eq!(state.offset(), (0, 240));
    }

    #[test]
    fn r609_set_scroll_offset_outcome_serializes_to_full_scroll_state_shape() {
        let owner = Owner::new();
        let state = bind_state(&owner, "list");
        state.set_max(0, 480);
        let outcome = set_scroll_offset(Some(&owner), &params_at("list", 0, 480)).unwrap();
        let json = serde_json::to_value(outcome).unwrap();
        // Wire shape = ScrollStateOutcome — same as read side.
        assert_eq!(json["tag"], "list");
        assert_eq!(json["offset"]["y"], 480);
        assert_eq!(json["max"]["y"], 480);
        assert_eq!(json["edges"]["at_bottom"], true);
    }

    #[test]
    fn r609_set_scroll_offset_subscribers_re_run_once_per_two_axis_write() {
        // ScrollState::scroll_to wraps both signal writes in
        // `reactive::batch` — a subscriber that reads both axes
        // re-runs at most once per call. R55.G.5.fix substrate
        // contract carried through the RPC write path.
        use pinion_core::reactive::Effect;
        use std::cell::Cell;
        use std::rc::Rc;
        let owner = Owner::new();
        let runs = Rc::new(Cell::new(0u32));
        let runs_clone = runs.clone();
        owner.run(|| {
            let _state = pinion_core::widgets::scroll::use_scroll_state("list");
            let _effect = Effect::new(&owner, move || {
                let s = pinion_core::widgets::scroll::use_scroll_state("list");
                let _ = s.offset();
                runs_clone.set(runs_clone.get() + 1);
            });
            let baseline = runs.get();
            // Pre-set max so the two-axis write is non-trivial.
            let state = pinion_core::widgets::scroll::use_scroll_state("list");
            state.set_max(600, 480);
            let pre_swap = runs.get();
            let _ = set_scroll_offset(Some(&owner), &params_at("list", 300, 240)).unwrap();
            assert_eq!(
                runs.get(),
                pre_swap + 1,
                "scroll_to coalesces both axis writes into one Effect re-run",
            );
            // baseline + 2 = baseline run + set_max run + set_scroll_offset
            // run. The set_scroll_offset is verified as +1 above.
            assert!(runs.get() > baseline);
        });
    }
}
