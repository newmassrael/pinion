//! Closed core Event enum with opaque External escape (§5.13, R16 slice 5).
//!
//! Top-level [`Event`] groups four categories:
//!   - [`Event::Window`] → [`WindowEvent`] (Close/Focus/Resize/DpiChange per
//!     R15 hedge bullet on §5.13).
//!   - [`Event::Pointer`] → [`PointerEvent`] (cursor/touch input).
//!   - [`Event::Key`] → [`KeyEvent`] (keyboard input).
//!   - [`Event::External`] → opaque escape parallel to §3, allowing
//!     IME/drag-drop/OS-specific events without registry pollution.
//!
//! Per §5.13 caveats, coordinates are *logical* (DPI-aware) and decoupled
//! from the variant via [`Coord::space`]. The R14 hedge `#[non_exhaustive]`
//! lets future variants (Gamepad/HID/Pointer3D, `World3D` coords) land in
//! a `SemVer` minor.
//!
//! Window routing is *not* an `Event` concern: per §5.17, the runtime
//! layer resolves which window an event belongs to before view-fn
//! invocation — `Event` itself stays window-agnostic.
//!
//! # ⚠ NOTHING IN THIS MODULE IS DELIVERED TO A BINDING (measured R1658,
//! restated R1757)
//!
//! [`Event`] is §5.13's ratified vocabulary and it is **not** the shape input
//! actually arrives in. Measured over the whole tree: the only place any
//! variant of it is CONSTRUCTED is this module's own test, and no dispatch
//! path reaches a binding through it. Real input flows through per-hook
//! signatures on [`WidgetCore`](crate::WidgetCore) —
//! [`apply_key_press`](crate::WidgetCore::apply_key_press),
//! `apply_wheel`, `apply_secondary_click`, and their siblings — each taking
//! the facts its own event needs.
//!
//! This is written here because the gap **cost a consumer a wrong PR**. An
//! embedder needing to know when a keystroke arrived read `KeyEvent`, saw it
//! carried only a key code, and asked for a timestamp field on it. Adding one
//! would have delivered nothing to anybody, because nobody receives a
//! `KeyEvent`; the capability landed on `apply_key_press` instead (R1658).
//! Reading a published vocabulary and believing it is what you receive is not
//! a mistake on the reader's part — so the type says so itself now.
//!
//! Reconciling the two is a **spec round**, not a cleanup: §5.13 ratified this
//! enum in Round 5, so deleting it is not this module's decision, and
//! converging dispatch onto it would rewrite every keyboard, pointer and
//! wheel hook in the tree. Until then, treat this module as the ratified
//! taxonomy and the `WidgetCore` hooks as the delivered one.

/// Closed core event categories (§5.13 ratify).
///
/// ⚠ Declared, not delivered — see the module doc. No dispatch path
/// constructs this.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum Event {
    Window(WindowEvent),
    Pointer(PointerEvent),
    Key(KeyEvent),
    /// Opaque escape per §5.13 alternative C — IME/drag-drop/OS-specific
    /// events that the closed core cannot model. Concrete payload typing
    /// arrives with §5.15 External integration contract.
    External(ExternalEventTag),
}

/// Window-scoped lifecycle events (§5.13 R15 hedge bullet).
#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum WindowEvent {
    Close,
    Focus { focused: bool },
    Resize { width: u32, height: u32 },
    DpiChange { scale: f32 },
}

/// Pointer/touch input. Coords are logical per §5.13 caveat.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum PointerEvent {
    Down {
        coord: Coord,
    },
    Up {
        coord: Coord,
    },
    Move {
        coord: Coord,
    },
    /// (R55.C.1 §5.45) Mouse wheel input. `coord` is the pointer
    /// location at the time of the wheel event (same convention as
    /// [`Self::Move`]); the unit-tagged [`WheelDelta`] carries the
    /// scroll magnitude on each axis. The runtime maps this into
    /// scroll-container offset updates via the §5.41 input router.
    Wheel {
        coord: Coord,
        delta: WheelDelta,
    },
}

/// (R55.C.1 §5.45) Mouse wheel delta with explicit unit. Mirrors
/// the W3C `WheelEvent.deltaMode` shape — wheel deltas arrive in
/// different units depending on the input hardware and driver
/// path, and the runtime must distinguish so it scales the scroll
/// offset correctly (one notch on a legacy mouse wheel is not the
/// same scroll magnitude as one pixel of trackpad inertia).
///
/// `#[non_exhaustive]` lets a future `Pages` variant (`PgUp` / `PgDn`
/// driven coarse scroll) or a `World3D`-style unit ride in a minor
/// bump without breaking downstream pattern matches.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WheelDelta {
    /// Logical pixel delta on each axis. The default unit for
    /// trackpads and high-resolution wheels that report
    /// fine-grained movement. Sign convention follows the W3C: a
    /// positive `dy` scrolls *downward* (content shifts up
    /// visually) and a positive `dx` scrolls *rightward*.
    Pixels { dx: f32, dy: f32 },
    /// Discrete line delta on each axis. Standard for legacy
    /// notched mouse wheels that report one click at a time. The
    /// runtime multiplies by [`LINE_HEIGHT_PX`] to derive pixel
    /// offsets. Sign convention matches [`Self::Pixels`].
    Lines { dx: f32, dy: f32 },
}

/// (R51.186 §5.45 R55.C.2; crate-home moved here R877) Default line-height in
/// logical pixels used to convert [`WheelDelta::Lines`] into pixel offsets. The 16-pixel value
/// matches the W3C `WheelEvent` default (`devicePixelRatio == 1.0`) and an embedded browser engine / Firefox
/// / Safari on every desktop platform.
///
/// R877 — this constant is part of the input-forwarding *contract*:
/// [`External::wheel`](crate::external::External::wheel) hands `Lines`
/// deltas pre-scaled by it, so a consumer recovering notch counts
/// (`dy / LINE_HEIGHT_PX` for a per-notch zoom exponent) must read the
/// SAME constant the router scales by. It therefore lives beside
/// [`WheelDelta`] in `pinion-core` (the contract crate), not in the
/// runtime that happens to apply it ([[helper-crate-home-ssot-axis]]).
/// A per-widget override (custom line-height for monospace text
/// containers) is a carry-forward sub-axis (R55.C.4) that lands on top
/// without breaking this API.
pub const LINE_HEIGHT_PX: f32 = 16.0;

/// R1533 §5.45 §5.38 — whole-notch accumulator for a **stepped** wheel
/// consumer: the toolkit's `offset_accumulated` /
/// `wheelDeltaRemainder`, stated once.
///
/// [`External::wheel`](crate::external::External::wheel) hands out a
/// *pixel* delta (`Lines` pre-scaled by [`LINE_HEIGHT_PX`]). A consumer
/// that transforms **continuously** — a canvas zoom exponent — divides and
/// is done, which is why the R877 doc above only mentions the constant. A
/// consumer that moves in **discrete steps** cannot: one notch is
/// [`LINE_HEIGHT_PX`] pixels, so a trackpad reporting 0.4 px an event would
/// round to zero forever and the widget would never move. The remainder has
/// to be banked between events — the same discipline the router keeps for
/// its integer scroll offsets (`wheel_remainders`), and whose absence on one
/// of two paths was already a shipped bug once (R881.1: the carry existed
/// only on the middle-pan copy, so the exact `PixelDelta` stream the docs
/// cited stalled on the wheel path). One home, so it cannot happen again.
///
/// ## The consume verdict (stated here for every stepped consumer)
///
/// A wheel handler answers three ways, and the toolkit's answers are the right
/// ones:
///
/// * **banked** — [`Self::feed`] returned `0`, the motion so far is under one
///   notch. **Consume** (`true`). The wheel belongs to the widget the moment
///   it starts moving over it; declining here would let the enclosing scroll
///   container jitter the page between notches of a slow trackpad drag.
/// * **stepped** — the value moved. Consume.
/// * **saturated** — whole notches fired but the value was already pinned at
///   its bound. **Decline** (`false`) and [`Self::reset`] the carry, so the
///   wheel the widget cannot use reaches the scroll container behind it.
///
/// The borrow checker is why this type offers the two halves rather than one
/// combinator: a `FnOnce(i32) -> bool` applying the step would capture the
/// widget that owns the accumulator.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct WheelStepper {
    /// Sub-notch pixels carried into the next event. Same sign as the
    /// motion that banked them.
    carry: f32,
}

impl WheelStepper {
    /// A stepper with nothing banked.
    #[must_use]
    pub const fn new() -> Self {
        Self { carry: 0.0 }
    }

    /// Fold one event's pixel delta into the carry and return the **whole**
    /// notches that came free (`0` when the motion banked without filling a
    /// notch). Sign follows the W3C convention the delta arrived in —
    /// positive `delta_px` scrolls *downward*, so a value-increasing consumer
    /// negates.
    ///
    /// A direction reversal drops the carry first (the toolkit's
    /// `offset_accumulated = 0` on a sign flip): a user who reverses the wheel
    /// expects the next notch to answer, not to first burn a carry banked the
    /// other way.
    ///
    /// A non-finite delta (a malformed wire payload) banks nothing and
    /// returns `0` rather than poisoning the carry with `NaN` — the
    /// `clamp_frame_dt` / `round_clamp_i32` guard precedent.
    #[allow(clippy::cast_possible_truncation)]
    pub fn feed(&mut self, delta_px: f32) -> i32 {
        if !delta_px.is_finite() {
            return 0;
        }
        if delta_px * self.carry < 0.0 {
            self.carry = 0.0;
        }
        let total = self.carry + delta_px;
        // `trunc` (toward zero), not `round`: a 0.9-notch motion has not
        // reached a notch and must bank whole, or the widget steps early and
        // the carry goes negative against its own direction.
        let notches = (total / LINE_HEIGHT_PX).trunc();
        self.carry = total - notches * LINE_HEIGHT_PX;
        // `f32 as i32` saturates at the integer bounds since Rust 1.45, which
        // is the wanted policy for an absurd wire delta: the caller clamps the
        // value it derives anyway.
        notches as i32
    }

    /// Drop the carry. Called on the **saturated** verdict above, so a wheel
    /// that pushed a pinned value keeps no residue to spend later on a step
    /// the user has since stopped asking for.
    pub fn reset(&mut self) {
        self.carry = 0.0;
    }
}

/// Keyboard input. The `key` field is a placeholder until §5.13 settles
/// the keycode taxonomy (W3C UI Events vs winit virtual key vs raw HID).
///
/// ⚠ **Declared, not delivered.** No dispatch path constructs this, and a
/// binding never receives one — see the module doc for the measurement and
/// for the consumer PR that fact cost. A keystroke reaches a binding as
/// [`KeyPress`](crate::KeyPress) through
/// [`WidgetCore::apply_key_press`](crate::WidgetCore::apply_key_press),
/// carrying the W3C key name, the modifiers, the auto-repeat flag and the
/// arrival. **Anything a keystroke needs to carry belongs on `KeyPress`**;
/// adding a field here delivers it to nobody.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum KeyEvent {
    Down { key: u32 },
    Up { key: u32 },
}

/// Logical DPI-aware coordinate carrying its space tag (§5.13 R14 hedge
/// — per-variant `CoordSpace` future-proofs 3D pointer).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Coord {
    pub x: f32,
    pub y: f32,
    pub space: CoordSpace,
}

impl Coord {
    #[must_use]
    pub const fn logical(x: f32, y: f32) -> Self {
        Self {
            x,
            y,
            space: CoordSpace::Logical,
        }
    }
}

/// Coordinate space discriminator (§5.13 R14 hedge bullet). `World3D` is
/// reserved for future 3D pointer integration; `non_exhaustive` keeps it
/// addable in a minor bump.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CoordSpace {
    Logical,
}

/// Opaque marker for [`Event::External`]. Concrete payload schema is
/// settled by the §5.15 External integration contract (R17+ work). Today
/// the marker only carries forward the *escape* shape so view-fn
/// pattern-matching can stay exhaustive.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, Default)]
pub struct ExternalEventTag;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_top_level_variants_construct() {
        let _ = Event::Window(WindowEvent::Close);
        let _ = Event::Pointer(PointerEvent::Down {
            coord: Coord::logical(0.0, 0.0),
        });
        let _ = Event::Key(KeyEvent::Down { key: 65 });
        let _ = Event::External(ExternalEventTag);
    }

    #[test]
    fn window_event_variants_construct() {
        let _ = WindowEvent::Close;
        let _ = WindowEvent::Focus { focused: true };
        let _ = WindowEvent::Resize {
            width: 800,
            height: 600,
        };
        let _ = WindowEvent::DpiChange { scale: 2.0 };
    }

    #[test]
    fn coord_carries_space() {
        let c = Coord::logical(1.5, 2.5);
        // f32 strict-compare is intentional here: Coord::logical stores
        // the inputs verbatim, no math intervenes.
        assert!((c.x - 1.5).abs() < f32::EPSILON);
        assert!((c.y - 2.5).abs() < f32::EPSILON);
        assert_eq!(c.space, CoordSpace::Logical);
    }

    #[test]
    fn match_arm_exhaustive_within_crate() {
        // Same guard pattern as scene.rs: in-crate exhaustive match
        // forces a maintainer to touch this test when a variant lands.
        let e = Event::Window(WindowEvent::Close);
        match e {
            Event::Window(_) | Event::Pointer(_) | Event::Key(_) | Event::External(_) => {}
        }
    }

    // ─────────────────────────────────────────────────────────────────
    // R55.C.1 §5.45 — Wheel input substrate. The variant carries a
    // unit-tagged [`WheelDelta`] so the runtime / scroll dispatch
    // layer can scale Pixels vs Lines correctly. Input-router
    // wiring to ScrollState lives on a downstream sub-axis.
    // ─────────────────────────────────────────────────────────────────

    #[test]
    fn r55_c1_wheel_pixels_variant_constructs() {
        // R55.C.1 — Pixels variant for trackpads / high-resolution
        // wheels. Sign convention: positive dy scrolls downward.
        let delta = WheelDelta::Pixels { dx: 0.0, dy: 12.5 };
        match delta {
            WheelDelta::Pixels { dx, dy } => {
                assert!((dx - 0.0).abs() < f32::EPSILON);
                assert!((dy - 12.5).abs() < f32::EPSILON);
            }
            WheelDelta::Lines { .. } => panic!("expected Pixels"),
        }
    }

    #[test]
    fn r55_c1_wheel_lines_variant_constructs() {
        // R55.C.1 — Lines variant for legacy notched wheels. The
        // runtime multiplies by a configurable line-height (carry,
        // future round) to derive pixel offsets.
        let delta = WheelDelta::Lines { dx: -1.0, dy: 3.0 };
        match delta {
            WheelDelta::Lines { dx, dy } => {
                assert!((dx + 1.0).abs() < f32::EPSILON);
                assert!((dy - 3.0).abs() < f32::EPSILON);
            }
            WheelDelta::Pixels { .. } => panic!("expected Lines"),
        }
    }

    #[test]
    fn r55_c1_wheel_event_round_trips_through_pointer() {
        // R55.C.1 — PointerEvent::Wheel carries (coord, delta).
        // The coord follows the same convention as Move / Down /
        // Up — pointer location at the time of the wheel input.
        let coord = Coord::logical(120.0, 240.0);
        let delta = WheelDelta::Pixels { dx: 0.0, dy: 8.0 };
        let event = Event::Pointer(PointerEvent::Wheel { coord, delta });
        match event {
            Event::Pointer(PointerEvent::Wheel { coord: c, delta: d }) => {
                assert_eq!(c, coord);
                assert_eq!(d, delta);
            }
            _ => panic!("expected Pointer(Wheel) variant"),
        }
    }

    #[test]
    fn r55_c1_pointer_event_match_exhaustive_within_crate() {
        // R55.C.1 — in-crate exhaustive match guard forces a
        // maintainer to touch this test when a new PointerEvent
        // variant lands. Same shape as the top-level Event guard.
        let pe = PointerEvent::Move {
            coord: Coord::logical(0.0, 0.0),
        };
        match pe {
            PointerEvent::Down { .. }
            | PointerEvent::Up { .. }
            | PointerEvent::Move { .. }
            | PointerEvent::Wheel { .. } => {}
        }
    }

    #[test]
    fn r55_c1_wheel_delta_match_exhaustive_within_crate() {
        // R55.C.1 — exhaustive match guard for the WheelDelta unit
        // enum. A future Pages variant (PgUp / PgDn) trips this.
        let d = WheelDelta::Pixels { dx: 0.0, dy: 0.0 };
        match d {
            WheelDelta::Pixels { .. } | WheelDelta::Lines { .. } => {}
        }
    }

    // ─────────────────────────────────────────────────────────────────
    // R1533 §5.45 §5.38 — [`WheelStepper`], the whole-notch accumulator
    // a stepped wheel consumer needs (the toolkit `offset_accumulated`).
    // ─────────────────────────────────────────────────────────────────

    #[test]
    fn r1533_one_notch_of_pixels_is_one_step() {
        let mut s = WheelStepper::new();
        assert_eq!(s.feed(LINE_HEIGHT_PX), 1, "one notch of pixels = one step");
        assert_eq!(s.feed(-LINE_HEIGHT_PX), -1, "and it signs with the motion");
    }

    #[test]
    fn r1533_sub_notch_motion_banks_until_it_fills_a_notch() {
        // The reason this type exists: a trackpad reporting a fraction of a
        // notch per event must still move the widget. Rounding each event
        // independently returns 0 forever.
        let mut s = WheelStepper::new();
        let tenth = LINE_HEIGHT_PX / 10.0;
        for i in 1..=9 {
            assert_eq!(s.feed(tenth), 0, "event {i} is under one notch");
        }
        assert_eq!(s.feed(tenth), 1, "the tenth tenth completes the notch");
        assert_eq!(s.feed(tenth), 0, "and the carry restarts from zero");
    }

    #[test]
    fn r1533_a_notch_and_a_half_steps_once_and_banks_the_half() {
        let mut s = WheelStepper::new();
        assert_eq!(s.feed(LINE_HEIGHT_PX * 1.5), 1, "the whole notch fires");
        assert_eq!(
            s.feed(LINE_HEIGHT_PX * 0.5),
            1,
            "the banked half plus a half is the second notch — a `round` \
             here would have spent the half early and owed it back"
        );
    }

    #[test]
    fn r1533_several_notches_at_once_all_step() {
        // A notched mouse wheel spun hard, or an RPC replay with a large
        // pixel delta: every whole notch in the event must count.
        let mut s = WheelStepper::new();
        assert_eq!(s.feed(LINE_HEIGHT_PX * 4.0), 4);
    }

    #[test]
    fn r1533_direction_reversal_drops_the_carry() {
        // The toolkit's sign-flip reset. Without it the first notch back the
        // other way has to burn a carry banked in the direction the user just
        // left.
        let mut s = WheelStepper::new();
        assert_eq!(s.feed(LINE_HEIGHT_PX * 0.9), 0, "0.9 notches banked down");
        assert_eq!(
            s.feed(-LINE_HEIGHT_PX),
            -1,
            "reversing answers with a full notch immediately"
        );
        assert_eq!(
            s.feed(-LINE_HEIGHT_PX * 0.9),
            0,
            "and the dropped 0.9 did not survive to leak into the reversal"
        );
    }

    #[test]
    fn r1533_reset_discards_the_carry() {
        let mut s = WheelStepper::new();
        assert_eq!(s.feed(LINE_HEIGHT_PX * 0.9), 0);
        s.reset();
        assert_eq!(
            s.feed(LINE_HEIGHT_PX * 0.5),
            0,
            "after a saturated step the banked 0.9 is gone — 0.5 alone is \
             under a notch"
        );
    }

    #[test]
    fn r1533_non_finite_delta_banks_nothing() {
        // A malformed `scene/wheel` payload must not poison the carry: once
        // NaN is banked every later comparison is false and the widget is
        // dead for the rest of the session.
        let mut s = WheelStepper::new();
        assert_eq!(s.feed(f32::NAN), 0);
        assert_eq!(s.feed(f32::INFINITY), 0);
        assert_eq!(
            s.feed(LINE_HEIGHT_PX),
            1,
            "the stepper still works after being handed garbage"
        );
    }
}
