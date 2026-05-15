//! Per-frame read-only context slot for view-fn signature (§6.3, R14 forward-compat hedge).

/// Read-only per-frame context passed to view-fn.
///
/// Currently a ZST; LLVM elides `&Frame` from ABI per §6.3 zero-cost guarantee.
/// Future fields (e.g. `dt`, `frame_index`) addable without `SemVer` major bump via `#[non_exhaustive]`.
#[non_exhaustive]
#[derive(Debug, Clone, Copy)]
pub struct Frame;

impl Frame {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Default for Frame {
    fn default() -> Self {
        Self::new()
    }
}

const _: () = assert!(core::mem::size_of::<Frame>() == 0);
