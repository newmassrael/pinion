//! R1006 §5.23 §5.22 — `use_viewport_size`: the view/effect-time "current
//! layout viewport `(width, height)`" reactive read seam.
//!
//! # Why this exists
//!
//! pinion's `view` is a pure function of reactive state and the shell holds
//! the layout viewport `(w, h)` it feeds [`compute_layout`], but it never
//! handed that size to the binding. An external producer whose grid fills the
//! viewport — a PTY terminal whose `(cols, rows) = viewport / cell` (the sprag
//! R24/R25 model) — must observe an OS window resize to re-derive its
//! dimensions and reflow (`TIOCSWINSZ`). That reaction is a **side-effect**
//! (an ioctl on a real fd), so the size must be readable from inside an
//! [`Effect`](super::effect::Effect), not just from the pure view.
//!
//! [`use_viewport_size`] is that edge. It is the value-read sibling of the
//! [`RepaintSink`](super::repaint::RepaintSink) wake edge (R999) and the
//! [`MonospaceMetrics`](super::font_metrics::MonospaceMetrics) measure edge
//! (R1003) — the same §6.3 boundary pattern: pinion-core owns the abstract
//! slot, the backend shell seeds it at boot. Unlike those two, which seed a
//! stable *capability* (a trait object), this seeds a **changing value** — so
//! the carrier is a [`Signal`], read with a tracked `get` so a reflow
//! [`Effect`](super::effect::Effect) re-fires when the viewport changes. It is
//! NOT a [`Frame`](crate::Frame) field: a `Frame` field would trap the size in
//! the pure view with no side-effect home.
//!
//! # Coordinate space — "layout viewport"
//!
//! The published value is the exact `(w, h)` the shell passes to
//! [`compute_layout`] (what the root scene fills), defined by reference to
//! layout's input rather than to a raw device size. pinion's layout is
//! currently scale-factor-naive (physical `inner_size`); when `HiDPI` logical-px
//! layout lands the seam value follows automatically, because the definition
//! tracks "what layout consumed", not "the device pixels".
//!
//! # Contract (R1006 corrections — load-bearing)
//!
//! 1. **The shell MUST `set` the signal inside its `root_owner` scope.** A
//!    `Signal::set` synchronously re-runs subscribed Effects, and an Effect
//!    body that calls [`use_viewport_size`] resolves
//!    [`Owner::current`](super::owner::Owner::current) — which reads the
//!    owner-handle stack, NOT the subscriber stack
//!    [`Effect`](super::effect::Effect) re-run pushes. Setting outside the
//!    `root_owner` scope leaves that stack empty and the first resize panics in
//!    [`use_viewport_size`]'s `expect`. (The `(0, 0)` boot value is set inside
//!    that scope by [`CoreShell`](../../../pinion_runtime/struct.CoreShell.html)
//!    for the same reason.)
//! 2. **`(0, 0)` means "viewport unknown"** — the honest boot value before the
//!    window is `resumed`. A consumer's reflow Effect runs eagerly once at
//!    registration with this value, so it MUST skip on `(0, 0)` (distinct from
//!    a plain unchanged-skip) to avoid a spurious `1 x 1` reflow at boot.
//! 3. **Introspection paint does not publish.** The RPC `scene/snapshot` and
//!    AccessKit producers run their own `view` + layout pass and never call the
//!    shell's publish — only the gated live paint
//!    (`compute_paint_scene_internal`) does. So an introspection paint, even at
//!    an off-live size, cannot fire the reflow side-effect: there is no `set`,
//!    hence nothing to gate. Equality-skip is the *live* path's own secondary
//!    defense (a same-size re-paint does not re-fire), and `is_simulating()`
//!    additionally suppresses `dry_run` / `simulate`. Only a future
//!    introspection path that *did* publish at an off-live size would need a
//!    [`SimulationGuard`](super::simulation::SimulationGuard) — none does today.
//!
//! [`compute_layout`]: ../../../pinion_runtime/fn.compute_layout.html
//! [`Signal`]: super::signal::Signal
//! [`Signal::set`]: super::signal::Signal::set

use super::signal::Signal;

/// Owner-cache newtype: [`Owner::cache`](super::owner::Owner::cache) stores
/// `Rc<dyn Any>`, so the [`Signal`] handle rides inside this holder (mirrors
/// [`MonospaceMetricsHolder`](super::font_metrics::MonospaceMetricsHolder)).
///
/// The wrapper is not actually needed — the cache keys on
/// `(TypeId::of::<V>(), key)`, so `Signal<(u32, u32)>` is already its own type.
/// It dies with this slot's R1366.x migration to
/// [`ProviderSlot`](super::provider_slot::ProviderSlot), as `RepaintSinkHolder`
/// did in R1366.1.
pub(crate) struct ViewportSizeHolder(pub(crate) Signal<(u32, u32)>);

/// Private owner-cache key for the single per-owner viewport-size slot
/// (mirrors the `MonospaceMetrics` private-key convention).
pub(crate) const VIEWPORT_SIZE_KEY: &str = "__pinion.reactive.viewport_size";

/// R1006 §5.23 §5.22 — read the current layout viewport `(width, height)` via
/// the active owner scope's seeded [`Signal`], subscribing the caller.
///
/// Returns `(0, 0)` ("viewport unknown") when no shell seeded a real signal
/// (headless / RPC / unit tests) or before the window is sized. Called inside a
/// reflow [`Effect`](super::effect::Effect) (which captures its consumer handle
/// rather than re-resolving `use_*` hooks); the tracked `get` re-fires that
/// Effect on every size change.
///
/// # Panics
///
/// Panics when called outside an [`Owner::run`](super::owner::Owner::run) scope
/// (same strict shape as [`use_repaint_sink`](super::repaint::use_repaint_sink)).
/// A missing *provider* is graceful (`(0, 0)`); a missing *scope* is a
/// programming error.
///
/// Strict on purpose, unlike the graceful
/// [`measured_monospace_cell`](super::font_metrics::measured_monospace_cell):
/// that one is read in a pure `view` fn (so a bare view-fn unit test must not
/// panic), whereas this is read in a reflow [`Effect`](super::effect::Effect)
/// that always runs in a scope. Strict makes contract (1) above fail loud — a
/// shell that mistakenly `set`s outside its `root_owner` scope panics here
/// rather than silently returning `(0, 0)` and reflowing to `1 x 1`. (A
/// production `view` fn may also read this — views run inside the owner scope —
/// but a standalone view-fn unit test must wrap in `Owner::run`.)
#[must_use]
pub fn use_viewport_size() -> (u32, u32) {
    super::owner::Owner::current()
        .expect("use_viewport_size requires an active Owner scope")
        .viewport_size_signal()
        .get()
}

#[cfg(test)]
mod tests {
    use super::super::effect::Effect;
    use super::super::owner::Owner;
    use super::super::signal::Signal;
    use super::super::simulation::SimulationGuard;
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    #[test]
    fn default_is_zero_unknown() {
        // No provider seeded: the lazy default signal reads (0, 0).
        let owner = Owner::new();
        assert_eq!(owner.viewport_size_signal().get(), (0, 0));
    }

    #[test]
    fn provided_signal_is_the_shared_handle() {
        let owner = Owner::new();
        let sig = Signal::new((640_u32, 480_u32));
        owner.provide_viewport_size_signal(sig.clone());
        assert_eq!(owner.viewport_size_signal().get(), (640, 480));
        // Same underlying cell: a later shell write is observed through the seam.
        sig.set((800, 600));
        assert_eq!(owner.viewport_size_signal().get(), (800, 600));
    }

    #[test]
    fn provide_is_first_write_wins() {
        let owner = Owner::new();
        let first = Signal::new((1_u32, 1_u32));
        let second = Signal::new((2_u32, 2_u32));
        owner.provide_viewport_size_signal(first.clone());
        owner.provide_viewport_size_signal(second.clone());
        // The first stays installed; the second is dropped (cache first-write-wins).
        assert_eq!(owner.viewport_size_signal().get(), (1, 1));
    }

    #[test]
    fn use_viewport_size_resolves_inside_owner_run() {
        let owner = Owner::new();
        let sig = Signal::new((320_u32, 200_u32));
        owner.provide_viewport_size_signal(sig);
        let got = owner.run(use_viewport_size);
        assert_eq!(got, (320, 200));
    }

    #[test]
    fn effect_refires_when_set_inside_owner_scope() {
        // R1006 blocker (B), positive path: set() inside the owner scope lets
        // the synchronous reflow-Effect re-run resolve Owner::current() and
        // observe the new viewport.
        let owner = Owner::new();
        let sig = Signal::new((0_u32, 0_u32));
        owner.provide_viewport_size_signal(sig.clone());
        let seen = Rc::new(RefCell::new(Vec::new()));
        let seen_c = Rc::clone(&seen);
        let _eff = owner.run(|| {
            Effect::new(&Owner::current().expect("inside run"), move || {
                seen_c.borrow_mut().push(use_viewport_size());
            })
        });
        assert_eq!(seen.borrow().as_slice(), &[(0, 0)]); // eager run
        owner.run(|| sig.set((120, 40)));
        assert_eq!(seen.borrow().as_slice(), &[(0, 0), (120, 40)]);
    }

    #[test]
    #[should_panic(expected = "use_viewport_size requires an active Owner scope")]
    fn set_outside_owner_scope_panics_blocker_b() {
        // R1006 blocker (B), the failure it guards: a set() with no active owner
        // scope leaves the handle stack empty, so the synchronous re-run's
        // use_viewport_size() -> Owner::current().expect() panics. This is why
        // the shell's set_viewport_size wraps the set in root_owner.run.
        let owner = Owner::new();
        let sig = Signal::new((0_u32, 0_u32));
        owner.provide_viewport_size_signal(sig.clone());
        let _eff = owner.run(|| {
            Effect::new(&Owner::current().expect("inside run"), || {
                let _ = use_viewport_size();
            })
        });
        sig.set((120, 40)); // no owner.run wrap -> panics during the re-run
    }

    #[test]
    fn set_during_simulation_does_not_refire_effect() {
        // R1006 correction (3): is_simulating() suppresses the reflow side-effect
        // for dry_run / simulate. Subscription stays intact; a post-guard set
        // re-fires normally.
        let owner = Owner::new();
        let sig = Signal::new((0_u32, 0_u32));
        owner.provide_viewport_size_signal(sig.clone());
        let count = Rc::new(Cell::new(0_u32));
        let count_c = Rc::clone(&count);
        let _eff = owner.run(|| {
            Effect::new(&Owner::current().expect("inside run"), move || {
                let _ = use_viewport_size();
                count_c.set(count_c.get() + 1);
            })
        });
        assert_eq!(count.get(), 1); // eager
        {
            let _sim = SimulationGuard::enter();
            owner.run(|| sig.set((80, 24)));
        }
        assert_eq!(count.get(), 1, "effect suppressed under is_simulating()");
        owner.run(|| sig.set((100, 30)));
        assert_eq!(count.get(), 2, "subscription intact post-guard");
    }

    #[test]
    fn same_size_set_is_inert() {
        // Acceptance: a same-size repaint is inert (Signal equality-skip), the
        // mechanism that keeps scene/snapshot paint side-effect-free at the live
        // viewport.
        let owner = Owner::new();
        let sig = Signal::new((10_u32, 20_u32));
        owner.provide_viewport_size_signal(sig.clone());
        let count = Rc::new(Cell::new(0_u32));
        let count_c = Rc::clone(&count);
        let _eff = owner.run(|| {
            Effect::new(&Owner::current().expect("inside run"), move || {
                let _ = use_viewport_size();
                count_c.set(count_c.get() + 1);
            })
        });
        assert_eq!(count.get(), 1);
        owner.run(|| sig.set((10, 20))); // same value -> no notify
        assert_eq!(count.get(), 1, "equality-skip: no re-fire");
        owner.run(|| sig.set((10, 21))); // changed
        assert_eq!(count.get(), 2);
    }
}
