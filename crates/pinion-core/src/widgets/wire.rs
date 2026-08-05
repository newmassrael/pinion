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
//!   (`[0, count)`, every out-of-bounds case → [`InterveneError::OutOfRange`],
//!   R1565 stating the range it was checked against)
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
///
/// R1565 §5.15 (PINION-PR82) — the refusal now **states the range**, and this
/// is the one place that composes it for eight surfaces. `what` names the thing
/// being indexed in the surface's own vocabulary (`"item"`, `"menu"`,
/// `"button"`), because a refusal read out of context has to say what the
/// number was an index INTO; the bound it is checked against is already here,
/// which is why the sentence belongs here and not at each call site.
///
/// The two failures are told apart on purpose. An index past the end names the
/// extent, so the caller can pick a valid one; a **negative** index cannot be
/// an index at all, and saying "0..3" about `-1` invites the reading that some
/// other negative would have worked.
pub(crate) fn resolve_index(what: &str, i: i64, count: usize) -> Result<usize, InterveneError> {
    let Ok(idx) = usize::try_from(i) else {
        return Err(InterveneError::out_of_range(format!(
            "{i} is not a {what} index"
        )));
    };
    if idx >= count {
        return Err(InterveneError::out_of_range(if count == 0 {
            format!("no {what} {idx}: this surface has none")
        } else {
            format!("no {what} {idx} here (it has {count}, so 0..{count})")
        }));
    }
    Ok(idx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_index_accepts_in_range() {
        assert_eq!(resolve_index("item", 0, 3), Ok(0));
        assert_eq!(resolve_index("item", 2, 3), Ok(2));
    }

    /// R1565 — the stated reason, or a panic naming what it said instead.
    fn why(r: Result<usize, InterveneError>) -> String {
        r.expect_err("expected a refusal")
            .reason()
            .expect("out-of-range states its range")
            .as_str()
            .to_owned()
    }

    #[test]
    fn resolve_index_rejects_out_of_range() {
        // R1565 — the four used to be one value. Each now says which it is,
        // and the two CLASSES are deliberately different sentences: a bound is
        // actionable, "not an index" is not.
        assert_eq!(why(resolve_index("item", -1, 3)), "-1 is not a item index");
        assert_eq!(
            why(resolve_index("item", 3, 3)),
            "no item 3 here (it has 3, so 0..3)"
        );
        assert_eq!(
            why(resolve_index("item", i64::MAX, 3)),
            format!("no item {} here (it has 3, so 0..3)", i64::MAX),
        );
        assert_eq!(
            why(resolve_index("item", 0, 0)),
            "no item 0: this surface has none",
            "an empty surface must not advertise the range 0..0, which admits nothing",
        );
    }
}
