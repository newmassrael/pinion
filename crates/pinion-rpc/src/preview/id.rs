//! [`PreviewId`] — opaque monotonic handle issued by [`PreviewLedger`].
//!
//! [`PreviewLedger`]: crate::preview::PreviewLedger

use std::num::NonZeroU64;

/// Stable opaque handle for an in-flight preview (§5.34).
///
/// Issued monotonically by [`PreviewLedger::propose`] from an internal
/// `AtomicU64` counter starting at `1`. IDs are **never reused**: once
/// a preview is cancelled, expired, or applied, its `PreviewId` is
/// permanently invalid and subsequent lookups return
/// [`ApplyError::UnknownPreview`].
///
/// Wrapping [`NonZeroU64`] keeps `Option<PreviewId>` the same size as
/// `PreviewId` (niche optimization) and prevents an all-zero default
/// value from accidentally matching a real handle.
///
/// [`PreviewLedger::propose`]: crate::preview::PreviewLedger::propose
/// [`ApplyError::UnknownPreview`]: crate::preview::ApplyError::UnknownPreview
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PreviewId(NonZeroU64);

impl PreviewId {
    /// Construct from a raw value. Crate-internal — callers obtain
    /// `PreviewId`s by calling
    /// [`crate::preview::PreviewLedger::propose`], never by minting
    /// their own.
    pub(crate) fn from_raw(raw: NonZeroU64) -> Self {
        Self(raw)
    }

    /// Underlying numeric value, intended for wire serialization
    /// (JSON-RPC payloads, log lines).
    #[must_use]
    pub fn get(self) -> u64 {
        self.0.get()
    }
}

impl std::fmt::Display for PreviewId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.get())
    }
}
