//! R649 §5.23 R27 — Effect side-effect suppression during `dry_run` /
//! `simulate` execution.
//!
//! Spec R27 mandates: "`dry_run` skips Effect side-effect; subscription
//! still tracked for memo invalidation." R641-R648 `dry_run` / `simulate`
//! satisfied the state-rollback half (R26) via [`Owner::snapshot`] +
//! [`Owner::restore`] but left Effect closures firing during the
//! mutation + rollback cycle. Non-idempotent Effects
//! (`counter.set(counter.get() + 1)`, network calls, telemetry
//! sends) saw two transient state values they did not need to —
//! one on the mutation, one on the rollback — and side effects
//! landed in both directions.
//!
//! R649 closes that gap with a thread-local guard:
//!
//! - [`SimulationGuard::enter`] flips an `IS_SIMULATING` cell to
//!   `true` and returns an RAII guard that restores the prior value
//!   on `Drop`. Nesting is safe — inner guards see `prior = true`
//!   and leave the cell `true` past their own scope.
//! - [`is_simulating`] is checked by [`crate::Effect`]'s `mark_dirty`
//!   path: when `true`, the Effect's `rerun` is skipped. The
//!   subscription set (the Signal observers list) is unaffected —
//!   the next post-simulation `Signal::set` fires the Effect
//!   normally.
//!
//! ## What R649 does NOT change
//!
//! - **Signal values** are still mutated by `External::intervene` →
//!   `Signal::set` paths. R26 (Owner snapshot/restore) keeps that
//!   reactive state correct.
//! - **Computed cache** still pollutes (R30 commitment open). Pin
//!   as R650 candidate.
//! - **`SCE` engine-level `dry_run` hook** (§5.8 ratified) still
//!   long-term roadmap; R649 + R26 are sufficient for v0.

use std::cell::Cell;

thread_local! {
    /// `true` while a [`SimulationGuard`] is active on this thread.
    /// Effect dispatch checks this flag and skips re-runs while set.
    static IS_SIMULATING: Cell<bool> = const { Cell::new(false) };
}

/// Returns `true` if the current thread is inside an active
/// [`SimulationGuard`] scope (a [`crate::Effect`]'s `mark_dirty`
/// short-circuits when this returns `true`).
#[must_use]
pub fn is_simulating() -> bool {
    IS_SIMULATING.with(Cell::get)
}

/// RAII guard that flips the thread-local simulation flag on
/// construction and restores it on `Drop`. The "restore prior, not
/// `false`" semantics make nesting safe — an inner guard saved a
/// `prior = true` from the outer guard and the outer scope stays
/// in simulation mode after the inner drops.
///
/// Construct via [`SimulationGuard::enter`]; binding name must NOT
/// be `_` (Rust would drop immediately).
#[must_use = "SimulationGuard must outlive the simulation scope; \
              `let _ = ...` drops immediately (use `let _guard = ...`)"]
pub struct SimulationGuard {
    prior: bool,
}

impl SimulationGuard {
    /// Enter simulation mode: set `IS_SIMULATING = true`, return a
    /// guard whose `Drop` restores the prior value.
    pub fn enter() -> Self {
        let prior = IS_SIMULATING.with(|c| c.replace(true));
        Self { prior }
    }
}

impl Drop for SimulationGuard {
    fn drop(&mut self) {
        IS_SIMULATING.with(|c| c.set(self.prior));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baseline_is_false() {
        // Each thread starts outside simulation mode.
        assert!(!is_simulating());
    }

    #[test]
    fn guard_flips_flag_and_restores_on_drop() {
        assert!(!is_simulating());
        {
            let _guard = SimulationGuard::enter();
            assert!(is_simulating());
        }
        assert!(!is_simulating());
    }

    #[test]
    fn nested_guards_keep_flag_set_until_outermost_drops() {
        assert!(!is_simulating());
        let outer = SimulationGuard::enter();
        assert!(is_simulating());
        {
            let _inner = SimulationGuard::enter();
            assert!(is_simulating());
        }
        // After inner drop, outer still active — prior was `true`.
        assert!(is_simulating());
        drop(outer);
        assert!(!is_simulating());
    }
}
