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
//! 3. **An introspection paint publishes only inside a containment scope**
//!    (R1468 — this clause replaces R1006's "introspection does not publish").
//!    Not publishing turned out to be the wrong half of the promise to keep:
//!    `scene/layout {viewport}` hands taffy a hypothetical extent, so a
//!    percentage child answered in the hypothetical while a
//!    [`use_viewport_size`] child answered in the live one — one request, two
//!    geometries, and since R1467 the window chrome and its content inset sat
//!    on the hypothetical side of the split. The mirror therefore DOES publish
//!    now, and it does so from inside
//!    [`IntrospectionPaint`](../../../pinion_runtime/struct.IntrospectionPaint.html):
//!    the extent it is laying out to, restored on drop, with the whole run
//!    wrapped in the [`SimulationGuard`](super::simulation::SimulationGuard)
//!    this clause always said such a path would need. So the reflow side-effect
//!    still cannot fire at a size no window has — the guarantee is unchanged;
//!    what changed is that it is now enforced by a scope instead of by the
//!    absence of a write. The live paint keeps its own secondary defense
//!    (equality-skip on a same-size re-paint), and the restore is what keeps
//!    that defense working: a mirror that left the hypothetical behind would
//!    turn the next live publish into a *change* and reflow one frame later,
//!    outside any guard.
//!
//! [`compute_layout`]: ../../../pinion_runtime/fn.compute_layout.html
//! [`Signal`]: super::signal::Signal
//! [`Signal::set`]: super::signal::Signal::set

use super::provider_slot::ProviderSlot;
use super::signal::Signal;

/// R1366.7 §5.23 §5.22 — the viewport-size slot: its key, its default and its
/// **`per_scope`** verdict as one expression — the FIRST slot whose verdict is
/// not `Inherited`.
///
/// `per_scope`, NOT `inherited`, and this is the verdict a reader doubts: the
/// shell DRIVES this at root, but its WRITE (`set_viewport_size`) is
/// primary-gated (`if window_key == DEFAULT_WINDOW`), so the root's value is *the
/// primary window's* size. Inheriting it would hand a secondary window the
/// primary's size — confidently wrong — where a per-scope `(0, 0)` is R1006's
/// honest "viewport unknown" (its contract already requires consumers to skip on
/// it). [`provider_slot_tests!`](crate::provider_slot_tests) EMITS the verdict,
/// and for `per_scope` it asserts a child scope does NOT resolve the root's.
///
/// The payload is the `Signal<(u32, u32)>` itself, no newtype wrapper:
/// [`Owner::cache`](super::owner::Owner::cache) keys on `(TypeId, key)`, so the
/// signal is already its own type — R1006's `ViewportSizeHolder` was the
/// `Rc<dyn Any>` storage showing through. A late seed now PANICS
/// ([`ProviderSlot::provide`](super::provider_slot::ProviderSlot::provide)); the
/// shell seeds once at boot before the first read.
pub static VIEWPORT_SIZE: ProviderSlot<Signal<(u32, u32)>> =
    ProviderSlot::per_scope("__pinion.reactive.viewport_size", || {
        Signal::new((0_u32, 0_u32))
    });

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
    let owner =
        super::owner::Owner::current().expect("use_viewport_size requires an active Owner scope");
    VIEWPORT_SIZE.resolve(&owner).get()
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

    // The verdict, EMITTED from the declaration. For `per_scope` this asserts a
    // child scope does NOT resolve the root's — the shell's viewport write is
    // primary-gated, so inheriting would hand a secondary window the primary's
    // size instead of an honest (0, 0) "unknown".
    crate::provider_slot_tests!(
        r1366_7_viewport_size_is_per_scope,
        super::VIEWPORT_SIZE,
        || Signal::new((0_u32, 0_u32))
    );

    #[test]
    fn r1366_8_1_a_child_scope_does_not_inherit_the_primary_viewport() {
        // Value-based discrimination the generated macro above cannot do — its
        // `ptr_eq` derives from `scope()`, so it passes under either verdict
        // (R1366.8.1 audit). A secondary window (child scope) must read its OWN
        // (0, 0) "unknown", NOT the primary's seeded size, because the shell's
        // viewport write is primary-gated. If `VIEWPORT_SIZE` were wrongly
        // `inherited`, the child would resolve the root's signal and read
        // (1920, 1080) — this asserts the semantic, so it FAILS on a wrong verdict.
        let root = Owner::new();
        VIEWPORT_SIZE.provide(&root, Signal::new((1920_u32, 1080_u32)));
        let child = Owner::new_child(&root);
        assert_eq!(
            child.run(use_viewport_size),
            (0, 0),
            "a child scope inherited the primary's viewport — the verdict must be per_scope",
        );
    }

    #[test]
    fn default_is_zero_unknown() {
        // No provider seeded: the lazy default signal reads (0, 0).
        let owner = Owner::new();
        assert_eq!(VIEWPORT_SIZE.resolve(&owner).get(), (0, 0));
    }

    #[test]
    fn provided_signal_is_the_shared_handle() {
        let owner = Owner::new();
        let sig = Signal::new((640_u32, 480_u32));
        VIEWPORT_SIZE.provide(&owner, sig.clone());
        assert_eq!(VIEWPORT_SIZE.resolve(&owner).get(), (640, 480));
        // Same underlying cell: a later shell write is observed through the seam.
        sig.set((800, 600));
        assert_eq!(VIEWPORT_SIZE.resolve(&owner).get(), (800, 600));
    }

    #[test]
    #[should_panic(expected = "already seeded")]
    fn r1366_7_a_late_seed_panics_where_it_used_to_be_dropped() {
        // The counterfactual of R1006's `provide_is_first_write_wins`, which
        // asserted a second seed was a silent no-op leaving readers on the first
        // signal. A dropped viewport seed is a window whose reflow reads a signal
        // the shell never writes — stuck at (0, 0); the shell seeds once, at boot.
        let owner = Owner::new();
        VIEWPORT_SIZE.provide(&owner, Signal::new((1_u32, 1_u32)));
        VIEWPORT_SIZE.provide(&owner, Signal::new((2_u32, 2_u32)));
    }

    #[test]
    fn use_viewport_size_resolves_inside_owner_run() {
        let owner = Owner::new();
        let sig = Signal::new((320_u32, 200_u32));
        VIEWPORT_SIZE.provide(&owner, sig);
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
        VIEWPORT_SIZE.provide(&owner, sig.clone());
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
        VIEWPORT_SIZE.provide(&owner, sig.clone());
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
        VIEWPORT_SIZE.provide(&owner, sig.clone());
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
        VIEWPORT_SIZE.provide(&owner, sig.clone());
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
