//! Shared External wire-form helper for the index-addressed widget
//! family.
//!
//! Several index-addressed widgets expose the same mechanical
//! `intervene` bounds-check over the §5.15
//! [`External`](crate::external::External) plane, and the decision must
//! stay byte-identical across them (a divergence would be a wire bug,
//! not a style choice — the `decode == inverse(encode)` lift class,
//! R743.1 / R745). It was copied per-widget until a third consumer made
//! the duplication a Rule-of-Three trigger; lifted here so the one
//! decision lives once:
//!
//! - [`resolve_index`] — the `intervene` index bounds-check
//!   (`[0, count)`, every out-of-bounds case → [`InterveneError::OutOfRange`])
//!   used by every index-addressed widget
//!   ([`menu`](super::menu), [`toolbar`](super::toolbar),
//!   [`context_menu`](super::context_menu), [`listbox`](super::listbox),
//!   [`radio_group`](super::radio_group),
//!   [`disclosure_group`](super::disclosure_group)).
//!
//! The `send`-wire pointer-event vocabulary
//! ([`PointerWireEvent`](crate::input::PointerWireEvent)) that those
//! same command-class widgets decode lives in
//! [`crate::input`] — it is shared with the
//! `pinion-runtime` router (the producer) and so belongs with the other
//! cross-crate input-event primitives, not in this widget-only module
//! (R773 §5.35).

use crate::external::InterveneError;

/// Validate a signed `intervene` index against `[0, count)`. Negative,
/// overflowing, and `>= count` all map to [`InterveneError::OutOfRange`].
pub(crate) fn resolve_index(i: i64, count: usize) -> Result<usize, InterveneError> {
    if i < 0 {
        return Err(InterveneError::OutOfRange);
    }
    let idx = usize::try_from(i).map_err(|_| InterveneError::OutOfRange)?;
    if idx >= count {
        return Err(InterveneError::OutOfRange);
    }
    Ok(idx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_index_accepts_in_range() {
        assert_eq!(resolve_index(0, 3), Ok(0));
        assert_eq!(resolve_index(2, 3), Ok(2));
    }

    #[test]
    fn resolve_index_rejects_out_of_range() {
        assert_eq!(resolve_index(-1, 3), Err(InterveneError::OutOfRange));
        assert_eq!(resolve_index(3, 3), Err(InterveneError::OutOfRange));
        assert_eq!(resolve_index(i64::MAX, 3), Err(InterveneError::OutOfRange));
        assert_eq!(resolve_index(0, 0), Err(InterveneError::OutOfRange));
    }
}
