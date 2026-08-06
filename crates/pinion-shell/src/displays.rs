//! R1576 §5.16 §5.41 §5.23 — the binding-facing "what monitors am I on?" hook.
//!
//! # Why this exists
//!
//! [`crate::WindowSpec::display`] lets a binding say *which monitor* a window's
//! position is measured from, and that declaration needs an **id**. An agent
//! driving the wire reads one from `scene/displays`; a binding driving its own
//! UI — a "move panel to…" menu, a layout preset picker, a tear-off that opens
//! on the monitor under the cursor — had nowhere to read one at all, which
//! would leave in-process code below Qt (`QGuiApplication::screens()` is
//! ordinary application API there) while the wire was past it.
//!
//! # Shape, and why it is a pull rather than a signal
//!
//! [`use_displays`] answers with the desk as it was at the surface's last
//! stamp. It is deliberately **not** a [`pinion_core::Signal`]:
//!
//! * winit 0.30 emits no monitor-change event, so a reactive value would have
//!   no honest moment to fire and would mostly be a signal that never changes.
//! * A binding reads the desk when it is about to *place* something — opening a
//!   menu, applying a preset — which is a pull, not a subscription.
//!
//! The stamp is refreshed when a window is created and at every RPC dispatch,
//! so the answer tracks a hot-plug as soon as anything asks. That limit is
//! stated rather than papered over: a binding that paints a monitor list and
//! nothing else will not see a monitor arrive until the next window event or
//! RPC call.
//!
//! # Why a handle and not a `ShellCore` method
//!
//! A binding never reaches the live [`crate::ShellCore`] — every `WidgetView` /
//! `WidgetCore` method is an associated fn with no `self`, and `AppShell`'s
//! `core` field is private. The same argument
//! [`crate::window_control`](mod@crate::window_control) makes, and the same
//! answer: a [`pinion_core::ProviderSlot`] the surface seeds, whose default is
//! an empty desk rather than a panic, so a binding under a bare `Owner` (a
//! unit test, a TUI backend) reads "no monitors" instead of failing.

use std::sync::{Arc, RwLock};

use pinion_core::ProviderSlot;
use pinion_core::display::DisplayTopology;

/// The shared cell the surface stamps and [`use_displays`] reads.
///
/// `RwLock` rather than `RefCell` because the slot's value is an `Arc` that may
/// be captured by an off-thread producer exactly as the sinks beside it are —
/// a data source thread deciding which monitor to open a window on is the same
/// shape [`pinion_core::RepaintSink`] exists for. Reads dominate by construction
/// (one stamp per dispatch, any number of pulls per frame).
#[derive(Debug, Default)]
pub struct DisplayHandle {
    desk: RwLock<DisplayTopology>,
}

impl DisplayHandle {
    /// A handle reporting a headless desk — the state before the surface has
    /// looked, and the honest answer for a backend with no window system.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Stamp the monitors attached right now. The surface's job.
    ///
    /// A poisoned lock is ignored rather than propagated: the writer holds it
    /// for one move and panics nowhere inside, so poisoning means another
    /// thread died elsewhere, and refusing to update the desk would turn an
    /// unrelated failure into a wrong answer about the monitors.
    pub fn set(&self, topology: DisplayTopology) {
        if let Ok(mut desk) = self.desk.write() {
            *desk = topology;
        }
    }

    /// The desk as of the last stamp. Empty before the first one.
    #[must_use]
    pub fn get(&self) -> DisplayTopology {
        self.desk
            .read()
            .map(|desk| desk.clone())
            .unwrap_or_default()
    }
}

/// The slot the surface seeds. Inherited, so a secondary window's `view` — which
/// runs under a child owner — resolves the same handle the root was given
/// (R1364's finding, and the reason `ProviderSlot::inherited` exists).
pub static DISPLAYS: ProviderSlot<Arc<DisplayHandle>> =
    ProviderSlot::inherited("__pinion.shell.displays", || Arc::new(DisplayHandle::new()));

/// R1576 §5.16 §5.41 §5.23 — binding-facing hook: the monitors attached as of
/// the surface's last stamp.
///
/// Answers [`DisplayTopology::empty`] when no surface seeded a handle — a unit
/// test, a TUI backend, a shell before its first window — which every
/// derivation on a topology is total on, so a caller never has to branch on
/// "was there a shell".
///
/// # Panics
///
/// Panics when called outside an `Owner::run(...)` scope, like every other
/// `use_*` hook — call it from a `view` / `create_extra_externals` / `invoke`
/// body.
#[must_use]
pub fn use_displays() -> DisplayTopology {
    DISPLAYS.resolve_current().get()
}

/// R1576 — the handle itself, for a producer that wants to read the desk later
/// from another thread. Resolve it at wiring time and hold the `Arc`, the
/// [`use_window_control_sink`](crate::use_window_control_sink) discipline.
///
/// # Panics
///
/// Panics when called outside an `Owner::run(...)` scope.
#[must_use]
pub fn use_display_handle() -> Arc<DisplayHandle> {
    Arc::clone(&DISPLAYS.resolve_current())
}

#[cfg(test)]
mod tests {
    use super::{DISPLAYS, DisplayHandle, use_display_handle, use_displays};
    use pinion_core::Owner;
    use pinion_core::display::{DisplayInfo, DisplayRect, DisplayTopology};
    use std::sync::Arc;

    fn desk() -> DisplayTopology {
        DisplayTopology::new(vec![
            DisplayInfo::new("DP-1", DisplayRect::new(0, 0, 1920, 1080)).as_primary(),
        ])
    }

    // The verdict, emitted from the declaration — the `Inherited` check a
    // secondary window's `view` depends on (R1365/R1366.4's shape).
    pinion_core::provider_slot_tests!(
        r1576_display_handle_inherits,
        super::DISPLAYS,
        || -> Arc<DisplayHandle> { Arc::new(DisplayHandle::new()) }
    );

    #[test]
    fn r1576_an_unseeded_scope_reads_a_headless_desk_rather_than_panicking() {
        // A binding under a bare Owner — a unit test, a TUI backend — must get
        // a value every derivation is total on, not a failure.
        let owner = Owner::new();
        let desk = owner.run(use_displays);
        assert!(desk.is_empty());
        assert!(desk.primary().is_none());
        assert!(desk.is_gap_free(), "vacuously, and without a special case");
    }

    #[test]
    fn r1576_a_binding_reads_the_desk_the_surface_stamped() {
        let owner = Owner::new();
        let handle = Arc::new(DisplayHandle::new());
        DISPLAYS.provide(&owner, Arc::clone(&handle));
        assert!(owner.run(use_displays).is_empty(), "before the stamp");
        handle.set(desk());
        let read = owner.run(use_displays);
        assert_eq!(read.len(), 1);
        assert_eq!(
            read.primary().map(|d| d.id().as_str().to_owned()),
            Some("dp-1".to_owned())
        );
    }

    #[test]
    fn r1576_a_held_handle_sees_a_later_stamp() {
        // The off-thread-producer discipline: resolve once at wiring time, read
        // later. A pull that captured a snapshot would silently answer with the
        // desk as it was when the producer was built.
        let owner = Owner::new();
        DISPLAYS.provide(&owner, Arc::new(DisplayHandle::new()));
        let held = owner.run(use_display_handle);
        assert!(held.get().is_empty());
        owner.run(|| use_display_handle().set(desk()));
        assert_eq!(held.get().len(), 1, "the handle is shared, not copied");
    }
}
