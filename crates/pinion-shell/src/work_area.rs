//! R1621 §5.16 §5.41 — the platform probe behind
//! [`pinion_core::display::UsableRegion`]: what part of the desk is not covered
//! by panels, docks and taskbars.
//!
//! # Why this is a probe and not geometry
//!
//! winit does not answer it. `MonitorHandle` exposes name, size, position,
//! refresh rate, scale factor and video modes, and nothing about a work area —
//! so every backend that wants one goes to the window system itself.
//!
//! On X11 the window system's answer is the EWMH `_NET_WORKAREA` property on
//! the root window: a list of `x, y, w, h` quadruples, one per virtual desktop,
//! in **desktop** coordinates. It is one rectangle for the whole desk, not one
//! per monitor, and there is no atom that gives per-monitor work areas — a
//! limitation the reference's own X11 plugin documents at length in its source
//! and resolves by returning the full screen bounds on any multi-head system.
//! What this module does with that is
//! [`pinion_core::display::usable_regions`]'s business; this module's job is to
//! **get the rectangle or find out there isn't one**, and to be honest about
//! which happened.
//!
//! # Wayland
//!
//! There is no equivalent. The protocol does not tell a client where the shell's
//! panels are — a client is not supposed to know, because it is not supposed to
//! position itself. So the probe answers `None` and every display reports
//! [`Unpublished`](pinion_core::display::UsableRegion::Unpublished): the
//! platform was asked and has nothing, which is a different fact from nobody
//! having asked.

use pinion_core::display::DisplayRect;

/// R1621 — read the desktop's work area from the window system, or `None` when
/// the platform does not publish one.
///
/// `None` is a real answer here, not a failure: it says the platform was asked.
/// The caller turns it into
/// [`Unpublished`](pinion_core::display::UsableRegion::Unpublished), which a
/// topology that was never probed can be told apart from.
///
/// Every failure inside the probe — no display, a refused connection, a missing
/// or malformed property — collapses to `None` deliberately. A work area is an
/// optimisation on window placement, and a shell that refused to start because
/// a property was absent would be trading a cosmetic answer for the whole
/// application.
#[must_use]
pub fn probe_work_area() -> Option<DisplayRect> {
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        x11_work_area()
    }
    #[cfg(not(all(unix, not(target_os = "macos"))))]
    {
        None
    }
}

/// R1621 — the EWMH `_NET_WORKAREA` read.
///
/// Returns the **first** quadruple, which is the current virtual desktop's work
/// area on every window manager that publishes more than one. Reading only the
/// first is what the reference does too, and for the same stated reason: the
/// later ones describe the WM's other virtual desktops, and a display model has
/// no concept of those to attribute them to.
#[cfg(all(unix, not(target_os = "macos")))]
fn x11_work_area() -> Option<DisplayRect> {
    use x11rb::connection::Connection as _;
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt as _};

    // A Wayland session has no X display unless XWayland is running, and under
    // XWayland the atom describes the X root window rather than the compositor's
    // panels — so an answer there would be about the wrong desk. Checking the
    // session type first keeps that guess from being made at all.
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        return None;
    }
    let (conn, screen_num) = x11rb::connect(None).ok()?;
    let root = conn.setup().roots.get(screen_num)?.root;
    let atom = conn
        .intern_atom(true, b"_NET_WORKAREA")
        .ok()?
        .reply()
        .ok()?
        .atom;
    if atom == x11rb::NONE {
        // The atom does not exist, so nothing has ever published a work area:
        // a bare window manager, or none at all.
        return None;
    }
    let reply = conn
        .get_property(false, root, atom, AtomEnum::CARDINAL, 0, 4)
        .ok()?
        .reply()
        .ok()?;
    let mut values = reply.value32()?;
    let x = i32::try_from(values.next()?).ok()?;
    let y = i32::try_from(values.next()?).ok()?;
    let w = values.next()?;
    let h = values.next()?;
    // A zero-area work area is a malformed publication, not a desk with no
    // usable space. Refusing it here keeps the "certainly wrong" answer — that
    // no part of the display can be used — off the wire.
    if w == 0 || h == 0 {
        return None;
    }
    Some(DisplayRect::new(x, y, w, h))
}

#[cfg(test)]
mod tests {
    use super::probe_work_area;

    /// R1621 — the probe answers without panicking whatever session this runs
    /// in, and whatever it answers is a usable rectangle rather than a
    /// degenerate one.
    ///
    /// Deliberately not asserting WHICH answer: CI is headless, a developer's
    /// box has a window manager, and a test that demanded either would fail on
    /// the other machine for a reason that is not a defect. What is universal
    /// is the shape — and the zero-area refusal, which is the case that would
    /// otherwise publish "none of this display is usable".
    #[test]
    fn r1621_the_probe_answers_a_usable_rect_or_nothing() {
        match probe_work_area() {
            None => {}
            Some(rect) => {
                assert!(rect.w > 0, "a published work area has width: {rect:?}");
                assert!(rect.h > 0, "and height: {rect:?}");
            }
        }
    }
}
