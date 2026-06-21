//! Per-frame read-only context slot for view-fn signature (§6.3).
//!
//! v0 carried a ZST `Frame`; R51.135 added the [`Frame::dt`] field as
//! the spring solver's caller-injected time delta substrate (§5.28
//! carry from R51.133 closed). The struct is `#[non_exhaustive]` so
//! further fields (e.g. `frame_index`, `scale_factor`) can be added
//! without a `SemVer` major bump.
//!
//! ABI note: as of R51.135, `Frame` is no longer a ZST — it carries
//! a single `f32`. The previous "LLVM elides `&Frame` from ABI" guarantee
//! is downgraded to "single-register pass on every supported target",
//! which is the textbook trade for surfacing a real time delta to
//! view-fn callers.

/// Read-only per-frame context passed to view-fn.
///
/// - `dt`: elapsed seconds since the previous frame's view-fn call.
///   Defaults to `0.0` for synthetic / dry-run / test contexts where
///   no real time has passed; production render paths supply the
///   real measured delta via [`Frame::with_dt`].
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub dt: f32,
}

impl Frame {
    /// Construct a frame with `dt = 0.0` — the natural choice for
    /// synthetic invocations (tests, `dry_run`, snapshot-only paint).
    /// Equivalent to [`Frame::default`].
    #[must_use]
    pub const fn new() -> Self {
        Self { dt: 0.0 }
    }

    /// Construct a frame with a specific `dt` (seconds). Real-time
    /// render paths (run loop / event-driven repaint with elapsed
    /// time) use this; synthetic paths use [`Frame::new`].
    #[must_use]
    pub const fn with_dt(dt: f32) -> Self {
        Self { dt }
    }
}

impl Default for Frame {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_has_zero_dt() {
        // Bit-exact zero — f32 equality via to_bits avoids the
        // clippy::float_cmp lint while preserving exact-zero semantics.
        assert_eq!(Frame::new().dt.to_bits(), 0.0_f32.to_bits());
    }

    #[test]
    fn with_dt_preserves_value() {
        let f = Frame::with_dt(1.0 / 60.0);
        assert!((f.dt - 1.0 / 60.0).abs() < 1e-9);
    }

    #[test]
    fn default_matches_new() {
        // PartialEq on Frame derives field-wise; bit-exact through
        // Default's zero-init path.
        assert_eq!(Frame::default().dt.to_bits(), Frame::new().dt.to_bits(),);
    }

    #[test]
    fn const_constructors_are_const() {
        // Both constructors must be usable in const context.
        const SYNTHETIC: Frame = Frame::new();
        const REAL_60FPS: Frame = Frame::with_dt(1.0 / 60.0);
        assert_eq!(SYNTHETIC.dt.to_bits(), 0.0_f32.to_bits());
        assert!((REAL_60FPS.dt - 1.0 / 60.0).abs() < 1e-9);
    }
}
