//! Shared External wire-form helpers for the command / selection widget
//! family.
//!
//! Several widgets expose the same two mechanical wire seams over the
//! §5.15 [`External`](crate::external::External) plane, and the decoders
//! must stay byte-identical across them (a divergence would be a wire
//! bug, not a style choice — the `decode == inverse(encode)` lift class,
//! R743.1 / R745). They were copied per-widget until a third consumer
//! made the duplication a Rule-of-Three trigger; lifted here so the one
//! decision lives once:
//!
//! - [`PointerWireEvent`] + [`parse_pointer_wire_event`] — the `send`-wire pointer
//!   event subset (`PointerEnter` / `Down` / `Up` / `Leave` / `Cancel`)
//!   that the composite-tag router feeds command-class widgets
//!   ([`menu`](super::menu), [`toolbar`](super::toolbar),
//!   [`context_menu`](super::context_menu)). Each widget still owns *how*
//!   it reacts to each variant; only the name→variant decode is shared.
//! - [`resolve_index`] — the `intervene` index bounds-check
//!   (`[0, count)`, every out-of-bounds case → [`InterveneError::OutOfRange`])
//!   used by every index-addressed widget
//!   ([`menu`](super::menu), [`toolbar`](super::toolbar),
//!   [`context_menu`](super::context_menu), [`listbox`](super::listbox),
//!   [`radio_group`](super::radio_group),
//!   [`disclosure_group`](super::disclosure_group)).

use crate::external::InterveneError;

/// Pointer events a command-class widget item accepts over the `send`
/// wire. The composite-tag router rewrites a paint hit-target into a
/// `"<sub>:<EventName>"` payload, whose `<EventName>` half this decodes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PointerWireEvent {
    Enter,
    Down,
    Up,
    Leave,
    Cancel,
}

/// Decode a W3C pointer-event name into a [`PointerWireEvent`]; `None` for an
/// unknown name (the caller rejects the `send` payload).
pub(crate) fn parse_pointer_wire_event(name: &str) -> Option<PointerWireEvent> {
    match name {
        "PointerEnter" => Some(PointerWireEvent::Enter),
        "PointerDown" => Some(PointerWireEvent::Down),
        "PointerUp" => Some(PointerWireEvent::Up),
        "PointerLeave" => Some(PointerWireEvent::Leave),
        "PointerCancel" => Some(PointerWireEvent::Cancel),
        _ => None,
    }
}

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
    fn parse_pointer_event_round_trips_known_names() {
        assert_eq!(parse_pointer_wire_event("PointerEnter"), Some(PointerWireEvent::Enter));
        assert_eq!(parse_pointer_wire_event("PointerDown"), Some(PointerWireEvent::Down));
        assert_eq!(parse_pointer_wire_event("PointerUp"), Some(PointerWireEvent::Up));
        assert_eq!(parse_pointer_wire_event("PointerLeave"), Some(PointerWireEvent::Leave));
        assert_eq!(parse_pointer_wire_event("PointerCancel"), Some(PointerWireEvent::Cancel));
        assert_eq!(parse_pointer_wire_event("PointerWheel"), None);
        assert_eq!(parse_pointer_wire_event(""), None);
    }

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
