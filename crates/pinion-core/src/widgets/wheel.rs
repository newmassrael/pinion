//! R1703 §5.45 §5.49 §5.15 — **what a wheel does here, said out loud**.
//!
//! A wheel is the one gesture whose meaning is entirely a matter of local
//! policy. The same motion over the same pixel scrolls a page, steps a value,
//! flips a tab or zooms a canvas, and *nothing about the event says which*.
//! Every toolkit resolves this the same way — the widget under the cursor
//! overrides a wheel handler and does whatever it does — and the cost is
//! identical everywhere: the behaviour is discoverable only by trying it.
//!
//! Measured on the reference toolkit at 6.11.1, by building a probe and running
//! it rather than by reading its documentation: across the four widget classes
//! that answer a wheel there, **309 introspectable properties and 172
//! introspectable methods contain zero** that name the wheel. The behaviour is
//! real, and no program — an assistive technology, a test, an agent driving the
//! application, the application's own settings screen — can ask what it is. The
//! same probe measured the consequence that costs users: a closed combo box
//! sitting in a scrolling form **changes its value** on a wheel the person aimed
//! at the form, whether or not it has focus, and nothing in the widget's
//! interface lets a form say otherwise.
//!
//! So a wheel consumer here **declares** what a wheel does at it
//! ([`External::wheel_intent`]) and the framework makes that declaration the
//! *precondition of dispatch*: a widget with no declaration is never offered a
//! wheel, and the wheel goes to the scroll chain behind it exactly as if the
//! widget were not there. The declaration is published on the wire
//! (`scene/wheel_intent`), so the question the reference cannot answer is one
//! sentence here — and, because the same declaration is what routes the event,
//! an answer that disagrees with the behaviour is not possible without the
//! behaviour disappearing.
//!
//! ## Why this is not the pointer story again
//!
//! A press has a target: whatever is under it takes it, and R1700 gave the
//! framework the means to check that the thing drawn there is the thing pressed.
//! A wheel has a *chain* — the W3C model this router already implements, where a
//! listener may consume ahead of the scroll default action. The chain is what
//! makes silence expensive: a widget that eats a wheel it did not need does not
//! fail visibly, it just makes the container behind it stop scrolling, and the
//! person cannot tell which of the things under the cursor did it.
//!
//! [`External::wheel_intent`]: crate::external::External::wheel_intent

use crate::event::WheelStepper;
use crate::input::{GesturePhase, Modifiers};

/// One wheel event, as the widget under the cursor reads it.
///
/// A struct rather than five positional parameters, because two of the five are
/// a coordinate and two are a delta and the pairs are the same shape — a
/// consumer that swapped them would compile. It is also the extension point:
/// this round added [`phase`](Self::phase) to the event, and doing so touched
/// the trait's implementors rather than every implementor's signature.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WheelReading {
    /// The cursor, normalised over the same rect
    /// [`capture_normalize`](crate::external::External::capture_normalize)
    /// selects for `pointer_move` — so a zoom anchored under the cursor and a
    /// drag share one coordinate basis.
    pub at: (f32, f32),
    /// The delta in logical pixels, W3C sign: positive `dy` scrolls content
    /// downward. `Lines` are pre-scaled by
    /// [`LINE_HEIGHT_PX`](crate::event::LINE_HEIGHT_PX) before they get here.
    pub delta: (f32, f32),
    /// Where this event sits in a continuous gesture.
    ///
    /// A mouse notch is its own beginning and end and arrives as
    /// [`Update`](GesturePhase::Update) — the phase winit reports for one; a
    /// trackpad's two-finger scroll is a stream that begins, updates and ends.
    /// pinion discarded the field at the winit boundary until R1703, which is
    /// why the rule [`WheelSteps::feed`] enforces had nowhere to live.
    ///
    /// The **same** [`GesturePhase`] the pinch and rotation gestures carry, not
    /// a wheel-shaped copy of it: the bracket is the gesture-agnostic half and
    /// two enums with the same four arms is how two producers come to disagree
    /// about what "cancelled" means.
    pub phase: GesturePhase,
    /// The keyboard modifiers held while it arrived.
    pub modifiers: Modifiers,
}

impl WheelReading {
    /// A reading at `at`, moving by `delta`.
    #[must_use]
    pub const fn new(
        at: (f32, f32),
        delta: (f32, f32),
        phase: GesturePhase,
        modifiers: Modifiers,
    ) -> Self {
        Self {
            at,
            delta,
            phase,
            modifiers,
        }
    }

    /// The vertical delta — the axis every mouse has.
    #[must_use]
    pub const fn dy(&self) -> f32 {
        self.delta.1
    }

    /// The horizontal delta.
    #[must_use]
    pub const fn dx(&self) -> f32 {
        self.delta.0
    }

    /// Which way the wheel went, or `None` for an event that carries no
    /// vertical motion at all (a phase-only end marker, a horizontal-only
    /// trackpad event, a malformed non-finite payload).
    ///
    /// A direction and not a magnitude, because that is what a *scale* wants: a
    /// canvas zoom is one step per event whichever way the platform reports the
    /// size of a notch, and reading the magnitude there is what makes a zoom
    /// leap on one mouse and crawl on another.
    #[must_use]
    pub fn direction(&self) -> Option<WheelDirection> {
        let dy = self.delta.1;
        if !dy.is_finite() || dy == 0.0 {
            return None;
        }
        // W3C sign: a positive `dy` scrolls the content DOWN, which is the
        // wheel pulled toward the person.
        Some(if dy < 0.0 {
            WheelDirection::Away
        } else {
            WheelDirection::Toward
        })
    }
}

/// Which way a wheel was turned, named from the person and not from the
/// coordinate system.
///
/// "Up" and "down" are already taken twice over — by the content's motion and by
/// the delta's sign, which are opposites — so a consumer reading either has to
/// remember which it holds. Nobody is confused about which way they pushed
/// their finger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WheelDirection {
    /// Pushed away from the person: content moves up, values and scales rise.
    Away,
    /// Pulled toward the person: content moves down, values and scales fall.
    Toward,
}

impl WheelDirection {
    /// `+1` away, `-1` toward — the multiplier a stepping consumer applies.
    #[must_use]
    pub const fn sign(self) -> i32 {
        match self {
            Self::Away => 1,
            Self::Toward => -1,
        }
    }

    /// `factor` applied once in this direction: `factor` away, its reciprocal
    /// toward.
    ///
    /// The multiplicative step a scale takes per event, so that zooming in and
    /// out the same number of times returns to where it started — which
    /// repeated addition of a percentage does not.
    #[must_use]
    pub fn scaled(self, factor: f64) -> f64 {
        match self {
            Self::Away => factor,
            Self::Toward => 1.0 / factor,
        }
    }
}

/// What one notch moves, for a consumer that moves in discrete steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StepUnit {
    /// One entry of a list: a combo box option, a tab, a row.
    Item,
    /// One step of a numeric value: a slider, a spin button.
    Value,
}

impl StepUnit {
    /// The word the wire uses.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Item => "item",
            Self::Value => "value",
        }
    }
}

/// What a wheel does at a surface.
///
/// Only the meanings a widget can *take away from the scroll chain* are here.
/// "It scrolls" is not an arm: scrolling is what happens when nobody declares
/// anything, and an arm nobody ever returns is a declaration the compiler cannot
/// tell from a mistake (R1684, where exactly such an arm sat unread for three
/// rounds).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WheelIntent {
    /// Move a discrete position by whole notches.
    Step(StepUnit),
    /// Change a view's scale, holding the canvas point under the cursor still.
    ///
    /// The anchor is not a parameter because a wheel zoom that does not hold the
    /// point under the cursor is the thing people complain about; a zoom
    /// anchored anywhere else is a *button*, and buttons do not arrive here.
    Zoom,
}

impl WheelIntent {
    /// The word the wire uses for this intent.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Step(_) => "step",
            Self::Zoom => "zoom",
        }
    }

    /// What one notch moves, for a stepping intent.
    #[must_use]
    pub const fn unit(self) -> Option<StepUnit> {
        match self {
            Self::Step(unit) => Some(unit),
            Self::Zoom => None,
        }
    }
}

/// A stepped consumer's whole-notch accumulator, with the gesture's end wired
/// in.
///
/// [`WheelStepper`] banks the sub-notch remainder and knows nothing about when
/// the gesture stops, because until this round the phase never reached it. This
/// is that accumulator plus the one rule the phase exists for, so the rule is
/// written once instead of at each of the widgets that step.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct WheelSteps {
    inner: WheelStepper,
}

impl WheelSteps {
    /// An accumulator with nothing banked.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            inner: WheelStepper::new(),
        }
    }

    /// The whole notches `reading` releases, banking the remainder — and
    /// dropping that remainder when the reading
    /// [settles](GesturePhase::settles) the gesture.
    ///
    /// Sign follows the delta's own W3C convention (positive is downward), so a
    /// value-raising consumer negates; [`WheelDirection`] is the alternative for
    /// a consumer that wants the person's word for it.
    pub fn feed(&mut self, reading: &WheelReading) -> i32 {
        let notches = self.inner.feed(reading.dy());
        if reading.phase.settles() {
            self.inner.reset();
        }
        notches
    }

    /// Drop the banked remainder — the **saturated** verdict, where whole
    /// notches fired against a value already at its bound and the wheel should
    /// reach the container behind.
    pub fn reset(&mut self) {
        self.inner.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reading(dy: f32, phase: GesturePhase) -> WheelReading {
        WheelReading::new((0.5, 0.5), (0.0, dy), phase, Modifiers::empty())
    }

    #[test]
    fn r1703_direction_is_the_persons_word_not_the_signs() {
        // W3C: a positive dy scrolls the content down, which is the wheel
        // pulled toward the person.
        assert_eq!(
            reading(-16.0, GesturePhase::Update).direction(),
            Some(WheelDirection::Away)
        );
        assert_eq!(
            reading(16.0, GesturePhase::Update).direction(),
            Some(WheelDirection::Toward)
        );
    }

    #[test]
    fn r1703_an_event_with_no_vertical_motion_has_no_direction() {
        assert_eq!(reading(0.0, GesturePhase::End).direction(), None);
        assert_eq!(reading(f32::NAN, GesturePhase::Update).direction(), None);
        assert_eq!(
            reading(f32::INFINITY, GesturePhase::Update).direction(),
            None
        );
    }

    #[test]
    fn r1703_a_scale_returns_to_where_it_started() {
        // The property a multiplicative step has and repeated addition of a
        // percentage does not: in and out the same number of times is identity.
        let start = 0.84_f64;
        let mut z = start;
        for _ in 0..7 {
            z *= WheelDirection::Away.scaled(1.12);
        }
        for _ in 0..7 {
            z *= WheelDirection::Toward.scaled(1.12);
        }
        assert!(
            (z - start).abs() < 1e-12,
            "seven steps out and back landed at {z}, not {start}"
        );
    }

    #[test]
    fn r1703_a_notch_is_banked_until_it_is_whole() {
        // A trackpad reporting a fraction of a notch an event moves nothing
        // until the fractions add up — and then moves exactly once.
        let mut steps = WheelSteps::new();
        assert_eq!(steps.feed(&reading(4.0, GesturePhase::Begin)), 0);
        assert_eq!(steps.feed(&reading(4.0, GesturePhase::Update)), 0);
        assert_eq!(steps.feed(&reading(4.0, GesturePhase::Update)), 0);
        assert_eq!(steps.feed(&reading(4.0, GesturePhase::Update)), 1);
    }

    #[test]
    fn r1703_the_end_of_a_gesture_takes_the_remainder_with_it() {
        // ★ The rule the phase exists for. Nine tenths of a notch banked by a
        // flick must not be spent by the NEXT gesture, which may be minutes
        // later and aimed at something else.
        let mut steps = WheelSteps::new();
        assert_eq!(steps.feed(&reading(14.0, GesturePhase::Begin)), 0);
        assert_eq!(steps.feed(&reading(0.0, GesturePhase::End)), 0);
        // A fresh gesture of the same size must still not have reached a notch.
        assert_eq!(steps.feed(&reading(14.0, GesturePhase::Begin)), 0);
    }

    #[test]
    fn r1703_a_cancel_settles_exactly_as_an_end_does() {
        // The platform taking the gesture away must not leave the widget
        // holding a remainder either — a cancelled flick is still finished.
        let mut steps = WheelSteps::new();
        assert_eq!(steps.feed(&reading(14.0, GesturePhase::Begin)), 0);
        assert_eq!(steps.feed(&reading(0.0, GesturePhase::Cancel)), 0);
        assert_eq!(steps.feed(&reading(14.0, GesturePhase::Begin)), 0);
    }

    #[test]
    fn r1703_without_the_end_the_remainder_survives_and_that_is_the_defect() {
        // The counterfactual for the rule above, stated as a test so the rule
        // cannot be removed silently: a stepper that never hears the end DOES
        // spend the carry on the next gesture.
        let mut steps = WheelSteps::new();
        assert_eq!(steps.feed(&reading(14.0, GesturePhase::Begin)), 0);
        assert_eq!(steps.feed(&reading(14.0, GesturePhase::Begin)), 1);
    }

    /// ★★★★★ R1703 — **what every catalog widget declares, in one place, as a
    /// table.**
    ///
    /// Found by a counterfactual that PASSED: making the slider claim it steps
    /// ITEMS rather than a VALUE left the whole of `pinion-core` green. The
    /// declaration was checked only by the integration demo that happened to
    /// look at it, so a widget could publish a wrong answer to
    /// `scene/wheel_intent` and every unit test would agree.
    ///
    /// A table rather than one assertion per widget, because the population is
    /// what matters: adding a wheel to a widget without adding its row leaves
    /// the new declaration unchecked, and the count below is what says so.
    #[test]
    fn r1703_every_catalog_wheel_declares_what_it_says_it_declares() {
        use crate::external::External;

        let slider = crate::widgets::slider::SliderExternal::new();
        let spin = crate::widgets::spin_button::SpinButtonExternal::new(0.0, 0.0, 10.0, 1.0);
        let list = crate::widgets::listbox::ListBoxExternal::new(3);
        let strip = crate::widgets::radio_group::RadioGroupExternal::new(3).with_wheel(true);
        let radios = crate::widgets::radio_group::RadioGroupExternal::new(3);

        let mut declared: Vec<(&str, Option<WheelIntent>)> = vec![
            ("slider", slider.wheel_intent((0.5, 0.5))),
            ("spin button", spin.wheel_intent((0.5, 0.5))),
            ("list", list.wheel_intent((0.5, 0.5))),
            ("tab strip", strip.wheel_intent((0.5, 0.5))),
            ("a form's radio set", radios.wheel_intent((0.5, 0.5))),
        ];
        let mut want: Vec<(&str, Option<WheelIntent>)> = vec![
            // A value with steps steps its value.
            ("slider", Some(WheelIntent::Step(StepUnit::Value))),
            ("spin button", Some(WheelIntent::Step(StepUnit::Value))),
            // A strip of destinations walks under a wheel — measured on the
            // floor's tab bar: a notch down at tab 0 lands on tab 1.
            ("tab strip", Some(WheelIntent::Step(StepUnit::Item))),
            // ★★ And the two that DECLINE, which is where this catalogue and
            // the floor genuinely differ, both ways round. A list scrolls
            // rather than stepping (measured: `currentRow 5 -> 5`,
            // `scrollbar 0 -> 3`) and declining is how it hands the event to
            // the scroll chain. A form's mutually exclusive answers must not
            // change because a person scrolled past them — which is exactly
            // what the floor's CLOSED, UNFOCUSED combo box does to a value.
            ("list", None),
            ("a form's radio set", None),
        ];
        declared.sort_by_key(|(name, _)| *name);
        want.sort_by_key(|(name, _)| *name);
        assert_eq!(
            declared, want,
            "a catalog widget's declared wheel is what `scene/wheel_intent` \
             publishes AND what the router routes by, so a wrong row here is a \
             wrong answer to an agent and a wrong dispatch at the same time"
        );
        assert_eq!(
            declared.iter().filter(|(_, i)| i.is_some()).count(),
            3,
            "three catalog widgets take a wheel; a fourth without a row here \
             would be declaring something nothing checks"
        );
    }

    #[test]
    fn r1703_an_intent_says_what_it_moves() {
        assert_eq!(WheelIntent::Step(StepUnit::Item).as_str(), "step");
        assert_eq!(
            WheelIntent::Step(StepUnit::Item).unit(),
            Some(StepUnit::Item)
        );
        assert_eq!(WheelIntent::Zoom.as_str(), "zoom");
        assert_eq!(WheelIntent::Zoom.unit(), None);
        assert_eq!(StepUnit::Value.as_str(), "value");
    }
}
