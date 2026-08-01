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

// SCE-002 §5.16 — the `WidgetStateName` / `WidgetEventName` impls for the
// sce-generated `SliderState` / `SliderEvent` enums are injected as
// `#[derive]`s by `build.rs` (`compile_scxml_with_derives`), reconstructed
// from the codegen's `#[default]` state + `EXTERNALLY_DRIVABLE_EVENTS`
// const (see `pinion-derive`); the per-widget `widget_{state,event}_name!`
// macros are retired. Bindings still opt into the derived
// `WidgetCore::read_state` + `WidgetCore::event_name` via
// `state_name_derive` + `event_name_derive` on `#[widget(...)]`.

use crate::event::WheelStepper;
use crate::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner, SchemaField, ThreadOwnership,
};
use crate::input::Modifiers;
use crate::intent::Intent;
use crate::widgets::{IntentEmitter, Widget, WidgetTransition};
use crate::{WidgetEventName, WidgetStateName};

/// R1533 §5.45 §5.38 — one wheel notch on a **continuous** slider, in
/// normalised units.
///
/// A discrete slider steps by its own [`Slider::step`]; a continuous one has
/// no such unit, and Qt offers no guidance because a `QSlider` is always an
/// integer range. 5% is the small step every slider binding in this repo
/// already spells in its arrow-key map (`hello-slider`, `hello-scrubber`,
/// `settings-panel`), which is the property worth preserving: Qt ties the
/// wheel and the arrow keys to ONE `singleStep`, so a wheel notch and an
/// `ArrowRight` moving the same distance is the contract, not a coincidence.
///
/// A per-slider override (Qt's `setSingleStep`) is the natural extension the
/// moment a binding wants a different one; none does today, so the constant
/// is not yet a builder ([[abstraction-needs-second-consumer]]).
pub const CONTINUOUS_WHEEL_STEP: f32 = 0.05;

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
    /// transition — value mutation flows through [`set_value`](Self::set_value).
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
                crate::widgets::commit::VALUE_COMMITTED_EVENT,
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
    /// R1533 §5.45 — sub-notch wheel carry (see [`Self::wheel`]). Per
    /// instance, exactly as each Qt `QAbstractSlider` owns its own
    /// `offset_accumulated`: two sliders on one screen must not spend each
    /// other's banked motion.
    wheel: WheelStepper,
}

impl SliderExternal {
    /// R1533 — the one place the non-statechart fields are initialised, so a
    /// fifth constructor cannot forget one.
    fn from_em(em: IntentEmitter<Slider>) -> Self {
        Self {
            em,
            wheel: WheelStepper::new(),
        }
    }

    /// Construct a horizontal Slider external. Backwards-compat: the
    /// pre-R51.39 default — no vertical-axis change for existing
    /// callers (hello-slider, RPC clients, integration tests).
    #[must_use]
    pub fn new() -> Self {
        Self::from_em(IntentEmitter::default())
    }

    /// R51.39 §5.38 — construct a Slider external with an explicit
    /// [`SliderAxis`]. Wraps a [`Slider::with_axis`] under the
    /// intent emitter so the `pointer_move` forward picks the
    /// correct axis and the `"orientation"` introspect field
    /// reports the right ARIA-aligned string.
    #[must_use]
    pub fn with_axis(axis: SliderAxis) -> Self {
        Self::from_em(IntentEmitter::new(Slider::with_axis(axis)))
    }

    /// R737 §5.38 — construct a *discrete* horizontal Slider external
    /// that snaps to multiples of `step` (normalised units). Wraps a
    /// [`Slider::new`]`.with_step(step)`; the `"step"` introspect field
    /// then reports the increment so an AI client (or the binding's
    /// tick-mark paint) can enumerate the stops. Combine with a
    /// vertical axis via [`Self::with_axis_step`].
    #[must_use]
    pub fn with_step(step: f32) -> Self {
        Self::from_em(IntentEmitter::new(Slider::new().with_step(step)))
    }

    /// R737 §5.38 — discrete Slider external on an explicit
    /// [`SliderAxis`] (the `with_axis` + `with_step` combination).
    #[must_use]
    pub fn with_axis_step(axis: SliderAxis, step: f32) -> Self {
        Self::from_em(IntentEmitter::new(Slider::with_axis(axis).with_step(step)))
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
    /// effective change. Returns whether the stored value actually moved,
    /// mirroring [`Slider::set_value`]'s own gate-by-effect return — R1533
    /// needs it, because a wheel that pushed an already-saturated value has
    /// to decline the event rather than swallow it (see [`Self::wheel`]).
    pub fn set_value(&mut self, v: f32) -> bool {
        let changed = self.em.inner.set_value(v);
        if changed {
            self.em.push(Intent::new_static(
                "value_changing",
                IntrospectValue::Float(f64::from(self.em.inner.value())),
            ));
        }
        changed
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
    /// framework's `InputRouter` keeps
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

    /// R1533 §5.45 §5.38 — the wheel steps the value: Qt
    /// `QAbstractSlider::wheelEvent`, and the reason every volume slider,
    /// zoom slider and DCC parameter track in a desktop tool answers a
    /// wheel without being clicked first.
    ///
    /// One notch ([`LINE_HEIGHT_PX`](crate::event::LINE_HEIGHT_PX) pixels) is
    /// one [`CONTINUOUS_WHEEL_STEP`] on a continuous slider, or one snap
    /// [`Slider::step`] on a discrete one — so a discrete slider walks its
    /// own stops and cannot land between them. Qt reaches the same place
    /// from the other side: there `singleStep` *is* the wheel step, and its
    /// slider is an integer range whose unit is that step.
    ///
    /// Sub-notch motion banks in a [`WheelStepper`] rather than rounding to
    /// nothing, so a trackpad moves the slider at all.
    ///
    /// Deliberately NOT Qt's `wheelScrollLines` multiplier (Qt travels
    /// **three** single-steps a notch): that constant exists because a Qt
    /// slider's step is usually 1 of a 0..99 range, whereas a step here is a
    /// normalised fraction the binding chose — `hello-slider-discrete` has
    /// six stops, and three of them a notch is not a slider, it is a jump.
    ///
    /// Only the **vertical** wheel axis is read, on both orientations. That
    /// is the axis every mouse has, and it keeps "wheel forward raises the
    /// value" true for a vertical and a horizontal track alike (Qt
    /// normalises orientation for the same reason). A horizontal wheel /
    /// trackpad axis on a horizontal track is a further refinement, not a
    /// different rule.
    ///
    /// Returns the [`WheelStepper`] verdict — consume while banking or
    /// stepping, **decline** once saturated so the wheel this slider cannot
    /// use reaches the scroll container behind it.
    fn wheel(
        &mut self,
        _x_rel: f32,
        _y_rel: f32,
        _dx: f32,
        dy: f32,
        _modifiers: Modifiers,
    ) -> bool {
        let notches = self.wheel.feed(dy);
        if notches == 0 {
            return true;
        }
        let step = self.em.inner.step().unwrap_or(CONTINUOUS_WHEEL_STEP);
        // W3C sign: a positive `dy` scrolls DOWN, so a wheel pushed forward
        // arrives negative and must RAISE the value.
        #[allow(
            clippy::cast_precision_loss,
            reason = "a notch count large enough to lose f32 precision is \
                      millions of screens of wheel in one event; the value it \
                      derives is clamped to [0, 1] either way"
        )]
        let target = self.em.inner.value() - notches as f32 * step;
        if !self.set_value(target) {
            self.wheel.reset();
            return false;
        }
        // A notch is atomic — there is no press to release — so the value it
        // leaves is settled, and the commit channel has to say so or a
        // consumer that persists / seeks on commit only (`hello-scrubber`,
        // `settings-panel`) would see the thumb move and never act. Qt's
        // wheel likewise emits `valueChanged`, not just `sliderMoved`.
        self.em.push(Intent::new_static(
            crate::widgets::commit::VALUE_COMMITTED_EVENT,
            IntrospectValue::Float(f64::from(self.em.inner.value())),
        ));
        true
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
        IntrospectSchema::new(
            const {
                &[
                    SchemaField::new("state", "string"),
                    SchemaField::new("value", "float"),
                    SchemaField::new("orientation", "string"),
                    // R737 §5.38 — discrete snap increment (normalised units);
                    // `0.0` is the continuous-slider sentinel.
                    SchemaField::new("step", "float"),
                    SchemaField::new("send", "string"),
                ]
            },
        )
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
                SchemaField::new("state", "string"),
                SchemaField::new("value", "float"),
                SchemaField::new("orientation", "string"),
                SchemaField::new("step", "float"),
                SchemaField::new("send", "string"),
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
        let fields: Vec<&str> = schema.fields.iter().map(|f| f.path).collect();
        assert!(fields.contains(&"orientation"), "fields = {fields:?}");
    }

    // ─────────────────────────────────────────────────────────────────
    // R1533 §5.45 §5.38 — the wheel steps the value (Qt
    // `QAbstractSlider::wheelEvent`).
    // ─────────────────────────────────────────────────────────────────

    /// One notch of forward wheel, in the pixel units the router hands out.
    /// Negative per the W3C sign convention (`dy > 0` scrolls DOWN), which is
    /// the half a sign error gets wrong.
    const NOTCH_UP: f32 = -crate::event::LINE_HEIGHT_PX;
    const NOTCH_DOWN: f32 = crate::event::LINE_HEIGHT_PX;

    fn wheel(sx: &mut SliderExternal, dy: f32) -> bool {
        External::wheel(sx, 0.5, 0.5, 0.0, dy, Modifiers::empty())
    }

    #[test]
    fn r1533_forward_wheel_raises_a_continuous_slider_by_one_step() {
        let mut sx = SliderExternal::new();
        sx.set_value(0.5);
        assert!(wheel(&mut sx, NOTCH_UP), "the slider consumed the wheel");
        assert!(
            (sx.value() - (0.5 + CONTINUOUS_WHEEL_STEP)).abs() < 1e-6,
            "forward wheel RAISES the value: 0.5 -> {}",
            sx.value()
        );
        assert!(wheel(&mut sx, NOTCH_DOWN));
        assert!(
            (sx.value() - 0.5).abs() < 1e-6,
            "and back the other way: {}",
            sx.value()
        );
    }

    #[test]
    fn r1533_a_discrete_slider_walks_its_own_stops() {
        // The property that makes the step the widget's and not the wheel's:
        // a 5-stop slider must land ON a stop, never 5% away from one.
        let mut sx = SliderExternal::with_step(0.2);
        sx.set_value(0.4);
        assert!(wheel(&mut sx, NOTCH_UP));
        assert!(
            (sx.value() - 0.6).abs() < 1e-6,
            "one notch = one stop, got {}",
            sx.value()
        );
        assert!(wheel(&mut sx, NOTCH_UP));
        assert!((sx.value() - 0.8).abs() < 1e-6, "got {}", sx.value());
    }

    #[test]
    fn r1533_a_vertical_slider_reads_the_same_wheel_axis() {
        // Wheel-forward raises on BOTH orientations — there is no vertical
        // wheel on a horizontal mouse, so keying the value axis to the track
        // would leave one of the two orientations dead.
        let mut sx = SliderExternal::with_axis(SliderAxis::Vertical);
        sx.set_value(0.5);
        assert!(wheel(&mut sx, NOTCH_UP));
        assert!(
            (sx.value() - (0.5 + CONTINUOUS_WHEEL_STEP)).abs() < 1e-6,
            "got {}",
            sx.value()
        );
    }

    #[test]
    fn r1533_sub_notch_wheel_is_consumed_without_moving() {
        // The banked verdict. Declining here would let the scroll container
        // behind the slider jitter the page between notches of a trackpad.
        let mut sx = SliderExternal::new();
        sx.set_value(0.5);
        assert!(
            wheel(&mut sx, NOTCH_UP / 4.0),
            "a quarter notch is still the slider's wheel"
        );
        assert!(
            (sx.value() - 0.5).abs() < f32::EPSILON,
            "and it did not move the value: {}",
            sx.value()
        );
        for _ in 0..3 {
            wheel(&mut sx, NOTCH_UP / 4.0);
        }
        assert!(
            (sx.value() - (0.5 + CONTINUOUS_WHEEL_STEP)).abs() < 1e-6,
            "four quarters make the notch: {}",
            sx.value()
        );
    }

    #[test]
    fn r1533_saturated_wheel_is_declined_so_the_page_can_scroll() {
        // The half the router acts on: a slider pinned at its bound must hand
        // the wheel back, or a settings page cannot be scrolled past it.
        let mut sx = SliderExternal::new();
        sx.set_value(1.0);
        assert!(
            !wheel(&mut sx, NOTCH_UP),
            "a slider at max declines a wheel that would raise it"
        );
        assert!(
            wheel(&mut sx, NOTCH_DOWN),
            "and still answers the direction it CAN move"
        );
        assert!((sx.value() - (1.0 - CONTINUOUS_WHEEL_STEP)).abs() < 1e-6);
    }

    #[test]
    fn r1533_saturation_drops_the_carry() {
        // Without the reset, pixels banked while pushing a pinned bound get
        // spent later on a step the user has stopped asking for.
        //
        // The recovery wheel has to go the SAME way as the saturating push:
        // a reversal is dropped by [`WheelStepper::feed`]'s own sign-flip
        // reset, so a test that reverses passes with this reset deleted (it
        // did — the counterfactual is what found it).
        let mut sx = SliderExternal::new();
        sx.set_value(1.0);
        // Bank most of a notch upward against the ceiling, then saturate.
        assert!(wheel(&mut sx, NOTCH_UP * 0.9));
        assert!(!wheel(&mut sx, NOTCH_UP * 0.9), "the second fills a notch");
        // A binding writes the value off the bound (`intervene`, a preference
        // restore, a linked control) while the pointer has not moved.
        sx.set_value(0.5);
        // Three tenths of a notch, still upward. On a dropped carry this
        // banks; on a kept one the 0.8-notch residue completes a notch at
        // once. 0.3 and not 0.2: 0.2 lands the sum EXACTLY on the notch
        // boundary, where binary rounding decides the trunc and the
        // counterfactual passes by luck — measured, that is what it did.
        assert!(wheel(&mut sx, NOTCH_UP * 0.3));
        assert!(
            (sx.value() - 0.5).abs() < f32::EPSILON,
            "the residue against the ceiling is gone, so three tenths of a \
             notch moves nothing; got {}",
            sx.value()
        );
    }

    #[test]
    fn r1533_a_notch_emits_both_the_changing_and_the_committed_intent() {
        // A notch is atomic: there is no drag to end, so if the commit
        // channel stayed silent every consumer that persists or seeks on
        // commit (`hello-scrubber`, `settings-panel`) would watch the thumb
        // move and do nothing.
        let mut sx = SliderExternal::new();
        sx.set_value(0.5);
        let mut drained = Vec::new();
        sx.drain_intents(&mut |i| drained.push(i));
        drained.clear();

        assert!(wheel(&mut sx, NOTCH_UP));
        sx.drain_intents(&mut |i| drained.push(i));
        let tags: Vec<&str> = drained.iter().map(Intent::tag_str).collect();
        assert_eq!(
            tags,
            vec!["value_changing", "value_committed"],
            "the notch reports the move and then that it is settled"
        );
        for i in &drained {
            match i.payload {
                IntrospectValue::Float(v) => assert!(
                    (v - f64::from(sx.value())).abs() < 1e-6,
                    "both carry the post-notch value, got {v}"
                ),
                ref other => panic!("expected Float payload, got {other:?}"),
            }
        }
    }

    #[test]
    fn r1533_a_declined_wheel_emits_nothing() {
        let mut sx = SliderExternal::new();
        sx.set_value(1.0);
        let mut drained = Vec::new();
        sx.drain_intents(&mut |i| drained.push(i));
        drained.clear();
        assert!(!wheel(&mut sx, NOTCH_UP));
        sx.drain_intents(&mut |i| drained.push(i));
        assert!(
            drained.is_empty(),
            "a wheel that moved nothing reports nothing, got {:?}",
            drained.iter().map(Intent::tag_str).collect::<Vec<_>>()
        );
    }

    #[test]
    fn r1533_the_wheel_does_not_disturb_the_interaction_statechart() {
        // A wheel is not a press. If it drove the SCXML the widget would
        // paint as dragging under the cursor and the drag-end commit would
        // fire a second time.
        let mut sx = SliderExternal::new();
        sx.send(SliderEvent::PointerEnter);
        assert!(matches!(sx.state(), SliderState::Hover));
        assert!(wheel(&mut sx, NOTCH_UP));
        assert!(
            matches!(sx.state(), SliderState::Hover),
            "still Hover, got {:?}",
            sx.state()
        );
    }
}
