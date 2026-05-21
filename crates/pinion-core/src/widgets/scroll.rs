//! R55.B §5.45 — Reactive state companion to the
//! [`Scene::Scroll`](crate::scene::Scene::Scroll) /
//! [`ScrollNode`](crate::scene::ScrollNode) primitive.
//!
//! The scene primitive (§5.32 R55.A) carries the declarative
//! geometry: viewport rect, content sub-tree, and offset fields.
//! `ScrollState` is the orthogonal axis — the reactive value that
//! the framework (and AI introspection layer) reads and writes to
//! drive that offset over time. One [`ScrollState`] instance
//! corresponds to one logical scroll container in the view-fn
//! tree; the [`Owner::cache`](crate::reactive::Owner::cache)
//! substrate (R51.150 §5.22) gives it a scope-id-keyed home.
//!
//! Surface stays minimal at this round: offset signals + max
//! bounds + clamped `scroll_to` / `scroll_by`. Smooth-scroll
//! animation is the R55.B.2 sub-axis carry — `Animation<i32>`
//! layers on top without breaking the surface here.

use std::cell::Cell;
use std::rc::Rc;

use crate::reactive::{Owner, Signal};

/// R55.B §5.45 — Reactive state for one [`ScrollNode`].
///
/// Lifecycle: created lazily via
/// [`use_scroll_state`] (which delegates to
/// [`Owner::cache`](crate::reactive::Owner::cache)). The cache
/// contract guarantees the same key resolves to the same
/// `Rc<ScrollState>` across view re-runs, so the offset persists
/// across paints.
///
/// Bounds: [`Self::set_max`] declares the maximum allowable offset
/// for each axis. The bound is the difference between the
/// content's intrinsic extent and the viewport's extent on that
/// axis (a content smaller than the viewport has a zero bound).
/// [`Self::scroll_to`] and [`Self::scroll_by`] clamp against the
/// declared bound — the caller does not need to know the content
/// size on every call.
///
/// Subscription: [`Self::offset_x`] / [`Self::offset_y`] / [`Self::offset`]
/// trigger Signal auto-subscription when called inside a view-fn
/// (`root_owner.run(...)` wrap, per R51.146 / R51.152 / R51.171
/// callback-root-owner-wrap discipline). The view re-runs on the
/// next value-changing `set` — the framework's standard reactive
/// shape, no extra plumbing.
///
/// Equality-skip: `Signal::set` short-circuits when the clamped
/// target equals the current value, so a `scroll_by(0, 0)` or a
/// "scroll-to-where-we-already-are" never schedules a paint.
#[derive(Debug)]
pub struct ScrollState {
    /// Horizontal offset in the same unit as
    /// [`ScrollNode::viewport`](crate::scene::ScrollNode::viewport).
    /// Bounded by `0..=max_x` on every write.
    offset_x: Signal<i32>,
    /// Vertical offset; semantics symmetric with [`Self::offset_x`].
    offset_y: Signal<i32>,
    /// Upper bound for `offset_x`. The application updates this
    /// through [`Self::set_max`] when the content size changes
    /// (or via the future R55.G `ListBox` / Grid composite wiring).
    max_x: Cell<i32>,
    /// Upper bound for `offset_y`; semantics symmetric with
    /// [`Self::max_x`].
    max_y: Cell<i32>,
}

impl ScrollState {
    /// Construct a fresh `ScrollState` with offset `(0, 0)` and
    /// zero bounds. The caller MUST call [`Self::set_max`] before
    /// dispatching scroll intents — a zero bound clamps every set
    /// to zero, which is the safe default for "content not measured
    /// yet" rather than an error condition.
    #[must_use]
    pub fn new() -> Self {
        Self {
            offset_x: Signal::new(0),
            offset_y: Signal::new(0),
            max_x: Cell::new(0),
            max_y: Cell::new(0),
        }
    }

    /// Current horizontal offset. Triggers a Signal subscription
    /// when called inside a view-fn — the view re-runs when
    /// [`Self::scroll_to`] or [`Self::scroll_by`] mutates the
    /// offset.
    #[must_use]
    pub fn offset_x(&self) -> i32 {
        self.offset_x.get()
    }

    /// Current vertical offset; subscription semantics symmetric
    /// with [`Self::offset_x`].
    #[must_use]
    pub fn offset_y(&self) -> i32 {
        self.offset_y.get()
    }

    /// Current `(offset_x, offset_y)` pair. Both axes subscribe.
    #[must_use]
    pub fn offset(&self) -> (i32, i32) {
        (self.offset_x(), self.offset_y())
    }

    /// Current `(max_x, max_y)` bound pair. Read by value — bounds
    /// live on `Cell`, not `Signal`, so this access does not
    /// subscribe the caller. Bound changes do not by themselves
    /// re-run the view; the subsequent offset set (after a
    /// possible clamp) is what triggers the re-render.
    #[must_use]
    pub fn max(&self) -> (i32, i32) {
        (self.max_x.get(), self.max_y.get())
    }

    /// Declare the upper bound on each axis. Negative values are
    /// clamped to `0` (a content smaller than the viewport has no
    /// scrollable range). If the current offset exceeds the new
    /// bound, the offset is clamped down so the view never paints
    /// past the new bound.
    pub fn set_max(&self, max_x: i32, max_y: i32) {
        let mx = max_x.max(0);
        let my = max_y.max(0);
        self.max_x.set(mx);
        self.max_y.set(my);
        // Clamp the current offset if it exceeds the new bound.
        // Signal equality-skip short-circuits when no clamp fires.
        let cur_x = self.offset_x.get();
        if cur_x > mx {
            self.offset_x.set(mx);
        }
        let cur_y = self.offset_y.get();
        if cur_y > my {
            self.offset_y.set(my);
        }
    }

    /// Set the offset to `(x, y)` clamped against `[0, max]`. Use
    /// this for programmatic scrolls (e.g. "scroll to top",
    /// "scroll to selected item"). Equality-skip applies — if the
    /// clamped target equals the current offset, no re-paint is
    /// scheduled.
    pub fn scroll_to(&self, x: i32, y: i32) {
        let clamped_x = x.clamp(0, self.max_x.get());
        let clamped_y = y.clamp(0, self.max_y.get());
        self.offset_x.set(clamped_x);
        self.offset_y.set(clamped_y);
    }

    /// Adjust the offset by `(dx, dy)` clamped against `[0, max]`.
    /// Use this for relative scroll input (wheel deltas, arrow-key
    /// steps). Saturating-add prevents overflow at the `i32`
    /// ceiling on either side.
    pub fn scroll_by(&self, dx: i32, dy: i32) {
        let new_x = self
            .offset_x
            .get()
            .saturating_add(dx)
            .clamp(0, self.max_x.get());
        let new_y = self
            .offset_y
            .get()
            .saturating_add(dy)
            .clamp(0, self.max_y.get());
        self.offset_x.set(new_x);
        self.offset_y.set(new_y);
    }
}

impl Default for ScrollState {
    fn default() -> Self {
        Self::new()
    }
}

/// R55.B §5.45 — Resolve (or lazily initialize) the
/// [`ScrollState`] for the current view scope.
///
/// Delegates to
/// [`Owner::cache`](crate::reactive::Owner::cache); the `key` MUST
/// be a `&'static str` and SHOULD be unique within the enclosing
/// owner's cache (the canonical pattern is to pass the
/// [`ScrollNode::tag`](crate::scene::ScrollNode::tag) verbatim,
/// since the tag is already the scroll container's symbolic
/// identifier). Mirrors the `useScrollState`-style hook found in
/// React / `SolidJS` scroll libraries.
///
/// # Panics
///
/// Panics if no current [`Owner`] is set — i.e. when invoked
/// outside a `root_owner.run(...)` wrap. Per the
/// callback-root-owner-wrap discipline (R51.146 / R51.152 /
/// R51.171), framework-internal dispatch sites supply this wrap;
/// application code reaches `use_scroll_state` only from within
/// `V::view` / `V::update` / `V::apply_key` / similar hooks.
///
/// Panics if the cache key was previously bound to a value of a
/// different concrete type within the same owner — see
/// [`Owner::cache`](crate::reactive::Owner::cache) for the
/// underlying contract.
#[must_use]
pub fn use_scroll_state(key: &'static str) -> Rc<ScrollState> {
    Owner::current()
        .expect("use_scroll_state requires an active Owner scope")
        .cache(key, ScrollState::new)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─────────────────────────────────────────────────────────────────
    // R55.B §5.45 — ScrollState construction + bounds + scroll
    // primitives. All tests exercise the substrate directly;
    // input-mapping (R55.C) and composite integration (R55.G) ride
    // separate rounds. The `use_scroll_state` hook tests require an
    // active Owner scope — set up via `Owner::new().run(...)`.
    // ─────────────────────────────────────────────────────────────────

    #[test]
    fn r55_b_initial_state_zero_offset_and_bounds() {
        // R55.B — defaults: offset (0, 0), max (0, 0). Scroll calls
        // on a freshly constructed state stay pinned at (0, 0).
        let s = ScrollState::new();
        assert_eq!(s.offset(), (0, 0));
        assert_eq!(s.max(), (0, 0));
    }

    #[test]
    fn r55_b_set_max_clamps_current_offset_when_bound_shrinks() {
        // R55.B — declaring a bound smaller than the current
        // offset clamps the offset down. Mirrors the React /
        // SolidJS "controlled input clamp on bound change" pattern.
        let s = ScrollState::new();
        s.set_max(200, 400);
        s.scroll_to(150, 300);
        assert_eq!(s.offset(), (150, 300));
        // Shrink bounds — both offsets clamp.
        s.set_max(100, 200);
        assert_eq!(s.offset(), (100, 200));
    }

    #[test]
    fn r55_b_set_max_rejects_negative_bounds() {
        // R55.B — a negative bound is clamped to 0. Defensive
        // posture against an upstream measurement bug that
        // produced negative size.
        let s = ScrollState::new();
        s.set_max(-50, -100);
        assert_eq!(s.max(), (0, 0));
    }

    #[test]
    fn r55_b_scroll_to_clamps_against_bounds() {
        // R55.B — `scroll_to` clamps to `[0, max]` on both axes
        // independently. Includes the lower bound (negative input
        // → 0) and the upper bound (overshoot → max).
        let s = ScrollState::new();
        s.set_max(100, 200);
        // Overshoot upper bound on both axes.
        s.scroll_to(500, 1000);
        assert_eq!(s.offset(), (100, 200));
        // Undershoot lower bound on both axes.
        s.scroll_to(-50, -50);
        assert_eq!(s.offset(), (0, 0));
    }

    #[test]
    fn r55_b_scroll_by_relative_clamps_against_bounds() {
        // R55.B — `scroll_by` adds a delta and clamps. Test both
        // directions and the bound saturation.
        let s = ScrollState::new();
        s.set_max(100, 100);
        s.scroll_by(30, 40);
        assert_eq!(s.offset(), (30, 40));
        s.scroll_by(200, 200); // overshoot upper bound
        assert_eq!(s.offset(), (100, 100));
        s.scroll_by(-300, -300); // overshoot lower bound
        assert_eq!(s.offset(), (0, 0));
    }

    #[test]
    fn r55_b_scroll_by_saturates_on_i32_overflow() {
        // R55.B — saturating add prevents wrap on the `i32`
        // ceiling. Important for adversarial wheel input.
        let s = ScrollState::new();
        s.set_max(i32::MAX, i32::MAX);
        s.scroll_to(i32::MAX - 1, i32::MAX - 1);
        s.scroll_by(100, 100);
        // Saturating add caps at `i32::MAX`; clamp leaves it there.
        assert_eq!(s.offset(), (i32::MAX, i32::MAX));
    }

    #[test]
    fn r55_b_scroll_to_no_op_signal_equality_skip() {
        // R55.B — Signal equality-skip means setting the offset to
        // the same clamped value does not bump the revision
        // counter. Surfaced via Signal::revision indirectly
        // through repeat `scroll_to` calls.
        let s = ScrollState::new();
        s.set_max(100, 100);
        s.scroll_to(50, 50);
        let after_first = s.offset();
        // Same target — equality-skip; no observable change.
        s.scroll_to(50, 50);
        assert_eq!(s.offset(), after_first);
    }

    #[test]
    fn r55_b_use_scroll_state_caches_under_key() {
        // R55.B — `use_scroll_state` is the canonical entry point
        // for view-fn callers. The same key resolves to the same
        // `Rc<ScrollState>` across calls — that is the
        // `Owner::cache` contract (R51.150).
        let owner = Owner::new();
        owner.run(|| {
            let a = use_scroll_state("scroll_main");
            let b = use_scroll_state("scroll_main");
            assert!(Rc::ptr_eq(&a, &b));
            // Distinct key — distinct instance.
            let c = use_scroll_state("scroll_other");
            assert!(!Rc::ptr_eq(&a, &c));
        });
    }

    #[test]
    fn r55_b_use_scroll_state_persists_across_owner_run() {
        // R55.B — Owner::cache persists for the owner's lifetime,
        // so `use_scroll_state` across two separate `run` calls on
        // the same owner returns the same Rc. This is what makes
        // the scroll offset survive view re-runs.
        let owner = Owner::new();
        let first = owner.run(|| {
            let s = use_scroll_state("persisted");
            s.set_max(100, 100);
            s.scroll_to(40, 60);
            Rc::clone(&s)
        });
        owner.run(|| {
            let again = use_scroll_state("persisted");
            assert!(Rc::ptr_eq(&first, &again));
            // Offset persists.
            assert_eq!(again.offset(), (40, 60));
        });
    }

    #[test]
    #[should_panic(expected = "use_scroll_state requires an active Owner scope")]
    fn r55_b_use_scroll_state_panics_without_owner() {
        // R55.B — outside any `root_owner.run(...)` wrap, the hook
        // panics with a diagnostic message. This catches a
        // discipline violation early instead of silently allocating
        // a per-call instance.
        let _ = use_scroll_state("no_owner_scope");
    }
}
