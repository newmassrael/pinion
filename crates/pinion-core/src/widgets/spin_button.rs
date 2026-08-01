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
//! `AccessValue::Float`
//! (`aria-valuenow` / `aria-valuemin` / `aria-valuemax`) on an
//! `AriaRole::SpinButton` node — the same numeric lowering a `Slider`
//! uses, on the *operable* role (Focus + Increment + Decrement AT
//! actions), the 3rd `Float` consumer after `Slider` + `ProgressBar`.

use crate::event::WheelStepper;
use crate::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner, SchemaField, ThreadOwnership,
};
use crate::input::Modifiers;
use crate::widgets::button::{Button, ButtonEvent, ButtonState};
use crate::{WidgetEventName, WidgetStateName};

/// R734 §5.38 — bounded stepped numeric value holder with two
/// interactive stepper sub-regions.
///
/// Stores the value in domain units (`f32`, matching the
/// `AccessValue::Float` lowering) with
/// an explicit `min` / `max` / `step` and a larger `page_step`. Every
/// mutation re-clamps into `[min, max]`; `NaN` saturates to `min` so a
/// malformed wire payload can never poison the value.
///
/// R734.2 §5.38 §5.35 — the decrement / increment affordances each own a
/// standard [`Button`] interaction state machine (`Idle` / `Hover` /
/// `Pressed` / `Disabled`), exactly as
/// [`RadioGroup`](crate::widgets::radio_group::RadioGroup) owns a
/// per-segment `Radio` state machine. A composite `"<sub>:<EventName>"`
/// send (R51.42 §5.35) drives the addressed stepper's SCXML; the
/// `Pressed → Hover` activate transition (or `KeyboardActivate`) steps the
/// value. The binding reads [`Self::dec_state`] / [`Self::inc_state`] to
/// paint the hover / pressed state layer — so the steppers have full
/// desktop-grade pointer feedback rather than being inert glyphs. The
/// field value itself carries no interaction state (it is a value display,
/// the HTML `<input type=number>` model); the *operability* lives in the
/// stepper SMs + the binding's arrow-key handler.
pub struct SpinButtonExternal {
    value: f32,
    min: f32,
    max: f32,
    /// Arrow-key / button single step.
    step: f32,
    /// `PageUp` / `PageDown` larger step.
    page_step: f32,
    /// Decrement stepper interaction state machine.
    dec: Button,
    /// Increment stepper interaction state machine.
    inc: Button,
    /// R1533 §5.45 — sub-notch wheel carry (see the `External::wheel` impl).
    /// Per instance, like Qt's per-spinbox `wheelDeltaRemainder`.
    wheel: WheelStepper,
}

impl SpinButtonExternal {
    /// Construct a spin button at `value`, bounded to `[min, max]`, with a
    /// single `step`. `page_step` defaults to `step` (override with
    /// [`Self::with_page_step`]). `value` is clamped into the range. Both
    /// steppers start `Idle`.
    #[must_use]
    pub fn new(value: f32, min: f32, max: f32, step: f32) -> Self {
        let mut s = Self {
            value: min,
            min,
            max,
            step,
            page_step: step,
            dec: Button::new(),
            inc: Button::new(),
            wheel: WheelStepper::new(),
        };
        s.set_value(value);
        s
    }

    /// Override the `PageUp` / `PageDown` larger step (defaults to `step`).
    #[must_use]
    pub fn with_page_step(mut self, page_step: f32) -> Self {
        self.page_step = page_step;
        self
    }

    /// Current decrement-stepper interaction state (drives the hover /
    /// pressed paint state layer).
    #[must_use]
    pub fn dec_state(&self) -> ButtonState {
        self.dec.state()
    }

    /// Current increment-stepper interaction state.
    #[must_use]
    pub fn inc_state(&self) -> ButtonState {
        self.inc.state()
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
        let clamped = if value.is_nan() {
            self.min
        } else {
            value.clamp(self.min, self.max)
        };
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

    /// Drive the addressed stepper's [`Button`] SCXML and, on the
    /// activate transition (`Pressed → Hover` or `KeyboardActivate` from a
    /// non-`Disabled` state), step the value. Returns the resulting value.
    /// Unknown sub-region / event names are harmless no-ops so the full
    /// pointer arc the router replays (Enter → Down → Up) drives the state
    /// machine without over-stepping.
    fn drive_stepper(&mut self, sub: &str, event: ButtonEvent) -> f32 {
        let btn = match sub {
            "dec" => &mut self.dec,
            "inc" => &mut self.inc,
            _ => return self.value,
        };
        let before = btn.state();
        btn.send(event);
        let after = btn.state();
        let activated = (matches!(before, ButtonState::Pressed)
            && matches!(after, ButtonState::Hover))
            || (matches!(event, ButtonEvent::KeyboardActivate)
                && !matches!(before, ButtonState::Disabled));
        if activated {
            match sub {
                "dec" => self.decrement(),
                "inc" => self.increment(),
                _ => false,
            };
        }
        self.value
    }
}

impl core::fmt::Debug for SpinButtonExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SpinButtonExternal")
            .field("value", &self.value)
            .field("min", &self.min)
            .field("max", &self.max)
            .field("step", &self.step)
            .field("page_step", &self.page_step)
            .field("dec", &self.dec.state())
            .field("inc", &self.inc.state())
            .field("wheel", &self.wheel)
            .finish()
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

    /// R1533 §5.45 §5.38 — the wheel steps the value: Qt
    /// `QAbstractSpinBox::wheelEvent`, the same input every native number
    /// field answers, and the [`SliderExternal`](crate::widgets::slider)
    /// wheel's sibling — one rule for both stepped value widgets, so a notch
    /// means "one step" wherever the cursor is.
    ///
    /// One notch ([`LINE_HEIGHT_PX`](crate::event::LINE_HEIGHT_PX) pixels) is
    /// one [`Self::step`], sub-notch motion banks in a [`WheelStepper`], and
    /// the verdict is that type's: consume while banking or stepping, decline
    /// once saturated.
    ///
    /// Two deliberate divergences from Qt's spin box, both toward the
    /// slider's rule:
    ///
    /// * Qt **always** accepts the wheel, which is why scrolling a Qt form
    ///   past a spin box pinned at its maximum eats the page scroll. A wheel
    ///   this widget cannot spend belongs to the scroll container behind it.
    /// * Qt's `Ctrl` multiplies the step by ten. The honest peer of that here
    ///   is the page step [`Self::page_up`] / [`Self::page_down`] already
    ///   drive, and the modifier arm is wired on neither widget yet — a
    ///   slider has no page step to answer with, and one rule on both beats a
    ///   modifier that means something here and nothing there.
    ///
    /// No intent: a spin button broadcasts none by design (see
    /// [`Self::drain_intents`]), so the AI-visible effect is the `value`
    /// introspect field, exactly as for `intervene` and the stepper buttons.
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
        // W3C sign: positive `dy` scrolls DOWN, so a forward wheel arrives
        // negative and must RAISE the value.
        #[allow(
            clippy::cast_precision_loss,
            reason = "a notch count that loses f32 precision is millions of \
                      screens of wheel in one event; `set_value` clamps"
        )]
        let target = self.value - notches as f32 * self.step;
        if !self.set_value(target) {
            self.wheel.reset();
            return false;
        }
        true
    }

    /// A spin button emits no §5.20 intents — its value is observed and
    /// driven through the introspect channel, never broadcast.
    fn drain_intents(&mut self, _sink: &mut dyn FnMut(crate::intent::Intent)) {}

    /// The value never changes on its own; every mutation arrives through a
    /// channel the framework already follows with a repaint (`intervene` /
    /// `invoke`, and since R1533 the [`Self::wheel`] hook — the router
    /// requests a redraw whenever the hovered widget consumed a wheel). So
    /// the spin button is never self-dirty.
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
        IntrospectSchema::new(
            const {
                &[
                    SchemaField::new("value", "float"),
                    SchemaField::new("min", "float"),
                    SchemaField::new("max", "float"),
                    SchemaField::new("step", "float"),
                    // R734.2 — per-stepper interaction state (paint state-layer +
                    // AI-observable hover / pressed feedback).
                    SchemaField::new("dec_state", "string"),
                    SchemaField::new("inc_state", "string"),
                    SchemaField::new("send", "string"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "value" => Some(IntrospectValue::Float(f64::from(self.value))),
            "min" => Some(IntrospectValue::Float(f64::from(self.min))),
            "max" => Some(IntrospectValue::Float(f64::from(self.max))),
            "step" => Some(IntrospectValue::Float(f64::from(self.step))),
            "dec_state" => Some(IntrospectValue::Text(
                self.dec.state().as_name().to_string(),
            )),
            "inc_state" => Some(IntrospectValue::Text(
                self.inc.state().as_name().to_string(),
            )),
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

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
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
            // R51.42 §5.35 composite pointer channel: the `spin#dec` /
            // `spin#inc` paint regions route the full pointer arc here as
            // `invoke("send", "dec:PointerEnter")` … `"dec:PointerUp"`. The
            // `"<sub>:<EventName>"` wire is split by the R660
            // [`composite_tag::parse_send_payload`] SSOT (shared with
            // RadioGroup / ListBox / Table); the key type is `String`
            // because the sub-region is named (`dec` / `inc`), not a
            // numeric index. R734.2 — every pointer edge drives the
            // addressed stepper's `Button` SCXML (so hover / pressed paint
            // tracks the cursor), and `Pressed → Hover` (or
            // `KeyboardActivate`) steps the value via [`Self::drive_stepper`].
            "send" => match args {
                IntrospectValue::Text(ref payload) => {
                    let value = match crate::composite_tag::parse_send_payload::<String>(payload) {
                        // R781 — modifiers ignored (no modifier-aware step).
                        Some((sub, event_name, _)) => match ButtonEvent::from_name(event_name) {
                            Some(ev) => self.drive_stepper(&sub, ev),
                            // Unknown / internal event name: no SM drive,
                            // report the current value (no over-step).
                            None => self.value,
                        },
                        None => self.value,
                    };
                    Ok(IntrospectValue::Float(f64::from(value)))
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
        assert!(
            (SpinButtonExternal::new(50.0, 0.0, 10.0, 1.0).value() - 10.0).abs() < f32::EPSILON
        );
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
        assert!(
            (a.value() - 1.0).abs() < f32::EPSILON,
            "default page_step == step"
        );
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
        s.intervene("value", IntrospectValue::Float(4.0))
            .expect("float accepted");
        assert!((s.value() - 4.0).abs() < f32::EPSILON);
        s.intervene("value", IntrospectValue::Int(99))
            .expect("int coerced + clamped");
        assert!((s.value() - 10.0).abs() < f32::EPSILON);
    }

    #[test]
    fn intervene_range_descriptors_read_only() {
        let mut s = SpinButtonExternal::new(0.0, 0.0, 10.0, 1.0);
        assert_eq!(
            s.intervene("min", IntrospectValue::Float(1.0)),
            Err(InterveneError::ReadOnly)
        );
        assert_eq!(
            s.intervene("max", IntrospectValue::Float(9.0)),
            Err(InterveneError::ReadOnly)
        );
        assert_eq!(
            s.intervene("step", IntrospectValue::Float(2.0)),
            Err(InterveneError::ReadOnly)
        );
    }

    #[test]
    fn intervene_unknown_path_and_type_mismatch() {
        let mut s = SpinButtonExternal::new(0.0, 0.0, 10.0, 1.0);
        assert_eq!(
            s.intervene("speed", IntrospectValue::Float(1.0)),
            Err(InterveneError::UnknownPath)
        );
        assert_eq!(
            s.intervene("value", IntrospectValue::Bool(true)),
            Err(InterveneError::TypeMismatch),
        );
    }

    #[test]
    fn invoke_increment_decrement_page_return_value() {
        let mut s = SpinButtonExternal::new(5.0, 0.0, 10.0, 1.0).with_page_step(3.0);
        assert_eq!(
            s.invoke("increment", IntrospectValue::Null),
            Ok(IntrospectValue::Float(6.0))
        );
        assert_eq!(
            s.invoke("decrement", IntrospectValue::Null),
            Ok(IntrospectValue::Float(5.0))
        );
        assert_eq!(
            s.invoke("page_up", IntrospectValue::Null),
            Ok(IntrospectValue::Float(8.0))
        );
        assert_eq!(
            s.invoke("page_down", IntrospectValue::Null),
            Ok(IntrospectValue::Float(5.0))
        );
        assert_eq!(
            s.invoke("bogus", IntrospectValue::Null),
            Err(InvokeError::UnknownPath)
        );
    }

    #[test]
    fn invoke_send_composite_arc_tracks_stepper_state_and_steps_on_activate() {
        let mut s = SpinButtonExternal::new(5.0, 0.0, 10.0, 1.0);
        // R734.2 — the full pointer arc drives the inc stepper's Button SM:
        // Enter -> Hover, Down -> Pressed (no step yet), Up -> Hover = activate.
        s.invoke("send", IntrospectValue::Text("inc:PointerEnter".into()))
            .unwrap();
        assert_eq!(s.inc_state(), ButtonState::Hover, "Enter -> stepper Hover");
        assert!(
            (s.value() - 5.0).abs() < f32::EPSILON,
            "hover does not step"
        );
        s.invoke("send", IntrospectValue::Text("inc:PointerDown".into()))
            .unwrap();
        assert_eq!(
            s.inc_state(),
            ButtonState::Pressed,
            "Down -> stepper Pressed"
        );
        assert!(
            (s.value() - 5.0).abs() < f32::EPSILON,
            "press alone does not step"
        );
        s.invoke("send", IntrospectValue::Text("inc:PointerUp".into()))
            .unwrap();
        assert_eq!(s.inc_state(), ButtonState::Hover, "Up -> back to Hover");
        assert!(
            (s.value() - 6.0).abs() < f32::EPSILON,
            "activate (Pressed->Hover) -> +step"
        );
        // The dec stepper stayed Idle the whole time (per-region state).
        assert_eq!(s.dec_state(), ButtonState::Idle, "dec untouched by inc arc");
        // Cancel path (Down then Leave) presses then returns to Idle WITHOUT
        // stepping — the W3C button cancel-on-drag-off path.
        s.invoke("send", IntrospectValue::Text("dec:PointerEnter".into()))
            .unwrap();
        s.invoke("send", IntrospectValue::Text("dec:PointerDown".into()))
            .unwrap();
        assert_eq!(s.dec_state(), ButtonState::Pressed);
        s.invoke("send", IntrospectValue::Text("dec:PointerLeave".into()))
            .unwrap();
        assert_eq!(
            s.dec_state(),
            ButtonState::Idle,
            "Leave from Pressed -> Idle (cancel)"
        );
        assert!(
            (s.value() - 6.0).abs() < f32::EPSILON,
            "cancelled press does not step"
        );
        // Unknown sub-region + malformed wire are harmless no-ops (R660 guard).
        s.invoke("send", IntrospectValue::Text("xyz:PointerUp".into()))
            .unwrap();
        s.invoke("send", IntrospectValue::Text("noseparator".into()))
            .unwrap();
        s.invoke("send", IntrospectValue::Text("inc:".into()))
            .unwrap();
        assert!(
            (s.value() - 6.0).abs() < f32::EPSILON,
            "no-op payloads do not step"
        );
    }

    #[test]
    fn query_exposes_stepper_states() {
        let mut s = SpinButtonExternal::new(5.0, 0.0, 10.0, 1.0);
        assert_eq!(
            s.query("dec_state"),
            Some(IntrospectValue::Text("Idle".into()))
        );
        assert_eq!(
            s.query("inc_state"),
            Some(IntrospectValue::Text("Idle".into()))
        );
        s.invoke("send", IntrospectValue::Text("inc:PointerEnter".into()))
            .unwrap();
        assert_eq!(
            s.query("inc_state"),
            Some(IntrospectValue::Text("Hover".into()))
        );
    }

    // ─────────────────────────────────────────────────────────────────
    // R1533 §5.45 §5.38 — the wheel steps the value (Qt
    // `QAbstractSpinBox::wheelEvent`), one rule with the slider.
    // ─────────────────────────────────────────────────────────────────

    const NOTCH_UP: f32 = -crate::event::LINE_HEIGHT_PX;
    const NOTCH_DOWN: f32 = crate::event::LINE_HEIGHT_PX;

    fn wheel(s: &mut SpinButtonExternal, dy: f32) -> bool {
        External::wheel(s, 0.5, 0.5, 0.0, dy, Modifiers::empty())
    }

    #[test]
    fn r1533_one_notch_is_one_step_in_domain_units() {
        // The spin button's step is in DOMAIN units, unlike the slider's
        // normalised one — the same notch means "one of whatever this field
        // counts".
        let mut s = SpinButtonExternal::new(5.0, 0.0, 10.0, 0.5);
        assert!(wheel(&mut s, NOTCH_UP), "consumed");
        assert!((s.value() - 5.5).abs() < 1e-6, "got {}", s.value());
        assert!(wheel(&mut s, NOTCH_DOWN));
        assert!((s.value() - 5.0).abs() < 1e-6, "got {}", s.value());
    }

    #[test]
    fn r1533_several_notches_in_one_event_all_step() {
        let mut s = SpinButtonExternal::new(5.0, 0.0, 100.0, 1.0);
        assert!(wheel(&mut s, NOTCH_UP * 3.0));
        assert!(
            (s.value() - 8.0).abs() < 1e-6,
            "three notches = three steps, got {}",
            s.value()
        );
    }

    #[test]
    fn r1533_sub_notch_wheel_is_consumed_without_moving() {
        let mut s = SpinButtonExternal::new(5.0, 0.0, 10.0, 1.0);
        assert!(wheel(&mut s, NOTCH_UP / 2.0), "half a notch is still ours");
        assert!((s.value() - 5.0).abs() < f32::EPSILON, "got {}", s.value());
        assert!(wheel(&mut s, NOTCH_UP / 2.0));
        assert!(
            (s.value() - 6.0).abs() < 1e-6,
            "two halves make the step, got {}",
            s.value()
        );
    }

    #[test]
    fn r1533_saturated_wheel_is_declined_so_the_page_can_scroll() {
        // The divergence from Qt, which always accepts and so eats the page
        // scroll of any form containing a pinned spin box.
        let mut s = SpinButtonExternal::new(10.0, 0.0, 10.0, 1.0);
        assert!(!wheel(&mut s, NOTCH_UP), "at max, a raise is declined");
        assert!(wheel(&mut s, NOTCH_DOWN), "the other direction still works");
        assert!((s.value() - 9.0).abs() < 1e-6, "got {}", s.value());

        let mut s = SpinButtonExternal::new(0.0, 0.0, 10.0, 1.0);
        assert!(!wheel(&mut s, NOTCH_DOWN), "and the same at min");
    }

    #[test]
    fn r1533_the_wheel_does_not_touch_the_stepper_statecharts() {
        // The steppers are Buttons with their own SCXML; a wheel over the
        // field is not a press on either arrow, and if it drove one the
        // widget would paint a phantom pressed arrow.
        let mut s = SpinButtonExternal::new(5.0, 0.0, 10.0, 1.0);
        assert!(wheel(&mut s, NOTCH_UP));
        assert_eq!(s.dec_state(), ButtonState::Idle);
        assert_eq!(s.inc_state(), ButtonState::Idle);
    }

    #[test]
    fn r1533_the_wheel_is_visible_through_introspect_like_every_other_step() {
        // A spin button broadcasts no intent, so `value` IS the AI-facing
        // report of a wheel — the same field `intervene` and the arrows move.
        let mut s = SpinButtonExternal::new(5.0, 0.0, 10.0, 1.0);
        assert!(wheel(&mut s, NOTCH_UP));
        assert_eq!(s.query("value"), Some(IntrospectValue::Float(6.0)));
    }
}
