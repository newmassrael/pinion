//! [`Proposal`] — abstract typed change descriptor stored in the ledger.

/// A typed change the AI agent is proposing against the scene (§5.34).
///
/// The ledger stores `Box<dyn Proposal>` so concrete variants
/// (`SetSignal`, `ReplaceView`, `SetStyle`, `DispatchIntent` — landed
/// in R40.5) can be added without modifying ledger code. Implementations
/// describe themselves through [`target_path`] (the primary anchor)
/// and [`affected_paths`] (the full diff set — equal to or wider than
/// the target).
///
/// # Object safety
///
/// This trait is intentionally object-safe: no generic methods, no
/// `Self`-typed returns, no associated types. The ledger always works
/// with `Box<dyn Proposal>` so the proposal type erases cleanly across
/// the RPC boundary.
///
/// [`target_path`]: Proposal::target_path
/// [`affected_paths`]: Proposal::affected_paths
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
    /// At minimum this includes [`target_path`]. Container-level
    /// proposals (e.g., subtree replacement) widen the set to include
    /// descendant paths; downstream consumers (overlay highlighting,
    /// dirty-region rendering) iterate this set rather than the
    /// `target_path` alone.
    ///
    /// [`target_path`]: Proposal::target_path
    fn affected_paths(&self) -> Vec<String>;
}
