//! R659 §5.16 §5.35 — composite-tag wire helpers shared by application
//! [`External`](crate::external::External) handlers that respond to
//! R51.42 §5.35 `<key>:<EventName>` send payloads.
//!
//! ## Why a substrate
//!
//! The R51.42 [`InputRouter`] composite-tag dispatch splits a paint
//! tag like `"todo_delete#42"` at the `#` separator, then forwards the
//! sub-index `42` to the resolved [`External`](crate::external::External)
//! as part of a wire-form payload `"42:PointerDown"`. The receiving
//! `invoke("send", Text(...))` arm has to parse the payload back into
//! a typed key (`u64` id, `usize` enum discriminant, …) plus the
//! event name suffix.
//!
//! R655 [`TodoDeleteExternal`] introduced the parse helper inline as a
//! 5-LOC private function. R658 [`TodoToggleExternal`] copy-pasted it.
//! R659 [`TodoFilterExternal`] is the **3rd consumer** of the same
//! shape — the Rule of Three from
//! [[abstraction-needs-second-consumer]] fires, so the helper lifts
//! into this module before the parsing logic forks across siblings.
//!
//! ## Why generic over [`FromStr`]
//!
//! The three consumers parse different key types:
//!
//! | Consumer             | Key type | Domain meaning                  |
//! |---------------------|----------|---------------------------------|
//! | `TodoDeleteExternal` | `u64`    | Stable monotonic todo id        |
//! | `TodoToggleExternal` | `u64`    | Stable monotonic todo id        |
//! | `TodoFilterExternal` | `usize`  | `FilterMode` discriminant 0..2  |
//!
//! Generic over [`core::str::FromStr`] keeps the substrate type-safe
//! at the call site — each consumer receives a parsed `K` they declared
//! up-front, with `K::Err` collapsed into `None` for the caller's
//! `.ok_or(InvokeError::Rejected)?` flow.
//!
//! Framework-side composites ([`crate::widgets::radio_group::RadioGroupExternal`],
//! [`crate::widgets::listbox::ListBoxExternal`]) parse the same shape
//! inline via `s.split_once(':')` today. They could become future
//! 4th/5th consumers of this helper in a follow-up audit — left as a
//! deliberate carry rather than expanding the R659 scope.
//!
//! [`InputRouter`]: pinion_runtime::InputRouter

use core::str::FromStr;

/// Parse a R51.42 §5.35 composite-tag send payload `"<key>:<EventName>"`
/// into `(key, event_name)`.
///
/// Returns `None` when the wire-form is malformed:
///
/// * missing `:` separator,
/// * empty event name (e.g. `"7:"`),
/// * non-parseable `<key>` for the requested `K` (e.g. `"xx:PointerDown"`
///   against `K = u64`).
///
/// On the happy path returns the typed key + a borrowed event-name
/// slice into `payload` (callers typically match the event-name
/// against `"PointerDown"` / `"PointerUp"` / `"PointerEnter"` /
/// `"PointerLeave"` / `"PointerCancel"`).
///
/// The dispatcher contract is documented at the [module level](self).
#[must_use]
pub fn parse_send_payload<K: FromStr>(payload: &str) -> Option<(K, &str)> {
    let (key_str, event_name) = payload.split_once(':')?;
    if event_name.is_empty() {
        return None;
    }
    let key = key_str.parse().ok()?;
    Some((key, event_name))
}

#[cfg(test)]
mod tests {
    use super::parse_send_payload;

    #[test]
    fn r659_parse_u64_pointer_down_happy_path() {
        let parsed: Option<(u64, &str)> = parse_send_payload("42:PointerDown");
        assert_eq!(parsed, Some((42_u64, "PointerDown")));
    }

    #[test]
    fn r659_parse_usize_arbitrary_event_name() {
        let parsed: Option<(usize, &str)> = parse_send_payload("0:PointerEnter");
        assert_eq!(parsed, Some((0_usize, "PointerEnter")));
    }

    #[test]
    fn r659_missing_colon_separator_returns_none() {
        let parsed: Option<(u64, &str)> = parse_send_payload("noseparator");
        assert_eq!(parsed, None);
    }

    #[test]
    fn r659_empty_event_name_returns_none() {
        let parsed: Option<(u64, &str)> = parse_send_payload("7:");
        assert_eq!(parsed, None);
    }

    #[test]
    fn r659_non_parseable_key_returns_none() {
        let parsed: Option<(u64, &str)> = parse_send_payload("xx:PointerDown");
        assert_eq!(parsed, None);
    }

    #[test]
    fn r659_event_name_with_internal_colon_keeps_suffix_intact() {
        // `split_once(':')` consumes only the FIRST `:` — a future
        // event name with a colon (e.g. namespaced `Pointer:Move`) stays
        // unaltered in the returned slice.
        let parsed: Option<(u64, &str)> = parse_send_payload("3:Pointer:Move");
        assert_eq!(parsed, Some((3_u64, "Pointer:Move")));
    }

    #[test]
    fn r659_zero_key_is_valid() {
        // `0` parses to all numeric `K`; the helper does not treat it
        // as a sentinel. Filter mode discriminant 0 (Active) and id 0
        // both land here legitimately.
        let parsed: Option<(usize, &str)> = parse_send_payload("0:PointerDown");
        assert_eq!(parsed, Some((0_usize, "PointerDown")));
        let parsed: Option<(u64, &str)> = parse_send_payload("0:PointerDown");
        assert_eq!(parsed, Some((0_u64, "PointerDown")));
    }

    #[test]
    fn r659_negative_key_against_unsigned_returns_none() {
        // FromStr::<u64> rejects `-1`, so the helper surfaces `None`
        // without panic — defensive against a stale composite tag
        // from an older protocol revision.
        let parsed: Option<(u64, &str)> = parse_send_payload("-1:PointerDown");
        assert_eq!(parsed, None);
    }
}
