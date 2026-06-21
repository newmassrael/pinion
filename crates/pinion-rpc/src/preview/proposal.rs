//! [`Proposal`] — abstract typed change descriptor stored in the ledger.
//!
//! Companion type [`ApplyContext`] is the per-apply runtime bundle
//! every variant's [`Proposal::apply`] receives — introduced R40.9
//! when the second-class variant (`DispatchIntent`) needed a side-
//! effect target beyond the scene tree. The bundle pattern mirrors
//! the R40.7 [`crate::DispatchContext`] decision: take one struct,
//! grow fields non-breakingly per `#[non_exhaustive]`.

use pinion_core::Scene;
use pinion_core::intent::Intent;

/// Per-apply runtime bundle handed to [`Proposal::apply`] (§5.34 R40.9).
///
/// `scene` is the mutable scene the variant mutates (signal write,
/// style edit, view replace). `emitted_intents` is an accumulator
/// the [`TypedProposal::DispatchIntent`](super::TypedProposal) variant
/// pushes into — read back by [`apply_preview`](super::apply_preview)
/// and surfaced in [`ApplyOutcome::emitted_intents`](super::ApplyOutcome).
///
/// `#[non_exhaustive]` so future variants (animation registry, effect
/// ledger, sound system) gain their own borrowed handle here without
/// touching the [`Proposal`] trait signature. Construct only via
/// [`ApplyContext::new`].
#[non_exhaustive]
#[derive(Debug)]
pub struct ApplyContext<'a> {
    /// Live scene the variant mutates. Borrowed mutably for the
    /// duration of a single apply call.
    pub scene: &'a mut Scene,
    /// Intents the variant emits during apply. Accumulator-style:
    /// variants `push` here; the caller drains it into the apply
    /// outcome. Non-empty only when a variant explicitly emits
    /// (currently `DispatchIntent`).
    pub emitted_intents: Vec<Intent>,
}

impl<'a> ApplyContext<'a> {
    /// Construct a fresh context wrapping `scene` with an empty
    /// intent accumulator. Used by [`apply_preview`](super::apply_preview)
    /// once per apply call.
    #[must_use]
    pub fn new(scene: &'a mut Scene) -> Self {
        Self {
            scene,
            emitted_intents: Vec::new(),
        }
    }
}

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

    /// Effect the proposed change against `ctx`.
    ///
    /// Called once by [`crate::preview::apply_preview`] after the
    /// [`PreviewLedger`](super::PreviewLedger) has extracted this
    /// entry (OCC base-revision already matched). Variants mutate
    /// `ctx.scene` (`SetSignal` / `SetStyle` / `ReplaceView`) or push into
    /// `ctx.emitted_intents` (`DispatchIntent`) — or both, for hybrid
    /// variants future R40.x sub-slices may introduce.
    ///
    /// # Errors
    ///
    /// Variant-specific. R40.6 `TypedProposal::SetSignal` routes
    /// through `crate::rewind::rewind`, surfacing its
    /// `RewindError`-derived tags. Future variants document their
    /// own tag sets.
    fn apply(&self, ctx: &mut ApplyContext<'_>) -> Result<(), String>;
}
