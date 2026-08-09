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
//!
//! ## Revealing appended content — which sibling to reach for
//!
//! "Content grew at the tail; take the viewport there" needs the bound
//! *after* the growth, which [`ScrollState::scroll_to`] cannot clamp against
//! until someone writes it. There are exactly two sources for that bound, and
//! a consumer belongs to whichever one it can compute:
//!
//! | extent is | reach for | bound comes from |
//! |---|---|---|
//! | arithmetic (`count × row_pitch`) | [`follow_tail`](crate::widgets::virtual_list::follow_tail) | the caller, via [`max_scroll_offset`], *now* |
//! | layout-measured (wrapped prose, mixed widgets) | [`ScrollState::follow_measured_tail`] | the layout pass, *next* |
//!
//! Nothing else is different — both are the same grow-then-pin idiom, both
//! leave the "was I following?" decision ([`at_bottom`](crate::widgets::virtual_list::at_bottom))
//! with the caller. R1445 added the second row because a consumer whose extent
//! only taffy/parley knows had no way to name the bound, and hand-rolled
//! `set_max(0, i32::MAX)` + `scroll_to(0, i32::MAX)` to dodge the clamp —
//! publishing a bound that is *false for a frame* to every other reader of the
//! same state.

use std::cell::Cell;
use std::rc::Rc;

use crate::reactive::{Owner, Signal, batch};
use crate::scene::ScrollAxis;

/// R55.B §5.45 — Reactive state for one [`ScrollNode`](crate::scene::ScrollNode).
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
    /// (R774 §5.27) Measured viewport width in the same unit as
    /// [`ScrollNode::viewport`](crate::scene::ScrollNode::viewport),
    /// written by the runtime layout pass from the *flex-computed*
    /// clip-window rect. Zero until the first layout pass measures
    /// the container ("not measured yet"), exactly like
    /// [`Self::max_x`]'s zero-bound default.
    ///
    /// This is the read side of the `AutoSizer` feedback loop
    /// (react-window's `AutoSizer`): a flex-sized scroll container
    /// does not know its own pixel extent until taffy lays it out,
    /// but the windowing math
    /// ([`compute_visible_range`](crate::widgets::virtual_list::compute_visible_range))
    /// runs in the *view fn*, before layout. The layout pass writes
    /// the measured extent here via [`Self::set_measured_viewport`];
    /// a flex-viewport consumer reads it back with
    /// [`Self::measured_viewport`] so the *next* view re-run windows
    /// against the true height. The first-paint chicken-and-egg is
    /// resolved by the same scroll-dirty same-frame re-pass that
    /// [`Self::set_max`] uses (R57.X §5.45).
    measured_w: Signal<u32>,
    /// (R774 §5.27) Measured viewport height; semantics symmetric
    /// with [`Self::measured_w`]. This is the axis the windowing
    /// math consumes — `viewport_h` feeds
    /// [`compute_visible_range`](crate::widgets::virtual_list::compute_visible_range),
    /// so a taller laid-out container materializes more rows.
    measured_h: Signal<u32>,
    /// (R1445 §5.45 §5.27) One-shot "pin to the tail the next layout pass
    /// measures", armed by [`Self::follow_measured_tail`] and consumed by
    /// [`Self::apply_measured_tail_pin`].
    ///
    /// A [`Cell`], not a [`Signal`]: this is an *intent in flight*, not
    /// observable state. A view that re-ran on it would re-run on a value the
    /// layout pass clears in the same frame — and the pin's effect (the
    /// offset) is already a Signal write every reader subscribes to. The RPC
    /// wire publishes the bit (`scene/scroll`'s `following_measured_tail`) so
    /// an agent can see a standing arming, which matters exactly when the
    /// scroll node is *absent* from this frame's scene (a hidden tab): no
    /// layout pass reaches it, so the arming stands until the node comes back.
    pending_tail_pin: Cell<bool>,
    /// (R51.190 §5.45) Canonical input-router / introspection tag
    /// for this scroll container. Set by [`use_scroll_state`] from
    /// the `Owner::cache` key so the matching [`ScrollNode`](crate::scene::ScrollNode) can
    /// derive its [`ScrollNode::tag`](crate::scene::ScrollNode::tag)
    /// in one call (via
    /// [`ScrollNode::from_state`](crate::scene::ScrollNode::from_state))
    /// rather than the caller repeating the string literal across
    /// `use_scroll_state(key)` + `ScrollNode::with_tag(key)`.
    ///
    /// `None` for states constructed via [`Self::new`] directly
    /// (test fixtures, manual wiring) — the matching `ScrollNode`
    /// then carries no tag unless the caller chains
    /// [`ScrollNode::with_tag`](crate::scene::ScrollNode::with_tag) explicitly.
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
            measured_w: Signal::new(0),
            measured_h: Signal::new(0),
            pending_tail_pin: Cell::new(false),
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
    ///
    /// (R57.X.scrollbar §5.45) Returns `true` when either the `max_x`
    /// or `max_y` Signal actually mutated — i.e. when at least one
    /// of the per-axis [`Signal::set`] calls landed past its
    /// equality-skip. The shell substrate's first-paint warmup uses
    /// this bit (bubbled through
    /// `update_scroll_state_bounds`)
    /// to detect the chicken-and-egg case where `V::view` ran with
    /// the pre-layout `max = 0` snapshot. The offset-clamp Signal
    /// writes (which fire only when the new bound shrinks the live
    /// offset) are intentionally excluded from the dirty bit —
    /// `set_max` callers care about "did the *bound* move", not
    /// "did the clamp fire", and the clamp is already its own
    /// observable Signal write.
    #[allow(
        clippy::must_use_candidate,
        reason = "the returned bool is the post-Signal-equality-skip \
                  dirty bit consumed by the shell substrate's first- \
                  paint warmup; setup paths (tests, manual seeding \
                  via use_scroll_state factories) deliberately ignore \
                  it. Forcing every caller to `let _ = …` would punish \
                  the common case for the substrate edge case."
    )]
    pub fn set_max(&self, max_x: i32, max_y: i32) -> bool {
        let mx = max_x.max(0);
        let my = max_y.max(0);
        let revisions_before = (self.max_x.revision(), self.max_y.revision());
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
        self.max_x.revision() != revisions_before.0 || self.max_y.revision() != revisions_before.1
    }

    /// (R774 §5.27) Current `(measured_w, measured_h)` — the
    /// flex-computed clip-window extent the layout pass last wrote
    /// via [`Self::set_measured_viewport`]. Subscribes both axes
    /// when called inside a view-fn, so a flex-viewport consumer
    /// re-runs the view when the container resizes (window drag,
    /// splitter drag, parent-flex redistribution).
    ///
    /// `(0, 0)` before the first layout pass. A flex-viewport
    /// virtualized list windows against the height axis: the first
    /// paint renders an empty window (height `0` → no rows), the
    /// layout pass measures the flex extent and writes it here, and
    /// the scroll-dirty same-frame re-pass re-runs the view with the
    /// true height — the same first-paint warmup [`Self::set_max`]
    /// relies on.
    #[must_use]
    pub fn measured_viewport(&self) -> (u32, u32) {
        (self.measured_w.get(), self.measured_h.get())
    }

    /// (R774 §5.27) Publish the measured viewport extent. Called by
    /// the runtime layout pass
    /// (`update_scroll_state_bounds`)
    /// with the flex-computed clip-window rect — the `AutoSizer` write
    /// side paired with the [`Self::measured_viewport`] read side.
    ///
    /// Returns `true` when either axis Signal actually mutated past
    /// its equality-skip, mirroring [`Self::set_max`]'s dirty bit.
    /// The shell substrate ORs this into the same first-paint warmup
    /// accumulator so a flex-viewport list re-windows on the frame
    /// the extent is first measured (or whenever a resize moves it),
    /// not one frame late. Wrapped in [`batch`] so the two per-axis
    /// writes collapse into one notification cascade — the atomic
    /// multi-axis reactive contract [`Self::set_max`] established.
    ///
    /// Steady-state frames with an unchanged extent are a no-op
    /// (Signal equality-skip), so a fixed-size scroll container —
    /// whose laid-out rect never moves — writes once on first paint
    /// and never schedules an extra re-pass thereafter.
    #[allow(
        clippy::must_use_candidate,
        reason = "the returned bool is the measured-viewport dirty bit \
                  consumed by the shell's first-paint warmup (OR'd with \
                  the set_max bit); test / manual-seeding paths ignore \
                  it, matching set_max."
    )]
    pub fn set_measured_viewport(&self, w: u32, h: u32) -> bool {
        let revisions_before = (self.measured_w.revision(), self.measured_h.revision());
        batch(|| {
            self.measured_w.set(w);
            self.measured_h.set(h);
        });
        self.measured_w.revision() != revisions_before.0
            || self.measured_h.revision() != revisions_before.1
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

    /// R1445 §5.45 §5.27 — the **layout-measured sibling** of
    /// [`follow_tail`](crate::widgets::virtual_list::follow_tail): arm a
    /// one-shot "content just grew at the tail; take the viewport to whatever
    /// bottom the next layout pass measures".
    ///
    /// `follow_tail` computes the post-growth bound itself, which requires the
    /// extent to be arithmetic (`count × row_pitch`). When the extent is only
    /// known *after* taffy/parley have run — a wrapped prose transcript, a
    /// column of mixed widgets — the caller cannot name the bound at all, and
    /// [`Self::scroll_to`] would clamp against the pre-growth one. This defers
    /// the pin to the layout pass instead of asking the caller to invent a
    /// bound: the runtime already writes the true bound every frame
    /// (`update_scroll_state_bounds`), so the arming rides along and
    /// [`Self::apply_measured_tail_pin`] fires immediately after it.
    ///
    /// **One-shot.** It is a *reducer*, not a mode — call it once per append,
    /// exactly as `follow_tail` is called once per append. Re-arming before it
    /// is spent is idempotent. R1458 — "one-shot" is about the *arming*, not
    /// about a single layout pass: the arming is spent by the first pass whose
    /// pin has nothing left to move, which for a windowed list is a pass or two
    /// after the first one reaches the node (see
    /// [`Self::apply_measured_tail_pin`]). Once spent, a reader who scrolls
    /// away is never pulled back.
    ///
    /// **The policy stays with the caller.** `follow_tail` parameterizes
    /// "should we follow?" as `was_following`; here the arming call *is* that
    /// parameter — a `tail -f` view arms it only when
    /// [`at_bottom`](crate::widgets::virtual_list::at_bottom) held *before*
    /// the append, while a view whose appended line is the answer to what the
    /// user just pressed arms it unconditionally.
    ///
    /// Call from a reducer / `update` / `External::invoke` — the sanctioned
    /// places to mutate reactive state — not from a view fn.
    pub fn follow_measured_tail(&self) {
        self.pending_tail_pin.set(true);
    }

    /// R1445 §5.45 — whether a [`Self::follow_measured_tail`] arming is still
    /// standing. `false` at every *settled* frame boundary for a scroll node
    /// that is in the scene; `true` between the arming call and the pass that
    /// spends it — or indefinitely for a node no layout pass reaches (a hidden
    /// tab), which is the case worth asking about.
    ///
    /// R1458 — "settled", not "reached". A pass that still moves the offset
    /// leaves the arming standing (see
    /// [`Self::apply_measured_tail_pin`]), so a frame whose bound is still
    /// being refined reports `true` between its passes. It goes `false` on the
    /// pass where the pin has nothing left to move.
    #[must_use]
    pub fn is_following_measured_tail(&self) -> bool {
        self.pending_tail_pin.get()
    }

    /// R1445 §5.45 — apply a standing [`Self::follow_measured_tail`] arming:
    /// pin the offset to the bound that is now published on this state.
    ///
    /// Called by the runtime layout pass (`apply_measured_tail_pins`) *after*
    /// every bound writer of the frame has run — [`Self::set_max`] from the
    /// laid-out content rect, and the measured-row
    /// [`harvest`](crate::widgets::measured_rows::MeasuredRowState::harvest)
    /// that may refine it further. That ordering is the whole guarantee: the
    /// pin lands on whatever bound *this pass* ended with, so it never needs to
    /// know which writer produced it.
    ///
    /// `axis` is the owning [`ScrollNode`](crate::scene::ScrollNode)'s
    /// declared axis — the node already says which axes scroll, so "the tail"
    /// is read off that declaration rather than re-decided here. The
    /// cross-axis offset is preserved (on a single-axis scroll the cross bound
    /// is `0` anyway, so preserving it and resetting it agree).
    ///
    /// # The arming is spent by a pin that moves nothing
    ///
    /// R1458 — one pass is not enough to know the tail. A **windowed** list
    /// only measures the rows the view materialized, so the bound the first
    /// pass publishes still counts the un-materialized tail at its estimate:
    /// the pin lands short, the view re-runs at the new offset, the harvest
    /// measures the rows that just came into the window, and only *then* is
    /// the bound the real one. Spending the arming on the first pass froze the
    /// viewport at that provisional bound — short by the whole refinement, with
    /// nothing left armed to carry it the rest of the way (the defect the tide
    /// field report measured as `543/693`).
    ///
    /// So the arming survives any pass whose pin still moved the offset, and is
    /// spent by the first pass where the pin has nothing to move. That is the
    /// same fixed point the caller is already iterating to — the returned bool
    /// *is* the frame's "not settled yet" bit — so the arming costs no extra
    /// pass: it clears on the pass the frame was going to run anyway. It stays
    /// one-shot in the sense that matters (a reader who scrolls away after it
    /// clears is never yanked back); it is simply spent on the settled bound
    /// rather than the first one.
    ///
    /// Returns whether the offset actually moved (post Signal equality-skip),
    /// mirroring [`Self::set_max`]'s dirty bit: the frame folds it into the
    /// same-frame re-pass so the paint carries the pinned offset instead of
    /// showing the pre-pin position for one frame. An arming that lands where
    /// the viewport already sits reports `false` — nothing observable changed,
    /// so nothing needs re-running, and there is nothing left to converge to.
    ///
    /// # What "re-pass" means here, and why a repaint is not it
    ///
    /// R1458 — that dirty bit buys a re-run of the **view fn**, not a second
    /// trip through the renderer, and the difference is the whole reason it is
    /// wired this way.
    /// [`ScrollNode::from_state`](crate::scene::ScrollNode::from_state) copies
    /// this state's offset into the node **when the view builds it**, and the
    /// paint adapter draws the copy. So a scene that was built before the pin
    /// landed carries the pre-pin offset in its own fields: painting it again,
    /// however many times, reproduces the same picture. Only building the
    /// scene again reads the new offset. A consumer who diagnoses "the pin
    /// landed but the screen did not move" and reaches for "request one more
    /// repaint" is reaching for the one remedy that cannot work; what the
    /// frame owes is another view + layout pass, which is what the returned
    /// bit asks for.
    #[must_use]
    pub fn apply_measured_tail_pin(&self, axis: ScrollAxis) -> bool {
        if !self.pending_tail_pin.get() {
            return false;
        }
        let (target_x, target_y) = match axis {
            ScrollAxis::Vertical => (self.offset_x.get(), self.max_y.get()),
            ScrollAxis::Horizontal => (self.max_x.get(), self.offset_y.get()),
            ScrollAxis::Both => (self.max_x.get(), self.max_y.get()),
        };
        let revisions_before = (self.offset_x.revision(), self.offset_y.revision());
        self.scroll_to(target_x, target_y);
        let moved = self.offset_x.revision() != revisions_before.0
            || self.offset_y.revision() != revisions_before.1;
        if !moved {
            self.pending_tail_pin.set(false);
        }
        moved
    }
}

impl Default for ScrollState {
    fn default() -> Self {
        Self::new()
    }
}

/// R996 §5.45 §5.27 — the maximum scroll offset for a content extent inside a
/// viewport: `content.saturating_sub(viewport)` clamped into `i32`
/// (`i32::MAX` for an extent that overflows the axis). A content smaller than
/// the viewport has no scrollable range, so the bound is `0`.
///
/// This is the single source of truth for the "how far can this axis scroll"
/// arithmetic. The runtime layout pass writes both
/// [`ScrollState`] bounds through it from the laid-out content rect (the
/// authoritative per-frame bound), and any application code that must know the
/// bound *before* the next layout pass — e.g. a streaming view that appends
/// rows and pins the viewport to the new tail in the same frame — computes the
/// identical value through it, so the two never diverge (the layout pass then
/// re-affirms the same number, and [`ScrollState::set_max`]'s Signal
/// equality-skip makes that a no-op).
///
/// R1445 — that pre-layout path presumes the caller *can* state its content
/// extent. A consumer whose extent is layout-measured (wrapped prose, mixed
/// widgets) cannot, and must not fake one: arm
/// [`ScrollState::follow_measured_tail`] and let the layout pass pin against
/// the bound it measures.
#[must_use]
pub fn max_scroll_offset(content: u32, viewport: u32) -> i32 {
    i32::try_from(content.saturating_sub(viewport)).unwrap_or(i32::MAX)
}

/// R1620 §5.45 §5.35 — **auto-scroll**: how a scroll region keeps moving while a
/// pointer holds a button near its edge.
///
/// A drag-select reaches the addresses it can see, and no further: the pointer
/// leaves the viewport and the rows below it are never entered, so the sweep
/// stops at the last painted one. Auto-scroll is what makes a drag able to
/// select more than a screenful — the reference names it `autoScroll` +
/// `autoScrollMargin` + a `startAutoScroll` entry point on its abstract item
/// view, and no view without it can select past its own bottom edge.
///
/// ## Speed is a function of the POINTER, not of elapsed time
///
/// This is the one place the design deliberately parts from the reference,
/// which was read rather than assumed. There, a counter starts at zero when
/// auto-scroll begins and increments by one per timer tick (capped at the page
/// step), and THAT counter is the per-tick scroll distance. So the speed
/// depends only on how long the drag has been going: a user who wants to move
/// faster cannot, and one who overshoots must wait for the ramp to restart.
/// The pointer's position inside the margin is read as a boolean — in, or out.
///
/// Here the margin is a **ramp**: at its inner edge the speed is zero and at
/// the viewport boundary it is [`max_speed`](Self::max_speed), linearly
/// between. Pushing further out goes faster, and easing back slows down, which
/// is the behaviour every drawing tool, timeline and code editor has and the
/// gesture people already know. It also removes a whole state: there is no ramp
/// counter to reset, because speed is a pure function of where the pointer is.
///
/// ## And it is in px/s against a real delta
///
/// The reference scrolls a fixed number of items per timer tick (150 ms per
/// item, 50 ms per pixel — hard-coded, with a source comment wishing it were a
/// style hint), so its speed is whatever the timer manages to deliver. This
/// carries a velocity and is integrated against the frame's own `dt`, so a
/// slow frame scrolls the same distance a fast one does and the gesture feels
/// the same at 30 fps and 144.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AutoScroll {
    /// The edge band, in logical pixels, inside which a held pointer scrolls.
    /// Zero disables auto-scroll for this region entirely — an explicit
    /// "off" rather than a separate boolean, because a zero-wide ramp IS no
    /// ramp and two spellings of off would need a rule about disagreement.
    pub margin: f64,
    /// The speed, in logical pixels per second, reached when the pointer is at
    /// the viewport boundary (and beyond it — the ramp saturates rather than
    /// running away when the pointer leaves the window).
    pub max_speed: f64,
}

impl AutoScroll {
    /// The default edge band: 16 logical pixels, the same width the reference
    /// picked, so a gesture tuned against one feels the same against the other.
    pub const DEFAULT_MARGIN: f64 = 16.0;

    /// The default top speed: 720 logical pixels per second — a 40-px row every
    /// ~56 ms at full push, which is about the reference's per-item cadence at
    /// its ramp's midpoint, reached here by pushing rather than by waiting.
    pub const DEFAULT_MAX_SPEED: f64 = 720.0;

    /// Auto-scroll off: no band, so no held pointer ever scrolls this region.
    #[must_use]
    pub const fn off() -> Self {
        Self {
            margin: 0.0,
            max_speed: 0.0,
        }
    }

    /// Whether this policy can ever scroll. A zero (or negative) band or a
    /// non-positive speed is off.
    #[must_use]
    pub fn is_enabled(self) -> bool {
        self.margin > 0.0 && self.max_speed > 0.0
    }

    /// The signed speed, in logical px/s, for a pointer at `pos` along an axis
    /// whose viewport spans `[lo, hi)`. Negative scrolls toward the origin
    /// (the pointer is near `lo`), positive away from it; `0.0` when the
    /// pointer is in the middle, or when this policy is off.
    ///
    /// Saturates outside the viewport rather than accelerating without bound:
    /// a pointer dragged far past the edge — or off the window entirely — asks
    /// for [`max_speed`](Self::max_speed) and no more. Without that clamp the
    /// distance travelled in one frame would depend on how far outside the
    /// window the user happened to be, which is not a control anyone is aiming.
    #[must_use]
    pub fn speed_at(self, pos: f64, lo: f64, hi: f64) -> f64 {
        if !self.is_enabled() || hi <= lo {
            return 0.0;
        }
        // A band wider than half the viewport would overlap itself in the
        // middle and make every position scroll. Clamp so a small region
        // degrades to "the two halves" instead of behaving unpredictably.
        let margin = self.margin.min((hi - lo) / 2.0);
        let depth = if pos < lo + margin {
            // Toward the origin: how far INTO the band, as a fraction.
            -((lo + margin - pos) / margin)
        } else if pos > hi - margin {
            (pos - (hi - margin)) / margin
        } else {
            0.0
        };
        self.max_speed * depth.clamp(-1.0, 1.0)
    }
}

impl Default for AutoScroll {
    /// On, with the default band and speed: a scroll region that says nothing
    /// still lets a drag reach past its edge.
    ///
    /// Defaulting ON is the fail-safe direction here, matching the reference
    /// (whose property defaults true). A region that auto-scrolls when it need
    /// not is a gesture the user can simply not make; one that does not when it
    /// should is a selection they cannot express at all.
    fn default() -> Self {
        Self {
            margin: Self::DEFAULT_MARGIN,
            max_speed: Self::DEFAULT_MAX_SPEED,
        }
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
/// the web UI library / `SolidJS` scroll libraries.
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
        // offset clamps the offset down. Mirrors the web UI library /
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

    // ─────────────────────────────────────────────────────────────────
    // R1445 §5.45 §5.27 — the layout-measured tail pin. `follow_tail`
    // (virtual_list) serves consumers whose extent is arithmetic
    // (`count × row_pitch`); this serves the ones whose extent only
    // taffy/parley know. Same grow-then-pin idiom — what differs is who
    // names the bound and when, so these tests all encode the frame
    // ORDER: the arming is made while the post-growth bound is still
    // unknown, and resolves against whatever is written afterwards.
    // ─────────────────────────────────────────────────────────────────

    #[test]
    fn r1445_pin_lands_on_the_bound_written_after_the_arming() {
        // The crux. The consumer appended content whose extent it cannot
        // compute, so it names no bound at all — it arms, and the layout
        // pass's `set_max` (below) is the first time the true extent exists.
        let s = ScrollState::new();
        s.set_max(0, 100); // last frame's measured bound
        s.scroll_to(0, 0); // reading from the top
        s.follow_measured_tail();
        assert_eq!(s.offset(), (0, 0), "arming alone moves nothing");
        s.set_max(0, 260); // THIS frame's layout write, post-append
        assert!(s.apply_measured_tail_pin(ScrollAxis::Vertical));
        assert_eq!(
            s.offset(),
            (0, 260),
            "pinned to the bound that was measured after the arming",
        );
    }

    #[test]
    fn r1445_pin_is_one_shot() {
        // A reducer, not a mode: once the arming is spent, later growth must
        // leave the viewport wherever the reader put it.
        let s = ScrollState::new();
        s.set_max(0, 100);
        s.follow_measured_tail();
        assert!(s.apply_measured_tail_pin(ScrollAxis::Vertical));
        assert_eq!(s.offset(), (0, 100));
        // R1458 — the pass that moved the offset does NOT spend the arming;
        // the settled pass right behind it does. Spend it the way the frame
        // does, by running to the fixed point.
        assert!(!s.apply_measured_tail_pin(ScrollAxis::Vertical));
        assert!(!s.is_following_measured_tail(), "spent by the settled pass");
        s.set_max(0, 300); // content grew again, no new arming
        assert!(
            !s.apply_measured_tail_pin(ScrollAxis::Vertical),
            "the arming is gone: growth alone does not follow",
        );
        assert_eq!(s.offset(), (0, 100), "no re-pin without a new arming");
    }

    #[test]
    fn r1458_arming_survives_a_pass_that_was_still_moving() {
        // The R1445 defect the tide field report measured. The frame's first
        // pass publishes a PROVISIONAL bound (a windowed list has not measured
        // its tail rows yet); the pass that lands on it is exactly the pass
        // whose result the next one refines. Spending the arming there froze
        // the viewport short of the tail with nothing left armed to finish the
        // trip.
        let s = ScrollState::new();
        s.set_max(0, 100); // pass 1: the estimate-derived bound
        s.follow_measured_tail();
        assert!(s.apply_measured_tail_pin(ScrollAxis::Vertical));
        assert_eq!(s.offset(), (0, 100));
        assert!(
            s.is_following_measured_tail(),
            "the pin moved, so the frame is still converging and the arming stands",
        );
        s.set_max(0, 260); // pass 2: the harvest refined it
        assert!(s.apply_measured_tail_pin(ScrollAxis::Vertical));
        assert_eq!(s.offset(), (0, 260), "carried the rest of the way");
        assert!(!s.apply_measured_tail_pin(ScrollAxis::Vertical), "settled");
        assert!(!s.is_following_measured_tail(), "and spent there");
    }

    #[test]
    fn r1445_arming_twice_before_a_pass_is_idempotent() {
        // Two appends between two paints (a burst) arm twice; the arming is one
        // bit, so the single pin lands on the final bound and one settled pass
        // clears it however many times it was made.
        let s = ScrollState::new();
        s.follow_measured_tail();
        s.follow_measured_tail();
        s.set_max(0, 80);
        assert!(s.apply_measured_tail_pin(ScrollAxis::Vertical));
        assert_eq!(s.offset(), (0, 80));
        assert!(!s.apply_measured_tail_pin(ScrollAxis::Vertical));
        assert!(
            !s.is_following_measured_tail(),
            "one settled pass clears the arming however many times it was made",
        );
    }

    #[test]
    fn r1445_axis_declares_which_edge_is_the_tail() {
        // The owning ScrollNode already declares which axes scroll; the pin
        // reads the tail off that declaration instead of re-deciding, and
        // leaves the cross axis where it was.
        let v = ScrollState::new();
        v.set_max(50, 200);
        v.scroll_to(20, 0);
        v.follow_measured_tail();
        assert!(v.apply_measured_tail_pin(ScrollAxis::Vertical));
        assert_eq!(v.offset(), (20, 200), "vertical tail; x preserved");

        let h = ScrollState::new();
        h.set_max(50, 200);
        h.scroll_to(0, 30);
        h.follow_measured_tail();
        assert!(h.apply_measured_tail_pin(ScrollAxis::Horizontal));
        assert_eq!(h.offset(), (50, 30), "horizontal tail; y preserved");

        let b = ScrollState::new();
        b.set_max(50, 200);
        b.follow_measured_tail();
        assert!(b.apply_measured_tail_pin(ScrollAxis::Both));
        assert_eq!(b.offset(), (50, 200), "a 2-D canvas pins its far corner");
    }

    #[test]
    fn r1458_a_shrinking_bound_cannot_clamp_without_reporting_dirt() {
        // R1458's registered debt D2 read `set_max`'s doc — the offset-clamp
        // writes are "intentionally excluded from the dirty bit" — and
        // concluded that a frame could therefore paint the pre-clamp offset.
        // REFUTED, and this is the test that refutes it rather than prose:
        // every offset writer (`scroll_to` / `scroll_by`, and the tail pin
        // through `scroll_to`) clamps against the LIVE bound, so `offset <=
        // max` is an invariant between calls. The clamp inside `set_max` can
        // therefore only fire when THIS call lowered the bound past the
        // offset — and lowering the bound is exactly what the dirty bit
        // reports. The exclusion is real but unreachable on its own.
        let s = ScrollState::new();
        s.set_max(0, 500);
        s.scroll_to(0, 400);
        assert_eq!(s.offset(), (0, 400));

        // The bound shrinks past the reader: the clamp fires AND the frame is
        // told, because the same call moved the bound.
        assert!(s.set_max(0, 300), "the bound moved, so the frame re-runs");
        assert_eq!(s.offset(), (0, 300), "and the offset came with it");

        // The control that makes the claim above load-bearing: re-affirming
        // the same bound reports clean, and there is nothing left to clamp —
        // so "clean" never hides a moved offset.
        assert!(!s.set_max(0, 300), "a re-affirmed bound is not a change");
        assert_eq!(s.offset(), (0, 300), "and nothing moved behind that");

        // The invariant the argument rests on, stated as a check: no writer
        // can park the offset past the bound for a later `set_max` to clamp
        // silently.
        s.scroll_to(0, 9_999);
        assert_eq!(s.offset(), (0, 300), "scroll_to clamps against the bound");
        s.scroll_by(0, 9_999);
        assert_eq!(s.offset(), (0, 300), "so does scroll_by");
    }

    #[test]
    fn r1445_pin_that_moves_nothing_reports_no_dirt() {
        // Following while already at the tail is the common case (a `tail -f`
        // view that never left the bottom). The arming is still consumed, but
        // the frame must not be re-run for an offset that did not move — the
        // same post-equality-skip dirty-bit contract `set_max` publishes.
        let s = ScrollState::new();
        s.set_max(0, 120);
        s.scroll_to(0, 120);
        s.follow_measured_tail();
        assert!(!s.apply_measured_tail_pin(ScrollAxis::Vertical));
        assert!(!s.is_following_measured_tail(), "consumed nonetheless");
        assert_eq!(s.offset(), (0, 120));
    }

    #[test]
    fn r1445_content_that_still_fits_pins_to_a_zero_bound() {
        // An append that leaves the content shorter than the viewport has
        // nowhere to go: `max` stays 0 and the pin is a no-op. The consumer
        // does not special-case this — it arms unconditionally.
        let s = ScrollState::new();
        s.follow_measured_tail();
        assert!(!s.apply_measured_tail_pin(ScrollAxis::Vertical));
        assert_eq!(s.offset(), (0, 0));
    }

    #[test]
    fn r1445_standing_arming_is_readable() {
        // The read side of the write (wire-form read/write symmetry): what
        // `scene/scroll` publishes as `following_measured_tail`.
        let s = ScrollState::new();
        assert!(!s.is_following_measured_tail());
        s.follow_measured_tail();
        assert!(
            s.is_following_measured_tail(),
            "standing until a pass reaches this state's node",
        );
        let _ = s.apply_measured_tail_pin(ScrollAxis::Vertical);
        assert!(!s.is_following_measured_tail());
    }

    #[test]
    fn r1445_pin_re_runs_a_subscribed_effect_once() {
        use crate::reactive::Effect;
        use std::cell::Cell;

        // The pin writes the offset through `scroll_to`, so it inherits the
        // atomic-batch contract: one observable change, not two per-axis ones.
        let s = ScrollState::new();
        s.set_max(0, 200);
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
        s_eff.follow_measured_tail();
        assert_eq!(runs.get(), 1, "arming is not observable state");
        assert!(s_eff.apply_measured_tail_pin(ScrollAxis::Vertical));
        assert_eq!(runs.get(), 2, "the pin re-runs the subscriber once");
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
