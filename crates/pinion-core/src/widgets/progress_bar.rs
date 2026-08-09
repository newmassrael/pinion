//! R718 §5.38 — `ProgressBar` widget: a descriptive (non-interactive)
//! status holder reporting the fraction of a long-running task.
//!
//! Unlike the operable widgets (Button / Slider / Checkbox …), a
//! progress bar owns **no interaction statechart**: it has no pointer
//! states, no keyboard model, and emits no §5.20 intents. It is a plain
//! value holder, mirroring [`TooltipExternal`](crate::widgets::tooltip)
//! (the other descriptive widget that is a hand-written [`External`]
//! rather than an SCXML-backed one). The single observable axis is the
//! normalized progress `value` in `0.0..=1.0`.
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
//! `AccessValue::Float`
//! (`aria-valuenow` / `aria-valuemin` / `aria-valuemax`) on an
//! `AriaRole::ProgressBar` node —
//! the same numeric lowering a `Slider` uses, but on a passive role (no
//! AT actions).
//!
//! **Determinate only (first slice).** An *indeterminate* progress bar (the
//! "busy, completion unknown" form: WAI-ARIA omits `aria-valuenow`, Material/another
//! retained-mode toolkit model it as a `null` value + a looping animation) needs
//! a *repeating* animation driver — the existing §5.28 spring substrate
//! settles to a target and stops, so a sawtooth/looping indeterminate sweep is
//! a separate animation axis. It lands additively (a `value: Option<f32>` widening + a looping
//! driver) once that substrate exists; today the value is always present.

use std::cell::Cell;
use std::rc::Rc;

use crate::animation::Tickable;
use crate::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, RepaintOwner, SchemaField, ThreadOwnership,
};
use crate::reactive::{Owner, Signal};

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
    /// Ignored for paint + a11y while [`Self::indeterminate`] is set.
    value: f32,
    /// R726 §5.38 — when `true` the bar is *indeterminate* ("busy,
    /// completion unknown"): the `value` is suppressed (WAI-ARIA omits
    /// `aria-valuenow`) and the binding paints a looping sweep driven by
    /// the [`IndeterminateSweep`] Tickable instead of a fraction fill.
    indeterminate: bool,
}

impl ProgressBarExternal {
    /// Construct an empty determinate progress bar (`value = 0.0`).
    #[must_use]
    pub fn new() -> Self {
        Self {
            value: 0.0,
            indeterminate: false,
        }
    }

    /// Whether the bar is indeterminate (busy, completion unknown).
    #[must_use]
    pub fn indeterminate(&self) -> bool {
        self.indeterminate
    }

    /// Set the indeterminate flag. Leaves `value` untouched (it is
    /// simply suppressed while indeterminate), so toggling back to
    /// determinate restores the last fraction.
    pub fn set_indeterminate(&mut self, indeterminate: bool) {
        self.indeterminate = indeterminate;
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
    /// Setting a fraction also clears [`Self::indeterminate`] — reporting
    /// a concrete value *is* the determinate signal.
    pub fn set_value(&mut self, value: f32) {
        self.value = if value.is_nan() {
            0.0
        } else {
            value.clamp(0.0, 1.0)
        };
        self.indeterminate = false;
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
        IntrospectSchema::new(
            const {
                &[
                    SchemaField::new("value", "float"),
                    SchemaField::new("min", "float"),
                    SchemaField::new("max", "float"),
                    SchemaField::new("indeterminate", "bool"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "value" => Some(IntrospectValue::Float(f64::from(self.value))),
            // The normalized range is fixed: a progress bar always
            // reports its fraction against `[0, 1]` (matching the
            // `AccessValue::Float` min/max the binding lowers).
            "min" => Some(IntrospectValue::Float(0.0)),
            "max" => Some(IntrospectValue::Float(1.0)),
            // R726 §5.38 — busy/determinate flag, AI-readable as data.
            "indeterminate" => Some(IntrospectValue::Bool(self.indeterminate)),
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
            // R726 §5.38 — toggle busy mode. An AI client / the task
            // driver flips this to switch the bar between a fraction
            // fill and the looping indeterminate sweep.
            "indeterminate" => match value {
                IntrospectValue::Bool(b) => {
                    self.set_indeterminate(b);
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

/// R726 §5.28 §5.38 — looping sweep driver for an *indeterminate*
/// progress bar. Unlike the one-shot
/// [`SnackbarTimer`](super::snackbar::SnackbarTimer) and the two-phase
/// [`CaretBlink`](super::caret_blink::CaretBlink), this is a *repeating*
/// [`Tickable`]: it advances a `0.0..1.0` sawtooth `position` forever
/// while active, so `is_at_rest` is **always `false`** when active (an
/// indeterminate bar never settles — that is the point). The binding
/// reads [`Self::position`] to place the moving indicator segment.
///
/// `set_active(false)` parks it (the determinate bar has no sweep), at
/// which point `is_at_rest` returns `true` so the backend releases
/// frames — the [`CaretBlink::set_enabled`](crate::widgets::caret_blink::CaretBlink::set_enabled) gating pattern.
#[derive(Debug)]
pub struct IndeterminateSweep {
    /// Sawtooth phase in `0.0..1.0` — auto-subscribes inside a view-fn
    /// so the indicator re-paints every tick.
    position: Signal<f32>,
    /// Seconds elapsed within the current cycle.
    elapsed: Cell<f32>,
    /// Gate — `false` parks the sweep (determinate bar) so it reports
    /// at-rest and the backend stops requesting frames.
    active: Cell<bool>,
}

impl IndeterminateSweep {
    /// One full sweep cycle in seconds (Material 3 linear indeterminate
    /// cadence is ~2 s; pinion uses a single moving segment).
    pub const PERIOD_SECS: f32 = 1.8;

    /// Construct a parked sweep (`position = 0`, inactive).
    #[must_use]
    pub fn new() -> Self {
        Self {
            position: Signal::new(0.0),
            elapsed: Cell::new(0.0),
            active: Cell::new(false),
        }
    }

    /// Current sawtooth phase in `0.0..1.0`. Auto-subscribes in a
    /// view-fn so the indicator follows the sweep.
    #[must_use]
    pub fn position(&self) -> f32 {
        self.position.get()
    }

    /// Whether the sweep is running.
    #[must_use]
    pub fn active(&self) -> bool {
        self.active.get()
    }

    /// Gate the sweep. `true` starts looping from phase 0; `false` parks
    /// it (and snaps the phase back to 0). Same-value is a no-op (Signal
    /// equality-skip), so a view-fn can call it every frame safely.
    pub fn set_active(&self, on: bool) {
        if self.active.get() == on {
            return;
        }
        self.active.set(on);
        self.elapsed.set(0.0);
        self.position.set(0.0);
    }
}

impl Default for IndeterminateSweep {
    fn default() -> Self {
        Self::new()
    }
}

impl Tickable for IndeterminateSweep {
    fn tick(&self, dt: f32) {
        if !self.active.get() {
            return;
        }
        let elapsed = (self.elapsed.get() + dt) % Self::PERIOD_SECS;
        self.elapsed.set(elapsed);
        self.position.set(elapsed / Self::PERIOD_SECS);
    }

    fn is_at_rest(&self, _epsilon: f32) -> bool {
        // A running indeterminate sweep is *never* at rest (it loops
        // forever); a parked one is, so the backend releases frames.
        !self.active.get()
    }
}

/// R726 §5.28 §5.38 — resolve (or lazily initialize) an
/// [`IndeterminateSweep`] for the current view scope and register it
/// with the animation driver exactly once. Mirrors
/// [`use_caret_blink`](super::caret_blink::use_caret_blink) /
/// [`use_snackbar_timer`](super::snackbar::use_snackbar_timer).
///
/// # Panics
///
/// Panics outside a `root_owner.run(...)` wrap (no current [`Owner`]),
/// or if the cache key was bound to a different type (see
/// [`Owner::cache`]).
#[must_use]
pub fn use_indeterminate_sweep(key: &'static str) -> Rc<IndeterminateSweep> {
    // R727 §5.28 — delegates to the `Owner::register_animation_once` SSOT.
    Owner::current()
        .expect("use_indeterminate_sweep requires an active Owner scope")
        .register_animation_once(key, IndeterminateSweep::new)
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
        p.intervene("value", IntrospectValue::Float(0.6))
            .expect("float accepted");
        assert!((p.value() - 0.6).abs() < f32::EPSILON);
        p.intervene("value", IntrospectValue::Float(2.0))
            .expect("clamps in set_value");
        assert!((p.value() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn intervene_value_int_coerces() {
        let mut p = ProgressBarExternal::new();
        p.intervene("value", IntrospectValue::Int(1))
            .expect("int coerced to 1.0");
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

    // R726 §5.38 — indeterminate flag.

    #[test]
    fn determinate_by_default() {
        assert!(!ProgressBarExternal::new().indeterminate());
    }

    #[test]
    fn intervene_indeterminate_toggles_and_set_value_clears_it() {
        let mut p = ProgressBarExternal::new();
        p.intervene("indeterminate", IntrospectValue::Bool(true))
            .expect("bool accepted");
        assert!(p.indeterminate());
        assert_eq!(p.query("indeterminate"), Some(IntrospectValue::Bool(true)));
        // Reporting a concrete fraction returns to determinate.
        p.set_value(0.5);
        assert!(!p.indeterminate());
    }

    #[test]
    fn intervene_indeterminate_wrong_type_is_type_mismatch() {
        let mut p = ProgressBarExternal::new();
        assert_eq!(
            p.intervene("indeterminate", IntrospectValue::Float(1.0)),
            Err(InterveneError::TypeMismatch),
        );
    }

    // R726 §5.28 — IndeterminateSweep looping Tickable.

    #[test]
    fn sweep_parked_until_activated() {
        let s = IndeterminateSweep::new();
        assert!(!s.active());
        assert!(s.is_at_rest(0.0), "parked sweep is at rest");
        s.tick(1.0);
        assert!(
            (s.position() - 0.0).abs() < f32::EPSILON,
            "parked sweep does not advance"
        );
    }

    #[test]
    fn active_sweep_loops_and_never_rests() {
        let s = IndeterminateSweep::new();
        s.set_active(true);
        assert!(!s.is_at_rest(0.0), "an active sweep never settles");
        s.tick(IndeterminateSweep::PERIOD_SECS / 2.0);
        assert!(
            (s.position() - 0.5).abs() < 1e-5,
            "half a period -> phase 0.5"
        );
        // Wraps past the period (sawtooth).
        s.tick(IndeterminateSweep::PERIOD_SECS);
        assert!(s.position() < 1.0, "phase stays in [0,1) after wrap");
    }

    #[test]
    fn deactivate_parks_and_resets() {
        let s = IndeterminateSweep::new();
        s.set_active(true);
        s.tick(IndeterminateSweep::PERIOD_SECS / 3.0);
        s.set_active(false);
        assert!(s.is_at_rest(0.0));
        assert!(
            (s.position() - 0.0).abs() < f32::EPSILON,
            "park snaps phase to 0"
        );
    }

    #[test]
    fn use_hook_registers_sweep_once_per_key() {
        let owner = Owner::new();
        owner.run(|| {
            let _a = use_indeterminate_sweep("pb");
            let _b = use_indeterminate_sweep("pb");
        });
        assert_eq!(owner.registered_animation_count(), 1);
    }
}
