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
//! R660 §5.16 — framework-side composites
//! ([`crate::widgets::radio_group::RadioGroupExternal`],
//! [`crate::widgets::listbox::ListBoxExternal`]) now route through this
//! helper too (5-of-5 framework substrate maturity). The Rule-of-Three
//! carry from R659 is repaid in the round after surface — every R51.42
//! composite-tag invoke arm in the workspace shares one parser.
//!
//! [`InputRouter`]: pinion_runtime::InputRouter

use crate::input::Modifiers;
use core::str::FromStr;

/// R781 §5.35 §5.41 — split a composite-tag send payload
/// `"<key>:<EventName>[:<mods>]"` into its three wire segments: the raw
/// key string, the event name, and the held [`Modifiers`] (empty when the
/// optional third segment is absent).
///
/// This is the `:` grammar SSOT that [`parse_send_payload`] (typed-key
/// consumers) and the row-only key parsers (e.g.
/// [`VirtualSelectExternal`](crate::widgets::virtual_select) via
/// [`GridSendKey`]) both decode through, so the modifier segment is
/// stripped in exactly one place — a divergence would let one consumer see
/// `"PointerUp:sc"` as the event name and break activation matching.
///
/// Returns `None` when malformed: missing `:` (no event), empty event name
/// (`"7:"`), or an unparseable modifier token (`Modifiers::from_wire_token`
/// rejects any non-`scam` letter). Event names are colon-free (the closed
/// `Pointer*` vocabulary), so the third `:` segment unambiguously belongs
/// to the modifiers — R781 retires the pre-R781 "colon kept in the event
/// suffix" behaviour, which never carried a real event name.
#[must_use]
pub fn split_send_payload(payload: &str) -> Option<(&str, &str, Modifiers)> {
    let mut parts = payload.splitn(3, ':');
    let key_str = parts.next()?;
    let event_name = parts.next()?;
    if event_name.is_empty() {
        return None;
    }
    let modifiers = match parts.next() {
        Some(token) => Modifiers::from_wire_token(token)?,
        None => Modifiers::empty(),
    };
    Some((key_str, event_name, modifiers))
}

/// Parse a R51.42 §5.35 composite-tag send payload
/// `"<key>:<EventName>[:<mods>]"` into `(key, event_name, modifiers)`.
///
/// Returns `None` when the wire-form is malformed:
///
/// * missing `:` separator,
/// * empty event name (e.g. `"7:"`),
/// * non-parseable `<key>` for the requested `K` (e.g. `"xx:PointerDown"`
///   against `K = u64`),
/// * an unparseable modifier token (the R781 third segment).
///
/// On the happy path returns the typed key, a borrowed event-name slice
/// into `payload` (callers typically match against `"PointerDown"` /
/// `"PointerUp"` / …), and the held [`Modifiers`] (R781 — empty for the
/// two-segment back-compat wire). Built on the [`split_send_payload`] `:`
/// SSOT.
///
/// The dispatcher contract is documented at the [module level](self).
#[must_use]
pub fn parse_send_payload<K: FromStr>(payload: &str) -> Option<(K, &str, Modifiers)> {
    let (key_str, event_name, modifiers) = split_send_payload(payload)?;
    let key = key_str.parse().ok()?;
    Some((key, event_name, modifiers))
}

/// R777.1 §5.16 §5.40 — the **grid composite send sub-key** grammar: the
/// `'#'`-split sub-tag a data-grid header or cell routes to its
/// [`External`](crate::external::External) through the R51.42 funnel. A
/// column-header click is `"h<col>"`; a data cell is `"<row>_<col>"`
/// (always an underscore, so the `h` prefix is unambiguous).
///
/// One codec so the paint **producer**
/// (`pinion_widget_paint::table::header_cell` / `…::data_row`), the
/// **a11y** gridcell node identity, and every **decoder** —
/// [`TableExternal`](crate::widgets::table) (eager, needs row+col) and
/// [`VirtualSelectExternal`](crate::widgets::virtual_select) (virtualized,
/// row-only) — share the `'_'` / `'h'` grammar instead of re-deriving it
/// inline. This is the R773 wire-form encode↔decode SSOT applied to the
/// grid cell key (R777.1 audit-correction): a divergence between the
/// producer's separator and a decoder's split would misroute every click,
/// so the grammar lives in exactly one place.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum GridSendKey {
    /// A column-header click (`"h<col>"`) — the sort-cycle target.
    Header {
        /// Zero-based column index.
        col: usize,
    },
    /// A data-cell click (`"<row>_<col>"`).
    Cell {
        /// Zero-based data-row index.
        row: usize,
        /// Zero-based column index.
        col: usize,
    },
}

impl GridSendKey {
    /// Decode a `'#'`-split sub-key. Returns `None` when it is neither a
    /// header (`h<col>`) nor a cell (`<row>_<col>`) key — e.g. a bare
    /// list-item index, which has no grid structure (the
    /// [`VirtualSelectExternal`](crate::widgets::virtual_select) list path
    /// falls back to a plain integer parse).
    #[must_use]
    pub fn parse(key: &str) -> Option<Self> {
        if let Some(col) = key.strip_prefix('h') {
            return col.parse().ok().map(|col| Self::Header { col });
        }
        let (row, col) = key.split_once('_')?;
        Some(Self::Cell {
            row: row.parse().ok()?,
            col: col.parse().ok()?,
        })
    }

    /// The data row this key addresses, or `None` for a header key — the
    /// row-only view a single-row selection coordinator
    /// ([`VirtualSelectExternal`](crate::widgets::virtual_select)) needs
    /// (WAI-ARIA / Qt `QItemSelectionModel` `SelectRows`: the column is
    /// irrelevant to a row selection).
    #[must_use]
    pub fn row(self) -> Option<usize> {
        match self {
            Self::Cell { row, .. } => Some(row),
            Self::Header { .. } => None,
        }
    }

    /// Encode the sub-key — the **producer** side, paired with
    /// [`parse`](Self::parse) so the paint tag and the decoder grammar
    /// cannot drift. The full composite paint tag is
    /// `format!("{tag}#{}", key.encode())`.
    #[must_use]
    pub fn encode(self) -> String {
        match self {
            Self::Header { col } => format!("h{col}"),
            Self::Cell { row, col } => format!("{row}_{col}"),
        }
    }
}

/// R742.4 §5.16 §5.35 — split a (possibly composite) paint tag at the
/// `#` separator into `(primary, Some(sub))`, the companion of
/// [`parse_send_payload`] for the *tag* side of the R51.42 protocol.
///
/// * `"group#2"` → `("group", Some("2"))`
/// * `"main_btn"` → `("main_btn", None)` (no separator)
/// * `"group#"` → `("group", None)` (empty sub-index is treated as
///   absent — the well-defined corner case the router relies on)
/// * `"a#b#c"` → `("a", Some("b#c"))` ([`str::split_once`] stops at the
///   first `#`; the remainder is opaque to the router today)
///
/// This is the canonical `#` splitter shared by the
/// [`InputRouter`](pinion_runtime::InputRouter) dispatch / drag / focus
/// paths, the shell's access-action router, and composite-widget
/// bindings (e.g. the reorder list) — the `#` SSOT paired with the `:`
/// SSOT above so neither separator is re-split inline (R742.4 review
/// consolidation; the prior copies were divergent in return shape but
/// shared this exact semantics).
#[must_use]
pub fn split_subindex(tag: &str) -> (&str, Option<&str>) {
    match tag.split_once('#') {
        Some((primary, idx)) if !idx.is_empty() => (primary, Some(idx)),
        Some((primary, _)) => (primary, None),
        None => (tag, None),
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_send_payload, split_send_payload, split_subindex, GridSendKey};
    use crate::input::Modifiers;

    const NONE: Modifiers = Modifiers::empty();

    #[test]
    fn grid_send_key_round_trips_cell_and_header() {
        // Encode ↔ parse are inverses (the divergence-is-a-bug guard).
        for key in [
            GridSendKey::Cell { row: 0, col: 0 },
            GridSendKey::Cell { row: 9_999, col: 2 },
            GridSendKey::Header { col: 1 },
        ] {
            assert_eq!(GridSendKey::parse(&key.encode()), Some(key), "round-trip {key:?}");
        }
        assert_eq!(GridSendKey::Cell { row: 4, col: 2 }.encode(), "4_2");
        assert_eq!(GridSendKey::Header { col: 1 }.encode(), "h1");
    }

    #[test]
    fn grid_send_key_row_view_and_rejects_non_grid() {
        assert_eq!(GridSendKey::parse("5_2").and_then(GridSendKey::row), Some(5));
        // A header key has no row (SelectRows: ignored by a row coordinator).
        assert_eq!(GridSendKey::parse("h2").and_then(GridSendKey::row), None);
        // A bare list-item index is not a grid key — the list path handles it.
        assert_eq!(GridSendKey::parse("5"), None);
        // Malformed grid keys decode to None (defensive against wire drift).
        assert_eq!(GridSendKey::parse("x_2"), None);
        assert_eq!(GridSendKey::parse("5_y"), None);
        assert_eq!(GridSendKey::parse("hx"), None);
    }

    #[test]
    fn split_subindex_covers_all_shapes() {
        assert_eq!(split_subindex("main_btn"), ("main_btn", None));
        assert_eq!(split_subindex("group#0"), ("group", Some("0")));
        assert_eq!(split_subindex("group#42"), ("group", Some("42")));
        // Empty sub-index is treated as absent.
        assert_eq!(split_subindex("group#"), ("group", None));
        // Empty primary — lookups will silently fail, but the split is
        // well-defined.
        assert_eq!(split_subindex("#0"), ("", Some("0")));
        // `split_once` stops at the first `#`; the remainder is opaque.
        assert_eq!(split_subindex("a#b#c"), ("a", Some("b#c")));
    }

    #[test]
    fn r659_parse_u64_pointer_down_happy_path() {
        let parsed: Option<(u64, &str, Modifiers)> = parse_send_payload("42:PointerDown");
        assert_eq!(parsed, Some((42_u64, "PointerDown", NONE)));
    }

    #[test]
    fn r659_parse_usize_arbitrary_event_name() {
        let parsed: Option<(usize, &str, Modifiers)> = parse_send_payload("0:PointerEnter");
        assert_eq!(parsed, Some((0_usize, "PointerEnter", NONE)));
    }

    #[test]
    fn r659_missing_colon_separator_returns_none() {
        let parsed: Option<(u64, &str, Modifiers)> = parse_send_payload("noseparator");
        assert_eq!(parsed, None);
    }

    #[test]
    fn r659_empty_event_name_returns_none() {
        let parsed: Option<(u64, &str, Modifiers)> = parse_send_payload("7:");
        assert_eq!(parsed, None);
    }

    #[test]
    fn r659_non_parseable_key_returns_none() {
        let parsed: Option<(u64, &str, Modifiers)> = parse_send_payload("xx:PointerDown");
        assert_eq!(parsed, None);
    }

    #[test]
    fn r781_third_segment_is_the_modifier_token() {
        // R781 retires the pre-R781 "internal colon kept in the event
        // suffix" behaviour: the third `:` segment is the modifier token
        // (event names are colon-free). `sc` → shift+ctrl.
        let parsed: Option<(u64, &str, Modifiers)> = parse_send_payload("3:PointerUp:sc");
        assert_eq!(
            parsed,
            Some((3_u64, "PointerUp", Modifiers { shift: true, ctrl: true, alt: false, meta: false })),
        );
        // The raw splitter agrees (the SSOT both paths decode through).
        assert_eq!(
            split_send_payload("4_2:PointerUp:s"),
            Some(("4_2", "PointerUp", Modifiers { shift: true, ctrl: false, alt: false, meta: false })),
        );
        // An unparseable modifier token rejects the whole payload.
        let bad: Option<(u64, &str, Modifiers)> = parse_send_payload("3:PointerUp:Move");
        assert_eq!(bad, None, "non-scam modifier token is malformed");
    }

    #[test]
    fn r659_zero_key_is_valid() {
        // `0` parses to all numeric `K`; the helper does not treat it
        // as a sentinel. Filter mode discriminant 0 (Active) and id 0
        // both land here legitimately.
        let parsed: Option<(usize, &str, Modifiers)> = parse_send_payload("0:PointerDown");
        assert_eq!(parsed, Some((0_usize, "PointerDown", NONE)));
        let parsed: Option<(u64, &str, Modifiers)> = parse_send_payload("0:PointerDown");
        assert_eq!(parsed, Some((0_u64, "PointerDown", NONE)));
    }

    #[test]
    fn r659_negative_key_against_unsigned_returns_none() {
        // FromStr::<u64> rejects `-1`, so the helper surfaces `None`
        // without panic — defensive against a stale composite tag
        // from an older protocol revision.
        let parsed: Option<(u64, &str, Modifiers)> = parse_send_payload("-1:PointerDown");
        assert_eq!(parsed, None);
    }
}
