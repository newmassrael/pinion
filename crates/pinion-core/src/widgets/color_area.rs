//! R709 §5.38 — `ColorArea` widget: a two-axis drag pad (the
//! saturation/value square of a colour picker). One thumb moves
//! freely in 2-D and writes a normalised `(x, y) = (saturation,
//! value)` pair; both axes are `0.0..=1.0`.
//!
//! `ColorArea` is a 2-D [`Slider`](super::slider::Slider). It owns its
//! own statechart (`color_area.scxml`) for the same reason every
//! catalog widget re-declares its interaction grammar rather than
//! `sce:use` a shared template — the RPC `scene/query "state"`
//! surface must read in the widget's own vocabulary (`"Dragging"`)
//! and a future `ColorArea`-only transition must not be forced to fork
//! a shared file. The drag grammar is byte-identical to the Slider's
//! (idle / hover / dragging / disabled), which is honest reuse, not
//! the R51.7 leak: there is no `pressed = dragging` reinterpretation;
//! dragging means dragging in both.
//!
//! The two f32 sidecars live in the Rust binding (SCXML "null
//! datamodel + typed Rust sidecar" split). Value semantics mirror
//! the Slider's two-phase Material / `SwiftUI` / Qt convention:
//!
//! * **`value_changing`** — every effective [`ColorArea::set_xy`]
//!   during drag emits a continuous intent carrying the live
//!   `{x, y}` pair (live-preview channel).
//! * **`value_committed`** — the `Dragging → Hover` activate
//!   transition (drag end via `PointerUp`) emits a single intent
//!   carrying the committed `{x, y}` (model-persistence channel).

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
    include!(concat!(env!("OUT_DIR"), "/color_area_sm.rs"));
}

use sm::ColorAreaPolicy;
pub use sm::{ColorAreaEvent, ColorAreaState};

// R709 §5.16 — pinion-side WidgetStateName + WidgetEventName impls for
// the sce-build-emitted enums (vendor/sce templates emit no
// pinion-side derives, [[sce-priority-over-pinion]]). The grammar
// mirrors the Slider's; only the `*Activate` raise variant is
// renamed.
crate::widget_state_name!(
    ColorAreaState,
    default = Idle,
    [Idle, Hover, Dragging, Disabled,]
);
crate::widget_event_name!(
    ColorAreaEvent,
    external = [
        PointerEnter,
        PointerLeave,
        PointerDown,
        PointerUp,
        PointerCancel,
        Disable,
        Enable,
    ],
    internal = [ColorAreaActivate, Null],
);

use crate::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner, ThreadOwnership,
};
use crate::intent::Intent;
use crate::widgets::{IntentEmitter, Widget, WidgetTransition};
use crate::{WidgetEventName, WidgetStateName};

/// `ColorArea` widget state machine + two `f32` value sidecars
/// (`x` = saturation, `y` = value, both `0.0..=1.0` normalised).
/// SCXML owns the interaction state (`color_area.scxml`); this
/// binding owns the typed 2-D value.
pub struct ColorArea {
    inner: Widget<ColorAreaPolicy>,
    x: f32,
    y: f32,
}

impl ColorArea {
    /// Construct a `ColorArea` in the `Idle` state at `(0.0, 0.0)`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Widget::new(),
            x: 0.0,
            y: 0.0,
        }
    }

    /// Drive a [`ColorAreaEvent`] through the SCXML. Pure state
    /// transition — value mutation flows through [`Self::set_xy`].
    pub fn send(&mut self, event: ColorAreaEvent) {
        self.inner.send(event);
    }

    /// Current interaction state.
    #[must_use]
    pub fn state(&self) -> ColorAreaState {
        self.inner.state()
    }

    /// Current normalised x (saturation), `0.0..=1.0`.
    #[must_use]
    pub fn x(&self) -> f32 {
        self.x
    }

    /// Current normalised y (value), `0.0..=1.0`.
    #[must_use]
    pub fn y(&self) -> f32 {
        self.y
    }

    /// Set both axes, each clamped to `0.0..=1.0`. Returns `true` if
    /// either stored axis actually changed (caller uses the return to
    /// gate the `value_changing` intent). State-independent — like
    /// [`Slider::set_value`](super::slider::Slider::set_value), the
    /// widget does not gate on `state() == Dragging` so programmatic
    /// updates (preference restore, keyboard step, RPC intervene)
    /// work in any state.
    pub fn set_xy(&mut self, x: f32, y: f32) -> bool {
        let cx = x.clamp(0.0, 1.0);
        let cy = y.clamp(0.0, 1.0);
        let changed = (cx - self.x).abs() >= f32::EPSILON || (cy - self.y).abs() >= f32::EPSILON;
        if changed {
            self.x = cx;
            self.y = cy;
        }
        changed
    }
}

impl Default for ColorArea {
    fn default() -> Self {
        Self::new()
    }
}

/// R709 §5.38 — `ColorArea` transition contract. Snapshot tuples the
/// interaction state with the `(x, y)` sidecar so detect can carry
/// the committed pair in the payload. The `Dragging → Hover` activate
/// path (drag end via `PointerUp`) emits the `"value_committed"`
/// intent carrying `after`'s `{x, y}` as an [`IntrospectValue::Json`]
/// object; the live-preview `"value_changing"` stream fires from
/// [`ColorAreaExternal::set_xy`] (a direct value mutation, not a
/// transition) and is not part of this contract.
impl WidgetTransition for ColorArea {
    type Event = ColorAreaEvent;
    type Snapshot = (ColorAreaState, (f32, f32));

    fn snapshot(&self) -> Self::Snapshot {
        (self.state(), (self.x(), self.y()))
    }

    fn drive(&mut self, event: Self::Event) {
        self.send(event);
    }

    fn detect(before: Self::Snapshot, _event: Self::Event, after: Self::Snapshot) -> Vec<Intent> {
        let (before_state, _) = before;
        let (after_state, (x, y)) = after;
        if matches!(before_state, ColorAreaState::Dragging)
            && matches!(after_state, ColorAreaState::Hover)
        {
            vec![Intent::new_static("value_committed", xy_json(x, y))]
        } else {
            Vec::new()
        }
    }
}

/// R709 §5.38 — pack a normalised `(x, y)` pair into the
/// `IntrospectValue::Json` object both value intents carry. A single
/// structured payload (rather than two `Float` intents) keeps the
/// saturation/value pair atomic for the AI-side observer.
fn xy_json(x: f32, y: f32) -> IntrospectValue {
    IntrospectValue::Json(serde_json::json!({
        "x": f64::from(x),
        "y": f64::from(y),
    }))
}

/// `External` adapter wrapping a [`ColorArea`]. Emits two intent
/// kinds, both carrying an `{x, y}` JSON object:
///
/// * `"value_changing"` on every effective [`Self::set_xy`] (live
///   preview channel).
/// * `"value_committed"` on `Dragging → Hover` activate (drag-end
///   commit channel).
pub struct ColorAreaExternal {
    em: IntentEmitter<ColorArea>,
}

impl ColorAreaExternal {
    /// Construct a `ColorArea` external in the `Idle` state at
    /// `(0.0, 0.0)`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            em: IntentEmitter::default(),
        }
    }

    /// Drive a [`ColorAreaEvent`] and queue a `"value_committed"`
    /// intent on drag-end (`Dragging → Hover`).
    pub fn send(&mut self, event: ColorAreaEvent) {
        self.em.dispatch(event);
    }

    /// Set both axes and queue a `"value_changing"` intent on
    /// effective change.
    pub fn set_xy(&mut self, x: f32, y: f32) {
        if self.em.inner.set_xy(x, y) {
            self.em.push(Intent::new_static(
                "value_changing",
                xy_json(self.em.inner.x(), self.em.inner.y()),
            ));
        }
    }

    /// Current interaction state.
    #[must_use]
    pub fn state(&self) -> ColorAreaState {
        self.em.inner.state()
    }

    /// Current normalised x (saturation).
    #[must_use]
    pub fn x(&self) -> f32 {
        self.em.inner.x()
    }

    /// Current normalised y (value).
    #[must_use]
    pub fn y(&self) -> f32 {
        self.em.inner.y()
    }
}

impl Default for ColorAreaExternal {
    fn default() -> Self {
        Self::new()
    }
}

impl core::fmt::Debug for ColorAreaExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ColorAreaExternal")
            .field("state", &self.state())
            .field("x", &self.x())
            .field("y", &self.y())
            .finish()
    }
}

impl External for ColorAreaExternal {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui, Backend::Rpc], BackendFallback::Skip)
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }

    /// R709 §5.15 + §5.35 — opt in to capture lock so the
    /// framework's `InputRouter` keeps the cursor pinned to this pad
    /// for the duration of the `pointer_down` → `pointer_up` span,
    /// even when the cursor strays outside the pad rect. Required for
    /// the canonical 2-D drag UX (drag past an edge without the press
    /// cancelling), exactly as the Slider does.
    fn wants_pointer_capture(&self) -> bool {
        true
    }

    /// R709 §5.15 + §5.35 + §5.38 — feed the pad-relative cursor into
    /// both f32 sidecars. `x_rel` drives saturation (left edge `0.0`,
    /// right edge `1.0`); `y_rel` drives value with an inversion —
    /// top edge (`y_rel = 0.0`) maps to `1.0` (brightest) and bottom
    /// edge maps to `0.0` (black), matching the conventional HSV
    /// square where "up = brighter". The framework forwards both
    /// press-time (click-to-position) and drag-time motion here.
    ///
    /// Both axes are clamped: under capture lock the cursor may stray
    /// off the rect (negative / past `1.0`); clamping preserves the
    /// `value_changing` gate-by-effect semantics so strays past a
    /// saturated axis are silent.
    fn pointer_move(&mut self, x_rel: f32, y_rel: f32) {
        self.set_xy(x_rel.clamp(0.0, 1.0), (1.0 - y_rel).clamp(0.0, 1.0));
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

impl ExternalIntrospect for ColorAreaExternal {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(&[
            ("state", "string"),
            ("x", "float"),
            ("y", "float"),
            ("send", "string"),
        ])
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "state" => Some(IntrospectValue::Text(self.state().as_name().to_string())),
            "x" => Some(IntrospectValue::Float(f64::from(self.x()))),
            "y" => Some(IntrospectValue::Float(f64::from(self.y()))),
            _ => None,
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        // `state` is SCXML-owned (driven via `send`). `x` and `y`
        // intervene independently; the unaddressed axis is preserved
        // by re-reading it from the live sidecar.
        let to_f32 = |v: &IntrospectValue| -> Option<f32> {
            match *v {
                #[allow(clippy::cast_possible_truncation)]
                IntrospectValue::Float(f) => Some(f as f32),
                #[allow(clippy::cast_precision_loss)]
                IntrospectValue::Int(i) => Some(i as f32),
                _ => None,
            }
        };
        match path {
            "state" => Err(InterveneError::ReadOnly),
            "x" => {
                let nx = to_f32(&value).ok_or(InterveneError::TypeMismatch)?;
                self.set_xy(nx, self.y());
                Ok(())
            }
            "y" => {
                let ny = to_f32(&value).ok_or(InterveneError::TypeMismatch)?;
                self.set_xy(self.x(), ny);
                Ok(())
            }
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
                    let ev = ColorAreaEvent::from_name(name).ok_or(InvokeError::Rejected)?;
                    self.send(ev);
                    Ok(IntrospectValue::Text(self.state().as_name().to_string()))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_state_is_idle_origin() {
        let a = ColorArea::new();
        assert_eq!(a.state(), ColorAreaState::Idle);
        assert!((a.x() - 0.0).abs() < f32::EPSILON);
        assert!((a.y() - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn set_xy_clamps_both_axes() {
        let mut a = ColorArea::new();
        assert!(a.set_xy(1.5, -0.5));
        assert!((a.x() - 1.0).abs() < f32::EPSILON);
        assert!((a.y() - 0.0).abs() < f32::EPSILON);
        // No-op re-set returns false (gate-by-effect).
        assert!(!a.set_xy(1.0, 0.0));
    }

    #[test]
    fn pointer_move_inverts_value_axis() {
        // Top edge (y_rel = 0.0) is the brightest value (y = 1.0).
        let mut sx = ColorAreaExternal::new();
        sx.pointer_move(0.25, 0.0);
        assert!((sx.x() - 0.25).abs() < f32::EPSILON);
        assert!((sx.y() - 1.0).abs() < f32::EPSILON);
        // Bottom edge (y_rel = 1.0) is black (y = 0.0).
        sx.pointer_move(0.25, 1.0);
        assert!((sx.y() - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn value_changing_emitted_on_drag_move() {
        let mut sx = ColorAreaExternal::new();
        sx.set_xy(0.4, 0.6);
        let mut intents = Vec::new();
        sx.drain_intents(&mut |i| intents.push(i));
        assert_eq!(intents.len(), 1);
        assert_eq!(intents[0].tag, "value_changing");
    }

    #[test]
    fn value_committed_on_drag_end_carries_xy() {
        let mut sx = ColorAreaExternal::new();
        // hover -> down -> set -> up should commit the final pair.
        sx.send(ColorAreaEvent::PointerEnter);
        sx.send(ColorAreaEvent::PointerDown);
        assert_eq!(sx.state(), ColorAreaState::Dragging);
        sx.set_xy(0.5, 0.75);
        sx.send(ColorAreaEvent::PointerUp);
        assert_eq!(sx.state(), ColorAreaState::Hover);
        let mut intents = Vec::new();
        sx.drain_intents(&mut |i| intents.push(i));
        // value_changing (from set_xy) then value_committed (drag end).
        assert!(intents.iter().any(|i| i.tag == "value_changing"));
        let committed = intents
            .iter()
            .find(|i| i.tag == "value_committed")
            .expect("drag end commits");
        if let IntrospectValue::Json(obj) = &committed.payload {
            assert!((obj["x"].as_f64().unwrap() - 0.5).abs() < 1e-6);
            assert!((obj["y"].as_f64().unwrap() - 0.75).abs() < 1e-6);
        } else {
            panic!("value_committed carries a JSON xy object");
        }
    }

    #[test]
    fn pointer_cancel_during_drag_returns_idle_without_commit() {
        let mut sx = ColorAreaExternal::new();
        sx.send(ColorAreaEvent::PointerEnter);
        sx.send(ColorAreaEvent::PointerDown);
        sx.set_xy(0.3, 0.3);
        sx.send(ColorAreaEvent::PointerCancel);
        assert_eq!(sx.state(), ColorAreaState::Idle);
        let mut intents = Vec::new();
        sx.drain_intents(&mut |i| intents.push(i));
        assert!(!intents.iter().any(|i| i.tag == "value_committed"));
    }

    #[test]
    fn intervene_axes_independently() {
        let mut sx = ColorAreaExternal::new();
        sx.intervene("x", IntrospectValue::Float(0.8)).unwrap();
        sx.intervene("y", IntrospectValue::Float(0.2)).unwrap();
        assert!((sx.x() - 0.8).abs() < f32::EPSILON);
        assert!((sx.y() - 0.2).abs() < f32::EPSILON);
        // x intervene preserves the existing y.
        sx.intervene("x", IntrospectValue::Float(0.1)).unwrap();
        assert!((sx.y() - 0.2).abs() < f32::EPSILON);
        assert!(matches!(
            sx.intervene("state", IntrospectValue::Text("x".into())),
            Err(InterveneError::ReadOnly)
        ));
    }

    #[test]
    fn invoke_send_drives_statechart() {
        let mut sx = ColorAreaExternal::new();
        let out = sx
            .invoke("send", IntrospectValue::Text("PointerEnter".into()))
            .unwrap();
        assert_eq!(out, IntrospectValue::Text("Hover".into()));
        assert_eq!(sx.state(), ColorAreaState::Hover);
    }
}
