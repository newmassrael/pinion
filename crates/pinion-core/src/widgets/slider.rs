//! R51.7 / R51.14 §5.38 — Slider widget: own self-contained
//! statechart (R51.14 cleanup of the R51.7 abstraction leak where
//! the shared button-like template was reused with a binding-layer
//! `Pressed = dragging` reinterpretation). State vocabulary now
//! reads cleanly at every layer: SCXML state `dragging`, Rust
//! [`SliderState::Dragging`], RPC `scene/query "state"` returns
//! `"Dragging"`. The f32 value sidecar (0.0..=1.0 normalised) stays
//! in the Rust binding — SCXML owns interaction state, the binding
//! owns the typed value (SCXML "null datamodel + typed Rust
//! sidecar" pattern).
//!
//! Value semantics split into two phases (Material / `SwiftUI` / Qt
//! convention):
//!
//! * **`value_changing`** — every effective [`Slider::set_value`]
//!   during drag emits a continuous intent. Applications can
//!   wire live previews / inline visual updates to this stream.
//! * **`value_committed`** — the `Dragging → Hover` activate
//!   transition (drag end via `PointerUp`) emits a single intent
//!   carrying the committed value. Applications wire model
//!   persistence / `onChangeEnd` semantics to this stream.

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
    include!(concat!(env!("OUT_DIR"), "/slider_sm.rs"));
}

pub use sm::{SliderEvent, SliderState};
// R738 §5.38 — the generated interaction policy is re-exported so the
// range-slider widget ([`crate::widgets::range_slider`]) can reuse the
// *identical* Idle/Hover/Dragging statechart instead of duplicating
// `slider.scxml` into a byte-for-byte `range_slider.scxml`. A dual-thumb
// slider's pointer interaction (enter → hover, down → dragging, up →
// hover, cancel → idle) is the same machine as a single-thumb slider's;
// the only difference (two values + an active thumb) is value-domain
// sidecar, not interaction state. Sharing the policy keeps the SCXML
// SSOT ([[sce-priority-over-pinion]]); see `range_slider.rs` for the
// rationale that distinguishes this from the R709/R734 "similar grammar
// ≠ shared statechart" caution (there the *transitions* diverged).
pub use sm::SliderPolicy;

// R645 §5.16 — pinion-side WidgetStateName + WidgetEventName impls
// for the sce-build-emitted enums. vendor/sce templates emit no
// pinion-side derives ([[sce-priority-over-pinion]]); the declarative
// macros below carry the impls. `state_name_derive` + `event_name_derive`
// on `#[widget(...)]` use these to drop the binding's per-binding
// `parse_slider_state` + `match` arms.
crate::widget_state_name!(
    SliderState,
    default = Idle,
    [Idle, Hover, Dragging, Disabled,]
);
crate::widget_event_name!(
    SliderEvent,
    external = [
        PointerEnter,
        PointerLeave,
        PointerDown,
        PointerUp,
        PointerCancel,
        Disable,
        Enable,
    ],
    internal = [SliderActivate, Null],
);

use crate::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner, ThreadOwnership,
};
use crate::intent::Intent;
use crate::widgets::{IntentEmitter, Widget, WidgetTransition};
use crate::{WidgetEventName, WidgetStateName};

/// R51.39 §5.38 — Slider track orientation. `Horizontal` (the
/// default) places the value progression along the X axis with `0.0`
/// at the left edge and `1.0` at the right edge; `Vertical` places
/// it along the Y axis with `0.0` at the *bottom* and `1.0` at the
/// *top* — the Material 3 / W3C ARIA `aria-orientation="vertical"`
/// convention (value max sits at the top, matching how humans read
/// "high" on a vertical scale). The axis is fixed at construction
/// time so the SCXML and the pointer-forward path don't need to
/// branch on a mutable state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SliderAxis {
    Horizontal,
    Vertical,
}

impl Default for SliderAxis {
    fn default() -> Self {
        Self::Horizontal
    }
}

/// Slider widget state machine + `f32` value sidecar
/// (0.0..=1.0 normalised). R51.14 own statechart with semantically
/// named `dragging` state (replaces the R51.7 `Pressed = dragging`
/// reinterpretation). R51.39 carries a [`SliderAxis`] so vertical
/// tracks can land without a widget-level breaking change — the
/// axis is fixed at construction; runtime orientation flips are
/// outside scope (the SCXML and ARIA semantics differ).
pub struct Slider {
    inner: Widget<SliderPolicy>,
    value: f32,
    axis: SliderAxis,
    /// R737 §5.38 — optional discrete snap increment in normalised
    /// (`0.0..=1.0`) units. `None` is the continuous slider (every
    /// real value reachable, the R51.7 default); `Some(s)` snaps every
    /// [`Self::set_value`] to the nearest multiple of `s` so a *single*
    /// snap funnel covers drag, keyboard, `intervene`, and RPC alike
    /// (the W3C ARIA discrete-slider / Material "tick mark" model).
    step: Option<f32>,
}

impl Slider {
    /// Construct a horizontal Slider in the `Idle` state with
    /// `value = 0.0`. Backwards-compat: pre-R51.39 callers see the
    /// same Horizontal default and need no migration.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Widget::new(),
            value: 0.0,
            axis: SliderAxis::Horizontal,
            step: None,
        }
    }

    /// R51.39 §5.38 — construct a Slider with an explicit
    /// [`SliderAxis`]. Use [`SliderAxis::Vertical`] for a vertical
    /// track; the pointer forward path then reads the Y cursor and
    /// inverts (top = 1.0, bottom = 0.0) per ARIA convention.
    #[must_use]
    pub fn with_axis(axis: SliderAxis) -> Self {
        Self {
            inner: Widget::new(),
            value: 0.0,
            axis,
            step: None,
        }
    }

    /// R737 §5.38 — make the slider *discrete*: snap every
    /// [`Self::set_value`] to the nearest multiple of `step`
    /// (normalised `0.0..=1.0` units, e.g. `0.2` for the six stops
    /// `0.0 / 0.2 / 0.4 / 0.6 / 0.8 / 1.0`). A non-positive or
    /// non-finite `step` is ignored (stays continuous), so a malformed
    /// argument can never freeze the value. Builder-style; chain after
    /// [`Self::new`] / [`Self::with_axis`]. The construction value is
    /// re-snapped so the initial readout already sits on a tick.
    #[must_use]
    pub fn with_step(mut self, step: f32) -> Self {
        self.step = (step.is_finite() && step > 0.0).then_some(step);
        // Re-snap the current value onto the new grid.
        let v = self.value;
        self.value = self.snap(v);
        self
    }

    /// R737 §5.38 — discrete snap increment (normalised units), or
    /// `None` for a continuous slider. Surfaced to the AI side through
    /// the `"step"` introspect field.
    #[must_use]
    pub fn step(&self) -> Option<f32> {
        self.step
    }

    /// R737 §5.38 — snap `v` (already caller-clamped or not) to the
    /// nearest discrete tick when [`Self::step`] is set, else return it
    /// clamped. The single snap primitive every value path funnels
    /// through ([`Self::set_value`]).
    fn snap(&self, v: f32) -> f32 {
        let clamped = v.clamp(0.0, 1.0);
        match self.step {
            Some(s) if s > 0.0 => ((clamped / s).round() * s).clamp(0.0, 1.0),
            _ => clamped,
        }
    }

    /// R51.39 §5.38 — track orientation, fixed at construction.
    /// Drives the `pointer_move` axis dispatch and the
    /// `"orientation"` introspect field surfaced to the AI side.
    #[must_use]
    pub fn axis(&self) -> SliderAxis {
        self.axis
    }

    /// Drive a [`SliderEvent`] through the SCXML. Pure state
    /// transition — value mutation flows through [`set_value`].
    pub fn send(&mut self, event: SliderEvent) {
        self.inner.send(event);
    }

    /// Current interaction state.
    #[must_use]
    pub fn state(&self) -> SliderState {
        self.inner.state()
    }

    /// Current normalised value (`0.0..=1.0`).
    #[must_use]
    pub fn value(&self) -> f32 {
        self.value
    }

    /// Set the value, clamping to `0.0..=1.0`. Returns `true` if the
    /// stored value actually changed (caller can use the return for
    /// gate-by-effect intent emission). State-independent — the
    /// caller (winit pointer handler, RPC `intervene` path, ...)
    /// decides when a change applies; the widget does not gate on
    /// `state() == Pressed` so non-drag programmatic updates
    /// (preference restore, keyboard step) work too.
    pub fn set_value(&mut self, v: f32) -> bool {
        // R737 §5.38 — funnel every value mutation through the snap
        // primitive, so a discrete slider snaps identically whether the
        // value arrives from a drag (`pointer_move`), a keyboard step,
        // `intervene`, or an RPC write. Continuous sliders snap to a
        // no-op (clamp only), preserving the pre-R737 behaviour.
        let snapped = self.snap(v);
        if (snapped - self.value).abs() < f32::EPSILON {
            return false;
        }
        self.value = snapped;
        true
    }
}

impl Default for Slider {
    fn default() -> Self {
        Self::new()
    }
}

/// R51.12 §5.38 — Slider transition contract. Snapshot tuples the
/// interaction state with the f32 value sidecar so detect can carry
/// the committed value in the payload. The `Dragging → Hover`
/// activate path (drag end via `PointerUp`) emits the
/// `"value_committed"` intent carrying `after`'s value as
/// [`IntrospectValue::Float`]; the live-preview `"value_changing"`
/// stream is **not** part of this contract — those intents fire from
/// [`SliderExternal::set_value`] (a direct value mutation, not a
/// transition) and use [`IntentEmitter::push`] outside the
/// `dispatch` pipeline.
impl WidgetTransition for Slider {
    type Event = SliderEvent;
    type Snapshot = (SliderState, f32);

    fn snapshot(&self) -> Self::Snapshot {
        (self.state(), self.value())
    }

    fn drive(&mut self, event: Self::Event) {
        self.send(event);
    }

    fn detect(before: Self::Snapshot, _event: Self::Event, after: Self::Snapshot) -> Vec<Intent> {
        let (before_state, _) = before;
        let (after_state, after_value) = after;
        if matches!(before_state, SliderState::Dragging)
            && matches!(after_state, SliderState::Hover)
        {
            vec![Intent::new_static(
                "value_committed",
                IntrospectValue::Float(f64::from(after_value)),
            )]
        } else {
            Vec::new()
        }
    }
}

/// `External` adapter wrapping a [`Slider`]. Emits two intent
/// kinds:
///
/// * `"value_changing"` carrying `IntrospectValue::Float(value)` on
///   every effective [`Self::set_value`] (live preview channel).
/// * `"value_committed"` carrying `IntrospectValue::Float(value)`
///   on `Dragging → Hover` activate (drag-end commit channel).
pub struct SliderExternal {
    em: IntentEmitter<Slider>,
}

impl SliderExternal {
    /// Construct a horizontal Slider external. Backwards-compat: the
    /// pre-R51.39 default — no vertical-axis change for existing
    /// callers (hello-slider, RPC clients, integration tests).
    #[must_use]
    pub fn new() -> Self {
        Self {
            em: IntentEmitter::default(),
        }
    }

    /// R51.39 §5.38 — construct a Slider external with an explicit
    /// [`SliderAxis`]. Wraps a [`Slider::with_axis`] under the
    /// intent emitter so the `pointer_move` forward picks the
    /// correct axis and the `"orientation"` introspect field
    /// reports the right ARIA-aligned string.
    #[must_use]
    pub fn with_axis(axis: SliderAxis) -> Self {
        Self {
            em: IntentEmitter::new(Slider::with_axis(axis)),
        }
    }

    /// R737 §5.38 — construct a *discrete* horizontal Slider external
    /// that snaps to multiples of `step` (normalised units). Wraps a
    /// [`Slider::new`]`.with_step(step)`; the `"step"` introspect field
    /// then reports the increment so an AI client (or the binding's
    /// tick-mark paint) can enumerate the stops. Combine with a
    /// vertical axis via [`Self::with_axis_step`].
    #[must_use]
    pub fn with_step(step: f32) -> Self {
        Self {
            em: IntentEmitter::new(Slider::new().with_step(step)),
        }
    }

    /// R737 §5.38 — discrete Slider external on an explicit
    /// [`SliderAxis`] (the `with_axis` + `with_step` combination).
    #[must_use]
    pub fn with_axis_step(axis: SliderAxis, step: f32) -> Self {
        Self {
            em: IntentEmitter::new(Slider::with_axis(axis).with_step(step)),
        }
    }

    /// R51.39 §5.38 — track orientation (delegates to
    /// [`Slider::axis`]). Diagnostic / test surface; consumers
    /// usually read the introspect `"orientation"` field instead.
    #[must_use]
    pub fn axis(&self) -> SliderAxis {
        self.em.inner.axis()
    }

    /// R737 §5.38 — discrete snap increment (delegates to
    /// [`Slider::step`]). `None` for a continuous slider. Diagnostic /
    /// test surface; consumers usually read the introspect `"step"`
    /// field instead.
    #[must_use]
    pub fn step(&self) -> Option<f32> {
        self.em.inner.step()
    }

    /// Drive a [`SliderEvent`] and queue a `"value_committed"`
    /// intent on drag-end (`Dragging → Hover`).
    ///
    /// R51.12 §5.38 refactor: pipeline on [`IntentEmitter::dispatch`],
    /// detection rule on [`WidgetTransition`] impl for [`Slider`].
    /// The live-preview `"value_changing"` channel still goes through
    /// [`Self::set_value`] directly (direct value mutation, not a
    /// state transition).
    pub fn send(&mut self, event: SliderEvent) {
        self.em.dispatch(event);
    }

    /// Set the value and queue a `"value_changing"` intent on
    /// effective change.
    pub fn set_value(&mut self, v: f32) {
        if self.em.inner.set_value(v) {
            self.em.push(Intent::new_static(
                "value_changing",
                IntrospectValue::Float(f64::from(self.em.inner.value())),
            ));
        }
    }

    /// Current interaction state.
    #[must_use]
    pub fn state(&self) -> SliderState {
        self.em.inner.state()
    }

    /// Current normalised value.
    #[must_use]
    pub fn value(&self) -> f32 {
        self.em.inner.value()
    }
}

impl Default for SliderExternal {
    fn default() -> Self {
        Self::new()
    }
}

impl core::fmt::Debug for SliderExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SliderExternal")
            .field("state", &self.state())
            .field("value", &self.value())
            .finish()
    }
}

impl External for SliderExternal {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui, Backend::Rpc], BackendFallback::Skip)
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }

    /// R51.35 §5.15 + §5.35 — opt in to capture lock so the
    /// framework's [`InputRouter`](pinion_runtime::InputRouter) keeps
    /// the cursor pinned to this Slider for the duration of the
    /// `pointer_down` → `pointer_up` span, even when the cursor
    /// strays outside the widget's track rect. Required for the
    /// canonical drag UX (Material / `SwiftUI` / Qt): the user can
    /// drag past the track ends without the press cancelling.
    fn wants_pointer_capture(&self) -> bool {
        true
    }

    /// R51.35 §5.15 + §5.35 + R51.39 §5.38 — feed the widget-
    /// relative cursor along the [`SliderAxis`] into the f32 value
    /// sidecar, clamping to the [`Slider`]'s `0.0..=1.0` range.
    /// The framework forwards both press-time (click-to-position)
    /// and drag-time motion through this hook.
    ///
    /// * **Horizontal** (R51.35 default): `x_rel` drives the value,
    ///   `y_rel` is ignored. Left edge maps to `0.0`, right edge to
    ///   `1.0`.
    /// * **Vertical** (R51.39): `y_rel` drives the value with an
    ///   inversion — top edge (`y_rel = 0.0`) maps to `1.0` and
    ///   bottom edge (`y_rel = 1.0`) maps to `0.0`, matching the
    ///   Material 3 / W3C ARIA `aria-orientation="vertical"`
    ///   convention. `x_rel` is ignored.
    ///
    /// The clamping is intentional: either axis may exceed
    /// `[0.0, 1.0]` or go negative when the cursor strays off the
    /// track rect under capture lock (R51.34 design point).
    /// Clamping here preserves the `value_changing` intent's
    /// gate-by-effect semantics — strays past the saturated value
    /// are silent.
    fn pointer_move(&mut self, x_rel: f32, y_rel: f32) {
        let value_axis = match self.em.inner.axis() {
            SliderAxis::Horizontal => x_rel,
            SliderAxis::Vertical => 1.0 - y_rel,
        };
        self.set_value(value_axis.clamp(0.0, 1.0));
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

impl ExternalIntrospect for SliderExternal {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(&[
            ("state", "string"),
            ("value", "float"),
            ("orientation", "string"),
            // R737 §5.38 — discrete snap increment (normalised units);
            // `0.0` is the continuous-slider sentinel.
            ("step", "float"),
            ("send", "string"),
        ])
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "state" => Some(IntrospectValue::Text(self.state().as_name().to_string())),
            "value" => Some(IntrospectValue::Float(f64::from(self.value()))),
            "orientation" => Some(IntrospectValue::Text(
                slider_axis_name(self.axis()).to_string(),
            )),
            // R737 §5.38 — `0.0` sentinel = continuous (no snap); any
            // positive value is the normalised tick increment.
            "step" => Some(IntrospectValue::Float(f64::from(
                self.step().unwrap_or(0.0),
            ))),
            _ => None,
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            // R51.39 §5.38 — `state` is SCXML-owned (the framework
            // drives it via `send`), and `orientation` is
            // construction-time fixed (the SCXML and ARIA semantics
            // differ between axes; a runtime flip would change the
            // meaning of in-flight intent emissions and the
            // introspect type contract). Both reject intervene.
            // R737 §5.38 — `step` is construction-fixed (like
            // `orientation`): a runtime grid change would re-snap
            // in-flight values and shift the introspect contract.
            "state" | "orientation" | "step" => Err(InterveneError::ReadOnly),
            "value" => match value {
                IntrospectValue::Float(v) => {
                    // f64 → f32 narrowing is deliberate: the wire
                    // type is f64 (IntrospectValue::Float), the
                    // stored type is f32 (Slider::set_value
                    // signature). Clamping happens inside set_value.
                    #[allow(clippy::cast_possible_truncation)]
                    self.set_value(v as f32);
                    Ok(())
                }
                IntrospectValue::Int(i) => {
                    #[allow(clippy::cast_precision_loss)]
                    self.set_value(i as f32);
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            },
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

/// R51.39 §5.38 — [`SliderAxis`] → introspect-surfaced
/// `"orientation"` string. Lowercased per the W3C `aria-orientation`
/// attribute convention (`"horizontal"` / `"vertical"`) so AI clients
/// observing the introspect schema can map the field straight to
/// ARIA without re-casing.
fn slider_axis_name(axis: SliderAxis) -> &'static str {
    match axis {
        SliderAxis::Horizontal => "horizontal",
        SliderAxis::Vertical => "vertical",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_state_is_idle_zero() {
        let s = Slider::new();
        assert_eq!(s.state(), SliderState::Idle);
        assert!((s.value() - 0.0).abs() < f32::EPSILON);
    }

    // R51.93 §5.35 — touch-cancel during a drag must NOT fire
    // `value_committed`. `value_changing` intents from the in-flight
    // `set_value` calls are still legitimate (the value really did
    // change during the drag), but the commit signal is suppressed.

    #[test]
    fn r51_93_pointer_cancel_during_drag_returns_to_idle_without_commit() {
        let mut sx = SliderExternal::new();
        sx.send(SliderEvent::PointerEnter);
        sx.send(SliderEvent::PointerDown);
        assert!(matches!(sx.state(), SliderState::Dragging));
        sx.set_value(0.5);
        // Drain the value_changing intents emitted so far so the
        // post-cancel `is_dirty` cleanly reports the commit absence.
        let mut harvested = Vec::new();
        sx.drain_intents(&mut |i| harvested.push(i));
        assert!(harvested.iter().all(|i| i.tag_str() == "value_changing"));
        sx.send(SliderEvent::PointerCancel);
        assert!(matches!(sx.state(), SliderState::Idle));
        // R51.93 §5.35 documented invariant: the in-flight `set_value`
        // calls during the drag stay applied — the OS revoked the
        // commit signal, not the user's in-flight drag updates.
        // Slider value is a continuous-domain sidecar, not a
        // commit-bound enum; the value_changing intents already
        // emitted are honest reports of where the drag was. Only
        // the commit (value_committed) is suppressed.
        assert!(
            (sx.value() - 0.5).abs() < f32::EPSILON,
            "set_value during drag stays applied across PointerCancel"
        );
        // No `value_committed` intent in the post-cancel drain.
        let mut post = Vec::new();
        sx.drain_intents(&mut |i| post.push(i));
        assert!(
            post.iter().all(|i| i.tag_str() != "value_committed"),
            "PointerCancel from Dragging must not fire value_committed"
        );
    }

    #[test]
    fn r51_93_parse_pointer_cancel_event_name() {
        assert_eq!(
            SliderEvent::from_name("PointerCancel"),
            Some(SliderEvent::PointerCancel)
        );
        // R699 §5.16 — slider has no KeyboardActivate; internal raise +
        // Null + unknown all reject from the external-drivable set.
        assert_eq!(SliderEvent::from_name("KeyboardActivate"), None);
        assert_eq!(SliderEvent::from_name("SliderActivate"), None);
        assert_eq!(SliderEvent::from_name("Null"), None);
        assert_eq!(SliderEvent::SliderActivate.as_name(), "SliderActivate");
    }

    #[test]
    fn set_value_clamps() {
        let mut s = Slider::new();
        s.set_value(1.5);
        assert!((s.value() - 1.0).abs() < f32::EPSILON);
        s.set_value(-0.5);
        assert!((s.value() - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn set_value_returns_false_on_no_op() {
        let mut s = Slider::new();
        assert!(s.set_value(0.5));
        assert!(!s.set_value(0.5), "same value is no-op");
    }

    #[test]
    fn full_drag_cycle_commits_on_pointer_up() {
        let mut sx = SliderExternal::new();
        sx.send(SliderEvent::PointerEnter);
        sx.send(SliderEvent::PointerDown);
        sx.set_value(0.3);
        sx.set_value(0.6);
        sx.set_value(0.8);
        let mut harvested = Vec::new();
        sx.drain_intents(&mut |i| harvested.push(i));
        assert_eq!(harvested.len(), 3, "3 value_changing intents");
        assert!(harvested.iter().all(|i| i.tag_str() == "value_changing"));

        sx.send(SliderEvent::PointerUp);
        let mut harvested = Vec::new();
        sx.drain_intents(&mut |i| harvested.push(i));
        assert_eq!(harvested.len(), 1);
        assert_eq!(harvested[0].tag_str(), "value_committed");
        let IntrospectValue::Float(v) = harvested[0].payload else {
            panic!("expected Float payload");
        };
        assert!((v - 0.8_f64).abs() < 1e-4, "got {v}");
    }

    #[test]
    fn cancel_drag_does_not_commit() {
        let mut sx = SliderExternal::new();
        sx.send(SliderEvent::PointerEnter);
        sx.send(SliderEvent::PointerDown);
        sx.set_value(0.5);
        sx.send(SliderEvent::PointerLeave);
        let mut harvested = Vec::new();
        sx.drain_intents(&mut |i| harvested.push(i));
        assert_eq!(
            harvested.len(),
            1,
            "only value_changing during drag, no commit on cancel"
        );
        assert_eq!(harvested[0].tag_str(), "value_changing");
    }

    #[test]
    fn set_value_no_op_emits_no_intent() {
        let mut sx = SliderExternal::new();
        sx.set_value(0.5);
        sx.set_value(0.5);
        let mut harvested = Vec::new();
        sx.drain_intents(&mut |i| harvested.push(i));
        assert_eq!(harvested.len(), 1, "second set_value is no-op");
    }

    #[test]
    fn external_query_state_and_value() {
        let mut sx = SliderExternal::new();
        sx.set_value(0.42);
        assert_eq!(
            sx.query("value").unwrap(),
            IntrospectValue::Float(f64::from(0.42_f32))
        );
        assert_eq!(
            sx.query("state").unwrap(),
            IntrospectValue::Text("Idle".to_string())
        );
    }

    #[test]
    fn external_intervene_value_float() {
        let mut sx = SliderExternal::new();
        let r = sx.intervene("value", IntrospectValue::Float(0.7));
        assert!(r.is_ok());
        assert!((sx.value() - 0.7).abs() < 1e-4);
    }

    #[test]
    fn external_intervene_value_int() {
        let mut sx = SliderExternal::new();
        // i64 → f32; 1 then clamp to 1.0
        let r = sx.intervene("value", IntrospectValue::Int(1));
        assert!(r.is_ok());
        assert!((sx.value() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn external_intervene_state_read_only() {
        let mut sx = SliderExternal::new();
        let r = sx.intervene("state", IntrospectValue::Text("Pressed".to_string()));
        assert_eq!(r, Err(InterveneError::ReadOnly));
    }

    #[test]
    fn external_invoke_send_drives_transition() {
        let mut sx = SliderExternal::new();
        let out = sx
            .invoke("send", IntrospectValue::Text("PointerEnter".to_string()))
            .unwrap();
        assert_eq!(out, IntrospectValue::Text("Hover".to_string()));
    }

    #[test]
    fn external_schema_declares_five_slots() {
        // R51.39 §5.38 — schema grew an `orientation` field; R737 §5.38
        // added the discrete `step` field after it. The legacy
        // `state`/`value`/`send` triple is preserved in the declaration
        // order pre-R51.39 callers observed (the additive fields slot in
        // before `send`).
        let sx = SliderExternal::new();
        let schema = sx.schema();
        assert_eq!(
            schema.fields,
            &[
                ("state", "string"),
                ("value", "float"),
                ("orientation", "string"),
                ("step", "float"),
                ("send", "string"),
            ]
        );
    }

    #[test]
    fn external_wants_pointer_capture() {
        // R51.35 §5.15 + §5.35 — Slider opts in so the framework's
        // InputRouter pins the cursor across the drag.
        let sx = SliderExternal::new();
        assert!(sx.wants_pointer_capture());
    }

    #[test]
    fn external_pointer_move_sets_value_clamped() {
        // R51.35 §5.35 — widget-relative cursor X drives the value.
        // y_rel is ignored (horizontal slider). Coordinates outside
        // [0, 1] clamp.
        let mut sx = SliderExternal::new();
        sx.pointer_move(0.25, 0.99);
        assert!((sx.value() - 0.25).abs() < 1e-4);
        sx.pointer_move(1.7, -0.4); // x clamps to 1.0
        assert!((sx.value() - 1.0).abs() < 1e-4);
        sx.pointer_move(-0.3, 0.5); // x clamps to 0.0
        assert!((sx.value() - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn external_pointer_move_emits_value_changing_intent() {
        // Each effective pointer_move (post-clamp value actually
        // changed) emits one value_changing intent on the §5.20
        // channel — same gate-by-effect path as intervene("value", ...)
        // because both flow through set_value.
        let mut sx = SliderExternal::new();
        sx.pointer_move(0.3, 0.0);
        sx.pointer_move(0.7, 0.0);
        sx.pointer_move(0.7, 0.0); // no-op (same value)
        let mut harvested = Vec::new();
        sx.drain_intents(&mut |i| harvested.push(i));
        assert_eq!(harvested.len(), 2);
        assert!(harvested.iter().all(|i| i.tag_str() == "value_changing"));
    }

    #[test]
    fn external_pointer_move_then_pointer_up_commits_value() {
        // End-to-end drag-end via the R51.34 + R51.35 path: capture
        // entered on PointerDown, value flows through pointer_move,
        // PointerUp triggers the Dragging→Hover transition that
        // emits value_committed carrying the final value.
        let mut sx = SliderExternal::new();
        sx.send(SliderEvent::PointerEnter);
        sx.send(SliderEvent::PointerDown);
        sx.pointer_move(0.42, 0.0);
        sx.send(SliderEvent::PointerUp);
        let mut harvested = Vec::new();
        sx.drain_intents(&mut |i| harvested.push(i));
        // Two intents: value_changing on the move, value_committed on
        // the drag-end transition.
        assert_eq!(harvested.len(), 2);
        assert_eq!(harvested[0].tag_str(), "value_changing");
        assert_eq!(harvested[1].tag_str(), "value_committed");
        let IntrospectValue::Float(v) = harvested[1].payload else {
            panic!("expected Float payload");
        };
        assert!((v - 0.42_f64).abs() < 1e-4, "committed {v}");
    }

    // ─── R51.39 §5.38 vertical axis future-proof ─────────────

    #[test]
    fn default_axis_is_horizontal() {
        // Backwards-compat invariant: `Slider::new` / default ctor
        // both yield a Horizontal track. Pre-R51.39 callers see no
        // behaviour change.
        assert_eq!(Slider::new().axis(), SliderAxis::Horizontal);
        assert_eq!(SliderExternal::new().axis(), SliderAxis::Horizontal);
        assert_eq!(SliderAxis::default(), SliderAxis::Horizontal);
    }

    #[test]
    fn with_axis_pins_orientation_at_construction() {
        // R51.39 §5.38 — the builder threads the axis through to
        // both Slider and SliderExternal without mutability.
        assert_eq!(
            Slider::with_axis(SliderAxis::Vertical).axis(),
            SliderAxis::Vertical,
        );
        assert_eq!(
            SliderExternal::with_axis(SliderAxis::Vertical).axis(),
            SliderAxis::Vertical,
        );
    }

    #[test]
    fn horizontal_pointer_move_reads_x_rel() {
        // Default Horizontal axis: x_rel drives the value, y_rel is
        // ignored. Regression guard against the R51.35 contract.
        let mut sx = SliderExternal::new();
        sx.pointer_move(0.7, 0.2);
        assert!((sx.value() - 0.7).abs() < 1e-4);
        // Vary y_rel — value must not move.
        sx.pointer_move(0.7, 0.9);
        assert!((sx.value() - 0.7).abs() < 1e-4);
    }

    #[test]
    fn vertical_pointer_move_inverts_y_rel() {
        // Vertical axis: value = 1.0 - y_rel (ARIA convention, top
        // = max). x_rel is ignored.
        let mut sx = SliderExternal::with_axis(SliderAxis::Vertical);
        sx.pointer_move(0.0, 0.0); // top edge
        assert!((sx.value() - 1.0).abs() < 1e-4);
        sx.pointer_move(0.0, 1.0); // bottom edge
        assert!((sx.value() - 0.0).abs() < 1e-4);
        sx.pointer_move(0.0, 0.3); // 30% from top → value 0.7
        assert!((sx.value() - 0.7).abs() < 1e-4);
        // Vary x_rel — value must not move.
        sx.pointer_move(0.5, 0.3);
        assert!((sx.value() - 0.7).abs() < 1e-4);
    }

    #[test]
    fn vertical_pointer_move_clamps_outside_rect() {
        // Cursor stray past either edge under capture lock — the
        // resulting `1.0 - y_rel` may go negative or exceed 1.0;
        // the `clamp(0.0, 1.0)` in pointer_move saturates.
        let mut sx = SliderExternal::with_axis(SliderAxis::Vertical);
        sx.pointer_move(0.0, -0.5); // above top → 1.5, clamps to 1.0
        assert!((sx.value() - 1.0).abs() < 1e-4);
        sx.pointer_move(0.0, 1.7); // below bottom → -0.7, clamps to 0.0
        assert!((sx.value() - 0.0).abs() < 1e-4);
    }

    #[test]
    fn orientation_query_returns_aria_string() {
        // R51.39 §5.38 — introspect `"orientation"` exposes the
        // axis as a W3C `aria-orientation`-aligned lowercase string
        // ("horizontal" / "vertical"). AI clients consume this
        // straight through to their ARIA model.
        let h = SliderExternal::new();
        let v = SliderExternal::with_axis(SliderAxis::Vertical);
        assert_eq!(
            h.query("orientation"),
            Some(IntrospectValue::Text("horizontal".to_string())),
        );
        assert_eq!(
            v.query("orientation"),
            Some(IntrospectValue::Text("vertical".to_string())),
        );
    }

    #[test]
    fn orientation_intervene_is_read_only() {
        // Axis is construction-time fixed; the SCXML and ARIA
        // semantics differ between axes, so a runtime flip would
        // break in-flight intent contracts. The intervene gate
        // matches `"state"` (also construction-anchored).
        let mut sx = SliderExternal::new();
        let r = sx.intervene("orientation", IntrospectValue::Text("vertical".to_string()));
        assert_eq!(r, Err(InterveneError::ReadOnly));
        // Original axis untouched.
        assert_eq!(sx.axis(), SliderAxis::Horizontal);
    }

    #[test]
    fn schema_lists_orientation_field() {
        // The introspect schema must include the new field so
        // schema-driven AI clients pick it up automatically.
        let sx = SliderExternal::new();
        let schema = sx.schema();
        let fields: Vec<&str> = schema.fields.iter().map(|(n, _)| *n).collect();
        assert!(fields.contains(&"orientation"), "fields = {fields:?}");
    }
}
