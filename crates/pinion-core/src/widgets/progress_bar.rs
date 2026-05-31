//! R718 §5.38 — `ProgressBar` widget: a descriptive (non-interactive)
//! status holder reporting the fraction of a long-running task.
//!
//! Unlike the operable widgets (Button / Slider / Checkbox …), a
//! progress bar owns **no interaction statechart**: it has no pointer
//! states, no keyboard model, and emits no §5.20 intents. It is a plain
//! value holder, mirroring [`TooltipExternal`](crate::widgets::tooltip)
//! (the other descriptive widget that is a hand-written [`External`]
//! rather than an SCXML-backed one). The single observable axis is the
//! normalized progress [`value`](Self::value) in `0.0..=1.0`.
//!
//! The value is **writable** through the §5.15 introspect channel
//! (`intervene("value", Float)`) — the same side door the RPC
//! `scene/intervene` route and the application's progress updater both
//! use, so the AI client and the host observe the identical observable
//! state. This matches [`SliderExternal`](crate::widgets::slider): a
//! slider's value is driven by the pointer, a progress bar's by the
//! task, but both expose one settable normalized `value`.
//!
//! a11y: the binding lowers the value into
//! [`AccessValue::Float`](pinion_a11y::AccessValue::Float)
//! (`aria-valuenow` / `aria-valuemin` / `aria-valuemax`) on an
//! [`AriaRole::ProgressBar`](pinion_a11y::AriaRole::ProgressBar) node —
//! the same numeric lowering a `Slider` uses, but on a passive role (no
//! AT actions).
//!
//! **Determinate only (first slice).** An *indeterminate* progress bar
//! (the "busy, completion unknown" form: WAI-ARIA omits `aria-valuenow`,
//! Material/Flutter model it as a `null` value + a looping animation)
//! needs a *repeating* animation driver — the existing §5.28 spring
//! substrate settles to a target and stops, so a sawtooth/looping
//! indeterminate sweep is a separate animation axis. It lands additively
//! (a `value: Option<f32>` widening + a looping driver) once that
//! substrate exists; today the value is always present.

use crate::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, RepaintOwner, ThreadOwnership,
};

/// R718 §5.38 — determinate linear progress value holder.
///
/// The single field is the normalized progress fraction, always kept in
/// `0.0..=1.0` by [`Self::set_value`]. There is no interaction state to
/// carry (a progress bar is not operable), so the struct is a plain
/// `Copy` value — distinct from the SCXML-backed operable widgets.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProgressBarExternal {
    /// Normalized progress fraction in `0.0..=1.0`. `0.0` is an empty
    /// bar (task not started), `1.0` is a full bar (task complete).
    value: f32,
}

impl ProgressBarExternal {
    /// Construct an empty progress bar (`value = 0.0`).
    #[must_use]
    pub fn new() -> Self {
        Self { value: 0.0 }
    }

    /// Construct a progress bar at a given starting fraction. The value
    /// is clamped into `0.0..=1.0` exactly as [`Self::set_value`] does,
    /// so an out-of-range argument saturates rather than storing a
    /// nonsensical fraction.
    #[must_use]
    pub fn with_value(value: f32) -> Self {
        let mut p = Self::new();
        p.set_value(value);
        p
    }

    /// Read the current normalized progress fraction (always in
    /// `0.0..=1.0`).
    #[must_use]
    pub fn value(&self) -> f32 {
        self.value
    }

    /// Set the normalized progress fraction, clamping into `0.0..=1.0`.
    /// `NaN` is treated as `0.0` (the clamp would otherwise propagate
    /// `NaN`), so a malformed wire payload can never poison the value.
    pub fn set_value(&mut self, value: f32) {
        self.value = if value.is_nan() { 0.0 } else { value.clamp(0.0, 1.0) };
    }
}

impl Default for ProgressBarExternal {
    fn default() -> Self {
        Self::new()
    }
}

impl External for ProgressBarExternal {
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

    /// A progress bar emits no §5.20 intents — its value is observed and
    /// driven through the introspect channel, never broadcast as a
    /// command / selection / value intent.
    fn drain_intents(&mut self, _sink: &mut dyn FnMut(crate::intent::Intent)) {}

    /// The value never changes on its own (no internal clock); every
    /// mutation arrives through `intervene`, which the framework already
    /// follows with a repaint. So the bar is never self-dirty.
    fn is_dirty(&self) -> bool {
        false
    }
}

impl ExternalIntrospect for ProgressBarExternal {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(&[
            ("value", "float"),
            ("min", "float"),
            ("max", "float"),
        ])
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "value" => Some(IntrospectValue::Float(f64::from(self.value))),
            // The normalized range is fixed: a progress bar always
            // reports its fraction against `[0, 1]` (matching the
            // `AccessValue::Float` min/max the binding lowers).
            "min" => Some(IntrospectValue::Float(0.0)),
            "max" => Some(IntrospectValue::Float(1.0)),
            _ => None,
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            // The progress fraction is the single writable axis. Accept
            // both `Float` and `Int` (an AI client may send `1` for a
            // full bar); clamping happens inside `set_value`. Mirrors
            // `SliderExternal::intervene("value", …)`.
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
            // The range bounds are fixed (normalized), so they reject
            // intervene — the same read-only treatment a slider gives
            // its construction-time-fixed axes.
            "min" | "max" => Err(InterveneError::ReadOnly),
            _ => Err(InterveneError::UnknownPath),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_starts_empty() {
        assert!((ProgressBarExternal::new().value() - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn with_value_stores_fraction() {
        assert!((ProgressBarExternal::with_value(0.42).value() - 0.42).abs() < f32::EPSILON);
    }

    #[test]
    fn set_value_clamps_below_zero() {
        let mut p = ProgressBarExternal::new();
        p.set_value(-0.5);
        assert!((p.value() - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn set_value_clamps_above_one() {
        let mut p = ProgressBarExternal::new();
        p.set_value(1.5);
        assert!((p.value() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn set_value_nan_falls_back_to_zero() {
        let mut p = ProgressBarExternal::with_value(0.7);
        p.set_value(f32::NAN);
        assert!((p.value() - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn query_reports_value_and_fixed_range() {
        let p = ProgressBarExternal::with_value(0.75);
        assert_eq!(p.query("value"), Some(IntrospectValue::Float(0.75)));
        assert_eq!(p.query("min"), Some(IntrospectValue::Float(0.0)));
        assert_eq!(p.query("max"), Some(IntrospectValue::Float(1.0)));
        assert_eq!(p.query("nope"), None);
    }

    #[test]
    fn intervene_value_float_sets_and_clamps() {
        let mut p = ProgressBarExternal::new();
        p.intervene("value", IntrospectValue::Float(0.6)).expect("float accepted");
        assert!((p.value() - 0.6).abs() < f32::EPSILON);
        p.intervene("value", IntrospectValue::Float(2.0)).expect("clamps in set_value");
        assert!((p.value() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn intervene_value_int_coerces() {
        let mut p = ProgressBarExternal::new();
        p.intervene("value", IntrospectValue::Int(1)).expect("int coerced to 1.0");
        assert!((p.value() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn intervene_value_wrong_type_is_type_mismatch() {
        let mut p = ProgressBarExternal::new();
        assert_eq!(
            p.intervene("value", IntrospectValue::Bool(true)),
            Err(InterveneError::TypeMismatch),
        );
    }

    #[test]
    fn intervene_range_bounds_are_read_only() {
        let mut p = ProgressBarExternal::new();
        assert_eq!(
            p.intervene("min", IntrospectValue::Float(0.1)),
            Err(InterveneError::ReadOnly),
        );
        assert_eq!(
            p.intervene("max", IntrospectValue::Float(0.9)),
            Err(InterveneError::ReadOnly),
        );
    }

    #[test]
    fn intervene_unknown_path_rejected() {
        let mut p = ProgressBarExternal::new();
        assert_eq!(
            p.intervene("speed", IntrospectValue::Float(1.0)),
            Err(InterveneError::UnknownPath),
        );
    }
}
