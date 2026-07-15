//! R55.D §5.45 — `ScrollBar` widget catalogue entry.
//!
//! Visible scrollbar peer of a [`ScrollNode`](crate::scene::ScrollNode).
//! The axis composes in three textbook sub-rounds, each landed as
//! its own independently-testable substrate.
//!
//! **R55.D.1** — paint geometry primitive: closed-form thumb-rect
//! derivation ([`scrollbar_thumb_rect`] + [`ScrollBarOrientation`] +
//! [`ScrollBarGeometry`]). Pure function of the scroll-axis scalars,
//! no statechart and no [`ScrollState`] borrow.
//!
//! **R55.D.2** — §5.38 SCXML statechart + widget binding
//! ([`ScrollBar`] / [`ScrollBarEvent`] / [`ScrollBarState`]):
//! four-state machine (Idle / Hover / Dragging / Disabled), Slider
//! mirror (R51.14 §5.38) for the drag-style vocabulary. The drag-
//! end activate emits the `"scroll_committed"` intent on the §5.20
//! channel.
//!
//! **R55.D.3** — pointer-event routing: translates `pointer_move`
//! under capture lock into
//! [`ScrollState::scroll_to`](crate::widgets::scroll::ScrollState::scroll_to)
//! calls. R55.D.3 carry.
//!
//! ## Why a closed-form helper instead of a widget binding first?
//!
//! pinion's §5.38 widget catalog pattern is `SCXML + Rust struct +
//! External`, with paint geometry derived from the cached state. A
//! visible scrollbar is the rare case where the paint geometry is
//! a **pure function of [`ScrollState`] + track rect**, not of a
//! statechart's transition history — the thumb position is `f(scroll
//! offset, content size, viewport size, track extent)`, period.
//!
//! Splitting the axis into "geometry first / statechart second /
//! routing third" lets the first sub-round land a textbook
//! standalone primitive without the
//! [[substrate-incompleteness-signal]] of forcing the first
//! consumer to also wire up the not-yet-extracted pointer routing.
//! R55.D.2 then layered the §5.38 SCXML statechart + binding on
//! top — both pieces compose without re-engineering R55.D.1.
//!
//! ## Reference layout math
//!
//! | Symbol | Meaning |
//! |---|---|
//! | `track_extent` | length of the scrollbar track along the scroll axis (`track.h` for vertical, `track.w` for horizontal) |
//! | `track_cross` | thickness of the scrollbar perpendicular to the scroll axis |
//! | `viewport_extent` | visible content size along the scroll axis (`ScrollNode.viewport.h` / `.w`) |
//! | `content_extent` | full content size along the scroll axis (after layout) |
//! | `scroll_offset` | current `ScrollState::offset_*` value, clamped to `0..=(content - viewport)` |
//! | `min_thumb_size` | floor for thumb size so the thumb stays grabbable even on very long content (Material/UIKit convention: 24–32 px) |
//!
//! ```text
//! thumb_extent = max(min_thumb_size, floor(track_extent * viewport_extent / content_extent))
//! scroll_max   = content_extent - viewport_extent           (clamped to 0)
//! thumb_pos    = floor((track_extent - thumb_extent) * scroll_offset / scroll_max)   if scroll_max > 0
//!              = 0                                                                   if scroll_max == 0
//! ```
//!
//! Degenerate cases:
//! - `content <= viewport`: nothing to scroll — thumb fills the
//!   track, no draggable peer needed.
//! - `track_extent == 0` or `content_extent == 0`: returns a zero-
//!   extent thumb at the track origin (paint backend can elide).
//! - `scroll_offset > scroll_max`: clamped saturating-down inside
//!   the helper, so the caller does not have to pre-validate.

use crate::scene::Rect;

/// Orientation of a scrollbar track. Mirrors the W3C
/// `aria-orientation` enumeration and the §5.38 Slider axis (R51.39)
/// so a future widget binding can reuse the same primitive without
/// translating between two orientation enums.
///
/// (R55.D.7 §5.45) `#[non_exhaustive]` matches the rest of the
/// pinion enum surface ([`Scene`](crate::scene::Scene),
/// [`Display`](crate::style::Display), [`FlexDirection`](crate::style::FlexDirection),
/// etc.). The two axes are W3C-stable today, so the attribute is
/// pure forward-compat hedge — `Diagonal` / per-axis snap modes are
/// hypothetical extensions a future round can land without a
/// `SemVer` major bump.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollBarOrientation {
    /// Track grows along the y axis; thumb slides up / down. Used
    /// for vertical scroll content (the common case for list-like
    /// widgets such as `hello-listbox`).
    Vertical,
    /// Track grows along the x axis; thumb slides left / right.
    /// Used for horizontally-scrolling content (table / timeline
    /// views, the future R55.E carry).
    Horizontal,
}

/// Result of [`scrollbar_thumb_rect`] — the track that was passed
/// in (so the caller can fan it out into a single paint helper
/// without re-threading the input) plus the derived thumb rect.
/// Both rects share the same coordinate frame as the input `track`.
///
/// (R55.D.7 §5.45) `#[non_exhaustive]` matches the pinion type
/// surface convention. The current three fields cover the canonical
/// scrollbar geometry but a future round may add e.g. a `hover_zone`
/// rect (the wider hit-test area around a thin visible thumb) — the
/// attribute guards downstream callers from a `SemVer` break.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollBarGeometry {
    /// Orientation echoed back so a paint helper can branch on the
    /// axis (e.g. cap shape / hover hit) without re-deriving it.
    pub orientation: ScrollBarOrientation,
    /// Track rectangle in the caller-supplied coordinate frame.
    /// The paint helper draws this as the scrollbar rail background.
    pub track: Rect,
    /// Thumb rectangle in the same frame as [`Self::track`]. Always
    /// contained inside `track` (modulo the `min_thumb_size` floor
    /// honoring — see module docs for the degenerate cases).
    pub thumb: Rect,
}

/// R55.D.1 §5.45 — closed-form thumb-rect derivation.
///
/// Pure function of the input arguments — no `ScrollState` borrow,
/// no `Signal` subscription, no allocation. The caller passes the
/// authoritative scroll-axis scalars (extracted via
/// [`ScrollState::offset`](crate::widgets::scroll::ScrollState::offset)
/// and the matching [`ScrollNode`](crate::scene::ScrollNode)'s
/// viewport / content extents at layout time) and the helper
/// returns the thumb rect for the paint pass.
///
/// `min_thumb_size` is the floor (Material/UIKit convention: 24
/// px). Setting it to `0` reverts to the strict ratio-of-content
/// thumb size, which lets thumb vanish for very long content — not
/// recommended for production, but useful for unit-test math
/// verification.
///
/// See module docs for the reference math, the closed-form
/// degenerate-case table, and the rationale for splitting geometry
/// (this sub-round) from statechart + pointer routing (R55.D.2 /
/// R55.D.3 carry).
#[must_use]
pub fn scrollbar_thumb_rect(
    orientation: ScrollBarOrientation,
    track: Rect,
    viewport_extent: u32,
    content_extent: u32,
    scroll_offset: u32,
    min_thumb_size: u32,
) -> ScrollBarGeometry {
    let (track_extent, track_cross) = match orientation {
        ScrollBarOrientation::Vertical => (track.h, track.w),
        ScrollBarOrientation::Horizontal => (track.w, track.h),
    };

    // Degenerate guards: empty track or empty content → zero thumb
    // anchored at the track origin. Paint backend can elide the
    // zero-area rect; no signed-arithmetic invariants to defend.
    if track_extent == 0 || content_extent == 0 {
        let thumb = thumb_at(orientation, track, 0, 0, track_cross);
        return ScrollBarGeometry {
            orientation,
            track,
            thumb,
        };
    }

    // Nothing to scroll: thumb fills the entire track. Material /
    // UIKit / web all hide the bar entirely in this case, but the
    // helper stays paint-agnostic; the caller can match `thumb ==
    // track` to elide the paint at the composition layer.
    if content_extent <= viewport_extent {
        let thumb = thumb_at(orientation, track, 0, track_extent, track_cross);
        return ScrollBarGeometry {
            orientation,
            track,
            thumb,
        };
    }

    // Thumb size = floor(track_extent * viewport / content), with
    // floor at `min_thumb_size` and ceiling at `track_extent`. The
    // u64 widening avoids overflow on the multiplication; the
    // inputs are u32 but their product can exceed u32::MAX (e.g.
    // 100_000 viewport × 100_000 track = 10^10). The division
    // result is mathematically `<= track_extent` (since content >
    // viewport on this path), so `u32::try_from` cannot fail; the
    // `unwrap_or(track_extent)` is the unreachable saturation
    // fallback that satisfies clippy's `cast_possible_truncation`
    // gate without an explicit allow.
    let product = u64::from(track_extent) * u64::from(viewport_extent);
    let ratio_thumb = u32::try_from(product / u64::from(content_extent)).unwrap_or(track_extent);
    let thumb_extent = ratio_thumb.max(min_thumb_size).min(track_extent);

    // Scroll range and offset clamp. content > viewport guard above
    // ensures `scroll_max >= 1`.
    let scroll_max = content_extent - viewport_extent;
    let clamped_offset = scroll_offset.min(scroll_max);

    // Available travel for the thumb's top-edge / left-edge. Same
    // mathematical bound as above: `(thumb_travel * clamped_offset)
    // / scroll_max <= thumb_travel <= track_extent <= u32::MAX`, so
    // `try_from` saturates only on the unreachable overflow case.
    let thumb_travel = track_extent - thumb_extent;
    let travel_product = u64::from(thumb_travel) * u64::from(clamped_offset);
    let thumb_pos_offset =
        u32::try_from(travel_product / u64::from(scroll_max)).unwrap_or(thumb_travel);

    let thumb = thumb_at(
        orientation,
        track,
        thumb_pos_offset,
        thumb_extent,
        track_cross,
    );
    ScrollBarGeometry {
        orientation,
        track,
        thumb,
    }
}

/// Helper that materialises the thumb rectangle in the right
/// orientation from the scroll-axis pose (`offset_along` from track
/// origin + `size_along` the scroll axis) and the cross-axis
/// thickness. Extracted so the three early-return paths in
/// [`scrollbar_thumb_rect`] share one source of truth for the
/// orientation-to-rect mapping.
fn thumb_at(
    orientation: ScrollBarOrientation,
    track: Rect,
    offset_along: u32,
    size_along: u32,
    cross: u32,
) -> Rect {
    match orientation {
        ScrollBarOrientation::Vertical => Rect::new(
            track.x,
            track.y.saturating_add(offset_along),
            cross,
            size_along,
        ),
        ScrollBarOrientation::Horizontal => Rect::new(
            track.x.saturating_add(offset_along),
            track.y,
            size_along,
            cross,
        ),
    }
}

// ─────────────────────────────────────────────────────────────────
// R55.D.2 §5.45 §5.38 — `ScrollBar` widget binding.
//
// Slider mirror (R51.14 §5.38): own self-contained statechart with
// the `dragging` state vocabulary. The four-state machine
// (Idle / Hover / Dragging / Disabled) is generated from
// `widgets/scroll_bar.scxml` and surfaces through the §2 #2 RPC
// introspection layer (RPC scene/query "state" returns one of
// "Idle" / "Hover" / "Dragging" / "Disabled"). The `dragging →
// hover` transition on pointer_up emits the §5.20
// `"scroll_committed"` intent — pure drag-end marker (no payload),
// because the authoritative offset already lives in `ScrollState`
// (R55.B §5.45) and propagates to the application through that
// signal stream. The R55.D.3 sub-axis (pointer-event routing)
// will close the loop by translating pointer_move under capture
// lock into `ScrollState::scroll_to` calls.
// ─────────────────────────────────────────────────────────────────

#[allow(
    non_snake_case,
    unused_imports,
    dead_code,
    unused_variables,
    unused_mut,
    unused_labels,
    unreachable_patterns,
    unreachable_code,
    unused_assignments,
    clippy::style,
    clippy::complexity,
    clippy::pedantic,
    clippy::all
)]
mod sm {
    include!(concat!(env!("OUT_DIR"), "/scroll_bar_sm.rs"));
}

pub use sm::{ScrollBarEvent, ScrollBarState};

// R698 §5.16 — route ScrollBarState <-> SCXML-id mapping through the
// R643 `WidgetStateName` SSOT primitive, replacing the hand-written
// `scroll_bar_state_name` fn (mirrors the R696.A Disclosure adoption).
crate::widget_state_name!(
    ScrollBarState,
    default = Idle,
    [Idle, Hover, Dragging, Disabled,]
);
// R699 §5.16 — ScrollBarEvent <-> SCXML-name mapping through the
// `WidgetEventName` SSOT primitive, replacing `parse_scroll_bar_event`.
// No KeyboardActivate (the scrollbar has no ARIA Space/Enter activate).
crate::widget_event_name!(
    ScrollBarEvent,
    external = [
        PointerEnter,
        PointerLeave,
        PointerDown,
        PointerUp,
        PointerCancel,
        Disable,
        Enable,
    ],
    internal = [ScrollbarActivate, Null],
);

// ─────────────────────────────────────────────────────────────────
// R660 §5.45 — reactive interaction-state mirror
//
// View-fn code (`pinion_widget_paint::scrollbar::view_vertical_scrollbar`)
// needs the live `ScrollBarState` to compute the Material 3 thumb
// state-layer overlay (Idle = `Outline` flat; Hover = +0.08 OnSurface
// overlay; Dragging = +0.16 OnSurface overlay; Disabled fades toward
// the track tint). The framework owns the `ScrollBarExternal`
// through [`WidgetCore::create_extra_externals`], so the view fn has
// no direct reference to it — the standard pinion bridge for this
// gap is an `Owner::cache` singleton holding a reactive primitive,
// mirroring the [[owner-cache-typed-key]] pattern that
// `use_text_edit_state` / `use_caret_blink` already follow.
//
// Forge-generated `ScrollBarState` carries `Debug + Clone + Copy +
// PartialEq + Eq + Hash` but lacks `Serialize / DeserializeOwned`,
// so it cannot back a `Signal<ScrollBarState>` directly. The
// canonical workaround (and the cheapest at the wire) is a numeric-
// tag mirror: `Signal<u8>` internally, typed `get` / `set` boundary
// on the outside.
// ─────────────────────────────────────────────────────────────────

/// (R660 §5.45) Reactive mirror of a paired
/// [`ScrollBarExternal`]'s interaction state. `Owner::cache` singleton
/// (one instance per `tag` per [`Owner`]); the application reads
/// through [`Self::get`] in its view fn (auto-subscribing the
/// rendering [`Effect`] so a hover repaints the thumb tint), the
/// framework-owned [`ScrollBarExternal`] writes through [`Self::set`]
/// every SCXML transition.
///
/// # SCE-004 stop-gap — numeric-tag wrapper
///
/// Internally stores [`ScrollBarState`] as a numeric tag in a
/// `Signal<u8>` because Forge-generated `ScrollBarState` lacks the
/// `Serialize + DeserializeOwned` bounds [`Signal::new`] requires.
/// Tag mapping is stable across releases: `0 = Idle`, `1 = Hover`,
/// `2 = Dragging`, `3 = Disabled` — pinned by the unit tests at the
/// bottom of this file so a future SCXML edit cannot silently
/// re-number a variant.
///
/// This wrapper is a **stop-gap**, not the textbook canonical shape.
/// The proper fix lives in `vendor/sce`: Forge codegen should emit
/// `#[derive(serde::Serialize, serde::Deserialize)]` on every state /
/// event enum (either default, or via `CompileOptions::state_enum_derives`).
/// The vendor/sce upstream debt is tracked as **SCE-004** alongside
/// **SCE-002** ([[sce-upstream-debts]] memory). Once Forge emits the
/// serde derives, this wrapper collapses to
/// `pub type ScrollBarInteractionSignal = Signal<ScrollBarState>;`
/// and the round-trip tests retire — the cost of carrying it now is
/// the ~100-LOC scaffold + one bookkeeping rule (extend `tag_for` /
/// `state_for` whenever the SCXML adds a state).
///
/// [`Effect`]: crate::reactive::Effect
/// [`Owner`]: crate::reactive::Owner
pub struct ScrollBarInteractionSignal {
    inner: Signal<u8>,
}

impl ScrollBarInteractionSignal {
    /// Construct a fresh signal initialised to [`ScrollBarState::Idle`].
    /// Matches the SCXML's initial state so the view-fn first paint
    /// sees the same value the framework starts with.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Signal::new(Self::tag_for(ScrollBarState::Idle)),
        }
    }

    /// Read the mirrored interaction state. Subscribes the current
    /// reactive scope (the view-fn's render `Effect`) so the next
    /// state transition fires a repaint.
    #[must_use]
    pub fn get(&self) -> ScrollBarState {
        Self::state_for(self.inner.get())
    }

    /// Write the mirrored interaction state. No-op (Signal equality-
    /// skip) when the tag is unchanged, so an idle SCXML re-entry
    /// does not chum the reactive graph.
    pub fn set(&self, state: ScrollBarState) {
        self.inner.set(Self::tag_for(state));
    }

    /// Stable numeric tag for a [`ScrollBarState`]. Public only at
    /// the test boundary; production callers reach the value
    /// through [`Self::get`] / [`Self::set`].
    fn tag_for(state: ScrollBarState) -> u8 {
        match state {
            ScrollBarState::Idle => 0,
            ScrollBarState::Hover => 1,
            ScrollBarState::Dragging => 2,
            ScrollBarState::Disabled => 3,
        }
    }

    /// Inverse of [`Self::tag_for`]. Unknown tags collapse to
    /// `Idle` — defensive against a corrupted snapshot / restore
    /// payload from a future schema bump.
    fn state_for(tag: u8) -> ScrollBarState {
        match tag {
            1 => ScrollBarState::Hover,
            2 => ScrollBarState::Dragging,
            3 => ScrollBarState::Disabled,
            _ => ScrollBarState::Idle,
        }
    }
}

impl Default for ScrollBarInteractionSignal {
    fn default() -> Self {
        Self::new()
    }
}

impl core::fmt::Debug for ScrollBarInteractionSignal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ScrollBarInteractionSignal")
            .field("state", &self.get())
            .finish()
    }
}

/// (R660 §5.45) [`Owner::cache`] hook returning the shared
/// [`ScrollBarInteractionSignal`] for `tag`. Same shape as
/// [`use_text_edit_state`](crate::widgets::text_edit::use_text_edit_state) —
/// per-`tag` singleton, lives for the [`Owner`]'s
/// lifetime, returned through an [`Rc`] so the application can clone
/// it once to thread into both [`ScrollBarExternal::attach_interaction`]
/// (the write side) and the view fn (the read side).
///
/// # Panics
///
/// Panics if there is no active [`Owner`]
/// scope. Framework-internal dispatch sites already supply the
/// `root_owner.run()` wrap; application code only reaches this hook
/// from inside `V::view` / `V::create_extra_externals` / similar
/// callbacks where the wrap is guaranteed.
#[must_use]
pub fn use_scrollbar_interaction(tag: &'static str) -> Rc<ScrollBarInteractionSignal> {
    Owner::current()
        .expect("use_scrollbar_interaction requires an active Owner scope")
        .cache(tag, ScrollBarInteractionSignal::new)
}
use sm::ScrollBarPolicy;

use std::rc::Rc;

use crate::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner, SchemaField, ThreadOwnership,
};
use crate::intent::Intent;
use crate::reactive::{Owner, Signal};
use crate::widgets::scroll::ScrollState;
use crate::widgets::{IntentEmitter, Widget, WidgetTransition};
use crate::{WidgetEventName, WidgetStateName};

/// `ScrollBar` widget state machine + orientation sidecar.
///
/// Slider-mirror four-state interaction model
/// (Idle / Hover / Dragging / Disabled). Unlike Slider's f32 value
/// sidecar, the authoritative scroll value lives in
/// [`ScrollState`] (R55.B
/// §5.45) — the orthogonal reactive container the framework owns.
/// `ScrollBar` therefore carries no per-state numeric sidecar; the
/// only construction-time sidecar is the
/// [`ScrollBarOrientation`] (which drives R55.D.3 pointer-event
/// routing along the correct axis and the ARIA-aligned introspect
/// `"orientation"` field).
///
/// The orientation is fixed at construction time. ARIA and the
/// R55.D.1 paint geometry both assume an immutable axis (the
/// `"scroll_committed"` intent contract and the SCXML transitions
/// stay unchanged across runtime axis flips, so no semantic value
/// is lost — and the W3C `aria-orientation` attribute is itself
/// authoring-time). Mirrors the [`SliderAxis`](crate::widgets::slider::SliderAxis)
/// R51.39 §5.38 immutability decision.
pub struct ScrollBar {
    inner: Widget<ScrollBarPolicy>,
    orientation: ScrollBarOrientation,
    /// R55.D.3 §5.45 — composition handle to the authoritative
    /// [`ScrollState`]. The `ScrollBar` is the visible peer; the
    /// state is the reactive value. Attaching is optional — a bare
    /// `ScrollBar::new()` carries no handle and `pointer_move`
    /// no-ops, which lets the R55.D.4 visible demo land its paint
    /// composition before the application wires up the reactive
    /// state (substrate-vs-application boundary).
    state: Option<Rc<ScrollState>>,
    /// R55.D.3 §5.45 — press-time snapshot driving the drag math.
    /// `None` outside the `Dragging` state. The framework's
    /// `InputRouter` opens a capture
    /// lock on `pointer_down` and forwards the press-time cursor as
    /// the first `pointer_move`; that frame captures the snapshot,
    /// every subsequent `pointer_move` applies the delta against it.
    drag_start: Option<DragStart>,
    /// R660 §5.45 — optional reactive mirror of [`Self::state`] the
    /// `send` path writes after every SCXML transition. View-fn code
    /// reads through the paired [`ScrollBarInteractionSignal::get`]
    /// to pick the Material 3 thumb state-layer overlay. `None` for
    /// bare bars (paint-only previews / unit tests that do not
    /// exercise the visual hover/drag styling).
    interaction: Option<Rc<ScrollBarInteractionSignal>>,
}

/// R55.D.3 §5.45 — drag-start snapshot. Captured on the first
/// `pointer_move` under capture lock so the delta-based drag math
/// uses the press-time origin even when the cursor strays off the
/// track rect. `scroll_max` is cached here (not re-read from
/// [`ScrollState`] every frame) so a mid-drag content insert / size
/// re-measurement does not warp the in-flight drag's mapping.
#[derive(Debug, Clone, Copy)]
struct DragStart {
    /// Cursor fraction (`0.0..=1.0` widget-relative) at press time,
    /// taken along the active [`ScrollBarOrientation`] axis.
    cursor_fraction: f32,
    /// Scroll offset at press time (along the active axis).
    offset_at_press: i32,
    /// [`ScrollState`] max bound at press time (along the active
    /// axis). Pinning this here is the textbook drag-stability
    /// guard — re-reading mid-drag would let a content insert warp
    /// the cursor↔offset mapping under the user's finger.
    scroll_max: i32,
}

impl ScrollBar {
    /// Construct a vertical `ScrollBar` in the `Idle` state. Vertical
    /// is the W3C ARIA `aria-orientation` default for the
    /// `scrollbar` role and the common case for list-like content
    /// (`hello-listbox`, future R55.E table / timeline views).
    ///
    /// No [`ScrollState`] is attached — call
    /// [`Self::attach_state`] to wire the bar to a reactive offset
    /// (R55.D.3). Bare bars are useful for paint-only previews and
    /// snapshot regression tests that do not exercise the drag
    /// routing.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Widget::new(),
            orientation: ScrollBarOrientation::Vertical,
            state: None,
            drag_start: None,
            interaction: None,
        }
    }

    /// Construct a `ScrollBar` with an explicit
    /// [`ScrollBarOrientation`]. Use [`ScrollBarOrientation::Horizontal`]
    /// for horizontally-scrolling content; the R55.D.3 pointer-
    /// event routing will then read the X cursor delta.
    #[must_use]
    pub fn with_orientation(orientation: ScrollBarOrientation) -> Self {
        Self {
            inner: Widget::new(),
            orientation,
            state: None,
            drag_start: None,
            interaction: None,
        }
    }

    /// R55.D.3 §5.45 — attach a [`ScrollState`] handle (composition).
    /// The `ScrollBar` becomes the visible peer for the state's
    /// reactive offset; subsequent `pointer_move` calls translate
    /// the cursor delta into [`ScrollState::scroll_to`] calls along
    /// the bar's orientation axis. Builder-style: chain after
    /// [`Self::new`] / [`Self::with_orientation`] for the fluent
    /// `ScrollBar::with_orientation(H).attach_state(rc)` shape.
    ///
    /// Detaching is not supported — drop the `ScrollBar` and build
    /// a fresh one instead. The `ScrollState` handle outlives the
    /// `ScrollBar` (refcounted by `Rc`), so detach/reattach mid-
    /// life would only complicate the contract without unlocking a
    /// real use case.
    #[must_use]
    pub fn attach_state(mut self, state: Rc<ScrollState>) -> Self {
        self.state = Some(state);
        self
    }

    /// R660 §5.45 — attach a [`ScrollBarInteractionSignal`] mirror so
    /// every SCXML state transition through [`Self::send`] writes the
    /// new [`ScrollBarState`] into the shared signal. View-fn code
    /// subscribes on the read side to repaint the Material 3 thumb
    /// state-layer (`Outline` flat at Idle; `+0.08 OnSurface` at
    /// Hover; `+0.16 OnSurface` at Dragging; faded at Disabled).
    ///
    /// Builder-style; chain after [`Self::new`] / [`Self::with_orientation`]
    /// / [`Self::attach_state`]. The initial write happens here so the
    /// view fn observes the seeded `Idle` state on the very first paint
    /// (cheap — Signal equality-skip drops the write when the slot
    /// already holds `Idle`).
    #[must_use]
    pub fn attach_interaction(mut self, handle: Rc<ScrollBarInteractionSignal>) -> Self {
        handle.set(self.inner.state());
        self.interaction = Some(handle);
        self
    }

    /// Track orientation, fixed at construction. Drives the
    /// R55.D.3 `pointer_move` axis dispatch and the introspect
    /// `"orientation"` field surfaced to the AI side.
    #[must_use]
    pub fn orientation(&self) -> ScrollBarOrientation {
        self.orientation
    }

    /// R55.D.3 §5.45 — read-only access to the attached
    /// [`ScrollState`] handle. `None` until [`Self::attach_state`]
    /// fires. Diagnostic / test surface; production callers usually
    /// reach the state through the same `Rc<ScrollState>` they
    /// passed in (the R55.B [`use_scroll_state`](crate::widgets::scroll::use_scroll_state)
    /// hook returns the canonical shared handle).
    #[must_use]
    pub fn scroll_state(&self) -> Option<&Rc<ScrollState>> {
        self.state.as_ref()
    }

    /// Drive a [`ScrollBarEvent`] through the SCXML. Pure state
    /// transition — the authoritative scroll offset lives in
    /// [`ScrollState`] and is
    /// mutated through that handle, not through this widget.
    ///
    /// R55.D.3 §5.45 — any transition that exits `Dragging`
    /// (`pointer_up`, `pointer_leave`, `pointer_cancel`, `disable`)
    /// clears the press-time `DragStart` snapshot so the next
    /// press starts clean.
    pub fn send(&mut self, event: ScrollBarEvent) {
        self.inner.send(event);
        if !matches!(self.inner.state(), ScrollBarState::Dragging) {
            self.drag_start = None;
        }
        // R660 §5.45 — mirror the post-transition state into the
        // shared reactive signal so view-fn code repaints the thumb
        // with the matching M3 state-layer (no-op if no mirror was
        // attached).
        if let Some(handle) = &self.interaction {
            handle.set(self.inner.state());
        }
    }

    /// Current interaction state.
    #[must_use]
    pub fn state(&self) -> ScrollBarState {
        self.inner.state()
    }
}

impl Default for ScrollBar {
    fn default() -> Self {
        Self::new()
    }
}

/// R55.D.2 §5.45 §5.38 — `ScrollBar` transition contract. The
/// snapshot is just the SCXML state (no value sidecar — the offset
/// lives in [`ScrollState`]). Detection emits a single
/// `"scroll_committed"` intent on the `Dragging → Hover` activate
/// transition (drag-end via [`ScrollBarEvent::PointerUp`]). The
/// `Dragging → Idle` cancel paths
/// ([`ScrollBarEvent::PointerLeave`] / [`ScrollBarEvent::PointerCancel`])
/// stay silent — matches the R51.93 §5.35 Slider drag-cancel
/// convention.
impl WidgetTransition for ScrollBar {
    type Event = ScrollBarEvent;
    type Snapshot = ScrollBarState;

    fn snapshot(&self) -> Self::Snapshot {
        self.state()
    }

    fn drive(&mut self, event: Self::Event) {
        self.send(event);
    }

    fn detect(before: Self::Snapshot, _event: Self::Event, after: Self::Snapshot) -> Vec<Intent> {
        if matches!(before, ScrollBarState::Dragging) && matches!(after, ScrollBarState::Hover) {
            vec![Intent::new_static(
                crate::widgets::commit::SCROLL_COMMITTED_EVENT,
                IntrospectValue::Null,
            )]
        } else {
            Vec::new()
        }
    }
}

/// `External` adapter wrapping a [`ScrollBar`]. Emits a single
/// intent kind:
///
/// * `"scroll_committed"` (no payload) on the `Dragging → Hover`
///   activate transition — pure drag-end marker. Applications that
///   want a live preview of the scroll offset subscribe to the
///   [`ScrollState::offset_x`](crate::widgets::scroll::ScrollState::offset_x)
///   / [`ScrollState::offset_y`](crate::widgets::scroll::ScrollState::offset_y)
///   Signal stream directly (the R55.B reactive contract). The
///   `"scroll_committed"` channel is for `onChangeEnd`-style hooks
///   (persistence, analytics, …).
pub struct ScrollBarExternal {
    em: IntentEmitter<ScrollBar>,
}

impl ScrollBarExternal {
    /// Construct a vertical `ScrollBar` external. ARIA / web
    /// canonical default for the `scrollbar` role.
    #[must_use]
    pub fn new() -> Self {
        Self {
            em: IntentEmitter::default(),
        }
    }

    /// Construct a `ScrollBar` external with an explicit
    /// [`ScrollBarOrientation`]. Wraps a
    /// [`ScrollBar::with_orientation`] under the intent emitter so
    /// the R55.D.3 `pointer_move` forward reads the correct axis and
    /// the `"orientation"` introspect field reports the matching
    /// ARIA string.
    #[must_use]
    pub fn with_orientation(orientation: ScrollBarOrientation) -> Self {
        Self {
            em: IntentEmitter::new(ScrollBar::with_orientation(orientation)),
        }
    }

    /// R55.D.3 §5.45 — attach a [`ScrollState`] handle to the inner
    /// [`ScrollBar`] (composition). Builder-style; chain after
    /// [`Self::new`] / [`Self::with_orientation`] for the fluent
    /// shape. See [`ScrollBar::attach_state`] for the contract.
    #[must_use]
    pub fn attach_state(mut self, state: Rc<ScrollState>) -> Self {
        // `mem::take` keeps the builder shape (consume-and-return)
        // through the wrapping `IntentEmitter`; `ScrollBar::default`
        // is the cheap `Default::default()` round-trip (no
        // observable state — drag_start / state both `None`, SCXML
        // freshly initialized to `Idle`).
        self.em.inner = std::mem::take(&mut self.em.inner).attach_state(state);
        self
    }

    /// R660 §5.45 — attach a [`ScrollBarInteractionSignal`] reactive
    /// mirror. Delegates to [`ScrollBar::attach_interaction`]; the
    /// inner `ScrollBar::send` path writes the new state into the
    /// signal on every transition so view-fn code repaints the M3
    /// state-layer overlay (Hover / Dragging / Disabled tints).
    /// Builder-style — chain alongside [`Self::attach_state`].
    #[must_use]
    pub fn attach_interaction(mut self, handle: Rc<ScrollBarInteractionSignal>) -> Self {
        self.em.inner = std::mem::take(&mut self.em.inner).attach_interaction(handle);
        self
    }

    /// Track orientation (delegates to [`ScrollBar::orientation`]).
    /// Diagnostic / test surface; consumers usually read the
    /// introspect `"orientation"` field instead.
    #[must_use]
    pub fn orientation(&self) -> ScrollBarOrientation {
        self.em.inner.orientation()
    }

    /// R55.D.3 §5.45 — attached [`ScrollState`] handle (delegates
    /// to [`ScrollBar::scroll_state`]). `None` until
    /// [`Self::attach_state`] fires.
    #[must_use]
    pub fn scroll_state(&self) -> Option<&Rc<ScrollState>> {
        self.em.inner.scroll_state()
    }

    /// Drive a [`ScrollBarEvent`] and queue a `"scroll_committed"`
    /// intent on drag-end (`Dragging → Hover`). Pipeline lives on
    /// [`IntentEmitter::dispatch`]; the detection rule lives on the
    /// [`WidgetTransition`] impl for [`ScrollBar`].
    pub fn send(&mut self, event: ScrollBarEvent) {
        self.em.dispatch(event);
    }

    /// Current interaction state.
    #[must_use]
    pub fn state(&self) -> ScrollBarState {
        self.em.inner.state()
    }
}

impl Default for ScrollBarExternal {
    fn default() -> Self {
        Self::new()
    }
}

/// R746 §5.45 — assemble the canonical scrollbar peer
/// [`ExtraExternal`](crate::widget_core::ExtraExternal): a
/// [`ScrollBarExternal`] bound to `scroll` (so drag mutates the shared
/// offset) and to the `scrollbar_tag`'s
/// [`ScrollBarInteractionSignal`] (the M3 idle→hover→dragging state-layer
/// mirror), registered under `scrollbar_tag`.
///
/// This is the byte-identical wiring six bindings (`hello-listbox`,
/// `hello-virtual-list`, `hello-variable-list`, `hello-virtual-select`,
/// `settings-panel`, `todomvc`) repeated inline before R746 — pure
/// mechanical composition with no per-binding opinion, so it lives at the
/// substrate next to its parts ([[abstraction-needs-second-consumer]]:
/// well past the Rule of Three). The caller still owns *which*
/// [`ScrollState`] (it resolves `use_scroll_state(key)` with its own key)
/// and passes it in, mirroring how `ScrollNode::from_state` takes the
/// state rather than a key.
#[must_use]
pub fn scrollbar_extra_external(
    scroll: Rc<ScrollState>,
    scrollbar_tag: &'static str,
) -> crate::widget_core::ExtraExternal {
    crate::widget_core::ExtraExternal::new(
        scrollbar_tag,
        Box::new(
            ScrollBarExternal::new()
                .attach_state(scroll)
                .attach_interaction(use_scrollbar_interaction(scrollbar_tag)),
        ),
    )
}

impl core::fmt::Debug for ScrollBarExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ScrollBarExternal")
            .field("state", &self.state())
            .field("orientation", &self.orientation())
            .finish()
    }
}

impl External for ScrollBarExternal {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui, Backend::Rpc], BackendFallback::Skip)
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }

    /// §5.15 + §5.35 (R51.35 mirror) — opt in to capture lock so
    /// the framework's `InputRouter` keeps the cursor pinned to
    /// this `ScrollBar` for the duration of the `pointer_down` →
    /// `pointer_up` span, even when the cursor strays outside the
    /// thumb rect. Required for the canonical scrollbar drag UX:
    /// the user can drag past the track ends (or off the bar
    /// entirely) without the press cancelling.
    fn wants_pointer_capture(&self) -> bool {
        true
    }

    /// R55.D.3 §5.45 §5.15 §5.35 — translate the widget-relative
    /// cursor under capture lock into a
    /// [`ScrollState::scroll_to`](crate::widgets::scroll::ScrollState::scroll_to)
    /// call along the bar's orientation axis.
    ///
    /// * **`Vertical`** orientation: `y_rel` drives the value;
    ///   `x_rel` is ignored. Top edge (`y_rel = 0.0`) → smallest
    ///   offset, bottom edge (`y_rel = 1.0`) → `scroll_max`. Mirrors
    ///   the natural scrollbar UX where dragging the thumb down
    ///   scrolls the content down. (Unlike [`Slider`](crate::widgets::slider::Slider)'s
    ///   R51.39 inversion — Slider's vertical axis honors ARIA
    ///   `aria-orientation="vertical"` where top = max; scrollbars
    ///   follow the orthogonal "drag direction = scroll direction"
    ///   convention that web / Material / `SwiftUI` all share.)
    /// * **`Horizontal`** orientation: `x_rel` drives; `y_rel` is
    ///   ignored. Left edge → smallest offset, right edge →
    ///   `scroll_max`.
    ///
    /// The first `pointer_move` after `pointer_down` (the press-
    /// time cursor frame the framework supplies under capture lock)
    /// captures a `DragStart` snapshot — cursor fraction +
    /// [`ScrollState`] offset + `scroll_max` — and does not move
    /// the offset. Each subsequent frame applies
    /// `delta_fraction × scroll_max` to the press-time offset and
    /// dispatches `ScrollState::scroll_to`. The clamping in
    /// `scroll_to` saturates strays past the track ends, mirroring
    /// the R51.35 Slider clamp-on-overshoot contract.
    ///
    /// No-op without an attached [`ScrollState`] (the bar is in
    /// "paint-only" mode — the R55.D.4 visible demo lands its
    /// preview before the application wires up the reactive state)
    /// or outside the `Dragging` state (capture chatter while the
    /// framework is still resolving a release path stays silent).
    fn pointer_move(&mut self, x_rel: f32, y_rel: f32) {
        if !matches!(self.state(), ScrollBarState::Dragging) {
            return;
        }
        let Some(state) = self.em.inner.state.as_ref() else {
            return;
        };

        let orientation = self.em.inner.orientation;
        let cursor_fraction = match orientation {
            ScrollBarOrientation::Vertical => y_rel,
            ScrollBarOrientation::Horizontal => x_rel,
        };

        if self.em.inner.drag_start.is_none() {
            // First pointer_move under the capture lock = press-
            // time cursor. Capture the snapshot, no offset change
            // on this frame (the user has not dragged yet).
            let (max_x, max_y) = state.max();
            let (offset_x, offset_y) = state.offset();
            let (scroll_max, offset_at_press) = match orientation {
                ScrollBarOrientation::Vertical => (max_y, offset_y),
                ScrollBarOrientation::Horizontal => (max_x, offset_x),
            };
            self.em.inner.drag_start = Some(DragStart {
                cursor_fraction,
                offset_at_press,
                scroll_max,
            });
            return;
        }

        // Drag in progress. The unwrap is unreachable — we just
        // checked `is_none()`. `round()` (not truncate) maps the
        // f32 cursor delta to its nearest integer offset; without
        // it, an f32-imprecise `delta_fraction × scroll_max` of
        // 79.999995 truncates to 79 instead of the user-expected
        // 80. `scroll_to` then clamps against `[0, scroll_max]`,
        // so an over-/underflow on either side resolves to the
        // track endpoint regardless of which sign the f32 round
        // produced.
        let drag_start = self.em.inner.drag_start.expect("drag_start just captured");
        let delta_fraction = cursor_fraction - drag_start.cursor_fraction;
        // `scroll_max as f32` loses precision past 2^24 — fine for
        // realistic scroll bounds (10^7 px content at most;
        // `f32::EPSILON × 2^24 ≈ 1 px`). The cast back to i32
        // saturates on overflow (R51.x cast contract).
        #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
        let delta_offset = (delta_fraction * drag_start.scroll_max as f32).round() as i32;
        let new_along = drag_start.offset_at_press.saturating_add(delta_offset);
        match orientation {
            ScrollBarOrientation::Vertical => {
                state.scroll_to(state.offset_x(), new_along);
            }
            ScrollBarOrientation::Horizontal => {
                state.scroll_to(new_along, state.offset_y());
            }
        }
    }

    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        Some(self)
    }

    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        Some(self)
    }

    fn drain_intents(&mut self, sink: &mut dyn FnMut(Intent)) {
        self.em.drain(sink);
    }

    fn is_dirty(&self) -> bool {
        self.em.is_dirty()
    }
}

impl ExternalIntrospect for ScrollBarExternal {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(
            const {
                &[
                    SchemaField::new("state", "string"),
                    SchemaField::new("orientation", "string"),
                    SchemaField::new("send", "string"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "state" => Some(IntrospectValue::Text(self.state().as_name().to_string())),
            "orientation" => Some(IntrospectValue::Text(
                scroll_bar_orientation_name(self.orientation()).to_string(),
            )),
            _ => None,
        }
    }

    fn intervene(&mut self, path: &str, _value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            // §5.38 — `state` is SCXML-owned (the framework drives
            // it via `send`); `orientation` is construction-time
            // fixed (mirrors the R51.39 Slider axis ReadOnly
            // contract — the SCXML and ARIA semantics differ
            // between axes, so a runtime flip would change the
            // meaning of in-flight intent emissions and the
            // introspect type contract).
            "state" | "orientation" => Err(InterveneError::ReadOnly),
            _ => Err(InterveneError::UnknownPath),
        }
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match path {
            "send" => match args {
                IntrospectValue::Text(ref name) => {
                    let ev = ScrollBarEvent::from_name(name).ok_or(InvokeError::Rejected)?;
                    self.send(ev);
                    Ok(IntrospectValue::Text(self.state().as_name().to_string()))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

/// [`ScrollBarOrientation`] → introspect-surfaced `"orientation"`
/// string. Lowercased per the W3C `aria-orientation` attribute
/// convention (`"horizontal"` / `"vertical"`) so AI clients
/// observing the introspect schema can map the field straight to
/// ARIA without re-casing. Mirrors the R51.39 Slider
/// `slider_axis_name` helper.
fn scroll_bar_orientation_name(orientation: ScrollBarOrientation) -> &'static str {
    match orientation {
        ScrollBarOrientation::Vertical => "vertical",
        ScrollBarOrientation::Horizontal => "horizontal",
    }
}

#[cfg(test)]
mod r55_d1_tests {
    //! R55.D.1 §5.45 — `scrollbar_thumb_rect` closed-form regression
    //! battery. Covers the textbook math (proportional thumb size +
    //! position) plus every degenerate case the module-doc table
    //! enumerates.

    use super::{ScrollBarGeometry, ScrollBarOrientation, scrollbar_thumb_rect};
    use crate::scene::Rect;

    fn v(track: Rect, viewport: u32, content: u32, offset: u32, min: u32) -> ScrollBarGeometry {
        scrollbar_thumb_rect(
            ScrollBarOrientation::Vertical,
            track,
            viewport,
            content,
            offset,
            min,
        )
    }

    fn h(track: Rect, viewport: u32, content: u32, offset: u32, min: u32) -> ScrollBarGeometry {
        scrollbar_thumb_rect(
            ScrollBarOrientation::Horizontal,
            track,
            viewport,
            content,
            offset,
            min,
        )
    }

    #[test]
    fn vertical_thumb_at_top_of_track() {
        // track 200 tall, viewport 100 of 400 content (1/4 visible),
        // offset 0 → thumb (h=50) anchored at track top.
        let g = v(Rect::new(0, 0, 8, 200), 100, 400, 0, 0);
        assert_eq!(g.thumb, Rect::new(0, 0, 8, 50));
        assert_eq!(g.orientation, ScrollBarOrientation::Vertical);
    }

    #[test]
    fn vertical_thumb_at_bottom_of_track() {
        // Same shape, offset = scroll_max (400-100=300) → thumb at
        // bottom of available travel (200-50=150).
        let g = v(Rect::new(0, 0, 8, 200), 100, 400, 300, 0);
        assert_eq!(g.thumb, Rect::new(0, 150, 8, 50));
    }

    #[test]
    fn vertical_thumb_at_mid_track() {
        // offset half of scroll_max → thumb at half of available
        // travel: floor(150 * 150 / 300) = 75.
        let g = v(Rect::new(0, 0, 8, 200), 100, 400, 150, 0);
        assert_eq!(g.thumb, Rect::new(0, 75, 8, 50));
    }

    #[test]
    fn horizontal_mirrors_vertical_math() {
        // Same math, rotated. content 400 wide, viewport 100, track
        // 200 wide, offset mid → thumb (w=50) at x=75.
        let g = h(Rect::new(0, 0, 200, 8), 100, 400, 150, 0);
        assert_eq!(g.thumb, Rect::new(75, 0, 50, 8));
        assert_eq!(g.orientation, ScrollBarOrientation::Horizontal);
    }

    #[test]
    fn content_smaller_than_viewport_fills_track() {
        // Content fits inside viewport → no scroll possible → thumb
        // fills the whole track. Composition layer can elide the
        // paint by matching `thumb == track`.
        let g = v(Rect::new(0, 0, 8, 200), 100, 50, 0, 24);
        assert_eq!(g.thumb, Rect::new(0, 0, 8, 200));
    }

    #[test]
    fn content_equals_viewport_fills_track() {
        // Edge case: content == viewport → scroll_max would be 0 →
        // helper short-circuits to "fill track" before the divide.
        let g = v(Rect::new(0, 0, 8, 200), 100, 100, 0, 24);
        assert_eq!(g.thumb, Rect::new(0, 0, 8, 200));
    }

    #[test]
    fn min_thumb_size_clamps_tiny_ratio() {
        // Very long content: 200 * 100 / 10_000 = 2 — would be
        // nearly invisible. min=24 raises the thumb to a grabbable
        // size. Travel reduces proportionally: (200-24) = 176.
        let g = v(Rect::new(0, 0, 8, 200), 100, 10_000, 0, 24);
        assert_eq!(g.thumb, Rect::new(0, 0, 8, 24));
    }

    #[test]
    fn min_thumb_size_position_uses_reduced_travel() {
        // Same shape, offset = scroll_max (9_900) → thumb at
        // bottom of reduced travel: (200-24) = 176.
        let g = v(Rect::new(0, 0, 8, 200), 100, 10_000, 9_900, 24);
        assert_eq!(g.thumb, Rect::new(0, 176, 8, 24));
    }

    #[test]
    fn saturating_offset_clamps_to_scroll_max() {
        // Caller passes offset > scroll_max — helper clamps
        // saturating-down rather than panicking. Output identical
        // to the "at bottom" case.
        let g = v(Rect::new(0, 0, 8, 200), 100, 400, 9_999_999, 0);
        assert_eq!(g.thumb, Rect::new(0, 150, 8, 50));
    }

    #[test]
    fn zero_track_extent_returns_zero_thumb() {
        // Layout pass collapsed the scrollbar to zero along the
        // scroll axis — defensive path. Thumb keeps the track's
        // cross-axis thickness (`w=8`) for layout consistency but
        // reports a zero `h`, so its area is zero and the paint
        // backend can elide.
        let g = v(Rect::new(10, 20, 8, 0), 100, 400, 0, 24);
        assert_eq!(g.thumb, Rect::new(10, 20, 8, 0));
    }

    #[test]
    fn zero_content_returns_zero_thumb() {
        // Content not measured yet (`set_max` not called). Helper
        // stays defensive — zero-extent thumb at track origin.
        let g = v(Rect::new(10, 20, 8, 200), 100, 0, 0, 24);
        assert_eq!(g.thumb, Rect::new(10, 20, 8, 0));
    }

    #[test]
    fn track_origin_offset_propagates() {
        // Track positioned at (x=100, y=50) inside a parent — thumb
        // rect honors the same origin (helper does NOT relativize
        // to the track's own origin; output shares the input
        // coordinate frame).
        let g = v(Rect::new(100, 50, 8, 200), 100, 400, 150, 0);
        assert_eq!(g.thumb, Rect::new(100, 125, 8, 50));
    }

    #[test]
    fn no_overflow_on_large_extents() {
        // 100_000 × 1_000_000 product overflows u32 (~10^11). The
        // helper widens to u64 internally; this test pins that the
        // widening is in place (a regression that removed it would
        // overflow and wrap).
        let g = v(Rect::new(0, 0, 8, 100_000), 100_000, 1_000_000, 450_000, 0);
        // ratio_thumb = 100_000 * 100_000 / 1_000_000 = 10_000.
        // thumb_travel = 100_000 - 10_000 = 90_000.
        // scroll_max = 1_000_000 - 100_000 = 900_000.
        // thumb_pos = 90_000 * 450_000 / 900_000 = 45_000.
        assert_eq!(g.thumb, Rect::new(0, 45_000, 8, 10_000));
    }
}

#[cfg(test)]
mod r55_d2_tests {
    //! R55.D.2 §5.45 §5.38 — `ScrollBar` widget binding regression
    //! battery. Mirror of the R51.7 / R51.14 Slider test layout —
    //! initial state + four-state transition graph + drag-end
    //! commit semantics + cancel-on-leave + introspect surface +
    //! orientation immutability.

    use super::{
        ScrollBar, ScrollBarEvent, ScrollBarExternal, ScrollBarOrientation, ScrollBarState,
        scroll_bar_orientation_name,
    };
    use crate::external::SchemaField;
    use crate::external::{
        Backend, External, ExternalIntrospect, InterveneError, IntrospectValue, InvokeError,
        RepaintOwner, ThreadOwnership,
    };
    use crate::{WidgetEventName, WidgetStateName};

    #[test]
    fn initial_state_is_idle() {
        let s = ScrollBar::new();
        assert_eq!(s.state(), ScrollBarState::Idle);
    }

    #[test]
    fn default_orientation_is_vertical() {
        // ARIA / web canonical default for the `scrollbar` role.
        assert_eq!(
            ScrollBar::new().orientation(),
            ScrollBarOrientation::Vertical
        );
        assert_eq!(
            ScrollBarExternal::new().orientation(),
            ScrollBarOrientation::Vertical,
        );
    }

    #[test]
    fn with_orientation_pins_axis_at_construction() {
        // Horizontal axis pinned through both layers without
        // mutability (the SCXML and ARIA semantics differ between
        // axes — construction-time fixed, mirrors R51.39 Slider).
        assert_eq!(
            ScrollBar::with_orientation(ScrollBarOrientation::Horizontal).orientation(),
            ScrollBarOrientation::Horizontal,
        );
        assert_eq!(
            ScrollBarExternal::with_orientation(ScrollBarOrientation::Horizontal).orientation(),
            ScrollBarOrientation::Horizontal,
        );
    }

    #[test]
    fn pointer_enter_transitions_idle_to_hover() {
        let mut s = ScrollBar::new();
        s.send(ScrollBarEvent::PointerEnter);
        assert_eq!(s.state(), ScrollBarState::Hover);
    }

    #[test]
    fn pointer_leave_returns_hover_to_idle() {
        let mut s = ScrollBar::new();
        s.send(ScrollBarEvent::PointerEnter);
        s.send(ScrollBarEvent::PointerLeave);
        assert_eq!(s.state(), ScrollBarState::Idle);
    }

    #[test]
    fn pointer_down_from_hover_enters_dragging() {
        let mut s = ScrollBar::new();
        s.send(ScrollBarEvent::PointerEnter);
        s.send(ScrollBarEvent::PointerDown);
        assert_eq!(s.state(), ScrollBarState::Dragging);
    }

    #[test]
    fn pointer_up_from_dragging_returns_to_hover() {
        let mut s = ScrollBar::new();
        s.send(ScrollBarEvent::PointerEnter);
        s.send(ScrollBarEvent::PointerDown);
        s.send(ScrollBarEvent::PointerUp);
        assert_eq!(s.state(), ScrollBarState::Hover);
    }

    #[test]
    fn pointer_leave_during_drag_returns_to_idle() {
        // Drag-cancel-on-leave (W3C HTML5 drag convention); no
        // commit signal fires — the external-level test below pins
        // the intent-side semantic.
        let mut s = ScrollBar::new();
        s.send(ScrollBarEvent::PointerEnter);
        s.send(ScrollBarEvent::PointerDown);
        s.send(ScrollBarEvent::PointerLeave);
        assert_eq!(s.state(), ScrollBarState::Idle);
    }

    #[test]
    fn pointer_cancel_during_drag_returns_to_idle() {
        // R51.93 §5.35 §5.13 mirror — touch-cancel sibling of
        // pointer_leave; drops `dragging → idle` without raising
        // `scrollbar.activate`.
        let mut s = ScrollBar::new();
        s.send(ScrollBarEvent::PointerEnter);
        s.send(ScrollBarEvent::PointerDown);
        s.send(ScrollBarEvent::PointerCancel);
        assert_eq!(s.state(), ScrollBarState::Idle);
    }

    #[test]
    fn disable_from_any_state_enters_disabled() {
        // Disable transitions out of idle / hover / dragging — the
        // SCXML lands the transition on every non-disabled state.
        let mut s = ScrollBar::new();
        s.send(ScrollBarEvent::Disable);
        assert_eq!(s.state(), ScrollBarState::Disabled);

        let mut s = ScrollBar::new();
        s.send(ScrollBarEvent::PointerEnter);
        s.send(ScrollBarEvent::Disable);
        assert_eq!(s.state(), ScrollBarState::Disabled);

        let mut s = ScrollBar::new();
        s.send(ScrollBarEvent::PointerEnter);
        s.send(ScrollBarEvent::PointerDown);
        s.send(ScrollBarEvent::Disable);
        assert_eq!(s.state(), ScrollBarState::Disabled);
    }

    #[test]
    fn enable_from_disabled_returns_to_idle() {
        let mut s = ScrollBar::new();
        s.send(ScrollBarEvent::Disable);
        s.send(ScrollBarEvent::Enable);
        assert_eq!(s.state(), ScrollBarState::Idle);
    }

    #[test]
    fn full_drag_cycle_emits_scroll_committed_on_pointer_up() {
        // End-to-end drag-end commit. Pre-drag PointerEnter +
        // PointerDown, drag-end PointerUp → single
        // `"scroll_committed"` intent with `Null` payload (no value
        // payload — the offset lives in `ScrollState`).
        let mut sx = ScrollBarExternal::new();
        sx.send(ScrollBarEvent::PointerEnter);
        sx.send(ScrollBarEvent::PointerDown);
        sx.send(ScrollBarEvent::PointerUp);
        let mut harvested = Vec::new();
        sx.drain_intents(&mut |i| harvested.push(i));
        assert_eq!(harvested.len(), 1, "exactly one commit per drag-end");
        assert_eq!(harvested[0].tag_str(), "scroll_committed");
        assert!(matches!(harvested[0].payload, IntrospectValue::Null));
    }

    #[test]
    fn cancel_drag_via_pointer_leave_does_not_emit_commit() {
        // R51.93 §5.35 mirror — `dragging → idle` via pointer_leave
        // is the W3C drag-cancel-on-leave path; no commit fires.
        let mut sx = ScrollBarExternal::new();
        sx.send(ScrollBarEvent::PointerEnter);
        sx.send(ScrollBarEvent::PointerDown);
        sx.send(ScrollBarEvent::PointerLeave);
        let mut harvested = Vec::new();
        sx.drain_intents(&mut |i| harvested.push(i));
        assert!(
            harvested.is_empty(),
            "drag-cancel-on-leave must not emit scroll_committed",
        );
    }

    #[test]
    fn pointer_cancel_during_drag_does_not_emit_commit() {
        // Touch-cancel sibling: OS revoked the gesture, so no
        // commit fires either. R51.93 §5.35 §5.13 contract.
        let mut sx = ScrollBarExternal::new();
        sx.send(ScrollBarEvent::PointerEnter);
        sx.send(ScrollBarEvent::PointerDown);
        sx.send(ScrollBarEvent::PointerCancel);
        let mut harvested = Vec::new();
        sx.drain_intents(&mut |i| harvested.push(i));
        assert!(
            harvested.is_empty(),
            "touch-cancel during drag must not emit scroll_committed",
        );
    }

    #[test]
    fn external_query_state_and_orientation() {
        let sx = ScrollBarExternal::new();
        assert_eq!(
            sx.query("state").unwrap(),
            IntrospectValue::Text("Idle".to_string()),
        );
        assert_eq!(
            sx.query("orientation").unwrap(),
            IntrospectValue::Text("vertical".to_string()),
        );
        // Unknown path: None (introspect contract).
        assert_eq!(sx.query("value"), None);
    }

    #[test]
    fn external_intervene_state_and_orientation_read_only() {
        let mut sx = ScrollBarExternal::new();
        assert_eq!(
            sx.intervene("state", IntrospectValue::Text("Dragging".to_string())),
            Err(InterveneError::ReadOnly),
        );
        assert_eq!(
            sx.intervene(
                "orientation",
                IntrospectValue::Text("horizontal".to_string()),
            ),
            Err(InterveneError::ReadOnly),
        );
        // Untouched.
        assert_eq!(sx.state(), ScrollBarState::Idle);
        assert_eq!(sx.orientation(), ScrollBarOrientation::Vertical);
    }

    #[test]
    fn external_intervene_unknown_path_rejects() {
        let mut sx = ScrollBarExternal::new();
        assert_eq!(
            sx.intervene("offset", IntrospectValue::Int(0)),
            Err(InterveneError::UnknownPath),
        );
    }

    #[test]
    fn external_invoke_send_drives_transition() {
        let mut sx = ScrollBarExternal::new();
        let out = sx
            .invoke("send", IntrospectValue::Text("PointerEnter".to_string()))
            .unwrap();
        assert_eq!(out, IntrospectValue::Text("Hover".to_string()));
        assert_eq!(sx.state(), ScrollBarState::Hover);
    }

    #[test]
    fn external_invoke_send_rejects_unknown_event() {
        let mut sx = ScrollBarExternal::new();
        let r = sx.invoke("send", IntrospectValue::Text("Click".to_string()));
        assert_eq!(r, Err(InvokeError::Rejected));
    }

    #[test]
    fn external_invoke_send_rejects_non_text_args() {
        let mut sx = ScrollBarExternal::new();
        let r = sx.invoke("send", IntrospectValue::Int(0));
        assert_eq!(r, Err(InvokeError::TypeMismatch));
    }

    #[test]
    fn external_invoke_unknown_path_rejects() {
        let mut sx = ScrollBarExternal::new();
        let r = sx.invoke("set_offset", IntrospectValue::Int(0));
        assert_eq!(r, Err(InvokeError::UnknownPath));
    }

    #[test]
    fn external_schema_declares_three_slots() {
        // R55.D.2 surface: state + orientation + send. The
        // `"value"` slot Slider carries does NOT exist — the
        // authoritative offset lives in `ScrollState` (R55.B), not
        // a ScrollBar value sidecar.
        let sx = ScrollBarExternal::new();
        let schema = sx.schema();
        assert_eq!(
            schema.fields,
            &[
                SchemaField::new("state", "string"),
                SchemaField::new("orientation", "string"),
                SchemaField::new("send", "string"),
            ],
        );
    }

    #[test]
    fn external_wants_pointer_capture() {
        // §5.15 + §5.35 mirror — the framework's InputRouter pins
        // the cursor across the drag, so the user can stray off
        // the thumb / track without the press cancelling.
        let sx = ScrollBarExternal::new();
        assert!(sx.wants_pointer_capture());
    }

    #[test]
    fn external_backends_gui_and_rpc() {
        // §5.15 — `ScrollBar` is a §5.45 visible peer (GUI-driven)
        // and an AI-introspect surface (RPC-driven). Skip is the
        // safe fallback for backends not in the list (e.g. headless
        // RPC-only loops still get the GUI variant via the typed
        // schema, no panics).
        let sx = ScrollBarExternal::new();
        let support = sx.backends();
        assert!(support.supported.contains(&Backend::Gui));
        assert!(support.supported.contains(&Backend::Rpc));
    }

    #[test]
    fn external_repaint_owner_is_framework() {
        // ScrollBar carries no opaque off-thread paint surface; the
        // framework owns repaints on each `ScrollState` Signal
        // change (R55.B reactive contract).
        let sx = ScrollBarExternal::new();
        assert!(matches!(sx.repaint_ownership(), RepaintOwner::Framework));
    }

    #[test]
    fn external_thread_ownership_is_ui_sync() {
        // SCXML transitions are pure synchronous state mutations —
        // safe to drive from the UI thread.
        let sx = ScrollBarExternal::new();
        assert!(matches!(
            sx.thread_ownership(),
            ThreadOwnership::UiThreadSync
        ));
    }

    #[test]
    fn orientation_query_returns_aria_string() {
        // Both axes surface their W3C `aria-orientation`-aligned
        // lowercase string. AI clients consume this straight
        // through to their ARIA model.
        let v = ScrollBarExternal::new();
        let h = ScrollBarExternal::with_orientation(ScrollBarOrientation::Horizontal);
        assert_eq!(
            v.query("orientation"),
            Some(IntrospectValue::Text("vertical".to_string())),
        );
        assert_eq!(
            h.query("orientation"),
            Some(IntrospectValue::Text("horizontal".to_string())),
        );
    }

    #[test]
    fn parse_event_name_table_covers_every_variant() {
        // Every closed-core pointer/enable event the SCXML
        // accepts must parse. A new variant landing in the SCXML
        // without a parse-table entry would silently break the
        // RPC `invoke("send", ...)` surface — this test pins the
        // mapping bidirectionally.
        let cases = [
            ("PointerEnter", ScrollBarEvent::PointerEnter),
            ("PointerLeave", ScrollBarEvent::PointerLeave),
            ("PointerDown", ScrollBarEvent::PointerDown),
            ("PointerUp", ScrollBarEvent::PointerUp),
            ("PointerCancel", ScrollBarEvent::PointerCancel),
            ("Disable", ScrollBarEvent::Disable),
            ("Enable", ScrollBarEvent::Enable),
        ];
        for (name, expected) in cases {
            let got = ScrollBarEvent::from_name(name);
            assert_eq!(got, Some(expected), "from_name({name:?})");
        }
        // Unknown name returns None.
        assert_eq!(ScrollBarEvent::from_name("PointerWheel"), None);
        // R699 §5.16 — internal raise + Null reject; `as_name` total.
        assert_eq!(ScrollBarEvent::from_name("ScrollbarActivate"), None);
        assert_eq!(ScrollBarEvent::from_name("Null"), None);
        assert_eq!(
            ScrollBarEvent::ScrollbarActivate.as_name(),
            "ScrollbarActivate"
        );
    }

    #[test]
    fn state_name_table_covers_every_variant() {
        // RPC scene/query "state" returns one of these four strings
        // verbatim. The §2 #2 RPC introspection layer relies on the
        // mapping being stable across SCXML codegen rebuilds.
        assert_eq!(ScrollBarState::Idle.as_name(), "Idle");
        assert_eq!(ScrollBarState::Hover.as_name(), "Hover");
        assert_eq!(ScrollBarState::Dragging.as_name(), "Dragging");
        assert_eq!(ScrollBarState::Disabled.as_name(), "Disabled");
    }

    #[test]
    fn orientation_name_table_is_aria_lowercase() {
        // W3C `aria-orientation` attribute convention.
        assert_eq!(
            scroll_bar_orientation_name(ScrollBarOrientation::Vertical),
            "vertical",
        );
        assert_eq!(
            scroll_bar_orientation_name(ScrollBarOrientation::Horizontal),
            "horizontal",
        );
    }

    #[test]
    fn debug_impl_surfaces_state_and_orientation() {
        // External adapters expose a Debug impl with the state +
        // orientation so structured logging / diagnostic dumps can
        // capture the widget without going through introspect.
        let sx = ScrollBarExternal::new();
        let dbg = format!("{sx:?}");
        assert!(dbg.contains("ScrollBarExternal"), "{dbg}");
        assert!(dbg.contains("Idle"), "{dbg}");
        assert!(dbg.contains("Vertical"), "{dbg}");
    }
}

#[cfg(test)]
mod r55_d3_tests {
    //! R55.D.3 §5.45 — `ScrollBar` pointer-event routing battery.
    //! Pins the composition contract (`attach_state` stores a shared
    //! `Rc<ScrollState>`), the drag math (press-time snapshot +
    //! `delta_fraction × scroll_max`), the axis dispatch (vertical
    //! reads `y_rel`, horizontal reads `x_rel`), and the drag-end
    //! cleanup (every `Dragging` exit clears the snapshot).

    use std::rc::Rc;

    use super::{
        ScrollBar, ScrollBarEvent, ScrollBarExternal, ScrollBarOrientation, ScrollBarState,
    };
    use crate::external::External;
    use crate::widgets::scroll::ScrollState;

    fn fresh_state(max_x: i32, max_y: i32) -> Rc<ScrollState> {
        let s = Rc::new(ScrollState::new());
        s.set_max(max_x, max_y);
        s
    }

    fn drag(bar: &mut ScrollBarExternal) {
        bar.send(ScrollBarEvent::PointerEnter);
        bar.send(ScrollBarEvent::PointerDown);
    }

    #[test]
    fn bare_constructor_has_no_state_attached() {
        let s = ScrollBar::new();
        assert!(s.scroll_state().is_none());
        let sx = ScrollBarExternal::new();
        assert!(sx.scroll_state().is_none());
    }

    #[test]
    fn attach_state_stores_handle_with_shared_rc() {
        // The `Rc<ScrollState>` is shared between caller and the
        // bar — modifications through the original handle surface
        // on the bar's `scroll_state()` query and vice versa.
        let state = fresh_state(0, 400);
        let sx = ScrollBarExternal::new().attach_state(Rc::clone(&state));
        let attached = sx.scroll_state().expect("state was attached");
        assert!(Rc::ptr_eq(attached, &state));
        // Mutate through the original handle.
        state.scroll_to(0, 50);
        // Visible through the bar's handle.
        assert_eq!(attached.offset(), (0, 50));
    }

    #[test]
    fn with_orientation_chains_attach_state() {
        let state = fresh_state(300, 0);
        let sx = ScrollBarExternal::with_orientation(ScrollBarOrientation::Horizontal)
            .attach_state(Rc::clone(&state));
        assert_eq!(sx.orientation(), ScrollBarOrientation::Horizontal);
        assert!(Rc::ptr_eq(sx.scroll_state().unwrap(), &state));
    }

    #[test]
    fn pointer_move_without_state_is_silent() {
        // Bare bar — no state attached. pointer_move under capture
        // lock is a no-op; nothing to assert other than "did not
        // panic" plus the bar still surfaces no scroll_state.
        let mut sx = ScrollBarExternal::new();
        drag(&mut sx);
        sx.pointer_move(0.0, 0.5);
        sx.pointer_move(0.0, 0.8);
        assert!(sx.scroll_state().is_none());
    }

    #[test]
    fn pointer_move_outside_dragging_is_silent() {
        // Capture chatter while idle / hover. The state's offset
        // must not move just because pointer_move fired.
        let state = fresh_state(0, 400);
        let mut sx = ScrollBarExternal::new().attach_state(Rc::clone(&state));
        // Idle → pointer_move (e.g. hover preview from outside the bar
        // for a sibling widget that lost focus). Capture lock would
        // not even be open here in production but the routing must
        // stay defensive.
        sx.pointer_move(0.0, 0.5);
        assert_eq!(state.offset(), (0, 0));
        sx.send(ScrollBarEvent::PointerEnter);
        sx.pointer_move(0.0, 0.5);
        assert_eq!(state.offset(), (0, 0));
    }

    #[test]
    fn first_pointer_move_captures_drag_start_without_offset_change() {
        // The framework supplies the press-time cursor as the
        // initial `pointer_move` after capture lock opens. That
        // frame must capture the snapshot; the offset must not
        // move yet (the user has not dragged anywhere).
        let state = fresh_state(0, 400);
        state.scroll_to(0, 80); // arbitrary press-time offset
        let mut sx = ScrollBarExternal::new().attach_state(Rc::clone(&state));
        drag(&mut sx);
        // Press-time cursor fraction y_rel = 0.3.
        sx.pointer_move(0.0, 0.3);
        // No offset change on press frame.
        assert_eq!(state.offset(), (0, 80));
    }

    #[test]
    fn vertical_drag_applies_fraction_delta_times_scroll_max() {
        // Vertical: y_rel drives. press at y=0.2, drag to y=0.5
        // (delta=+0.3), scroll_max=400 → delta_offset = 120, new
        // offset = 80 + 120 = 200.
        let state = fresh_state(0, 400);
        state.scroll_to(0, 80);
        let mut sx = ScrollBarExternal::new().attach_state(Rc::clone(&state));
        drag(&mut sx);
        sx.pointer_move(0.0, 0.2); // press
        sx.pointer_move(0.0, 0.5); // drag
        assert_eq!(state.offset(), (0, 200));
    }

    #[test]
    fn horizontal_drag_reads_x_rel_and_ignores_y_rel() {
        // Horizontal: x_rel drives, y_rel is ignored. press at
        // x=0.1, drag to x=0.4 (delta=+0.3) on scroll_max=200 → +60.
        let state = fresh_state(200, 0);
        state.scroll_to(40, 0);
        let mut sx = ScrollBarExternal::with_orientation(ScrollBarOrientation::Horizontal)
            .attach_state(Rc::clone(&state));
        drag(&mut sx);
        sx.pointer_move(0.1, 0.7); // press; y ignored
        sx.pointer_move(0.4, 0.9); // drag; y ignored
        assert_eq!(state.offset(), (100, 0));
    }

    #[test]
    fn vertical_drag_ignores_x_rel() {
        // Symmetric to the horizontal case: Vertical reads y only.
        let state = fresh_state(200, 400);
        state.scroll_to(50, 80);
        let mut sx = ScrollBarExternal::new().attach_state(Rc::clone(&state));
        drag(&mut sx);
        sx.pointer_move(0.1, 0.2); // press
        sx.pointer_move(0.9, 0.2); // x changed, y same → no offset change
        assert_eq!(state.offset(), (50, 80));
    }

    #[test]
    fn backward_drag_decreases_offset() {
        // Dragging up (smaller y_rel) decreases the offset.
        let state = fresh_state(0, 400);
        state.scroll_to(0, 200);
        let mut sx = ScrollBarExternal::new().attach_state(Rc::clone(&state));
        drag(&mut sx);
        sx.pointer_move(0.0, 0.6); // press
        sx.pointer_move(0.0, 0.2); // drag back (delta = -0.4 × 400 = -160)
        assert_eq!(state.offset(), (0, 40));
    }

    #[test]
    fn drag_clamps_to_zero_on_underflow() {
        // delta_offset would be negative enough to undershoot 0.
        // `ScrollState::scroll_to` clamps saturating-down to 0.
        let state = fresh_state(0, 400);
        state.scroll_to(0, 50);
        let mut sx = ScrollBarExternal::new().attach_state(Rc::clone(&state));
        drag(&mut sx);
        sx.pointer_move(0.0, 0.5); // press
        sx.pointer_move(0.0, 0.0); // drag to top (delta = -0.5 × 400 = -200)
        // 50 + (-200) = -150 → clamps to 0.
        assert_eq!(state.offset(), (0, 0));
    }

    #[test]
    fn drag_clamps_to_scroll_max_on_overshoot() {
        // delta past `scroll_max` saturates at the bottom.
        let state = fresh_state(0, 400);
        state.scroll_to(0, 200);
        let mut sx = ScrollBarExternal::new().attach_state(Rc::clone(&state));
        drag(&mut sx);
        sx.pointer_move(0.0, 0.0); // press at top
        sx.pointer_move(0.0, 1.0); // drag to bottom (delta = +1.0 × 400 = +400)
        // 200 + 400 = 600 → clamps to scroll_max=400.
        assert_eq!(state.offset(), (0, 400));
    }

    #[test]
    fn pointer_up_clears_drag_start_so_next_press_starts_fresh() {
        // After drag-end, the next press should re-capture from
        // the (post-drag) offset, not the previous drag's press.
        let state = fresh_state(0, 400);
        let mut sx = ScrollBarExternal::new().attach_state(Rc::clone(&state));
        // First drag: 0.0 → 0.25 = +100 offset.
        drag(&mut sx);
        sx.pointer_move(0.0, 0.0);
        sx.pointer_move(0.0, 0.25);
        sx.send(ScrollBarEvent::PointerUp);
        assert_eq!(state.offset(), (0, 100));
        // Second drag from the new offset 100: 0.5 → 0.75 = +100 offset.
        sx.send(ScrollBarEvent::PointerDown);
        sx.pointer_move(0.0, 0.5);
        sx.pointer_move(0.0, 0.75);
        assert_eq!(
            state.offset(),
            (0, 200),
            "second drag should add to new offset"
        );
    }

    #[test]
    fn pointer_leave_clears_drag_start() {
        // Drag-cancel-on-leave: the SCXML drops `dragging → idle`,
        // and the next press must start fresh. (No offset rollback —
        // matches R51.93 Slider value sticky-on-cancel.)
        let state = fresh_state(0, 400);
        let mut sx = ScrollBarExternal::new().attach_state(Rc::clone(&state));
        drag(&mut sx);
        sx.pointer_move(0.0, 0.0);
        sx.pointer_move(0.0, 0.5);
        assert_eq!(state.offset(), (0, 200));
        sx.send(ScrollBarEvent::PointerLeave);
        // Restart the drag — the bar must re-snapshot, not carry
        // the prior `drag_start.cursor_fraction = 0.0` across a
        // cancel.
        sx.send(ScrollBarEvent::PointerEnter);
        sx.send(ScrollBarEvent::PointerDown);
        sx.pointer_move(0.0, 0.5); // press (post-cancel) at y=0.5
        sx.pointer_move(0.0, 0.6); // drag (delta = +0.1 × 400 = +40)
        assert_eq!(state.offset(), (0, 240));
    }

    #[test]
    fn pointer_cancel_clears_drag_start() {
        // Touch-cancel analog of pointer_leave (R51.93 §5.35 mirror).
        let state = fresh_state(0, 400);
        let mut sx = ScrollBarExternal::new().attach_state(Rc::clone(&state));
        drag(&mut sx);
        sx.pointer_move(0.0, 0.0);
        sx.pointer_move(0.0, 0.4); // +160 offset
        sx.send(ScrollBarEvent::PointerCancel);
        assert_eq!(sx.state(), ScrollBarState::Idle);
        // Press again — fresh snapshot.
        sx.send(ScrollBarEvent::PointerEnter);
        sx.send(ScrollBarEvent::PointerDown);
        sx.pointer_move(0.0, 0.5);
        sx.pointer_move(0.0, 0.5); // no delta
        assert_eq!(state.offset(), (0, 160), "no drift on stationary repress");
    }

    #[test]
    fn disable_during_drag_clears_drag_start() {
        // Disable mid-drag: SCXML jumps `Dragging → Disabled`,
        // and the press-time snapshot is wiped. Subsequent
        // pointer_move calls (capture lock may still be open until
        // the framework's release path catches up) are silent.
        let state = fresh_state(0, 400);
        let mut sx = ScrollBarExternal::new().attach_state(Rc::clone(&state));
        drag(&mut sx);
        sx.pointer_move(0.0, 0.1);
        sx.pointer_move(0.0, 0.3); // +80 offset
        sx.send(ScrollBarEvent::Disable);
        assert_eq!(sx.state(), ScrollBarState::Disabled);
        // Late pointer_move: silent (state != Dragging).
        sx.pointer_move(0.0, 0.9);
        assert_eq!(state.offset(), (0, 80), "disabled bar must not move offset");
    }

    #[test]
    fn drag_pins_scroll_max_from_press_time() {
        // Mid-drag layout re-measurement: caller shrinks
        // `scroll_max`. The bar's drag math must keep using the
        // press-time `scroll_max`, otherwise a content insert mid-
        // drag would warp the cursor↔offset mapping under the
        // user's finger.
        let state = fresh_state(0, 400);
        state.scroll_to(0, 100);
        let mut sx = ScrollBarExternal::new().attach_state(Rc::clone(&state));
        drag(&mut sx);
        sx.pointer_move(0.0, 0.0); // press; pinned scroll_max = 400
        // Caller shrinks max mid-drag (e.g. content shrank). Note:
        // `ScrollState::set_max` will clamp the existing offset to
        // the new max if the offset exceeds it — but our offset
        // here (100) is still inside the new max (200), so no
        // mid-call clamp fires.
        state.set_max(0, 200);
        sx.pointer_move(0.0, 0.5);
        // Press-time max=400 → delta = 0.5 × 400 = +200. press
        // offset was 100 → new = 300. `ScrollState::scroll_to`
        // then clamps against the *new* max=200 → final = 200.
        assert_eq!(state.offset(), (0, 200));
    }

    #[test]
    fn fresh_drag_snapshots_current_offset_not_zero() {
        // First drag mutates the offset; second drag (after the
        // explicit drag-end) must snapshot the post-first-drag
        // offset, not the constructor's 0.
        let state = fresh_state(0, 400);
        let mut sx = ScrollBarExternal::new().attach_state(Rc::clone(&state));
        // First drag: offset 0 → 80.
        drag(&mut sx);
        sx.pointer_move(0.0, 0.5);
        sx.pointer_move(0.0, 0.7); // +0.2 × 400 = +80
        sx.send(ScrollBarEvent::PointerUp);
        assert_eq!(state.offset(), (0, 80));
        // Second drag: must use post-first-drag offset 80 as the
        // press-time snapshot, NOT 0.
        sx.send(ScrollBarEvent::PointerDown);
        sx.pointer_move(0.0, 0.0); // press
        sx.pointer_move(0.0, 0.1); // +0.1 × 400 = +40
        // 80 + 40 = 120.
        assert_eq!(
            state.offset(),
            (0, 120),
            "fresh drag must snapshot 80, not 0"
        );
    }
}
