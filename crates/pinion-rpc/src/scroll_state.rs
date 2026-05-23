//! `scene/scroll_state` RPC method dispatch — R602 §5.45 + §5.7.
//!
//! Projects the [`ScrollState`] cached on the substrate's root
//! [`Owner`] under the supplied `tag` so AI agents can verify scroll
//! position without resorting to [`scene/snapshot`] pixel diffs.
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
use pinion_core::widgets::scroll::{use_scroll_state, ScrollState};
use serde::Serialize;

/// Typed errors the [`scroll_state`] dispatcher can return. Every
/// variant maps onto a JSON-RPC `-32602 Invalid params` at the
/// dispatch layer with the variant name in `error.data`.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScrollStateError {
    /// No [`runtime_owner`](crate::DispatchContext) registered on
    /// the dispatch context.
    RuntimeOwnerUnavailable,
    /// `params.tag` was missing — required because scroll states do
    /// not share a canonical default tag (each widget uses its own).
    TagRequired,
    /// The owner has no [`ScrollState`] cached under `tag` yet. The
    /// application typically binds it on the first view-fn run via
    /// [`use_scroll_state`].
    NotBound { tag: String },
}

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ScrollStateOutcome {
    /// Echoes the request's `params.tag` so the response self-identifies.
    pub tag: &'static str,
    /// Current `(offset_x, offset_y)`.
    pub offset: ScrollAxisPair,
    /// Current `(max_x, max_y)` bound — the upper clamp on `offset`.
    pub max: ScrollAxisPair,
    /// Per-axis "at this edge" predicates derived from `offset` / `max`.
    pub edges: ScrollEdges,
}

impl ScrollStateOutcome {
    fn from_state(tag: &'static str, state: &ScrollState) -> Self {
        let (ox, oy) = state.offset();
        let (mx, my) = state.max();
        Self {
            tag,
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
/// `tag` must be supplied — there is no canonical default. The
/// `&'static str` requirement matches
/// [`use_scroll_state`](pinion_core::widgets::scroll::use_scroll_state)'s
/// [`Owner::cache`] key; callers leak the tag through
/// [`Box::leak`] when bridging from a JSON-RPC `String`.
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
/// None. The [`Owner::cache_contains`] gate routes the no-slot case
/// to [`ScrollStateError::NotBound`] so the call never creates an
/// uninitialised state on a typo'd tag. The `Owner::run` wrap only
/// activates [`Owner::current`] for the [`use_scroll_state`] hook;
/// no reactive computation is established.
pub fn scroll_state(
    runtime_owner: Option<&Owner>,
    tag: &'static str,
) -> Result<ScrollStateOutcome, ScrollStateError> {
    let Some(owner) = runtime_owner else {
        return Err(ScrollStateError::RuntimeOwnerUnavailable);
    };
    if !owner.cache_contains::<ScrollState>(tag) {
        return Err(ScrollStateError::NotBound {
            tag: tag.to_owned(),
        });
    }
    let state: std::rc::Rc<ScrollState> = owner.run(|| use_scroll_state(tag));
    Ok(ScrollStateOutcome::from_state(tag, &state))
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
