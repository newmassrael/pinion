//! R1468 §5.23 §5.22 §5.16 §2 #3 §2 #7 — the containment scope an
//! introspection paint runs inside.
//!
//! # The two halves of one question
//!
//! A binding can express "half the window" two ways, and both are correct:
//! through the layout engine (`height: 50%`, which taffy resolves against
//! whatever root extent it is handed) or through the R1006 reactive seam
//! ([`use_viewport_size`](pinion_core::use_viewport_size), which a producer
//! needs because a reflow is a *side effect* — a PTY's `TIOCSWINSZ` — and so
//! must be readable from inside an [`Effect`](pinion_core::Effect), not only
//! from the pure view).
//!
//! `scene/layout {viewport}` asks a hypothetical: *lay this binding out as if
//! the window were W×H*. Pre-R1468 the two halves answered it differently.
//! taffy got the hypothetical extent, so a percentage child reported the
//! hypothetical size; the seam was never republished, so a
//! `use_viewport_size`-derived child reported the **live** size. One request,
//! two geometries — with, since R1467, the window chrome and its content inset
//! measured on the hypothetical side of that split. An agent asking "how does
//! this look at 400×1200?" got an answer no window will ever show.
//!
//! [`IntrospectionPaint`] closes it by publishing the extent the mirror is
//! actually laying out to, for the duration of that layout, and putting the
//! live value back afterwards.
//!
//! # Why publishing needs a guard
//!
//! Republishing is a `Signal::set`, and a `Signal::set` synchronously re-runs
//! subscribed Effects — the reflow Effects the seam exists for. Firing those
//! at a size the window does not have would push a hypothetical down an ioctl
//! to a real fd. R1006's contract anticipated exactly this and named the
//! remedy: *"Only a future introspection path that did publish at an off-live
//! size would need a [`SimulationGuard`]"*. This is that path, and this is
//! that guard.
//!
//! The guard earns its place a second way. `is_simulating()` is the flag the
//! two request mailboxes ([`focus_request`](pinion_core::focus_request),
//! [`modal_scope_request`](pinion_core::modal_scope_request)) consult before
//! queueing a real focus move, so entering it here also makes a mirror run
//! unable to queue one. That is not hypothetical either: the shell's mirror
//! runs **after** the dispatch's own drain point, so a request the mirror
//! produced used to sit in the mailbox until a later, unrelated RPC drained it.
//!
//! # What it deliberately does not contain
//!
//! `Signal` *values* other than the viewport are still written by a mirrored
//! layout pass — a scroll bound, a measured-row height. Those are layout's own
//! feedback (the [`settle_to_fixed_point`](crate::settle_to_fixed_point)
//! chain), they are recomputed by the next live paint from the live extent, and
//! containing them would mean snapshotting the whole reactive graph per
//! introspection read. R26's `Owner::snapshot` / `restore` is that tool and
//! `simulate` already pays for it; a plain `scene/layout` does not, and the
//! honest boundary is stated here rather than implied.

use pinion_core::reactive::{Owner, Signal, SimulationGuard};

/// R1468 — RAII scope for one introspection (mirror) paint.
///
/// `Drop` restores the viewport the live window published, so the scope is
/// exception-safe and cannot leave the binding believing it is a size it is
/// not — the property that makes publishing the hypothetical safe at all.
///
/// Construct with [`IntrospectionPaint::enter`]; the binding name must not be
/// `_`, which would drop it immediately and restore before the view ever runs.
#[must_use = "IntrospectionPaint must outlive the mirrored view + layout; \
              `let _ = ...` drops immediately (use `let _mirror = ...`)"]
pub struct IntrospectionPaint<'a> {
    /// Suppresses Effect re-runs and both focus mailboxes for the scope.
    /// Nesting is safe — a mirror reached from inside `simulate` sees
    /// `prior = true` and leaves the flag set past its own drop.
    _sim: SimulationGuard,
    /// `Some` when this scope republished the viewport, and then everything
    /// `Drop` needs to undo it.
    restore: Option<ViewportRestore<'a>>,
}

/// R1468 — the write `Drop` owes the live window back.
///
/// A named struct rather than the tuple this began as: the three members are a
/// signal, the owner its write must run inside, and a size, and only the field
/// names say which size it is (`live`, not the hypothetical that was just
/// published — the one confusion that would silently invert the restore).
struct ViewportRestore<'a> {
    /// The scope the `set` must run inside (R1006 blocker B).
    owner: &'a Owner,
    /// The seam this scope republished.
    signal: &'a Signal<(u32, u32)>,
    /// The extent the live window had before this scope touched it.
    live: (u32, u32),
}

impl<'a> IntrospectionPaint<'a> {
    /// Enter the scope for a mirror about to lay out to `viewport`.
    ///
    /// `viewport_signal` is `None` for a window whose size must not reach the
    /// seam. That is not an optimisation: the R1006 signal is seeded once at
    /// the root and the live publish is primary-window-gated, so a secondary
    /// window republishing here would hand the binding a size belonging to a
    /// different window — the very confusion the primary gate exists to
    /// prevent. Such a scope still enters the simulation guard, because the
    /// mailbox containment is not window-scoped.
    ///
    /// The write runs inside `owner`'s scope for R1006 blocker B: a
    /// `Signal::set` re-runs subscribed Effects, and an Effect body resolving
    /// [`Owner::current`](pinion_core::Owner::current) reads the owner-handle
    /// stack, which the subscriber re-run does not push. The guard makes those
    /// re-runs inert, but the wrap stays — correctness must not depend on the
    /// suppression, or the first Effect the guard ever misses panics here.
    pub fn enter(
        owner: &'a Owner,
        viewport_signal: Option<&'a Signal<(u32, u32)>>,
        viewport: (u32, u32),
    ) -> Self {
        // Guard FIRST: the publish below is itself a `Signal::set` whose
        // Effect re-runs must already be suppressed when it lands.
        let sim = SimulationGuard::enter();
        let restore = viewport_signal.and_then(|signal| {
            let live = owner.run(|| signal.get());
            // A mirror at the live extent — the R684 first-paint finalize, the
            // R705 re-store, every `scene/snapshot` without a `viewport`
            // override — publishes the value already there, which `Signal`'s
            // equality-skip makes a no-op. Skipping the restore too keeps that
            // (overwhelmingly common) case free rather than merely cheap.
            if live == viewport {
                return None;
            }
            owner.run(|| signal.set(viewport));
            Some(ViewportRestore {
                owner,
                signal,
                live,
            })
        });
        Self { _sim: sim, restore }
    }
}

impl Drop for IntrospectionPaint<'_> {
    fn drop(&mut self) {
        if let Some(ViewportRestore {
            owner,
            signal,
            live,
        }) = self.restore
        {
            owner.run(|| signal.set(live));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::reactive::is_simulating;
    use std::cell::Cell;
    use std::rc::Rc;

    fn seeded() -> (Owner, Signal<(u32, u32)>) {
        let owner = Owner::new();
        let signal = Signal::new((400_u32, 300_u32));
        (owner, signal)
    }

    #[test]
    fn the_scope_publishes_the_extent_it_will_lay_out_to() {
        let (owner, signal) = seeded();
        let seen = {
            let _mirror = IntrospectionPaint::enter(&owner, Some(&signal), (400, 1200));
            owner.run(|| signal.get())
        };
        assert_eq!(
            seen,
            (400, 1200),
            "a view running inside the scope reads the hypothetical extent",
        );
    }

    #[test]
    fn dropping_the_scope_restores_the_live_extent() {
        let (owner, signal) = seeded();
        {
            let _mirror = IntrospectionPaint::enter(&owner, Some(&signal), (400, 1200));
        }
        assert_eq!(
            owner.run(|| signal.get()),
            (400, 300),
            "the live window's extent is what the binding sees afterwards",
        );
    }

    #[test]
    fn the_scope_suppresses_effects_the_republish_would_fire() {
        let (owner, signal) = seeded();
        let runs = Rc::new(Cell::new(0_u32));
        let (r, s) = (Rc::clone(&runs), signal.clone());
        let _effect = pinion_core::Effect::new(&owner, move || {
            let _ = s.get();
            r.set(r.get() + 1);
        });
        let at_registration = runs.get();
        assert!(
            at_registration >= 1,
            "premise: the Effect subscribed by running once eagerly",
        );
        {
            let _mirror = IntrospectionPaint::enter(&owner, Some(&signal), (400, 1200));
            assert!(is_simulating(), "the scope declares itself a simulation");
        }
        assert_eq!(
            runs.get(),
            at_registration,
            "★neither the publish nor the restore fires the reflow Effect",
        );
        // …and the seam is live again, so the NEXT real resize does fire it.
        owner.run(|| signal.set((400, 900)));
        assert_eq!(
            runs.get(),
            at_registration + 1,
            "★suppression is scoped, not sticky: a real resize still reflows",
        );
    }

    #[test]
    fn a_window_with_no_seam_still_gets_the_containment() {
        let owner = Owner::new();
        {
            let _mirror = IntrospectionPaint::enter(&owner, None, (400, 1200));
            assert!(
                is_simulating(),
                "★a secondary window publishes nothing but is still contained",
            );
        }
        assert!(!is_simulating(), "…and the flag is scoped to the paint");
    }

    #[test]
    fn a_mirror_at_the_live_extent_touches_nothing() {
        let (owner, signal) = seeded();
        let runs = Rc::new(Cell::new(0_u32));
        let (r, s) = (Rc::clone(&runs), signal.clone());
        let _effect = pinion_core::Effect::new(&owner, move || {
            let _ = s.get();
            r.set(r.get() + 1);
        });
        let at_registration = runs.get();
        {
            let _mirror = IntrospectionPaint::enter(&owner, Some(&signal), (400, 300));
        }
        assert_eq!(owner.run(|| signal.get()), (400, 300));
        assert_eq!(
            runs.get(),
            at_registration,
            "the common case (mirror at the live size) writes nothing at all",
        );
    }

    #[test]
    fn the_flag_is_restored_not_cleared_when_nested() {
        let (owner, signal) = seeded();
        let outer = SimulationGuard::enter();
        {
            let _mirror = IntrospectionPaint::enter(&owner, Some(&signal), (400, 1200));
        }
        assert!(
            is_simulating(),
            "★a mirror reached from inside simulate leaves the outer scope intact",
        );
        drop(outer);
        assert!(!is_simulating());
    }
}
