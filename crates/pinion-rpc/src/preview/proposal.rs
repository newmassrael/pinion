//! [`Proposal`] — abstract typed change descriptor stored in the ledger.

use pinion_core::Scene;

/// A typed change the AI agent is proposing against the scene (§5.34).
///
/// The ledger stores `Box<dyn Proposal>` so concrete variants
/// (`SetSignal`, `ReplaceView`, `SetStyle`, `DispatchIntent` — landed
/// progressively from R40.5) can be added without modifying ledger
/// code. Implementations describe themselves through
/// [`target_path`](Self::target_path) (the primary anchor) and
/// [`affected_paths`](Self::affected_paths) (the full diff set —
/// equal to or wider than the target), and provide their own
/// runtime side-effect via [`apply`](Self::apply).
///
/// # Object safety
///
/// This trait is intentionally object-safe: no generic methods, no
/// `Self`-typed returns, no associated types. The ledger always works
/// with `Box<dyn Proposal>` so the proposal type erases cleanly across
/// the RPC boundary, and [`apply`](Self::apply) dispatches through
/// the vtable when called from `crate::preview::apply_preview`.
pub trait Proposal: std::fmt::Debug + Send + Sync + 'static {
    /// Primary scene path this proposal anchors against.
    ///
    /// Used by the ledger for `list_previews` summary output and for
    /// diagnostic messages. Returned as a borrowed slice so impls can
    /// store either an owned `String` or a `&'static str` constant.
    fn target_path(&self) -> &str;

    /// Every scene path whose render output changes when this
    /// proposal applies.
    ///
    /// At minimum this includes [`target_path`](Self::target_path).
    /// Container-level proposals (e.g., subtree replacement) widen
    /// the set to include descendant paths; downstream consumers
    /// (overlay highlighting, dirty-region rendering) iterate this
    /// set rather than the `target_path` alone.
    fn affected_paths(&self) -> Vec<String>;

    /// Effect the proposed change against `scene`.
    ///
    /// Called once by [`crate::preview::apply_preview`] after the
    /// [`PreviewLedger`](super::PreviewLedger) has extracted this
    /// entry (OCC base-revision already matched). Implementations
    /// return `Ok(())` when the side-effect lands, or a short tag
    /// string identifying the rejection class (`"UnsupportedPath"`,
    /// `"TypeMismatch"`, etc.) intended for machine pattern-matching
    /// in the AI agent's branch logic.
    ///
    /// # Errors
    ///
    /// Variant-specific. R40.6 `TypedProposal::SetSignal` routes
    /// through `crate::rewind::rewind`, surfacing its
    /// `RewindError`-derived tags. Future variants document their
    /// own tag sets.
    fn apply(&self, scene: &mut Scene) -> Result<(), String>;
}
