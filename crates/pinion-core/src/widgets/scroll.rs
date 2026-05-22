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

use std::rc::Rc;

use crate::reactive::{Owner, Signal, batch};

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
    /// (or via the runtime `compute_layout` pass for the
    /// layout-driven path landed in R55.G.5).
    ///
    /// (R55.G.5.fix §5.45 — R55.G.5 follow-up) The bound lives on a
    /// `Signal`, not a `Cell`. The R55.G.5 round wrote the bound
    /// from the runtime layout pass but the reader (a sibling
    /// scrollbar peer in the view-fn — see
    /// [`hello-listbox`](https://docs.rs/hello-listbox) R55.D.4)
    /// still saw a `Cell`, so the first paint after the layout
    /// pass produced a thumb sized against the still-zero bound.
    /// Promoting the bound to a `Signal` makes
    /// [`Self::max`] subscribe its caller, so the application's
    /// next view re-runs with the freshly-laid bound. Signal's
    /// equality-skip ensures a no-op `set_max` does not re-trigger
    /// the view.
    max_x: Signal<i32>,
    /// Upper bound for `offset_y`; semantics symmetric with
    /// [`Self::max_x`].
    max_y: Signal<i32>,
    /// (R51.190 §5.45) Canonical input-router / introspection tag
    /// for this scroll container. Set by [`use_scroll_state`] from
    /// the `Owner::cache` key so the matching [`ScrollNode`] can
    /// derive its [`ScrollNode::tag`](crate::scene::ScrollNode::tag)
    /// in one call (via
    /// [`ScrollNode::from_state`](crate::scene::ScrollNode::from_state))
    /// rather than the caller repeating the string literal across
    /// `use_scroll_state(key)` + `ScrollNode::with_tag(key)`.
    ///
    /// `None` for states constructed via [`Self::new`] directly
    /// (test fixtures, manual wiring) — the matching `ScrollNode`
    /// then carries no tag unless the caller chains
    /// [`ScrollNode::with_tag`] explicitly.
    tag: Option<&'static str>,
}

impl ScrollState {
    /// Construct a fresh `ScrollState` with offset `(0, 0)` and
    /// zero bounds. The caller MUST call [`Self::set_max`] before
    /// dispatching scroll intents — a zero bound clamps every set
    /// to zero, which is the safe default for "content not measured
    /// yet" rather than an error condition.
    ///
    /// (R51.190 §5.45) The tag is left `None`. Most application
    /// code reaches `ScrollState` via [`use_scroll_state`] (which
    /// calls [`Self::with_tag`] under the hood); direct callers
    /// are typically tests + manual fixtures where the tag carries
    /// no useful information.
    #[must_use]
    pub fn new() -> Self {
        Self {
            offset_x: Signal::new(0),
            offset_y: Signal::new(0),
            max_x: Signal::new(0),
            max_y: Signal::new(0),
            tag: None,
        }
    }

    /// (R51.190 §5.45) Construct a `ScrollState` tagged with `key`.
    /// Used by [`use_scroll_state`] as the [`Owner::cache`] factory
    /// so the [`ScrollNode::from_state`](crate::scene::ScrollNode::from_state)
    /// convenience can derive the matching node's
    /// [`ScrollNode::tag`](crate::scene::ScrollNode::tag) without
    /// the caller repeating the string literal.
    #[must_use]
    pub fn with_tag(key: &'static str) -> Self {
        Self {
            tag: Some(key),
            ..Self::new()
        }
    }

    /// (R51.190 §5.45) Canonical tag for this scroll container.
    /// Returns the `key` passed to [`use_scroll_state`] (or
    /// [`Self::with_tag`]); `None` for states constructed via
    /// [`Self::new`] directly. Read by
    /// [`ScrollNode::from_state`](crate::scene::ScrollNode::from_state)
    /// to wire `ScrollNode::tag` automatically.
    #[must_use]
    pub fn tag(&self) -> Option<&'static str> {
        self.tag
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

    /// Current `(max_x, max_y)` bound pair. Subscribes both axes
    /// when called inside a view-fn — the view re-runs when
    /// [`Self::set_max`] mutates either bound (Signal equality-skip
    /// short-circuits when the new bound equals the current one).
    ///
    /// (R55.G.5.fix §5.45) Pre-R55.G.5.fix this read did not
    /// subscribe — the bound lived on a `Cell`. The follow-up
    /// promoted both bounds to `Signal<i32>` so that the layout
    /// pass writing the post-measure bound during the first paint
    /// drives a clean view re-run on the next frame; the textbook
    /// reactive shape for any value that is read from a view-fn
    /// and written from outside.
    #[must_use]
    pub fn max(&self) -> (i32, i32) {
        (self.max_x.get(), self.max_y.get())
    }

    /// Declare the upper bound on each axis. Negative values are
    /// clamped to `0` (a content smaller than the viewport has no
    /// scrollable range). If the current offset exceeds the new
    /// bound, the offset is clamped down so the view never paints
    /// past the new bound.
    ///
    /// (R55.G.5.fix §5.45) Wrapped in [`batch`] so the per-axis
    /// signal writes (`max_x`, `max_y`, and the optional offset
    /// clamps) collapse into one notification cascade. Without the
    /// batch a subscribed `Effect` / `Owner` re-ran once per signal
    /// touched (two to four times for one `set_max` call) — the
    /// textbook reactive contract for an atomic multi-axis write
    /// is "one observable change".
    pub fn set_max(&self, max_x: i32, max_y: i32) {
        let mx = max_x.max(0);
        let my = max_y.max(0);
        batch(|| {
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
        });
    }

    /// Set the offset to `(x, y)` clamped against `[0, max]`. Use
    /// this for programmatic scrolls (e.g. "scroll to top",
    /// "scroll to selected item"). Equality-skip applies — if the
    /// clamped target equals the current offset, no re-paint is
    /// scheduled.
    ///
    /// (R55.G.5.fix §5.45) Wrapped in [`batch`] so the `offset_x`
    /// and `offset_y` writes collapse into one notification
    /// cascade — same atomic-update reactive contract that
    /// [`Self::set_max`] established. Subscribers see "one scroll
    /// landed" instead of two interleaved single-axis writes.
    pub fn scroll_to(&self, x: i32, y: i32) {
        let clamped_x = x.clamp(0, self.max_x.get());
        let clamped_y = y.clamp(0, self.max_y.get());
        batch(|| {
            self.offset_x.set(clamped_x);
            self.offset_y.set(clamped_y);
        });
    }

    /// Adjust the offset by `(dx, dy)` clamped against `[0, max]`.
    /// Use this for relative scroll input (wheel deltas, arrow-key
    /// steps). Saturating-add prevents overflow at the `i32`
    /// ceiling on either side.
    ///
    /// (R55.G.5.fix §5.45) Wrapped in [`batch`] for the same atomic
    /// reactive contract as [`Self::set_max`] / [`Self::scroll_to`].
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
        batch(|| {
            self.offset_x.set(new_x);
            self.offset_y.set(new_y);
        });
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
    // (R51.190 §5.45) The factory closure captures `key` so the
    // cached `ScrollState` records its own tag —
    // [`ScrollNode::from_state`](crate::scene::ScrollNode::from_state)
    // then derives the matching node's
    // [`ScrollNode::tag`](crate::scene::ScrollNode::tag) without
    // the caller repeating the string. Pre-R51.190 the factory was
    // the no-arg [`ScrollState::new`]; the closure shape is the
    // canonical way to bind extra arguments to an [`Owner::cache`]
    // factory.
    Owner::current()
        .expect("use_scroll_state requires an active Owner scope")
        .cache(key, || ScrollState::with_tag(key))
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

    // ─────────────────────────────────────────────────────────────────
    // R55.G.5.fix §5.45 — `max` bounds live on `Signal`, not `Cell`.
    // The reactive contract `[[textbook-long-term-correct]]` requires
    // that any value read from a view-fn and written from outside
    // (here: the runtime `compute_layout` pass, R55.G.5) re-runs the
    // view on change. Pre-fix the bound lived on a `Cell`, so the
    // first paint after layout produced a thumb sized against the
    // stale zero bound — the [[hello-listbox]] R55.D.4 scrollbar
    // visual was the surfacing consumer. The reactive test pins the
    // new contract: `set_max` re-runs an `Effect` that read the
    // bound, exactly like `scroll_to` already does for the offset.
    // ─────────────────────────────────────────────────────────────────

    #[test]
    fn r55_g5_fix_set_max_triggers_subscribed_effect() {
        use crate::reactive::Effect;
        use std::cell::Cell;

        // R55.G.5.fix — set_max with a *changed* bound re-runs an
        // effect that subscribed via `max()`. Cell-counter pattern
        // mirrors the [[effect-driven-driver-monotonic-counter]] use.
        let s = ScrollState::new();
        let owner = Owner::new();
        let runs = Rc::new(Cell::new(0u32));
        let runs_eff = Rc::clone(&runs);
        let s_eff = Rc::new(s);
        let s_inside = Rc::clone(&s_eff);
        let _eff = Effect::new(&owner, move || {
            let _ = s_inside.max();
            runs_eff.set(runs_eff.get() + 1);
        });
        // Effect fires once on creation (the eager subscribe sweep).
        assert_eq!(runs.get(), 1, "effect runs once on create");
        // Layout pass writes a non-zero bound → effect re-runs ONCE
        // (atomic batch collapses the per-axis cascade).
        s_eff.set_max(100, 200);
        assert_eq!(runs.get(), 2, "set_max atomic re-runs effect once");
        // Same bound — Signal equality-skip; effect must NOT re-run.
        s_eff.set_max(100, 200);
        assert_eq!(runs.get(), 2, "equal set_max equality-skips");
        // Change just one axis — effect re-runs once (the changed
        // signal notifies; equal-axis short-circuits).
        s_eff.set_max(100, 300);
        assert_eq!(runs.get(), 3, "one-axis change re-runs effect once");
    }

    #[test]
    fn r55_g5_fix_scroll_to_atomic_re_runs_effect_once() {
        use crate::reactive::Effect;
        use std::cell::Cell;

        // R55.G.5.fix — `scroll_to` is an atomic two-axis write.
        // Subscribers that read both axes (`offset()`) must see one
        // notification, not two interleaved single-axis writes.
        let s = ScrollState::new();
        s.set_max(100, 200);
        let owner = Owner::new();
        let runs = Rc::new(Cell::new(0u32));
        let runs_eff = Rc::clone(&runs);
        let s_eff = Rc::new(s);
        let s_inside = Rc::clone(&s_eff);
        let _eff = Effect::new(&owner, move || {
            let _ = s_inside.offset();
            runs_eff.set(runs_eff.get() + 1);
        });
        assert_eq!(runs.get(), 1, "effect runs once on create");
        // Both axes change — collapses to a single re-run.
        s_eff.scroll_to(20, 40);
        assert_eq!(runs.get(), 2, "atomic scroll_to re-runs once");
        // Same target — equality-skip; no re-run.
        s_eff.scroll_to(20, 40);
        assert_eq!(runs.get(), 2, "no-op scroll_to skips");
        // One-axis change — still a single re-run.
        s_eff.scroll_to(20, 60);
        assert_eq!(runs.get(), 3, "one-axis scroll_to re-runs once");
    }

    #[test]
    fn r55_g5_fix_scroll_by_atomic_re_runs_effect_once() {
        use crate::reactive::Effect;
        use std::cell::Cell;

        // R55.G.5.fix — `scroll_by` is symmetric to `scroll_to` for
        // the atomic-batch contract.
        let s = ScrollState::new();
        s.set_max(100, 200);
        let owner = Owner::new();
        let runs = Rc::new(Cell::new(0u32));
        let runs_eff = Rc::clone(&runs);
        let s_eff = Rc::new(s);
        let s_inside = Rc::clone(&s_eff);
        let _eff = Effect::new(&owner, move || {
            let _ = s_inside.offset();
            runs_eff.set(runs_eff.get() + 1);
        });
        assert_eq!(runs.get(), 1, "effect runs once on create");
        s_eff.scroll_by(10, 20);
        assert_eq!(runs.get(), 2, "atomic scroll_by re-runs once");
        // (0, 0) delta — both axes equality-skip → no re-run.
        s_eff.scroll_by(0, 0);
        assert_eq!(runs.get(), 2, "no-op scroll_by skips");
    }

    // ─────────────────────────────────────────────────────────────────
    // R51.190 §5.45 — ScrollState tag substrate. The tag field
    // closes the boilerplate gap that the R51.180-188 substrate
    // round left open: pre-R51.190 the canonical view-fn shape
    // had to repeat the key literal across `use_scroll_state(key)`
    // and `ScrollNode::with_tag(key)`. The new field lets
    // `ScrollNode::from_state` derive the tag from the state's
    // own record, eliminating the duplication.
    // ─────────────────────────────────────────────────────────────────

    #[test]
    fn r51_190_new_leaves_tag_none() {
        // R51.190 — direct construction via `ScrollState::new` is
        // the test-fixture / manual-wiring path. No tag context is
        // available; the tag stays `None`.
        let s = ScrollState::new();
        assert_eq!(s.tag(), None);
    }

    #[test]
    fn r51_190_with_tag_records_key() {
        // R51.190 — explicit constructor stores the key for later
        // retrieval. Direct callers that want the canonical
        // behaviour without going through `use_scroll_state` use
        // this entry point.
        let s = ScrollState::with_tag("explicit_key");
        assert_eq!(s.tag(), Some("explicit_key"));
    }

    #[test]
    fn r51_190_use_scroll_state_populates_tag() {
        // R51.190 — the canonical hook records its key in the
        // returned state. This is the substrate guarantee
        // `ScrollNode::from_state` relies on to derive the matching
        // node's tag in one call.
        let owner = Owner::new();
        owner.run(|| {
            let s = use_scroll_state("hooked_key");
            assert_eq!(s.tag(), Some("hooked_key"));
        });
    }
}
