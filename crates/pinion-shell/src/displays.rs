//! R1576 §5.16 §5.41 §5.23 — the binding-facing "what monitors am I on?" hook.
//!
//! # Why this exists
//!
//! [`crate::WindowSpec::display`] lets a binding say *which monitor* a window's position is measured
//! from, and that declaration needs an **id**. An agent driving the wire reads
//! one from `scene/displays`; a binding driving its own UI — a "move panel to…" menu, a
//! layout preset picker, a tear-off that opens on the monitor under the cursor
//! — had nowhere to read one at all, which would leave in-process code below
//! the toolkit (`screens()` is ordinary application API there) while the wire was past
//! it.
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

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use pinion_core::ProviderSlot;
use pinion_core::display::{DisplayHome, DisplayId, DisplayRect, DisplayTopology};

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
    /// R1617 — per live window, its actual outer rectangle and the display the
    /// window system itself names for it. Stamped by the same surface pass that
    /// stamps [`Self::desk`], so the two are one reading and a home derived
    /// here cannot be a rectangle measured against yesterday's desk.
    homes: RwLock<HashMap<String, (DisplayRect, Option<DisplayId>)>>,
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

    /// R1617 — stamp where each live window actually is, and what the window
    /// system says about it. The surface's job, alongside [`Self::set`].
    ///
    /// Replaced wholesale rather than merged: a window the surface no longer
    /// reports is one nobody can vouch for, and a stale entry would answer with
    /// yesterday's monitor. Poisoning is ignored for [`Self::set`]'s reason.
    pub fn set_homes(&self, homes: Vec<(String, DisplayRect, Option<DisplayId>)>) {
        if let Ok(mut slot) = self.homes.write() {
            *slot = homes
                .into_iter()
                .map(|(id, rect, platform)| (id, (rect, platform)))
                .collect();
        }
    }

    /// R1617 — which display `window_id` is on, per both answerers, against the
    /// desk of the same stamp. `None` when the surface reported nothing for
    /// that window: no stamp yet, an unknown id, or a window whose outer
    /// position the platform declined to give.
    ///
    /// Derived on read rather than stored, so it cannot become a second stale
    /// copy of where the window is — the one-fact-one-source rule the whole
    /// declared-placement design rests on.
    #[must_use]
    pub fn home_of(&self, window_id: &str) -> Option<DisplayHome> {
        let desk = self.desk.read().ok()?;
        let homes = self.homes.read().ok()?;
        let (rect, platform) = homes.get(window_id)?;
        Some(desk.home_of(*rect, platform.clone()))
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

/// R1617 §5.16 §5.41 §2 #7 — binding-facing hook: which display `window_id` is
/// on right now, according to **both** this framework's derivation from the
/// window's live rectangle and the window system's own opinion.
///
/// The in-process peer of `scene/windows`' `display_home`, and it exists for
/// the reason [`use_displays`] does: leaving a binding below what a mainstream
/// toolkit already gives in-process — a window's screen accessor is ordinary
/// application API there — while the wire is past it would be the wrong
/// asymmetry. A tear-off deciding which monitor to open the next panel on is a
/// binding-side question.
///
/// It answers *more* than that toolkit's does, and the difference is the point:
/// there the application gets the platform plugin's stored answer and cannot
/// reach the geometric derivation at all, because that resolver is private.
/// Here both come back, and so does the relation between them — see
/// [`DisplayHome`].
///
/// `None` when nobody looked: no surface stamped one (a unit test, a TUI
/// backend, a shell before its first window), an unknown window id, or a window
/// whose outer position the platform declined to report.
///
/// # Panics
///
/// Panics when called outside an `Owner::run(...)` scope, like every other
/// `use_*` hook.
#[must_use]
pub fn use_window_home(window_id: &str) -> Option<DisplayHome> {
    DISPLAYS.resolve_current().home_of(window_id)
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
    use pinion_core::display::{DisplayId, DisplayInfo, DisplayRect, DisplayTopology};
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
    fn r1617_an_unseeded_scope_has_no_home_for_any_window() {
        use super::use_window_home;
        // Nobody looked, so nothing is claimed — and asking about a window that
        // does not exist is the same answer rather than a panic.
        let owner = Owner::new();
        assert!(owner.run(|| use_window_home("main")).is_none());
        assert!(owner.run(|| use_window_home("no-such-window")).is_none());
    }

    #[test]
    fn r1617_a_binding_reads_the_home_against_the_desk_of_the_same_stamp() {
        use super::use_window_home;
        use pinion_core::display::DisplayRect;

        let owner = Owner::new();
        let handle = Arc::new(DisplayHandle::new());
        DISPLAYS.provide(&owner, Arc::clone(&handle));
        handle.set(DisplayTopology::new(vec![
            DisplayInfo::new("DP-1", DisplayRect::new(0, 0, 1920, 1080)).as_primary(),
            DisplayInfo::new("DP-2", DisplayRect::new(1920, 0, 1920, 1080)),
        ]));
        // A desk with no window stamp still answers nothing: the home needs
        // BOTH halves, and half an answer would be an invented one.
        assert!(owner.run(|| use_window_home("panel")).is_none());

        handle.set_homes(vec![
            (
                "panel".to_owned(),
                DisplayRect::new(2000, 40, 400, 300),
                Some(DisplayId::new("dp-2")),
            ),
            // Straddling, larger share on the right, platform names the left.
            (
                "main".to_owned(),
                DisplayRect::new(1820, 0, 400, 100),
                Some(DisplayId::new("dp-1")),
            ),
        ]);
        let panel = owner.run(|| use_window_home("panel")).expect("stamped");
        assert!(panel.agrees());
        assert_eq!(panel.name(), "agreed");
        assert_eq!(panel.derived().map(DisplayId::as_str), Some("dp-2"));

        let main = owner.run(|| use_window_home("main")).expect("stamped");
        assert_eq!(main.name(), "diverged");
        assert_eq!(main.derived().map(DisplayId::as_str), Some("dp-2"));
        assert_eq!(main.platform().map(DisplayId::as_str), Some("dp-1"));

        // A later stamp is what the binding sees: the map is replaced, so a
        // window the surface stopped reporting stops having a home rather than
        // keeping yesterday's.
        handle.set_homes(vec![(
            "panel".to_owned(),
            DisplayRect::new(10, 10, 400, 300),
            Some(DisplayId::new("dp-1")),
        )]);
        assert_eq!(
            owner
                .run(|| use_window_home("panel"))
                .expect("stamped")
                .derived()
                .map(DisplayId::as_str),
            Some("dp-1"),
        );
        assert!(
            owner.run(|| use_window_home("main")).is_none(),
            "a window the surface no longer reports is one nobody can vouch for",
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
