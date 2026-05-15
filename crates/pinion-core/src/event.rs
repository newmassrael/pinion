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
}
