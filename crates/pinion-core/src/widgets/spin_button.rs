//! R734 §5.38 — `SpinButton` widget: a numeric input the user steps
//! through a bounded range (the WAI-ARIA `spinbutton` / desktop
//! "number input" pattern; a property-panel field in a DCC / CAD tool).
//!
//! Like [`ProgressBarExternal`](crate::widgets::progress_bar) (and unlike
//! the operable Button / Slider / Checkbox widgets) a spin button owns
//! **no interaction statechart** — there is no hover / press / drag state
//! machine on the value itself. It is a plain value holder with a bounded
//! stepped range. Its *operability* is value arithmetic (increment /
//! decrement / page / clamp), not a pointer-state SCXML; the keyboard
//! model lives in the binding's `apply_key` and the optional decrement /
//! increment paint regions route through the R51.42 §5.35 composite hit-target
//! protocol (`spin#dec` / `spin#inc` → `invoke("send", "dec:PointerUp")`).
//!
//! Distinct from [`SliderExternal`](crate::widgets::slider): a slider is a
//! *continuous* normalized `0.0..=1.0` drag along a track; a spin button
//! holds a value in **domain units** with an explicit `min` / `max` /
//! `step`, stepped discretely. That domain difference is why the spin
//! button is its own widget rather than a slider re-skin (the
//! "similar-grammar-is-not-a-shared-statechart" rule).
//!
//! a11y: the binding lowers the value into
//! [`AccessValue::Float`](pinion_a11y::AccessValue::Float)
//! (`aria-valuenow` / `aria-valuemin` / `aria-valuemax`) on an
//! `AriaRole::SpinButton` node — the same numeric lowering a `Slider`
//! uses, on the *operable* role (Focus + Increment + Decrement AT
//! actions), the 3rd `Float` consumer after `Slider` + `ProgressBar`.

use crate::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner, ThreadOwnership,
};

/// R734 §5.38 — bounded stepped numeric value holder.
///
/// Stores the value in domain units (`f32`, matching the
/// [`AccessValue::Float`](pinion_a11y::AccessValue::Float) lowering) with
/// an explicit `min` / `max` / `step` and a larger `page_step`. Every
/// mutation re-clamps into `[min, max]`; `NaN` saturates to `min` so a
/// malformed wire payload can never poison the value. There is no
/// interaction state to carry, so the struct is a plain `Copy` value
/// (distinct from the SCXML-backed operable widgets).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpinButtonExternal {
    value: f32,
    min: f32,
    max: f32,
    /// Arrow-key / button single step.
    step: f32,
    /// `PageUp` / `PageDown` larger step.
    page_step: f32,
}

impl SpinButtonExternal {
    /// Construct a spin button at `value`, bounded to `[min, max]`, with a
    /// single `step`. `page_step` defaults to `step` (override with
    /// [`Self::with_page_step`]). `value` is clamped into the range.
    #[must_use]
    pub fn new(value: f32, min: f32, max: f32, step: f32) -> Self {
        let mut s = Self { value: min, min, max, step, page_step: step };
        s.set_value(value);
        s
    }

    /// Override the `PageUp` / `PageDown` larger step (defaults to `step`).
    #[must_use]
    pub fn with_page_step(mut self, page_step: f32) -> Self {
        self.page_step = page_step;
        self
    }

    /// Current value (always within `[min, max]`).
    #[must_use]
    pub fn value(&self) -> f32 {
        self.value
    }

    /// Lower range bound.
    #[must_use]
    pub fn min(&self) -> f32 {
        self.min
    }

    /// Upper range bound.
    #[must_use]
    pub fn max(&self) -> f32 {
        self.max
    }

    /// Single arrow-key / button step.
    #[must_use]
    pub fn step(&self) -> f32 {
        self.step
    }

    /// Set the value, clamping into `[min, max]`. `NaN` saturates to
    /// `min`. Returns `true` if the stored value actually changed.
    pub fn set_value(&mut self, value: f32) -> bool {
        let clamped = if value.is_nan() { self.min } else { value.clamp(self.min, self.max) };
        if (clamped - self.value).abs() < f32::EPSILON {
            return false;
        }
        self.value = clamped;
        true
    }

    /// Step up by `step` (clamped at `max`). Returns `true` if it moved.
    pub fn increment(&mut self) -> bool {
        self.set_value(self.value + self.step)
    }

    /// Step down by `step` (clamped at `min`). Returns `true` if it moved.
    pub fn decrement(&mut self) -> bool {
        self.set_value(self.value - self.step)
    }

    /// Step up by `page_step` (clamped at `max`). Returns `true` if moved.
    pub fn page_up(&mut self) -> bool {
        self.set_value(self.value + self.page_step)
    }

    /// Step down by `page_step` (clamped at `min`). Returns `true` if moved.
    pub fn page_down(&mut self) -> bool {
        self.set_value(self.value - self.page_step)
    }
}

impl External for SpinButtonExternal {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui, Backend::Rpc], BackendFallback::Skip)
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }

    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        Some(self)
    }

    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        Some(self)
    }

    /// A spin button emits no §5.20 intents — its value is observed and
    /// driven through the introspect channel, never broadcast.
    fn drain_intents(&mut self, _sink: &mut dyn FnMut(crate::intent::Intent)) {}

    /// The value never changes on its own; every mutation arrives through
    /// `intervene` / `invoke`, which the framework already follows with a
    /// repaint. So the spin button is never self-dirty.
    fn is_dirty(&self) -> bool {
        false
    }
}

impl ExternalIntrospect for SpinButtonExternal {
    fn schema(&self) -> IntrospectSchema {
        // `value` — settable current value (query + intervene).
        // `min` / `max` / `step` — construction-fixed range descriptors
        //   (query only). `send` — the R51.42 §5.35 composite pointer channel
        //   for the decrement / increment paint regions.
        IntrospectSchema::new(&[
            ("value", "float"),
            ("min", "float"),
            ("max", "float"),
            ("step", "float"),
            ("send", "string"),
        ])
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "value" => Some(IntrospectValue::Float(f64::from(self.value))),
            "min" => Some(IntrospectValue::Float(f64::from(self.min))),
            "max" => Some(IntrospectValue::Float(f64::from(self.max))),
            "step" => Some(IntrospectValue::Float(f64::from(self.step))),
            _ => None,
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            // The value is the single writable axis. Accept Float + Int
            // (an AI client may send `5`); clamping happens in set_value.
            "value" => match value {
                IntrospectValue::Float(v) => {
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
            // Range descriptors are construction-fixed (read-only), the
            // same treatment a slider gives its fixed axes.
            "min" | "max" | "step" => Err(InterveneError::ReadOnly),
            _ => Err(InterveneError::UnknownPath),
        }
    }

    fn invoke(&mut self, path: &str, args: IntrospectValue) -> Result<IntrospectValue, InvokeError> {
        match path {
            // AI-first step actions (the same arithmetic the keyboard
            // arrows / PageUp-Down drive). Each returns the resulting
            // value so the caller sees the outcome in one round-trip.
            "increment" => {
                self.increment();
                Ok(IntrospectValue::Float(f64::from(self.value)))
            }
            "decrement" => {
                self.decrement();
                Ok(IntrospectValue::Float(f64::from(self.value)))
            }
            "page_up" => {
                self.page_up();
                Ok(IntrospectValue::Float(f64::from(self.value)))
            }
            "page_down" => {
                self.page_down();
                Ok(IntrospectValue::Float(f64::from(self.value)))
            }
            // R51.42 §5.35 composite pointer channel: a click on the
            // `spin#dec` / `spin#inc` paint region routes here as
            // `invoke("send", "dec:PointerUp")` / `"inc:PointerUp"`. The
            // `"<sub>:<EventName>"` wire is split by the R660
            // [`composite_tag::parse_send_payload`] SSOT (shared with
            // RadioGroup / ListBox / Table) rather than an inline
            // `split_once(':')` — the key type is `String` because the
            // sub-region is named (`dec` / `inc`), not a numeric index.
            // Only the activate (`PointerUp`) edge steps; every other
            // pointer edge (Enter / Leave / Down / Cancel) is a harmless
            // no-op so the full pointer arc the router replays does not
            // over-step.
            "send" => match args {
                IntrospectValue::Text(ref payload) => {
                    if let Some((sub, event)) =
                        crate::composite_tag::parse_send_payload::<String>(payload)
                    {
                        if event == "PointerUp" {
                            match sub.as_str() {
                                "inc" => {
                                    self.increment();
                                }
                                "dec" => {
                                    self.decrement();
                                }
                                _ => {}
                            }
                        }
                    }
                    Ok(IntrospectValue::Float(f64::from(self.value)))
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
    fn new_clamps_start_into_range() {
        assert!((SpinButtonExternal::new(50.0, 0.0, 10.0, 1.0).value() - 10.0).abs() < f32::EPSILON);
        assert!((SpinButtonExternal::new(-5.0, 0.0, 10.0, 1.0).value() - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn increment_decrement_step_and_clamp() {
        let mut s = SpinButtonExternal::new(8.0, 0.0, 10.0, 1.0);
        assert!(s.increment());
        assert!((s.value() - 9.0).abs() < f32::EPSILON);
        assert!(s.increment());
        assert!((s.value() - 10.0).abs() < f32::EPSILON);
        // At the ceiling: increment is a no-op (no change).
        assert!(!s.increment(), "clamped at max -> no change");
        assert!((s.value() - 10.0).abs() < f32::EPSILON);
        for _ in 0..20 {
            s.decrement();
        }
        assert!((s.value() - 0.0).abs() < f32::EPSILON, "saturates at min");
        assert!(!s.decrement(), "clamped at min -> no change");
    }

    #[test]
    fn page_step_defaults_to_step_and_overrides() {
        let mut a = SpinButtonExternal::new(0.0, 0.0, 100.0, 1.0);
        a.page_up();
        assert!((a.value() - 1.0).abs() < f32::EPSILON, "default page_step == step");
        let mut b = SpinButtonExternal::new(0.0, 0.0, 100.0, 1.0).with_page_step(10.0);
        b.page_up();
        assert!((b.value() - 10.0).abs() < f32::EPSILON);
        b.page_down();
        assert!((b.value() - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn set_value_nan_saturates_to_min() {
        let mut s = SpinButtonExternal::new(5.0, 2.0, 10.0, 1.0);
        assert!(s.set_value(f32::NAN));
        assert!((s.value() - 2.0).abs() < f32::EPSILON, "NaN -> min");
    }

    #[test]
    fn set_value_same_returns_false() {
        let mut s = SpinButtonExternal::new(5.0, 0.0, 10.0, 1.0);
        assert!(!s.set_value(5.0), "unchanged value -> false");
        assert!(s.set_value(6.0));
    }

    #[test]
    fn query_reports_value_and_range() {
        let s = SpinButtonExternal::new(3.0, 0.0, 10.0, 1.0);
        assert_eq!(s.query("value"), Some(IntrospectValue::Float(3.0)));
        assert_eq!(s.query("min"), Some(IntrospectValue::Float(0.0)));
        assert_eq!(s.query("max"), Some(IntrospectValue::Float(10.0)));
        assert_eq!(s.query("step"), Some(IntrospectValue::Float(1.0)));
        assert_eq!(s.query("nope"), None);
    }

    #[test]
    fn intervene_value_sets_and_clamps_float_and_int() {
        let mut s = SpinButtonExternal::new(0.0, 0.0, 10.0, 1.0);
        s.intervene("value", IntrospectValue::Float(4.0)).expect("float accepted");
        assert!((s.value() - 4.0).abs() < f32::EPSILON);
        s.intervene("value", IntrospectValue::Int(99)).expect("int coerced + clamped");
        assert!((s.value() - 10.0).abs() < f32::EPSILON);
    }

    #[test]
    fn intervene_range_descriptors_read_only() {
        let mut s = SpinButtonExternal::new(0.0, 0.0, 10.0, 1.0);
        assert_eq!(s.intervene("min", IntrospectValue::Float(1.0)), Err(InterveneError::ReadOnly));
        assert_eq!(s.intervene("max", IntrospectValue::Float(9.0)), Err(InterveneError::ReadOnly));
        assert_eq!(s.intervene("step", IntrospectValue::Float(2.0)), Err(InterveneError::ReadOnly));
    }

    #[test]
    fn intervene_unknown_path_and_type_mismatch() {
        let mut s = SpinButtonExternal::new(0.0, 0.0, 10.0, 1.0);
        assert_eq!(s.intervene("speed", IntrospectValue::Float(1.0)), Err(InterveneError::UnknownPath));
        assert_eq!(
            s.intervene("value", IntrospectValue::Bool(true)),
            Err(InterveneError::TypeMismatch),
        );
    }

    #[test]
    fn invoke_increment_decrement_page_return_value() {
        let mut s = SpinButtonExternal::new(5.0, 0.0, 10.0, 1.0).with_page_step(3.0);
        assert_eq!(s.invoke("increment", IntrospectValue::Null), Ok(IntrospectValue::Float(6.0)));
        assert_eq!(s.invoke("decrement", IntrospectValue::Null), Ok(IntrospectValue::Float(5.0)));
        assert_eq!(s.invoke("page_up", IntrospectValue::Null), Ok(IntrospectValue::Float(8.0)));
        assert_eq!(s.invoke("page_down", IntrospectValue::Null), Ok(IntrospectValue::Float(5.0)));
        assert_eq!(s.invoke("bogus", IntrospectValue::Null), Err(InvokeError::UnknownPath));
    }

    #[test]
    fn invoke_send_composite_pointer_steps_only_on_pointer_up() {
        let mut s = SpinButtonExternal::new(5.0, 0.0, 10.0, 1.0);
        // PointerDown / Enter do not step (the router replays the arc).
        s.invoke("send", IntrospectValue::Text("inc:PointerDown".into())).unwrap();
        s.invoke("send", IntrospectValue::Text("inc:PointerEnter".into())).unwrap();
        assert!((s.value() - 5.0).abs() < f32::EPSILON, "non-activate edges do not step");
        // PointerUp on the increment / decrement region steps once.
        s.invoke("send", IntrospectValue::Text("inc:PointerUp".into())).unwrap();
        assert!((s.value() - 6.0).abs() < f32::EPSILON, "inc:PointerUp -> +step");
        s.invoke("send", IntrospectValue::Text("dec:PointerUp".into())).unwrap();
        assert!((s.value() - 5.0).abs() < f32::EPSILON, "dec:PointerUp -> -step");
        // Unknown sub-region is a harmless no-op.
        s.invoke("send", IntrospectValue::Text("xyz:PointerUp".into())).unwrap();
        assert!((s.value() - 5.0).abs() < f32::EPSILON);
        // Malformed wire (no separator / empty event) is rejected by the
        // shared `parse_send_payload` guard -> no step (R660 SSOT).
        s.invoke("send", IntrospectValue::Text("noseparator".into())).unwrap();
        s.invoke("send", IntrospectValue::Text("inc:".into())).unwrap();
        assert!((s.value() - 5.0).abs() < f32::EPSILON, "malformed payload does not step");
    }
}
