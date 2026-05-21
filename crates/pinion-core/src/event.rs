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

/// Closed core event categories (§5.13 ratify).
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
    Down { coord: Coord },
    Up { coord: Coord },
    Move { coord: Coord },
    /// (R55.C.1 §5.45) Mouse wheel input. `coord` is the pointer
    /// location at the time of the wheel event (same convention as
    /// [`Self::Move`]); the unit-tagged [`WheelDelta`] carries the
    /// scroll magnitude on each axis. The runtime maps this into
    /// scroll-container offset updates via the §5.41 input router.
    Wheel { coord: Coord, delta: WheelDelta },
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
    /// runtime multiplies by a configurable line-height to derive
    /// pixel offsets. Sign convention matches [`Self::Pixels`].
    Lines { dx: f32, dy: f32 },
}

/// Keyboard input. The `key` field is a placeholder until §5.13 settles
/// the keycode taxonomy (W3C UI Events vs winit virtual key vs raw HID).
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
}
