//! Overlay event types — transport-agnostic per §5.33.
//!
//! These do not depend on winit, web, or any other backend. A consumer
//! (typically an example or `pinion-runtime`) lowers backend-specific
//! input (raw mouse coordinates, keyboard scancodes, touch gestures) to
//! these variants. That mapping is intentionally outside `pinion-overlay`
//! — keeping the crate dependency surface small lets the same overlay
//! logic drive GUI / TUI / headless test consumers without conditional
//! compilation.
//!
//! Coordinate units match §5.32 [`scene/locate`](fn@pinion_rpc::locate):
//! viewport-relative *logical* pixels (CSS px). DPI / device-pixel
//! conversion is the backend's responsibility before constructing an
//! [`OverlayEvent`].

/// Discrete AI-overlay input. Closed enum at v0 — adding a variant is
/// a §5.33 carry-forward decision (a future `Hover`, `KeyChord`, or
/// `Touch` would each have a separate ratify step).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayEvent {
    /// Single-point selection. Lowered from a primary pointer click or
    /// touch tap. Coordinates are logical px, viewport-relative.
    Click { x: u32, y: u32 },
    /// Rectangular region selection. Coordinates form the two diagonal
    /// corners; semantically the *normalised* rect (with non-negative
    /// width/height) is what consumers should pass to
    /// [`pinion_rpc::locate_region`]. The enum stores raw corners so
    /// backends do not need to normalise before lowering.
    Drag { x1: u32, y1: u32, x2: u32, y2: u32 },
    /// Cancel / dismiss the current overlay state. Consumers typically
    /// respond by calling [`crate::clear_highlights`].
    Escape,
    /// Acknowledge / confirm — equivalent to "accept the AI's
    /// suggestion". Reserved for the R40+ `scene/propose_change`
    /// surface; v0 consumers may ignore it.
    Acknowledge,
}

impl OverlayEvent {
    /// Normalise a [`Self::Drag`] into a top-left + width/height tuple.
    /// Returns the same `(x, y, w, h)` as
    /// [`pinion_rpc::locate_region`] expects. Other variants return
    /// `None`.
    #[must_use]
    pub fn drag_as_rect(&self) -> Option<(u32, u32, u32, u32)> {
        match *self {
            Self::Drag { x1, y1, x2, y2 } => {
                let x = x1.min(x2);
                let y = y1.min(y2);
                let w = x1.abs_diff(x2);
                let h = y1.abs_diff(y2);
                Some((x, y, w, h))
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drag_as_rect_normalises_corner_order() {
        // Backend may report drag start at any corner; the rect is the
        // bounding box regardless of direction.
        let forward = OverlayEvent::Drag {
            x1: 10,
            y1: 20,
            x2: 60,
            y2: 80,
        };
        let backward = OverlayEvent::Drag {
            x1: 60,
            y1: 80,
            x2: 10,
            y2: 20,
        };
        let mixed = OverlayEvent::Drag {
            x1: 60,
            y1: 20,
            x2: 10,
            y2: 80,
        };
        let expected = (10, 20, 50, 60);
        assert_eq!(forward.drag_as_rect(), Some(expected));
        assert_eq!(backward.drag_as_rect(), Some(expected));
        assert_eq!(mixed.drag_as_rect(), Some(expected));
    }

    #[test]
    fn drag_as_rect_zero_extent_when_same_point() {
        let e = OverlayEvent::Drag {
            x1: 5,
            y1: 5,
            x2: 5,
            y2: 5,
        };
        assert_eq!(e.drag_as_rect(), Some((5, 5, 0, 0)));
    }

    #[test]
    fn non_drag_variants_return_none_from_drag_as_rect() {
        assert!(OverlayEvent::Click { x: 0, y: 0 }.drag_as_rect().is_none());
        assert!(OverlayEvent::Escape.drag_as_rect().is_none());
        assert!(OverlayEvent::Acknowledge.drag_as_rect().is_none());
    }
}
