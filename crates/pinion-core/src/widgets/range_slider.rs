//! R738 §5.38 — **range slider** (dual-thumb / two-value slider): the
//! WAI-ARIA "Slider (Multi-Thumb)" pattern and the Material range-slider
//! form factor (a price filter, a histogram level window, an audio/video
//! trim, a min/max constraint pair).
//!
//! ## Why a new coordinating substrate (and not two `SliderExternal`s)
//!
//! A range slider cannot be composed from two independent
//! [`SliderExternal`](crate::widgets::slider::SliderExternal)s. The two
//! thumbs share **one** track rect, so a single press / drag forwards
//! exactly one widget-relative cursor position
//! ([`External::pointer_move`]) — the framework's `InputRouter` has no
//! way to know which of two stacked externals the user grabbed. The
//! coordination that *only* a single owning external can provide is:
//!
//! * **nearest-thumb pick** — on the first forwarded move of a drag,
//!   latch the thumb closest to the cursor and drive only that thumb for
//!   the rest of the gesture (so a drag started on the low thumb keeps
//!   moving the low thumb even after the cursor passes the high thumb);
//! * **the monotonic constraint** — the low value never exceeds the high
//!   value (each setter clamps to the other thumb), the dual-thumb
//!   invariant `0 <= low <= high <= 1`.
//!
//! ## Interaction statechart is *shared* with the single slider
//!
//! [`RangeSlider`] embeds a [`Widget<SliderPolicy>`] — the **identical**
//! Idle/Hover/Dragging machine the single slider uses. A dual-thumb
//! slider's pointer interaction (enter → hover, down → dragging, up →
//! hover, cancel → idle) is byte-for-byte the single slider's; the only
//! difference (two values + which thumb is active) is value-domain
//! sidecar, exactly the SCXML "null datamodel + typed Rust sidecar"
//! split the single slider already uses. Authoring a `range_slider.scxml`
//! would be a literal copy of `slider.scxml` — an SSOT violation — so the
//! widget reuses [`SliderPolicy`] (re-exported from
//! [`crate::widgets::slider`]). This is *not* the R709/R734 "similar
//! grammar ≠ shared statechart" anti-pattern: there the state
//! *transitions* diverged (a continuous drag vs. a domain-step
//! increment); here the transitions are genuinely the same machine.
//!
//! ## Two values, two thumbs, one active
//!
//! The f32 sidecar carries `low` and `high` (normalised `0.0..=1.0`,
//! `low <= high`) plus the [`ThumbId`] last moved (`active`, surfaced so
//! a binding can highlight the grabbed thumb and an AI client can read
//! which thumb a value mutation landed on). A separate `dragging` latch
//! holds the thumb a pointer gesture grabbed for the span of the drag.
//!
//! ## Intent channels (mirror the single slider)
//!
//! * **`value_changing`** — every effective thumb mutation (drag /
//!   keyboard / `intervene`) emits a continuous intent carrying the
//!   driven thumb's new value as [`IntrospectValue::Float`].
//! * **`value_committed`** — the `Dragging → Hover` activate (drag end)
//!   emits one intent carrying the active thumb's committed value.

use crate::external::{
    Backend, BackendFallback, BackendSupport, CaptureNormalize, External, ExternalIntrospect,
    InterveneError, IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner, ThreadOwnership,
};
use crate::intent::Intent;
use crate::widgets::slider::{SliderAxis, SliderEvent, SliderPolicy, SliderState};
use crate::widgets::{IntentEmitter, Widget};
use crate::{WidgetEventName, WidgetStateName};

/// R738 §5.38 — which of the two thumbs a value mutation or a drag
/// gesture targets. `Low` is the lower-bound thumb (constrained to
/// `[0, high]`), `High` is the upper-bound thumb (constrained to
/// `[low, 1]`).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ThumbId {
    /// The lower-bound thumb (`low` value, clamps to `[0.0, high]`).
    Low,
    /// The upper-bound thumb (`high` value, clamps to `[low, 1.0]`).
    High,
}

impl ThumbId {
    /// The introspect-surfaced `"active"` string (`"low"` / `"high"`),
    /// the W3C-aligned thumb identifier an AI client reads to learn
    /// which thumb a `value_changing` intent or a drag last moved.
    #[must_use]
    pub fn as_name(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::High => "high",
        }
    }

    /// Parse the `"active"` introspect string back to a [`ThumbId`]
    /// (`"low"` / `"high"`); any other token is `None`. The peer of
    /// [`Self::as_name`].
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "low" => Some(Self::Low),
            "high" => Some(Self::High),
            _ => None,
        }
    }
}

/// R738 §5.38 — dual-thumb slider model. Embeds the single slider's
/// [`Widget<SliderPolicy>`] interaction statechart and adds the two-value
/// and active-thumb sidecar. See the module docs for why the statechart
/// is shared and why a single coordinating model (rather than two
/// independent sliders) is required.
pub struct RangeSlider {
    /// Shared Idle/Hover/Dragging interaction machine (reused from the
    /// single slider; see module docs).
    inner: Widget<SliderPolicy>,
    /// Lower-bound value (`0.0..=high`).
    low: f32,
    /// Upper-bound value (`low..=1.0`).
    high: f32,
    /// The thumb a value mutation last landed on (drag pick / keyboard /
    /// `intervene`). Surfaced as the `"active"` introspect field.
    active: ThumbId,
    /// The thumb a pointer drag latched, held for the gesture's span so
    /// the driven thumb never switches mid-drag. `None` outside a drag.
    dragging: Option<ThumbId>,
    /// Track orientation, fixed at construction (mirrors [`SliderAxis`]).
    axis: SliderAxis,
}

impl RangeSlider {
    /// Construct a horizontal range slider with the full span selected
    /// (`low = 0.0`, `high = 1.0`, active = `High`). Use
    /// [`Self::with_values`] to seed a sub-range.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Widget::new(),
            low: 0.0,
            high: 1.0,
            active: ThumbId::High,
            dragging: None,
            axis: SliderAxis::Horizontal,
        }
    }

    /// Construct a range slider on an explicit [`SliderAxis`] (vertical
    /// inverts the pointer Y mapping exactly as the single slider does).
    #[must_use]
    pub fn with_axis(axis: SliderAxis) -> Self {
        Self {
            axis,
            ..Self::new()
        }
    }

    /// Builder: seed the initial `(low, high)` sub-range. The pair is
    /// normalised (clamped to `0.0..=1.0` and ordered so `low <= high`)
    /// so a malformed argument can never violate the dual-thumb
    /// invariant. Chain after [`Self::new`] / [`Self::with_axis`].
    #[must_use]
    pub fn with_values(mut self, low: f32, high: f32) -> Self {
        let lo = low.clamp(0.0, 1.0);
        let hi = high.clamp(0.0, 1.0);
        // Order defensively: a caller passing low > high gets the pair
        // sorted rather than a violated invariant.
        self.low = lo.min(hi);
        self.high = lo.max(hi);
        self
    }

    /// Current interaction state (shared with the single slider).
    #[must_use]
    pub fn state(&self) -> SliderState {
        self.inner.state()
    }

    /// Lower-bound value (`0.0..=high`).
    #[must_use]
    pub fn low(&self) -> f32 {
        self.low
    }

    /// Upper-bound value (`low..=1.0`).
    #[must_use]
    pub fn high(&self) -> f32 {
        self.high
    }

    /// The thumb a value mutation last landed on (`"active"` field).
    #[must_use]
    pub fn active(&self) -> ThumbId {
        self.active
    }

    /// Track orientation, fixed at construction.
    #[must_use]
    pub fn axis(&self) -> SliderAxis {
        self.axis
    }

    /// Drive a [`SliderEvent`] through the shared interaction SCXML.
    /// Pure state transition — value mutation flows through
    /// [`Self::set_low`] / [`Self::set_high`] / [`Self::drive_drag`].
    pub fn send(&mut self, event: SliderEvent) {
        self.inner.send(event);
    }

    /// Set the lower-bound thumb, clamping to `[0.0, high]` (the
    /// monotonic invariant — the low thumb can never pass the high
    /// thumb) and marking `Low` active. Returns `true` if the stored
    /// value changed (gate-by-effect intent emission). State-independent
    /// like the single slider's setter, so keyboard / `intervene` /
    /// preference-restore writes all work.
    pub fn set_low(&mut self, v: f32) -> bool {
        self.active = ThumbId::Low;
        let clamped = v.clamp(0.0, self.high);
        if (clamped - self.low).abs() < f32::EPSILON {
            return false;
        }
        self.low = clamped;
        true
    }

    /// Set the upper-bound thumb, clamping to `[low, 1.0]` and marking
    /// `High` active. Returns `true` on an effective change. See
    /// [`Self::set_low`].
    pub fn set_high(&mut self, v: f32) -> bool {
        self.active = ThumbId::High;
        let clamped = v.clamp(self.low, 1.0);
        if (clamped - self.high).abs() < f32::EPSILON {
            return false;
        }
        self.high = clamped;
        true
    }

    /// Pick the thumb nearest the normalised position `pos`. Resolves
    /// the canonical dual-thumb rule: a position below `low` grabs the
    /// low thumb, above `high` grabs the high thumb, and one between the
    /// thumbs grabs whichever is closer. When the thumbs are stacked
    /// (`low == high`) the comparison is direction-sensitive (a position
    /// at or below the stack grabs `Low`, above grabs `High`), so a drag
    /// off a stacked pair separates the thumbs in the drag direction.
    #[must_use]
    pub fn pick(&self, pos: f32) -> ThumbId {
        if pos < self.low {
            ThumbId::Low
        } else if pos > self.high {
            ThumbId::High
        } else if (pos - self.low) <= (self.high - pos) {
            ThumbId::Low
        } else {
            ThumbId::High
        }
    }

    /// Drive a pointer gesture to the normalised position `pos`. On the
    /// first call of a drag (no thumb latched) the nearest thumb is
    /// picked via [`Self::pick`] and latched for the gesture; subsequent
    /// calls keep driving the latched thumb. Returns `true` on an
    /// effective value change. The latch is released by
    /// [`Self::end_drag`] when the interaction leaves `Dragging`.
    pub fn drive_drag(&mut self, pos: f32) -> bool {
        // `Option<ThumbId>` is `Copy`, so reading `self.dragging` by value
        // does not hold a borrow across the `self.pick` call.
        let thumb = self.dragging.unwrap_or_else(|| self.pick(pos));
        self.dragging = Some(thumb);
        match thumb {
            ThumbId::Low => self.set_low(pos),
            ThumbId::High => self.set_high(pos),
        }
    }

    /// Release the pointer-drag latch (called when the interaction state
    /// leaves `Dragging`). A no-op when no drag is in flight.
    pub fn end_drag(&mut self) {
        self.dragging = None;
    }
}

impl Default for RangeSlider {
    fn default() -> Self {
        Self::new()
    }
}

/// R738 §5.38 — `External` adapter wrapping a [`RangeSlider`]. Emits the
/// same two intent kinds as the single slider (`value_changing` per
/// effective thumb mutation, `value_committed` on drag end), each
/// carrying the *driven* / *active* thumb's value as
/// [`IntrospectValue::Float`].
pub struct RangeSliderExternal {
    em: IntentEmitter<RangeSlider>,
}

impl RangeSliderExternal {
    /// Construct a horizontal range external with the full span selected.
    #[must_use]
    pub fn new() -> Self {
        Self {
            em: IntentEmitter::default(),
        }
    }

    /// Construct a range external on an explicit [`SliderAxis`].
    #[must_use]
    pub fn with_axis(axis: SliderAxis) -> Self {
        Self {
            em: IntentEmitter::new(RangeSlider::with_axis(axis)),
        }
    }

    /// Construct a horizontal range external seeded to `(low, high)`.
    #[must_use]
    pub fn with_values(low: f32, high: f32) -> Self {
        Self {
            em: IntentEmitter::new(RangeSlider::new().with_values(low, high)),
        }
    }

    /// Track orientation (delegates to [`RangeSlider::axis`]).
    #[must_use]
    pub fn axis(&self) -> SliderAxis {
        self.em.inner.axis()
    }

    /// Current interaction state.
    #[must_use]
    pub fn state(&self) -> SliderState {
        self.em.inner.state()
    }

    /// Lower-bound value.
    #[must_use]
    pub fn low(&self) -> f32 {
        self.em.inner.low()
    }

    /// Upper-bound value.
    #[must_use]
    pub fn high(&self) -> f32 {
        self.em.inner.high()
    }

    /// The thumb a value mutation last landed on.
    #[must_use]
    pub fn active(&self) -> ThumbId {
        self.em.inner.active()
    }

    /// Drive a [`SliderEvent`] and queue a `"value_committed"` intent on
    /// drag end (`Dragging → Hover`). The pointer-drag latch is released
    /// whenever the interaction leaves `Dragging`, so the next press
    /// re-picks the nearest thumb.
    pub fn send(&mut self, event: SliderEvent) {
        let before = self.em.inner.state();
        self.em.inner.send(event);
        let after = self.em.inner.state();
        // Commit channel: a drag-end (Dragging → Hover via PointerUp)
        // emits one intent carrying the active thumb's committed value,
        // mirroring the single slider's `onChangeEnd` contract. Detected
        // here (not via `WidgetTransition::detect`) because the committed
        // value depends on which thumb is active — context the static
        // snapshot tuple does not carry.
        if matches!(before, SliderState::Dragging) && matches!(after, SliderState::Hover) {
            let committed = self.active_value();
            self.em.push(Intent::new_static(
                crate::widgets::commit::VALUE_COMMITTED_EVENT,
                IntrospectValue::Float(committed),
            ));
        }
        if !matches!(after, SliderState::Dragging) {
            self.em.inner.end_drag();
        }
    }

    /// Set the low thumb and queue a `"value_changing"` intent on an
    /// effective change.
    pub fn set_low(&mut self, v: f32) {
        if self.em.inner.set_low(v) {
            self.em.push(Intent::new_static(
                "value_changing",
                IntrospectValue::Float(f64::from(self.em.inner.low())),
            ));
        }
    }

    /// Set the high thumb and queue a `"value_changing"` intent on an
    /// effective change.
    pub fn set_high(&mut self, v: f32) {
        if self.em.inner.set_high(v) {
            self.em.push(Intent::new_static(
                "value_changing",
                IntrospectValue::Float(f64::from(self.em.inner.high())),
            ));
        }
    }

    /// The active thumb's value as `f64` (commit-intent payload helper).
    fn active_value(&self) -> f64 {
        f64::from(match self.em.inner.active() {
            ThumbId::Low => self.em.inner.low(),
            ThumbId::High => self.em.inner.high(),
        })
    }
}

impl Default for RangeSliderExternal {
    fn default() -> Self {
        Self::new()
    }
}

impl core::fmt::Debug for RangeSliderExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("RangeSliderExternal")
            .field("state", &self.state())
            .field("low", &self.low())
            .field("high", &self.high())
            .field("active", &self.active())
            .finish()
    }
}

impl External for RangeSliderExternal {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui, Backend::Rpc], BackendFallback::Skip)
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }

    /// Opt in to capture lock (same as the single slider) so the drag
    /// survives the cursor straying off the track.
    fn wants_pointer_capture(&self) -> bool {
        true
    }

    /// R738 §5.35 — the value spans the whole track, so a drag that
    /// captured a thumb sub-tag (`range#low` / `range#high`) normalizes the
    /// cursor against the **primary** (track) rect, not the small thumb rect.
    fn capture_normalize(&self) -> CaptureNormalize<'_> {
        CaptureNormalize::Primary
    }

    /// Feed the widget-relative cursor along the [`SliderAxis`] into the
    /// active thumb. The first move of a drag picks the nearest thumb
    /// ([`RangeSlider::drive_drag`]); subsequent moves keep driving it.
    /// Horizontal reads `x_rel`; vertical reads `1.0 - y_rel` (top = 1.0,
    /// ARIA `aria-orientation="vertical"` convention) — identical to the
    /// single slider's mapping.
    fn pointer_move(&mut self, x_rel: f32, y_rel: f32) {
        let pos = match self.em.inner.axis() {
            SliderAxis::Horizontal => x_rel,
            SliderAxis::Vertical => 1.0 - y_rel,
        }
        .clamp(0.0, 1.0);
        if self.em.inner.drive_drag(pos) {
            let driven = match self.em.inner.active() {
                ThumbId::Low => self.em.inner.low(),
                ThumbId::High => self.em.inner.high(),
            };
            self.em.push(Intent::new_static(
                "value_changing",
                IntrospectValue::Float(f64::from(driven)),
            ));
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

impl ExternalIntrospect for RangeSliderExternal {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(&[
            ("state", "string"),
            ("low", "float"),
            ("high", "float"),
            // Which thumb a value mutation last landed on ("low"/"high").
            ("active", "string"),
            ("orientation", "string"),
            ("send", "string"),
        ])
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "state" => Some(IntrospectValue::Text(self.state().as_name().to_string())),
            "low" => Some(IntrospectValue::Float(f64::from(self.low()))),
            "high" => Some(IntrospectValue::Float(f64::from(self.high()))),
            "active" => Some(IntrospectValue::Text(self.active().as_name().to_string())),
            "orientation" => Some(IntrospectValue::Text(
                range_axis_name(self.axis()).to_string(),
            )),
            _ => None,
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            // `state`/`orientation` are SCXML- / construction-owned, and
            // `active` is derived (it follows whichever thumb a `low` /
            // `high` write moved) — all reject intervene.
            "state" | "orientation" | "active" => Err(InterveneError::ReadOnly),
            "low" => self.intervene_thumb(&value, true),
            "high" => self.intervene_thumb(&value, false),
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
                    let ev = SliderEvent::from_name(name).ok_or(InvokeError::Rejected)?;
                    self.send(ev);
                    Ok(IntrospectValue::Text(self.state().as_name().to_string()))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

impl RangeSliderExternal {
    /// Shared `intervene` body for the two thumb paths: narrow the wire
    /// `Float`/`Int` to `f32` and route to [`Self::set_low`] /
    /// [`Self::set_high`] (which clamp + emit `value_changing`).
    fn intervene_thumb(
        &mut self,
        value: &IntrospectValue,
        is_low: bool,
    ) -> Result<(), InterveneError> {
        let v = match value {
            #[allow(clippy::cast_possible_truncation)]
            IntrospectValue::Float(v) => *v as f32,
            #[allow(clippy::cast_precision_loss)]
            IntrospectValue::Int(i) => *i as f32,
            _ => return Err(InterveneError::TypeMismatch),
        };
        if is_low {
            self.set_low(v);
        } else {
            self.set_high(v);
        }
        Ok(())
    }
}

/// R738 §5.38 — [`SliderAxis`] → introspect `"orientation"` string,
/// lowercased per the W3C `aria-orientation` convention. (Mirror of the
/// single slider's `slider_axis_name`; kept local so the range widget has
/// no cross-widget private dependency.)
fn range_axis_name(axis: SliderAxis) -> &'static str {
    match axis {
        SliderAxis::Horizontal => "horizontal",
        SliderAxis::Vertical => "vertical",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::external::IntrospectValue;

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    #[test]
    fn new_full_span_horizontal() {
        let r = RangeSlider::new();
        assert!(approx(r.low(), 0.0));
        assert!(approx(r.high(), 1.0));
        assert_eq!(r.state(), SliderState::Idle);
        assert_eq!(r.axis(), SliderAxis::Horizontal);
    }

    #[test]
    fn with_values_seeds_subrange() {
        let r = RangeSlider::new().with_values(0.25, 0.75);
        assert!(approx(r.low(), 0.25));
        assert!(approx(r.high(), 0.75));
    }

    #[test]
    fn with_values_orders_and_clamps_a_malformed_pair() {
        // low > high arrives sorted; out-of-range arrives clamped.
        let r = RangeSlider::new().with_values(0.9, 0.1);
        assert!(approx(r.low(), 0.1), "sorted low");
        assert!(approx(r.high(), 0.9), "sorted high");
        let r2 = RangeSlider::new().with_values(-1.0, 2.0);
        assert!(approx(r2.low(), 0.0) && approx(r2.high(), 1.0), "clamped");
    }

    #[test]
    fn set_low_clamps_to_high_monotonic() {
        let mut r = RangeSlider::new().with_values(0.2, 0.6);
        assert!(r.set_low(0.4));
        assert!(approx(r.low(), 0.4));
        // Cannot pass the high thumb: 0.8 clamps to high (0.6).
        assert!(r.set_low(0.8));
        assert!(approx(r.low(), 0.6), "low clamps at high");
        assert_eq!(r.active(), ThumbId::Low);
    }

    #[test]
    fn set_high_clamps_to_low_monotonic() {
        let mut r = RangeSlider::new().with_values(0.3, 0.7);
        assert!(r.set_high(0.5));
        assert!(approx(r.high(), 0.5));
        // Cannot drop below the low thumb: 0.1 clamps to low (0.3).
        assert!(r.set_high(0.1));
        assert!(approx(r.high(), 0.3), "high clamps at low");
        assert_eq!(r.active(), ThumbId::High);
    }

    #[test]
    fn set_low_no_op_returns_false() {
        let mut r = RangeSlider::new().with_values(0.2, 0.8);
        assert!(!r.set_low(0.2), "same value is a no-op");
    }

    #[test]
    fn pick_nearest_thumb() {
        let r = RangeSlider::new().with_values(0.2, 0.8);
        assert_eq!(r.pick(0.0), ThumbId::Low, "below low");
        assert_eq!(r.pick(1.0), ThumbId::High, "above high");
        assert_eq!(r.pick(0.3), ThumbId::Low, "closer to low");
        assert_eq!(r.pick(0.7), ThumbId::High, "closer to high");
        assert_eq!(r.pick(0.5), ThumbId::Low, "tie → low (<=)");
    }

    #[test]
    fn pick_stacked_thumbs_is_direction_sensitive() {
        let r = RangeSlider::new().with_values(0.5, 0.5);
        assert_eq!(r.pick(0.4), ThumbId::Low, "below the stack → low");
        assert_eq!(r.pick(0.6), ThumbId::High, "above the stack → high");
    }

    #[test]
    fn drag_latches_picked_thumb_for_the_gesture() {
        let mut r = RangeSlider::new().with_values(0.2, 0.8);
        // First move at 0.25 picks the low thumb (nearest) and latches.
        assert!(r.drive_drag(0.25));
        assert!(approx(r.low(), 0.25));
        assert_eq!(r.active(), ThumbId::Low);
        // A later move past the high thumb keeps driving the LOW thumb,
        // clamped to the high thumb (monotonic) — it does NOT jump to
        // the high thumb just because the cursor is now closer to it.
        assert!(r.drive_drag(0.95));
        assert!(approx(r.low(), 0.8), "latched low thumb clamps at high");
        assert!(approx(r.high(), 0.8), "high thumb untouched");
        // Ending the drag releases the latch; the next gesture re-picks.
        r.end_drag();
        assert!(r.drive_drag(0.9));
        assert_eq!(
            r.active(),
            ThumbId::High,
            "re-picked nearest (high) after end_drag"
        );
    }

    #[test]
    fn external_send_commits_active_thumb_on_drag_end() {
        let mut rx = RangeSliderExternal::with_values(0.2, 0.8);
        rx.send(SliderEvent::PointerEnter);
        rx.send(SliderEvent::PointerDown);
        assert!(matches!(rx.state(), SliderState::Dragging));
        // Drag the low thumb to 0.35.
        rx.pointer_move(0.35, 0.0);
        assert!(approx(rx.low(), 0.35));
        assert_eq!(rx.active(), ThumbId::Low);
        // Harvest the value_changing stream.
        let mut changing = Vec::new();
        rx.drain_intents(&mut |i| changing.push(i));
        assert!(changing.iter().all(|i| i.tag_str() == "value_changing"));
        // Drag end commits the active thumb's value once.
        rx.send(SliderEvent::PointerUp);
        assert!(matches!(rx.state(), SliderState::Hover));
        let mut post = Vec::new();
        rx.drain_intents(&mut |i| post.push(i));
        let commits: Vec<_> = post
            .iter()
            .filter(|i| i.tag_str() == "value_committed")
            .collect();
        assert_eq!(commits.len(), 1, "exactly one commit on drag end");
    }

    #[test]
    fn pointer_cancel_releases_latch_without_commit() {
        let mut rx = RangeSliderExternal::with_values(0.2, 0.8);
        rx.send(SliderEvent::PointerEnter);
        rx.send(SliderEvent::PointerDown);
        rx.pointer_move(0.3, 0.0);
        let mut drained = Vec::new();
        rx.drain_intents(&mut |i| drained.push(i));
        rx.send(SliderEvent::PointerCancel);
        assert!(matches!(rx.state(), SliderState::Idle));
        let mut post = Vec::new();
        rx.drain_intents(&mut |i| post.push(i));
        assert!(
            post.iter().all(|i| i.tag_str() != "value_committed"),
            "PointerCancel must not commit"
        );
    }

    #[test]
    fn introspect_reports_both_values_and_active() {
        let rx = RangeSliderExternal::with_values(0.25, 0.75);
        let intro = External::introspect(&rx).expect("opts in");
        assert!(
            matches!(intro.query("low"), Some(IntrospectValue::Float(v)) if (v - 0.25).abs() < 1e-6)
        );
        assert!(
            matches!(intro.query("high"), Some(IntrospectValue::Float(v)) if (v - 0.75).abs() < 1e-6)
        );
        assert!(matches!(intro.query("active"), Some(IntrospectValue::Text(ref s)) if s == "high"));
        assert!(
            matches!(intro.query("orientation"), Some(IntrospectValue::Text(ref s)) if s == "horizontal")
        );
    }

    #[test]
    fn intervene_low_and_high_clamp_monotonic() {
        let mut rx = RangeSliderExternal::with_values(0.2, 0.8);
        let intro = External::introspect_mut(&mut rx).expect("opts in");
        intro.intervene("low", IntrospectValue::Float(0.5)).unwrap();
        assert!(approx(rx.low(), 0.5));
        let intro = External::introspect_mut(&mut rx).expect("opts in");
        // High write below the low thumb clamps to low.
        intro
            .intervene("high", IntrospectValue::Float(0.1))
            .unwrap();
        assert!(approx(rx.high(), 0.5), "high clamps at low");
    }

    #[test]
    fn intervene_state_and_active_are_read_only() {
        let mut rx = RangeSliderExternal::with_values(0.2, 0.8);
        let intro = External::introspect_mut(&mut rx).expect("opts in");
        assert!(matches!(
            intro.intervene("state", IntrospectValue::Text("Dragging".into())),
            Err(InterveneError::ReadOnly)
        ));
        let intro = External::introspect_mut(&mut rx).expect("opts in");
        assert!(matches!(
            intro.intervene("active", IntrospectValue::Text("low".into())),
            Err(InterveneError::ReadOnly)
        ));
    }

    #[test]
    fn vertical_pointer_inverts_y() {
        // Vertical axis inverts Y (ARIA `aria-orientation="vertical"`
        // top-is-max): y_rel = 0.0 (top) maps to value 1.0, y_rel = 1.0
        // (bottom) maps to value 0.0. Start from a [0.2, 0.8] window so
        // each drive is a *real* change, not a no-op against a bound.
        let mut rx = RangeSliderExternal::with_axis(SliderAxis::Vertical);
        // Seed the window via the admin setters (with_axis starts full-span).
        rx.set_low(0.2);
        rx.set_high(0.8);
        assert_eq!(rx.axis(), SliderAxis::Vertical);
        rx.send(SliderEvent::PointerEnter);
        rx.send(SliderEvent::PointerDown);
        // Top of the track (y_rel 0.0) → pos 1.0 → nearest is the high
        // thumb, driven up to 1.0 (a real move from 0.8).
        rx.pointer_move(0.0, 0.0);
        assert!(approx(rx.high(), 1.0), "top maps to value 1.0 (high thumb)");
        assert_eq!(rx.active(), ThumbId::High);
        // End the gesture and start a new one at the bottom (y_rel 1.0 →
        // pos 0.0): nearest is the low thumb, driven down to 0.0.
        rx.send(SliderEvent::PointerUp);
        rx.send(SliderEvent::PointerDown);
        rx.pointer_move(0.0, 1.0);
        assert!(
            approx(rx.low(), 0.0),
            "bottom maps to value 0.0 (low thumb)"
        );
        assert_eq!(rx.active(), ThumbId::Low);
    }

    #[test]
    fn thumb_id_name_round_trip() {
        assert_eq!(
            ThumbId::from_name(ThumbId::Low.as_name()),
            Some(ThumbId::Low)
        );
        assert_eq!(
            ThumbId::from_name(ThumbId::High.as_name()),
            Some(ThumbId::High)
        );
        assert_eq!(ThumbId::from_name("middle"), None);
    }
}
