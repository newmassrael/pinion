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
    clippy::all,
)]
mod sm {
    include!(concat!(env!("OUT_DIR"), "/slider_sm.rs"));
}

pub use sm::{SliderEvent, SliderState};
use sm::SliderPolicy;

use crate::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect,
    InterveneError, IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner,
    ThreadOwnership,
};
use crate::intent::Intent;
use crate::widgets::{IntentEmitter, Widget, WidgetTransition};

/// Slider widget state machine + `f32` value sidecar
/// (0.0..=1.0 normalised). R51.14 own statechart with semantically
/// named `dragging` state (replaces the R51.7 `Pressed = dragging`
/// reinterpretation).
pub struct Slider {
    inner: Widget<SliderPolicy>,
    value: f32,
}

impl Slider {
    /// Construct a Slider in the `Idle` state with `value = 0.0`.
    #[must_use]
    pub fn new() -> Self {
        Self { inner: Widget::new(), value: 0.0 }
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
        let clamped = v.clamp(0.0, 1.0);
        if (clamped - self.value).abs() < f32::EPSILON {
            return false;
        }
        self.value = clamped;
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

    fn detect(before: Self::Snapshot, after: Self::Snapshot) -> Option<Intent> {
        let (before_state, _) = before;
        let (after_state, after_value) = after;
        if matches!(before_state, SliderState::Dragging)
            && matches!(after_state, SliderState::Hover)
        {
            Some(Intent::new_static(
                "value_committed",
                IntrospectValue::Float(f64::from(after_value)),
            ))
        } else {
            None
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
    #[must_use]
    pub fn new() -> Self {
        Self { em: IntentEmitter::default() }
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

    /// R51.35 §5.15 + §5.35 — feed widget-relative cursor X (`x_rel`)
    /// into the f32 value sidecar, clamping to the [`Slider`]'s
    /// `0.0..=1.0` range. `y_rel` is ignored — this is a horizontal
    /// slider. The framework forwards both press-time
    /// (click-to-position) and drag-time motion through this hook.
    ///
    /// The clamping is intentional: `x_rel` may exceed `[0.0, 1.0]`
    /// or go negative when the cursor strays off the track rect
    /// under capture lock (R51.34 design point). Clamping here
    /// preserves the `value_changing` intent's gate-by-effect
    /// semantics — strays past the saturated value are silent.
    fn pointer_move(&mut self, x_rel: f32, _y_rel: f32) {
        self.set_value(x_rel.clamp(0.0, 1.0));
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
            ("send", "string"),
        ])
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "state" => Some(IntrospectValue::Text(
                slider_state_name(self.state()).to_string(),
            )),
            "value" => Some(IntrospectValue::Float(f64::from(self.value()))),
            _ => None,
        }
    }

    fn intervene(
        &mut self,
        path: &str,
        value: IntrospectValue,
    ) -> Result<(), InterveneError> {
        match path {
            "state" => Err(InterveneError::ReadOnly),
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
                    let ev = parse_slider_event(name).ok_or(InvokeError::Rejected)?;
                    self.send(ev);
                    Ok(IntrospectValue::Text(
                        slider_state_name(self.state()).to_string(),
                    ))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

fn slider_state_name(state: SliderState) -> &'static str {
    match state {
        SliderState::Idle => "Idle",
        SliderState::Hover => "Hover",
        SliderState::Dragging => "Dragging",
        SliderState::Disabled => "Disabled",
    }
}

fn parse_slider_event(name: &str) -> Option<SliderEvent> {
    match name {
        "PointerEnter" => Some(SliderEvent::PointerEnter),
        "PointerLeave" => Some(SliderEvent::PointerLeave),
        "PointerDown" => Some(SliderEvent::PointerDown),
        "PointerUp" => Some(SliderEvent::PointerUp),
        "Disable" => Some(SliderEvent::Disable),
        "Enable" => Some(SliderEvent::Enable),
        _ => None,
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
    fn external_schema_declares_three_slots() {
        let sx = SliderExternal::new();
        let schema = sx.schema();
        assert_eq!(
            schema.fields,
            &[("state", "string"), ("value", "float"), ("send", "string")]
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
}
