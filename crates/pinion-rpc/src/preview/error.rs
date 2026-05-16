//! Error types for [`crate::preview::PreviewLedger`].

/// Reasons [`crate::preview::PreviewLedger::propose`] can fail.
///
/// Per §5.34 conflict policy Q2=C (independent ledger), proposing
/// against a target that already has another active preview is **not**
/// an error — multiple previews coexist and conflict is checked at
/// apply time. The only propose-time rejection is the bounded-memory
/// capacity check.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProposeError {
    /// The ledger is already at `capacity` active entries. Caller
    /// should cancel a previous preview, wait for an existing one to
    /// expire, or call
    /// [`crate::preview::PreviewLedger::sweep_expired`] to reclaim
    /// deadline-passed slots.
    CapacityFull {
        /// Configured capacity (entries already at this count).
        capacity: usize,
    },
}

impl std::fmt::Display for ProposeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CapacityFull { capacity } => {
                write!(f, "preview ledger full (capacity={capacity})")
            }
        }
    }
}

impl std::error::Error for ProposeError {}

/// Reasons [`crate::preview::PreviewLedger::apply_extract`] (and the
/// higher-level [`crate::preview::apply_preview`]) can fail.
///
/// Lifecycle invariants per §5.34: `cancel` is idempotent and total
/// (no error surface), `list` is read-only and total (no error
/// surface); only `apply` can fail, with the categories enumerated
/// here.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyError {
    /// The id was never issued, has already been applied, or has
    /// already been cancelled / swept.
    UnknownPreview,
    /// The handle was valid at propose time but its server-clamped
    /// deadline has now passed. The entry is removed from the
    /// ledger as a side-effect of this call; the caller must
    /// propose a fresh preview.
    Expired,
    /// Optimistic-concurrency conflict: the scene revision captured
    /// at propose time differs from the `current_scene_revision`
    /// the caller supplied to `apply_extract`. The preview is **not**
    /// removed — the caller may still inspect it (via `list_previews`)
    /// or cancel it explicitly.
    BaseRevisionConflict {
        /// The base-revision the entry was proposed against.
        expected: u64,
        /// The live scene revision the caller passed in.
        actual: u64,
    },
    /// The proposal was successfully extracted from the ledger but
    /// the runtime side-effect (signal write, intent dispatch, etc.)
    /// failed. The entry has already been **removed** — applying a
    /// proposal is a one-shot consume, so a recoverable retry needs
    /// a fresh `propose_change`.
    ///
    /// The payload string carries a short reason tag intended for
    /// machine pattern-matching (e.g. `"UnsupportedPath"`,
    /// `"TypeMismatch"`, `"UnknownPath"`); the caller's AI agent
    /// switches on it without parsing prose.
    ApplyRejected(String),
}

impl std::fmt::Display for ApplyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownPreview => f.write_str("preview id not present in ledger"),
            Self::Expired => f.write_str("preview deadline passed before apply"),
            Self::BaseRevisionConflict { expected, actual } => write!(
                f,
                "preview base_revision={expected} but current scene revision={actual}"
            ),
            Self::ApplyRejected(reason) => {
                write!(f, "apply rejected at runtime: {reason}")
            }
        }
    }
}

impl std::error::Error for ApplyError {}
