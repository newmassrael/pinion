//! R659 §5.16 §5.35 — composite-tag wire helpers shared by application
//! [`External`](crate::external::External) handlers that respond to
//! R51.42 §5.35 `<key>:<EventName>` send payloads.
//!
//! ## Why a substrate
//!
//! The R51.42 `InputRouter` composite-tag dispatch splits a paint
//! tag like `"todo_delete#42"` at the `#` separator, then forwards the
//! sub-index `42` to the resolved [`External`](crate::external::External)
//! as part of a wire-form payload `"42:PointerDown"`. The receiving
//! `invoke("send", Text(...))` arm has to parse the payload back into
//! a typed key (`u64` id, `usize` enum discriminant, …) plus the
//! event name suffix.
//!
//! R655 `TodoDeleteExternal` introduced the parse helper inline as a
//! 5-LOC private function. R658 `TodoToggleExternal` copy-pasted it.
//! R659 `TodoFilterExternal` is the **3rd consumer** of the same
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
//! up-front, with `K::Err` collapsed into `None`.
//!
//! R1564 §5.15 — the `Option`-returning decoders have `Result` peers
//! ([`require_send_payload`], [`require_parsed_send_payload`],
//! [`require_pair`]) that state **why** the decode failed, because a refusal
//! that names nothing is what
//! [`RefusalReason`](crate::external::RefusalReason) exists to end. Prefer
//! them from an `invoke` arm; the `Option` forms remain for callers that
//! branch on absence rather than refusing.
//!
//! R660 §5.16 — framework-side composites
//! ([`crate::widgets::radio_group::RadioGroupExternal`],
//! [`crate::widgets::listbox::ListBoxExternal`]) now route through this
//! helper too (5-of-5 framework substrate maturity). The Rule-of-Three
//! carry from R659 is repaid in the round after surface — every R51.42
//! composite-tag invoke arm in the workspace shares one parser.

use crate::external::InvokeError;
use crate::input::{Modifiers, PointerButtons};
use core::str::FromStr;

/// R1619 §5.35 §5.41 — one decoded composite-tag send payload: the address it
/// names, the event that happened, and the **input context that was live when
/// it happened**.
///
/// ## Why a struct and not a tuple
///
/// The wire has grown a segment three times (R51.42 `<key>:<Event>`, R781
/// `:<mods>`, R1619 `:<buttons>`), and each growth re-broke every consumer's
/// destructuring — not because the consumer was wrong, but because an
/// anonymous 3-tuple has no way to say "there is now a fourth fact and you
/// may ignore it". Naming the fields ends that: a consumer reads
/// [`event`](Self::event) and is unaffected by a fifth context axis, while a
/// consumer that *wants* the context does not have to count commas to find
/// it. This is the same argument [`GridSendKey`] makes for the `'#'` sub-key
/// — the grammar is a type, not a shape.
///
/// ## The context fields are state, not edges
///
/// [`event`](Self::event) is the edge (*what just happened*);
/// [`modifiers`](Self::modifiers) and [`buttons`](Self::buttons) are state
/// (*what was held while it happened*). A drag-select is exactly the
/// conjunction: `PointerEnter` **with** [`buttons`](Self::buttons) non-empty
/// is a range extension, the same event with an empty set is a hover.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SendPayload<'a> {
    /// The raw `<key>` segment — the `'#'` sub-index the paint tag carried,
    /// or `""` for the R880 bare (background) target.
    pub key: &'a str,
    /// The event name (`"PointerEnter"`, `"PointerUp"`, `"DoubleClick"`, …),
    /// borrowed out of the payload.
    pub event: &'a str,
    /// R781 — the keyboard modifiers held at this event; empty when the
    /// optional third segment is absent.
    pub modifiers: Modifiers,
    /// R1619 — the pointer buttons held at this event (the W3C
    /// `PointerEvent.buttons` state); empty when the optional fourth segment
    /// is absent.
    pub buttons: PointerButtons,
}

impl SendPayload<'_> {
    /// The `<key>` segment parsed as `K`, or `None` when it is not one —
    /// the typed-address view [`parse_send_payload`] is built from.
    #[must_use]
    pub fn key_as<K: FromStr>(&self) -> Option<K> {
        self.key.parse().ok()
    }
}

/// R781 §5.35 §5.41 — split a composite-tag send payload
/// `"<key>:<EventName>[:<mods>[:<buttons>]]"` into its wire segments: the raw
/// key string, the event name, the held [`Modifiers`] (empty when the optional
/// third segment is absent) and the held [`PointerButtons`] (empty when the
/// optional fourth segment is absent).
///
/// This is the `:` grammar SSOT that [`parse_send_payload`] (typed-key
/// consumers) and the row-only key parsers (e.g.
/// [`VirtualSelectExternal`](crate::widgets::virtual_select) via
/// [`GridSendKey`]) both decode through, so the context segments are
/// stripped in exactly one place — a divergence would let one consumer see
/// `"PointerUp:sc"` as the event name and break activation matching.
///
/// Returns `None` when malformed: missing `:` (no event), empty event name
/// (`"7:"`), an unparseable modifier token (`Modifiers::from_wire_token`
/// rejects any non-`scam` letter), or an unparseable button token
/// (`PointerButtons::from_wire_token` rejects any non-`lmr` letter). Event
/// names are colon-free (the closed `Pointer*` vocabulary), so the trailing
/// `:` segments unambiguously belong to the context — R781 retires the
/// pre-R781 "colon kept in the event suffix" behaviour, which never carried a
/// real event name.
///
/// R1619 — the *modifier* segment may be empty while the button segment is
/// present (`"3:PointerEnter::l"`): the two context axes are independent, and
/// an empty `scam` token is a legal spelling of "no modifiers"
/// ([`Modifiers::from_wire_token`] accepts `""`). Only the **trailing** empty
/// segments are elided by the encoder, so there is exactly one spelling of
/// every state.
///
/// R880 — the key segment may be **empty**: `":<EventName>:<token>"` is the
/// bare-target (background) context wire an
/// [`External::wants_bare_send_modifiers`](crate::external::External::wants_bare_send_modifiers)
/// opt-in receives. The empty key means "no sub-target"; consumers map it
/// to their background arm. A bare send *without* held context stays the
/// colon-free `"<EventName>"` (this fn returns `None` for it — the
/// established "`None` = bare event" decode contract).
#[must_use]
pub fn split_send_payload(payload: &str) -> Option<SendPayload<'_>> {
    let mut parts = payload.splitn(4, ':');
    let key = parts.next()?;
    let event = parts.next()?;
    if event.is_empty() {
        return None;
    }
    let modifiers = match parts.next() {
        Some(token) => Modifiers::from_wire_token(token)?,
        None => Modifiers::empty(),
    };
    let buttons = match parts.next() {
        Some(token) => PointerButtons::from_wire_token(token)?,
        None => PointerButtons::empty(),
    };
    Some(SendPayload {
        key,
        event,
        modifiers,
        buttons,
    })
}

/// R880.1 — the **encode twin** of [`split_send_payload`]: build the send
/// wire from its segments. `Some(key)` is a composite target (a
/// `'#'` sub-index); `None` is a bare (background) target, which emits the
/// colon-free event name unless context is held, in which case the
/// R880 empty-key wire `":<EventName>:<token>"` is produced.
/// An empty context always yields the back-compat form (no trailing
/// segments).
///
/// One producer + one splitter in one module (the [`GridSendKey`] R773
/// precedent: encoders and decoders share the grammar instead of
/// re-deriving it inline), so the R781/R880/R1619 `:` grammar cannot fork —
/// the runtime router is the wire's only emitter and goes through here.
/// Callers gate *whether* to pass context (the router's
/// `wants_bare_send_modifiers` opt-in for bare targets); this fn only
/// encodes.
///
/// R1619 — the button segment can only be reached through a modifier
/// segment, so a button-only state emits an **empty** third segment
/// (`"3:PointerEnter::l"`). Trailing-empty elision keeps the encoding
/// canonical: `split(compose(x)) == x` for every input, and no state has two
/// spellings.
#[must_use]
pub fn compose_send_payload(
    key: Option<&str>,
    event_name: &str,
    mods: Modifiers,
    buttons: PointerButtons,
) -> String {
    let head = match key {
        Some(key) => format!("{key}:{event_name}"),
        None if mods.is_empty() && buttons.is_empty() => return event_name.to_string(),
        None => format!(":{event_name}"),
    };
    match (mods.is_empty(), buttons.is_empty()) {
        (true, true) => head,
        (_, true) => format!("{head}:{}", mods.as_wire_token()),
        (_, false) => format!(
            "{head}:{}:{}",
            mods.as_wire_token(),
            buttons.as_wire_token()
        ),
    }
}

/// The `<key>` a send payload addresses **on its activation edge** —
/// [`split_send_payload`] + the activation gate
/// ([`is_activation_event`](crate::input::is_activation_event)) — or
/// `None` for a non-activation event (hover / press) or a malformed
/// payload.
///
/// The one home of the click-to-activate decode every `<base>#<key>`
/// consumer shares: a click routes the full `PointerEnter` /
/// `PointerDown` / `PointerUp` / `PointerLeave` cycle through the
/// External `send` wire, and a list / palette / status bar wants to act
/// once, on the `PointerUp` edge, against `<key>`. What the caller does
/// with the key is its own concern — parse it as a row index
/// ([`send_activation_index`]) or match it as a named segment (R913
/// `hello-status-bar`'s `mode` / `encoding`). Lifted so no two consumers
/// can decode the activation gate differently — a divergence would be a
/// routing bug, not a style choice (the
/// [`is_activation_event`](crate::input::is_activation_event) precedent).
#[must_use]
pub fn send_activation_key(payload: &str) -> Option<&str> {
    let decoded = split_send_payload(payload)?;
    crate::input::is_activation_event(decoded.event).then_some(decoded.key)
}

/// The numeric sub-target index a send payload addresses on its
/// activation edge — [`send_activation_key`] parsed as a `usize`, or
/// `None` for a non-activation event or a non-numeric key. The indexed
/// specialization the `<base>#<i>` row-select consumers use (R910
/// `hello-inspector` select, R912 `hello-command-palette` select + run).
#[must_use]
pub fn send_activation_index(payload: &str) -> Option<usize> {
    send_activation_key(payload)?.parse::<usize>().ok()
}

/// R1231 — decode a `<prefix><i>` composite-tag key (`reset3`, `toggle0`,
/// `elem1`, `opt2`) to the trailing `usize` index, or `None` when `key` lacks
/// `prefix` or the tail is not an index. The mechanical strip-and-parse SSOT the
/// per-widget send-key dispatchers read: the *prefix vocabulary* (`reset` /
/// `toggle` / `opt` / …) is the widget's own, but this decode was independently
/// hand-rolled in the inspector, the property grid, and the data grid — three
/// sites, so it lifts here (the R727/R732 3rd-consumer mandate). A widget that
/// maps the index into its own value type composes: `prefixed_index(k, ELEM)?`
/// then `.map(ValueRef::Elem)`.
#[must_use]
pub fn prefixed_index(key: &str, prefix: &str) -> Option<usize> {
    key.strip_prefix(prefix)?.parse().ok()
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
/// On the happy path returns the typed key alongside the whole decoded
/// [`SendPayload`] (whose `key` is the raw slice the typed one was parsed
/// from). Built on the [`split_send_payload`] `:` SSOT.
///
/// The dispatcher contract is documented at the [module level](self).
#[must_use]
pub fn parse_send_payload<K: FromStr>(payload: &str) -> Option<(K, SendPayload<'_>)> {
    let decoded = split_send_payload(payload)?;
    let key = decoded.key_as()?;
    Some((key, decoded))
}

/// R1451 §5.35 — parse a **typed two-value argument payload** —
/// `"<a><sep><b>"`, each half trimmed — into `(A, B)`.
///
/// The argument wire form every `invoke` that takes a *pair* speaks:
/// `move_section` / `swap_sections` (`"<from>:<to>"`), `resize_section`
/// (`"<logical>:<px>"`), `set_section_hidden` (`"<logical>:<bool>"`), a
/// `byte_window` fraction span (`"<low>,<high>"`), a drag delta
/// (`"<dx>,<dy>"`). Those had each re-derived `split_once` +
/// `trim().parse()` inline; the separator differs by wire form (`:` for the
/// R51.42 composite family, `,` for coordinate-ish pairs) but nothing else
/// does, so `sep` is the one parameter and the rest is shared.
///
/// Distinct from [`parse_send_payload`], which decodes the *event* composite
/// (`"<key>:<EventName>[:<mods>]"`) — a three-segment grammar with a borrowed
/// middle slice, not a typed pair.
///
/// `None` when the separator is absent or either half does not parse as its
/// requested type. Prefer [`require_pair`] from an `invoke` arm — it says
/// which of those it was.
#[must_use]
pub fn parse_pair<A: FromStr, B: FromStr>(payload: &str, sep: char) -> Option<(A, B)> {
    let (a, b) = payload.split_once(sep)?;
    Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
}

/// R1564 §5.15 (PINION-PR82) — [`split_send_payload`] as a refusal that states
/// why, for the `invoke` arms that decode a composite send.
///
/// The `None` those decoders answer with is one bit, and every site that
/// mapped it to a bare `InvokeError::Rejected` discarded three facts it was
/// already holding: which action was addressed, which wire form was expected,
/// and what arrived instead. Composing that sentence *here* is the same
/// argument this module already makes about the grammar — one codec, so two
/// decoders cannot disagree about what "malformed" means, and now they cannot
/// disagree about how to say so either.
///
/// `action` is the invoke path the caller was serving; it leads the sentence
/// because a refusal read out of context has to name what was refused.
///
/// # Errors
///
/// [`InvokeError::Rejected`] naming the malformed payload, when
/// [`split_send_payload`] answers `None`.
pub fn require_send_payload<'a>(
    action: &str,
    payload: &'a str,
) -> Result<SendPayload<'a>, InvokeError> {
    split_send_payload(payload).ok_or_else(|| {
        InvokeError::rejected(format!(
            "{action}: malformed send payload {payload:?} \
             (expected \"<key>:<EventName>[:<mods>[:<buttons>]]\")"
        ))
    })
}

/// R1564 §5.15 — [`parse_send_payload`] as a refusal that states why.
///
/// Distinguishes the two failures the `Option` form fuses: a payload that does
/// not have the composite shape at all, and one that does but whose `<key>`
/// segment is not a `K`. They are different operator actions — fix the caller's
/// encoding, or address a target that exists — which is precisely the
/// distinction PINION-PR82 measured six sprag CLI paths guessing at.
///
/// # Errors
///
/// [`InvokeError::Rejected`] naming either the malformed payload or the
/// unparseable key.
pub fn require_parsed_send_payload<'a, K: FromStr>(
    action: &str,
    payload: &'a str,
) -> Result<(K, SendPayload<'a>), InvokeError> {
    let decoded = require_send_payload(action, payload)?;
    let key = decoded.key_as().ok_or_else(|| {
        let key_str = decoded.key;
        InvokeError::rejected(format!(
            "{action}: send key {key_str:?} is not a valid target for this surface"
        ))
    })?;
    Ok((key, decoded))
}

/// R1564 §5.15 — [`parse_pair`] as a refusal that states why, for the `invoke`
/// arms whose argument is a typed pair.
///
/// # Errors
///
/// [`InvokeError::Rejected`] naming the malformed argument and the separator
/// it was expected to carry.
pub fn require_pair<A: FromStr, B: FromStr>(
    action: &str,
    payload: &str,
    sep: char,
) -> Result<(A, B), InvokeError> {
    parse_pair(payload, sep).ok_or_else(|| {
        InvokeError::rejected(format!(
            "{action}: malformed argument {payload:?} (expected \"<a>{sep}<b>\")"
        ))
    })
}

/// R777.1 §5.16 §5.40 — the **grid composite send sub-key** grammar: the
/// `'#'`-split sub-tag a data-grid header or cell routes to its
/// [`External`](crate::external::External) through the R51.42 funnel. A
/// column-header click is `"h<col>"`; a data cell is `"<row>_<col>"`
/// (always an underscore, so the `h` prefix is unambiguous). R1562 adds the
/// vertical section axis's two addresses: a **row**-header press is `"r<row>"`
/// and the corner where the two axes meet is `"c"`.
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
    /// R1562 — a **row**-header click (`"r<row>"`): the vertical section axis's press
    /// address, the toolkit `sectionPressed` on a `Vertical` header.
    ///
    /// The peer of [`Header`](Self::Header) on the other axis, and the one that
    /// carries a row — which is the whole point. The toolkit reaches the same
    /// behaviour through a *second implementation*: a header press runs `mousePressEvent` ->
    /// `selectRow`, while a cell press runs `mousePressEvent` -> `selectionCommand(index, event)`. Here a row header **is** an
    /// address of its row ([`row`](Self::row) answers with it), so the press
    /// reaches the selection transition a cell press reaches, by the same
    /// call, and the band cannot disagree with the body about what a chorded
    /// click means.
    RowHeader {
        /// Zero-based **data**-row index (the R778 sort permutation already
        /// applied, exactly as a [`Cell`](Self::Cell)'s row is).
        row: usize,
    },
    /// R1562 — the cell where the two section axes meet (`"c"`): the toolkit's
    /// table corner button, the control `setCornerButtonEnabled`
    /// enables.
    ///
    /// Addresses no row and no column — it addresses the **whole model**, which
    /// is why it is its own arm rather than a section index. [`row`](Self::row)
    /// answers `None`: a selection coordinator that read a row out of this would
    /// be selecting one line for a press that means every line.
    Corner,
    /// A data-cell click (`"<row>_<col>"`).
    Cell {
        /// Zero-based data-row index.
        row: usize,
        /// Zero-based column index.
        col: usize,
    },
    /// A group-header click (`"g<group>"`) — the collapse-toggle target on a
    /// grouped grid (R892). Parallel to [`Header`](Self::Header) for the
    /// group axis: it addresses a group id, not a data row.
    Group {
        /// Group id (an index into the consumer's label table).
        group: usize,
    },
    /// R1555 — a step affordance of an open
    /// [`EditorForm::Stepper`](crate::cell_value::EditorForm::Stepper) editor (`"su<row>_<col>"` up / `"sd<row>_<col>"`
    /// down): the toolkit spin box's two arrow sub-controls.
    ///
    /// The **only** editor form that needs its own address. A toggle's checkbox,
    /// a selector's chevron and a swatch's colour chip each fill their cell, so
    /// a pointer landing on one already resolves to the cell's own tag; a
    /// swatch's hex half is a real
    /// [`TextField`](crate::widgets::text_field::TextField) carrying its own.
    /// Up and down are two targets inside one cell with nothing else to tell
    /// them apart, so they are the case that needs a sub-key rather than the
    /// case a sub-key family was invented for.
    EditorStep {
        /// Zero-based data-row index.
        row: usize,
        /// Zero-based column index.
        col: usize,
        /// Whether this is the increment affordance.
        up: bool,
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
        // R1562 — the corner is a whole key, not a prefix: it addresses the
        // model rather than a section, so there is no index to follow it.
        // Tried before the `'r'` prefix for no ordering reason (they cannot
        // collide) but with the other prefix-free arms, ahead of anything that
        // parses a number.
        if key == "c" {
            return Some(Self::Corner);
        }
        if let Some(row) = key.strip_prefix('r') {
            return row.parse().ok().map(|row| Self::RowHeader { row });
        }
        if let Some(group) = key.strip_prefix('g') {
            return group.parse().ok().map(|group| Self::Group { group });
        }
        // R1555 — the two-letter step prefixes are tried before the bare
        // `<row>_<col>` split, which they would otherwise fall into and fail
        // (`"su3".parse::<usize>()` is an error, so a pre-R1555 decoder
        // answered `None` for these keys rather than mis-routing them).
        for (prefix, up) in [("su", true), ("sd", false)] {
            if let Some(rest) = key.strip_prefix(prefix) {
                let (row, col) = rest.split_once('_')?;
                return Some(Self::EditorStep {
                    row: row.parse().ok()?,
                    col: col.parse().ok()?,
                    up,
                });
            }
        }
        let (row, col) = key.split_once('_')?;
        Some(Self::Cell {
            row: row.parse().ok()?,
            col: col.parse().ok()?,
        })
    }

    /// The data row this key addresses, or `None` for a header key — the row-only
    /// view a single-row selection coordinator
    /// ([`VirtualSelectExternal`](crate::widgets::virtual_select)) needs (WAI-ARIA / the toolkit
    /// item selection model `SelectRows`: the column is irrelevant to a row selection).
    #[must_use]
    /// R1555 — an [`EditorStep`](Self::EditorStep) answers with its row for the
    /// same reason a [`Cell`](Self::Cell) does: the affordance is *inside* a
    /// cell of that row, so a press on it is a press in the row, and a selection
    /// coordinator that answered `None` here would leave the selection behind
    /// whenever the user reached for an arrow instead of the cell body.
    /// R1562 — a [`RowHeader`](Self::RowHeader) answers with **its** row. That
    /// one line is what makes the vertical band's selection a *derivation*
    /// rather than a second implementation of it: the band press and the cell
    /// press reach `select` / `toggle` / `extend_to` through the same call, so
    /// no chord can mean one thing in the band and another in the body.
    ///
    /// [`Corner`](Self::Corner) answers `None` for the opposite reason — it
    /// addresses every row, which is not a row.
    pub fn row(self) -> Option<usize> {
        match self {
            Self::Cell { row, .. } | Self::EditorStep { row, .. } | Self::RowHeader { row } => {
                Some(row)
            }
            Self::Header { .. } | Self::Group { .. } | Self::Corner => None,
        }
    }

    /// R1563 — the **column** this key addresses, if it addresses one.
    ///
    /// The mirror of [`row`](Self::row), and it earns its place for the same
    /// reason that one did: a [`Header`](Self::Header) answering with its column
    /// is what lets a horizontal section press reach the very
    /// `select_column` / `toggle_column` / `extend_to_column` transition a
    /// [`Cell`](Self::Cell) press reaches under
    /// [`SelectionBehavior::SelectColumns`](crate::widgets::cell_selection::SelectionBehavior::SelectColumns),
    /// rather than a second implementation of the chord vocabulary.
    ///
    /// [`RowHeader`](Self::RowHeader) and [`Corner`](Self::Corner) answer
    /// `None` — a row's section addresses every column, and the corner
    /// addresses every one of both, and neither of those is *a* column. Note
    /// the two accessors are not complements: a [`Cell`](Self::Cell) answers on
    /// both, which is what a cell is.
    #[must_use]
    pub fn col(self) -> Option<usize> {
        match self {
            Self::Header { col } | Self::Cell { col, .. } | Self::EditorStep { col, .. } => {
                Some(col)
            }
            Self::RowHeader { .. } | Self::Group { .. } | Self::Corner => None,
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
            Self::RowHeader { row } => format!("r{row}"),
            Self::Corner => "c".to_string(),
            Self::Cell { row, col } => format!("{row}_{col}"),
            Self::Group { group } => format!("g{group}"),
            Self::EditorStep { row, col, up } => {
                let dir = if up { "su" } else { "sd" };
                format!("{dir}{row}_{col}")
            }
        }
    }
}
/// (R1554 §5.39 §5.40) The composite-tag SSOT for a **group box** — the
/// toolkit group box, HTML `<fieldset>`/`<legend>`.
///
/// Three addresses, because a group box is three things an agent addresses
/// separately: the group itself (`tag`, the `role=group` frame), its title
/// (`"{tag}_title"`, which is the click target and Tab stop of a *checkable*
/// group's checkbox), and its content region (`"{tag}_content"`, the node that
/// carries the `disabled` declaration and therefore the one `scene/disabled`
/// names as `declared_by`).
///
/// The content region has its own tag rather than reusing the group's for a
/// concrete reason: the frame and the title must stay live while the contents
/// are inert, which is the toolkit's checkable-group behaviour, and a region
/// cannot be disabled without being addressable — `focus/set`'s `tag_disabled` answer hands this
/// tag back as the thing to act on.
pub struct GroupBoxTag;

impl GroupBoxTag {
    /// The title band — `"{tag}_title"`. Carries the legend, and the checkbox
    /// of a checkable group.
    #[must_use]
    pub fn title(tag: &str) -> String {
        format!("{tag}_title")
    }

    /// The content region — `"{tag}_content"`. The node a checkable group
    /// declares `disabled` on.
    #[must_use]
    pub fn content(tag: &str) -> String {
        format!("{tag}_content")
    }
}

/// R862 §5.16 §5.40 — the **grid container-tag** scheme: the presentational
/// `'_'`-separated container tags a virtualized data-grid / tree-grid
/// *paints* (the header band, each data-row strip, each column-header cell),
/// which the **a11y** builder
/// ([`windowed_grid_nodes`](../../pinion_a11y/fn.windowed_grid_nodes.html))
/// *re-emits* to resolve each node's bounds by string-matching the tag into
/// the paint scene (`pinion_runtime::layout::rect_for_tag`).
///
/// Unlike [`GridSendKey`] these are never *decoded* — they are only produced
/// by paint and re-derived by a11y — but the cross-crate paint↔a11y string
/// match is **load-bearing**: a divergence between the paint producer's
/// `format!` and the a11y builder's would silently mis-resolve every grid
/// Row / header / column-header bounds (the AT tree would point at the wrong
/// pixels). So the scheme lives in exactly one place, applying the R773
/// wire-form encode SSOT to the container-tag family (R862 audit-correction:
/// the prior copies were hand-synced literals across `pinion-widget-paint`
/// and `pinion-a11y` — the [[verify-seed-claims-audit-first]] re-examination
/// that found the R803 `'#'`-send-wire reject did not govern this `'_'`
/// presentational family).
///
/// Tags: header band `"{tag}_hrow"`, column-header `"{tag}_ch{col}"`, data
/// row `"{tag}_row{id}"`, and the R859/R860 frozen-split additions — frozen
/// header band `"{tag}_fhrow"`, frozen data row `"{tag}_frow{id}"`,
/// tree-grid metadata row `"{tag}_drow{id}"`. The R863 tree-grid `treegrid`
/// a11y adds the metadata cell `"{tag}_dcell{id}_{col}"` and the tree-column
/// header `"{tag}_chtree"`. R1548 adds the **vertical** section axis: row
/// header `"{tag}_rh{row}"`, its mark `"{tag}_rhdeco{row}"`, and the corner
/// where the two axes meet `"{tag}_hcorner"`.
pub struct GridTag;

impl GridTag {
    /// The header-row band container — `"{tag}_hrow"`.
    #[must_use]
    pub fn header_row(tag: &str) -> String {
        format!("{tag}_hrow")
    }

    /// A column-header cell — `"{tag}_ch{col}"`.
    #[must_use]
    pub fn col_header(tag: &str, col: usize) -> String {
        format!("{tag}_ch{col}")
    }

    /// A data-row strip — `"{tag}_row{id}"`. `id` is generic over
    /// [`Display`](std::fmt::Display) because the data-grid keys rows by a
    /// numeric index (`usize`) while the tree-grid keys them by a string
    /// node id — the one SSOT serves both id spaces.
    #[must_use]
    pub fn data_row(tag: &str, id: impl core::fmt::Display) -> String {
        format!("{tag}_row{id}")
    }

    /// (R859) The frozen pane's header-row band — `"{tag}_fhrow"`.
    #[must_use]
    pub fn frozen_header_row(tag: &str) -> String {
        format!("{tag}_fhrow")
    }

    /// (R859) The frozen pane's data-row strip — `"{tag}_frow{id}"`.
    #[must_use]
    pub fn frozen_data_row(tag: &str, id: impl core::fmt::Display) -> String {
        format!("{tag}_frow{id}")
    }

    /// (R860) The tree-grid metadata-row strip (the scrolling pane's row) —
    /// `"{tag}_drow{id}"`.
    #[must_use]
    pub fn metadata_row(tag: &str, id: impl core::fmt::Display) -> String {
        format!("{tag}_drow{id}")
    }

    /// (R863) One tree-grid metadata **cell** — `"{tag}_dcell{id}_{col}"`,
    /// the `col`-th display cell of the metadata strip for row `id`. Stays
    /// in the `'_'` presentational family (no `'#'`) so it never enters the
    /// composite click-router namespace — a click on a metadata cell remains
    /// a no-op, exactly as the untagged cell was. Tagging it lets the a11y
    /// `treegrid` resolve a per-cell `gridcell` bounds rect (the metadata
    /// columns were AT-invisible while untagged).
    #[must_use]
    pub fn metadata_cell(tag: &str, id: impl core::fmt::Display, col: usize) -> String {
        format!("{tag}_dcell{id}_{col}")
    }

    /// (R1536) One cell's **decoration** mark — `"{tag}_deco{row}_{col}"`, the
    /// `DecorationRole` answer painted inside cell `(row, col)`.
    ///
    /// Stays in the `'_'` presentational family (no `'#'`) so it never enters
    /// the composite click-router namespace: the addressable *target* of a
    /// click on a mark is still the cell, exactly as it was while the mark was
    /// untagged. Tagging it is the R863 [`Self::metadata_cell`] move — a
    /// painted thing with no tag cannot be asked about, so the mark was
    /// reachable only by walking a cell's children **by position**, which is a
    /// layout detail no client should have to know.
    ///
    /// A tag and hit-testability are independent axes (`pinion_overlay`'s focus
    /// ring is tagged *and* pointer-transparent); R1535 conflated them and left
    /// the mark anonymous for a reason that does not hold.
    #[must_use]
    pub fn cell_decoration(tag: &str, row: impl core::fmt::Display, col: usize) -> String {
        format!("{tag}_deco{row}_{col}")
    }

    /// (R1547) One column header's **decoration** mark —
    /// `"{tag}_hdeco{col}"`, the `DecorationRole` answer painted inside the
    /// header of column `col` (the toolkit `headerData(section, Horizontal,
    /// DecorationRole)`).
    ///
    /// The section-axis peer of [`Self::cell_decoration`] and addressable for
    /// the same reason: a painted thing with no tag can only be reached by
    /// walking a header cell's children by position. It needs its own prefix
    /// rather than a `row`-less `cell_decoration` because a header is not row
    /// `0` — `"{tag}_deco0_3"` is a real *cell* address, and reusing it would
    /// make the two collide in the one namespace that must stay unique.
    ///
    /// Presentational (`'_'`, no `'#'`), so the click target of a marked header
    /// stays the header itself — which here means the mark cannot swallow a
    /// sort click.
    #[must_use]
    pub fn header_decoration(tag: &str, col: usize) -> String {
        format!("{tag}_hdeco{col}")
    }

    /// (R1548) One **row** header's cell — `"{tag}_rh{row}"`: the vertical section axis's
    /// peer of [`Self::col_header`] (the toolkit `headerData(section, Vertical, …)`, painted by `verticalHeader()`).
    ///
    /// `row` is a `usize` where [`Self::data_row`] takes any
    /// [`Display`](core::fmt::Display), and the narrowing is deliberate: the
    /// string id space belongs to the tree-grid, and a tree view has no
    /// vertical header at all. Typing it out also keeps this address
    /// unambiguous against [`Self::row_header_decoration`] — with a free-form
    /// id, `"t_rhdeco3"` would be a legal reading of both.
    ///
    /// Presentational (`'_'`, no `'#'`), like every other tag in this family:
    /// tagging a painted thing is what makes it *askable*, and is independent
    /// of whether it is a click target.
    #[must_use]
    pub fn row_header(tag: &str, row: usize) -> String {
        format!("{tag}_rh{row}")
    }

    /// (R1548) One row header's **decoration** mark — `"{tag}_rhdeco{row}"`,
    /// the `DecorationRole` answer painted ahead of row `row`'s label.
    ///
    /// The vertical-axis peer of [`Self::header_decoration`], and it needs its
    /// own prefix for the reason that one did: a mark's address must not be
    /// readable as some other node's.
    #[must_use]
    pub fn row_header_decoration(tag: &str, row: usize) -> String {
        format!("{tag}_rhdeco{row}")
    }

    /// (R1548) The corner above the row-header band — `"{tag}_hcorner"`, the cell where the
    /// two section axes meet (the toolkit's table corner button).
    ///
    /// Painted so the two bands align, and tagged so its extent can be read;
    /// it names neither axis, so it carries no a11y node — the toolkit exposes
    /// it as an unlabelled "select all" button, and without that behaviour
    /// such a node would be noise in the tree rather than information.
    #[must_use]
    pub fn header_corner(tag: &str) -> String {
        format!("{tag}_hcorner")
    }

    /// (R863) The tree-grid's **name**-column header cell — `"{tag}_chtree"`.
    /// The frozen tree column's `columnheader` (the metadata columns reuse
    /// the numeric [`Self::col_header`]); a dedicated tag keeps the
    /// metadata-column index space 0-based and free of an off-by-one for the
    /// leading tree column.
    #[must_use]
    pub fn tree_col_header(tag: &str) -> String {
        format!("{tag}_chtree")
    }
}

/// R1551 §5.36 §5.40 — paint-tag SSOT for a **rich-text document**: the
/// container and the paragraphs inside it.
///
/// Stays in the [`GridTag`] `'_'` presentational family (no `'#'`), so a
/// paragraph never enters the composite click-router namespace — a document is
/// read, not clicked, and a block that declares a heading needs a tag only so
/// assistive technology and the `scene/text_blocks` wire can address it.
///
/// Tags: the document container `"{tag}_doc"`, block `i` `"{tag}_blk{i}"`.
pub struct DocumentTag;

impl DocumentTag {
    /// The document's container node — `"{tag}_doc"`.
    #[must_use]
    pub fn document(tag: &str) -> String {
        format!("{tag}_doc")
    }

    /// The `i`-th paragraph — `"{tag}_blk{i}"`.
    ///
    /// One encode point, because two things read a block's tag and they must
    /// agree: the a11y heading pass (which finds it in the painted scene) and
    /// any client addressing a paragraph over the wire. A format string spelled
    /// twice is the drift this family exists to prevent.
    #[must_use]
    pub fn block(tag: &str, i: usize) -> String {
        format!("{tag}_blk{i}")
    }

    /// R1559 — the `k`-th **list** the document's numbering derived, in
    /// document order — `"{tag}_lst{k}"`.
    ///
    /// A list is an object in its own right (the toolkit's text list), so it
    /// is addressable in its own right: it is the node whose box encloses its
    /// items, the WAI-ARIA `list` the items are `listitem`s of, and the row `scene/text_lists` reports.
    /// `k` counts lists, not blocks — a document whose first list starts at
    /// block 4 still names it `_lst0`.
    #[must_use]
    pub fn list(tag: &str, k: usize) -> String {
        format!("{tag}_lst{k}")
    }

    /// R1559 — the `i`-th paragraph's **item row**, when it is a list item —
    /// `"{tag}_itm{i}"`.
    ///
    /// The row is the marker and the paragraph side by side, so it is what a
    /// caller measures to ask how far an item reaches; the paragraph inside it
    /// keeps [`Self::block`] and stays the addressable text.
    #[must_use]
    pub fn item(tag: &str, i: usize) -> String {
        format!("{tag}_itm{i}")
    }

    /// R1559 — the `i`-th paragraph's painted **marker** — `"{tag}_mrk{i}"`.
    ///
    /// Addressable because it is real text: the toolkit draws its unordered
    /// markers as geometry with no accessor at all, so "where is this item's
    /// bullet, and what does it say" is a question only one of the two
    /// toolkits can answer.
    #[must_use]
    pub fn marker(tag: &str, i: usize) -> String {
        format!("{tag}_mrk{i}")
    }

    /// R1560 — the `k`-th **table** the document's addressing derived, in
    /// document order — `"{tag}_tbl{k}"`.
    ///
    /// A table is an object in its own right (the toolkit's text table): the
    /// node whose box encloses its cells, the WAI-ARIA `table` its rows belong to,
    /// and the row `scene/text_tables` reports. `k` counts tables, not blocks.
    #[must_use]
    pub fn table(tag: &str, k: usize) -> String {
        format!("{tag}_tbl{k}")
    }

    /// R1560 — row `r` (0-based) of the `k`-th table — `"{tag}_tbl{k}r{r}"`.
    ///
    /// A row exists as a painted node even though a grid places cells
    /// directly: it is the band a header or a stripe is drawn on, the box a
    /// caller measures to ask how tall a row is, and the WAI-ARIA `row` that
    /// owns the cells. Its tag is derived from the table's, so a row cannot be
    /// addressed as belonging to a table it is not in.
    #[must_use]
    pub fn table_row(tag: &str, k: usize, r: u32) -> String {
        format!("{tag}_tbl{k}r{r}")
    }

    /// R1560 — the **cell** opened by the `i`-th paragraph — `"{tag}_cel{i}"`.
    ///
    /// Keyed by the block that opens the cell rather than by its address, for
    /// the reason the address exists at all: an address moves when an earlier
    /// cell's span changes, so a tag built from one would rename cells that
    /// nothing was done to. Every block of a multi-block cell carries this one
    /// tag, which is what makes them one cell.
    #[must_use]
    pub fn cell(tag: &str, i: usize) -> String {
        format!("{tag}_cel{i}")
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
/// `InputRouter` dispatch / drag / focus
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
    use super::{
        GridSendKey, GridTag, SendPayload, compose_send_payload, parse_pair, parse_send_payload,
        prefixed_index, send_activation_index, send_activation_key, split_send_payload,
        split_subindex,
    };
    use crate::input::{Modifiers, PointerButton, PointerButtons};

    const NONE: Modifiers = Modifiers::empty();
    const SHIFT: Modifiers = Modifiers {
        shift: true,
        ctrl: false,
        alt: false,
        meta: false,
    };
    const SHIFT_CTRL: Modifiers = Modifiers {
        shift: true,
        ctrl: true,
        alt: false,
        meta: false,
    };
    /// No button held — named `UP` so an assertion reads as the state it is
    /// (`PointerButtons::empty()` spelled out at 40 call sites would bury the
    /// axis that varies).
    const UP: PointerButtons = PointerButtons::empty();
    const LEFT: PointerButtons = PointerButtons::empty().with(PointerButton::Left);
    const MIDDLE: PointerButtons = PointerButtons::empty().with(PointerButton::Middle);
    const LEFT_RIGHT: PointerButtons = PointerButtons::empty()
        .with(PointerButton::Left)
        .with(PointerButton::Right);

    #[test]
    fn parse_pair_reads_each_half_at_its_own_type() {
        // R1451 — the two halves are parsed independently, so a pair may be
        // heterogeneous (`set_section_hidden`'s `"<logical>:<bool>"`).
        assert_eq!(parse_pair::<usize, usize>("2:5", ':'), Some((2, 5)));
        assert_eq!(parse_pair::<usize, bool>("0:true", ':'), Some((0, true)));
        assert_eq!(parse_pair::<usize, u32>("1:140", ':'), Some((1, 140)));
        // The coordinate-ish family uses `,` — the one parameter that differs.
        assert_eq!(parse_pair::<f32, f32>("0.25,0.75", ','), Some((0.25, 0.75)));
        assert_eq!(parse_pair::<i32, i32>("-4,7", ','), Some((-4, 7)));
        // Surrounding space is the caller's slack, not a rejection.
        assert_eq!(parse_pair::<usize, usize>(" 3 : 1 ", ':'), Some((3, 1)));
    }

    #[test]
    fn parse_pair_rejects_missing_separator_and_either_bad_half() {
        // A missing separator, a bad left half, and a bad right half are all
        // one `None` — every caller maps them to a single `Rejected`.
        assert_eq!(parse_pair::<usize, usize>("25", ':'), None);
        assert_eq!(parse_pair::<usize, usize>("x:5", ':'), None);
        assert_eq!(parse_pair::<usize, usize>("2:y", ':'), None);
        assert_eq!(parse_pair::<usize, usize>("2:", ':'), None);
        // A negative value is not a `usize` — the type is the validation.
        assert_eq!(parse_pair::<usize, usize>("-1:2", ':'), None);
        // The wrong separator is a missing separator.
        assert_eq!(parse_pair::<usize, usize>("2,5", ':'), None);
    }

    #[test]
    fn grid_container_tag_scheme_is_pinned() {
        // R862 — the cross-crate paint↔a11y SSOT: pin the exact strings so a
        // change here is a deliberate, single-site edit (not a silent
        // divergence between the paint producer and the a11y re-emit).
        assert_eq!(GridTag::header_row("vtbl"), "vtbl_hrow");
        assert_eq!(GridTag::col_header("vtbl", 2), "vtbl_ch2");
        assert_eq!(GridTag::data_row("vtbl", 41), "vtbl_row41");
        assert_eq!(GridTag::frozen_header_row("vtbl"), "vtbl_fhrow");
        assert_eq!(GridTag::frozen_data_row("vtbl", 41), "vtbl_frow41");
        // The tree-grid keys metadata rows by a string node id.
        assert_eq!(GridTag::metadata_row("tg", "f3-o1"), "tg_drowf3-o1");
        assert_eq!(GridTag::data_row("tg", "f3-o1"), "tg_rowf3-o1");
        // R863 — metadata cell + tree-column header.
        assert_eq!(GridTag::metadata_cell("tg", "f3-o1", 2), "tg_dcellf3-o1_2");
        assert_eq!(GridTag::tree_col_header("tg"), "tg_chtree");
        // R1547 — the two decoration addresses, pinned together because the
        // point of the header's own prefix is that it cannot collide with a
        // cell's. `_deco0_3` is cell (0, 3); the header of column 3 is not it.
        assert_eq!(GridTag::cell_decoration("vtbl", 0, 3), "vtbl_deco0_3");
        assert_eq!(GridTag::header_decoration("vtbl", 3), "vtbl_hdeco3");
        assert_ne!(
            GridTag::header_decoration("vtbl", 3),
            GridTag::cell_decoration("vtbl", 0, 3)
        );
        // R1548 — the vertical section axis. Pinned against every address the
        // family already had, because the whole value of a per-axis prefix is
        // that no other node's address can be read as one of these.
        assert_eq!(GridTag::row_header("vtbl", 3), "vtbl_rh3");
        assert_eq!(GridTag::row_header_decoration("vtbl", 3), "vtbl_rhdeco3");
        assert_eq!(GridTag::header_corner("vtbl"), "vtbl_hcorner");
        for other in [
            GridTag::col_header("vtbl", 3),
            GridTag::header_decoration("vtbl", 3),
            GridTag::data_row("vtbl", 3),
            GridTag::cell_decoration("vtbl", 0, 3),
            GridTag::header_row("vtbl"),
        ] {
            assert_ne!(GridTag::row_header("vtbl", 3), other);
            assert_ne!(GridTag::row_header_decoration("vtbl", 3), other);
            assert_ne!(GridTag::header_corner("vtbl"), other);
        }
    }

    #[test]
    fn grid_send_key_round_trips_cell_and_header() {
        // Encode ↔ parse are inverses (the divergence-is-a-bug guard).
        for key in [
            GridSendKey::Cell { row: 0, col: 0 },
            GridSendKey::Cell { row: 9_999, col: 2 },
            GridSendKey::Header { col: 1 },
            GridSendKey::Group { group: 3 },
            GridSendKey::EditorStep {
                row: 0,
                col: 0,
                up: true,
            },
            GridSendKey::EditorStep {
                row: 7_000,
                col: 3,
                up: false,
            },
            GridSendKey::RowHeader { row: 0 },
            GridSendKey::RowHeader { row: 9_999 },
            GridSendKey::Corner,
        ] {
            assert_eq!(
                GridSendKey::parse(&key.encode()),
                Some(key),
                "round-trip {key:?}"
            );
        }
        assert_eq!(GridSendKey::Cell { row: 4, col: 2 }.encode(), "4_2");
        assert_eq!(GridSendKey::Header { col: 1 }.encode(), "h1");
        assert_eq!(GridSendKey::Group { group: 3 }.encode(), "g3");
        assert_eq!(GridSendKey::RowHeader { row: 4 }.encode(), "r4");
        assert_eq!(GridSendKey::Corner.encode(), "c");
        assert_eq!(
            GridSendKey::EditorStep {
                row: 4,
                col: 2,
                up: true
            }
            .encode(),
            "su4_2"
        );
        assert_eq!(
            GridSendKey::EditorStep {
                row: 4,
                col: 2,
                up: false
            }
            .encode(),
            "sd4_2"
        );
    }

    #[test]
    fn r1555_a_step_key_is_not_confused_with_a_cell_key() {
        // The two prefixes are tried before the bare `<row>_<col>` split, and
        // the directions do not collapse into each other.
        assert_eq!(
            GridSendKey::parse("su4_2"),
            Some(GridSendKey::EditorStep {
                row: 4,
                col: 2,
                up: true
            })
        );
        assert_eq!(
            GridSendKey::parse("sd4_2"),
            Some(GridSendKey::EditorStep {
                row: 4,
                col: 2,
                up: false
            })
        );
        // A step affordance is inside a cell of its row, so a press on it is a
        // press in the row — a coordinator must not lose the selection when the
        // user reaches for an arrow.
        assert_eq!(
            GridSendKey::parse("su4_2").and_then(GridSendKey::row),
            Some(4)
        );
        // Malformed step keys decode to None rather than to a cell.
        assert_eq!(GridSendKey::parse("su4"), None);
        assert_eq!(GridSendKey::parse("sux_2"), None);
        assert_eq!(GridSendKey::parse("sq4_2"), None, "an unknown direction");
    }

    /// R1562 — the vertical axis's two addresses decode to themselves, and the
    /// **row header answers with its row** while the corner answers with none.
    /// That asymmetry is the round's whole selection behaviour: a band press
    /// travels the cell press's transition, a corner press cannot be mistaken
    /// for one line.
    #[test]
    fn r1562_a_row_header_addresses_its_row_and_the_corner_addresses_none() {
        assert_eq!(
            GridSendKey::parse("r4"),
            Some(GridSendKey::RowHeader { row: 4 })
        );
        assert_eq!(
            GridSendKey::parse("r4").and_then(GridSendKey::row),
            Some(4),
            "a row header IS an address of its row"
        );
        assert_eq!(GridSendKey::parse("c"), Some(GridSendKey::Corner));
        assert_eq!(
            GridSendKey::parse("c").and_then(GridSendKey::row),
            None,
            "the corner addresses every row, which is not a row"
        );
        // The new prefix does not swallow the neighbouring grammars, and a
        // malformed row-header key decodes to None rather than to a cell.
        assert_eq!(GridSendKey::parse("rx"), None);
        assert_eq!(GridSendKey::parse("r"), None);
        assert_eq!(
            GridSendKey::parse("c1"),
            None,
            "the corner is a whole key, not a prefix"
        );
        assert_eq!(
            GridSendKey::parse("4_2"),
            Some(GridSendKey::Cell { row: 4, col: 2 })
        );
        assert_eq!(
            GridSendKey::parse("h1"),
            Some(GridSendKey::Header { col: 1 })
        );
    }

    #[test]
    fn grid_send_key_row_view_and_rejects_non_grid() {
        assert_eq!(
            GridSendKey::parse("5_2").and_then(GridSendKey::row),
            Some(5)
        );
        // A header key has no row (SelectRows: ignored by a row coordinator).
        assert_eq!(GridSendKey::parse("h2").and_then(GridSendKey::row), None);
        // A group-header key has no row either (the group axis, R892).
        assert_eq!(GridSendKey::parse("g1").and_then(GridSendKey::row), None);
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

    /// The decoded shape every test below asserts against — spelled once so a
    /// new context axis adds a field here rather than a tuple element at each
    /// call.
    fn decoded<'a>(
        key: &'a str,
        event: &'a str,
        modifiers: Modifiers,
        buttons: PointerButtons,
    ) -> SendPayload<'a> {
        SendPayload {
            key,
            event,
            modifiers,
            buttons,
        }
    }

    #[test]
    fn r659_parse_u64_pointer_down_happy_path() {
        let parsed: Option<(u64, SendPayload<'_>)> = parse_send_payload("42:PointerDown");
        assert_eq!(
            parsed,
            Some((42_u64, decoded("42", "PointerDown", NONE, UP)))
        );
    }

    #[test]
    fn r659_parse_usize_arbitrary_event_name() {
        let parsed: Option<(usize, SendPayload<'_>)> = parse_send_payload("0:PointerEnter");
        assert_eq!(
            parsed,
            Some((0_usize, decoded("0", "PointerEnter", NONE, UP)))
        );
    }

    #[test]
    fn r659_missing_colon_separator_returns_none() {
        let parsed: Option<(u64, SendPayload<'_>)> = parse_send_payload("noseparator");
        assert!(parsed.is_none());
    }

    #[test]
    fn r659_empty_event_name_returns_none() {
        let parsed: Option<(u64, SendPayload<'_>)> = parse_send_payload("7:");
        assert!(parsed.is_none());
    }

    #[test]
    fn r659_non_parseable_key_returns_none() {
        let parsed: Option<(u64, SendPayload<'_>)> = parse_send_payload("xx:PointerDown");
        assert!(parsed.is_none());
    }

    #[test]
    fn r781_third_segment_is_the_modifier_token() {
        // R781 retires the pre-R781 "internal colon kept in the event
        // suffix" behaviour: the third `:` segment is the modifier token
        // (event names are colon-free). `sc` -> shift+ctrl.
        let parsed: Option<(u64, SendPayload<'_>)> = parse_send_payload("3:PointerUp:sc");
        assert_eq!(
            parsed,
            Some((3_u64, decoded("3", "PointerUp", SHIFT_CTRL, UP))),
        );
        // The raw splitter agrees (the SSOT both paths decode through).
        assert_eq!(
            split_send_payload("4_2:PointerUp:s"),
            Some(decoded("4_2", "PointerUp", SHIFT, UP)),
        );
        // An unparseable modifier token rejects the whole payload.
        let bad: Option<(u64, SendPayload<'_>)> = parse_send_payload("3:PointerUp:Move");
        assert!(bad.is_none(), "non-scam modifier token is malformed");
    }

    /// R1619 — the **fourth** segment is the held-button set, and the two
    /// context axes are independent: a button-only state spells the modifier
    /// segment empty rather than moving the buttons up a slot, so no state has
    /// two spellings and no decoder has to guess which axis a lone token means.
    #[test]
    fn r1619_fourth_segment_is_the_held_button_set() {
        // Buttons with no modifiers — the drag-select case.
        assert_eq!(
            split_send_payload("3:PointerEnter::l"),
            Some(decoded("3", "PointerEnter", NONE, LEFT)),
        );
        // Both axes at once, and the set is order-insensitive on decode.
        assert_eq!(
            split_send_payload("3:PointerEnter:sc:lr"),
            Some(decoded("3", "PointerEnter", SHIFT_CTRL, LEFT_RIGHT)),
        );
        assert_eq!(
            split_send_payload("3:PointerEnter::rl"),
            Some(decoded("3", "PointerEnter", NONE, LEFT_RIGHT)),
        );
        // A `scam` letter in the button slot is malformed, not a silent drop —
        // the two vocabularies are disjoint and the decoder enforces it.
        assert!(split_send_payload("3:PointerEnter::s").is_none());
        assert!(split_send_payload("3:PointerEnter:l:").is_none());
        // A fifth segment is not swallowed by the fourth: `splitn(4)` puts the
        // remainder in the button slot, where it fails the `lmr` vocabulary.
        assert!(split_send_payload("3:PointerEnter::l:x").is_none());
        // The bare (background) target reaches the button axis too.
        assert_eq!(
            split_send_payload(":PointerEnter::m"),
            Some(decoded("", "PointerEnter", NONE, MIDDLE)),
        );
    }

    /// R1619 — **every** state round-trips through compose -> split, including
    /// the ones whose encoding elides a segment. This is the property the two
    /// halves exist to keep: an encoder that dropped a held-button set, or a
    /// decoder that read the button token as modifiers, breaks exactly here.
    #[test]
    fn r1619_compose_and_split_are_inverses_over_the_whole_context_grid() {
        let keys = [Some("4"), Some("4_2"), None];
        let mods = [NONE, SHIFT, SHIFT_CTRL];
        let buttons = [UP, LEFT, MIDDLE, LEFT_RIGHT];
        for key in keys {
            for m in mods {
                for b in buttons {
                    let wire = compose_send_payload(key, "PointerEnter", m, b);
                    let want_key = key.unwrap_or("");
                    match split_send_payload(&wire) {
                        Some(got) => assert_eq!(
                            got,
                            decoded(want_key, "PointerEnter", m, b),
                            "round-trip {wire:?}"
                        ),
                        // The one non-splitting form is the colon-free bare
                        // event, which is only legal when there is no context
                        // at all (the "None = bare event" decode contract).
                        None => assert!(
                            key.is_none() && m.is_empty() && b.is_empty(),
                            "{wire:?} lost context"
                        ),
                    }
                }
            }
        }
        // Pin the exact spellings the grid asserts round-trip, so a change to
        // the encoding is a deliberate single-site edit rather than a
        // self-consistent drift both halves agree on.
        assert_eq!(
            compose_send_payload(Some("4"), "PointerEnter", NONE, LEFT),
            "4:PointerEnter::l"
        );
        assert_eq!(
            compose_send_payload(Some("4"), "PointerEnter", SHIFT, LEFT),
            "4:PointerEnter:s:l"
        );
        assert_eq!(
            compose_send_payload(Some("4"), "PointerEnter", SHIFT, UP),
            "4:PointerEnter:s"
        );
        assert_eq!(
            compose_send_payload(Some("4"), "PointerEnter", NONE, UP),
            "4:PointerEnter"
        );
        assert_eq!(
            compose_send_payload(None, "PointerEnter", NONE, LEFT),
            ":PointerEnter::l"
        );
        assert_eq!(
            compose_send_payload(None, "PointerEnter", NONE, UP),
            "PointerEnter"
        );
    }

    #[test]
    fn r880_1_compose_round_trips_through_split() {
        // The encode twin and the splitter share one grammar: every
        // composed form decodes back to its inputs, and the bare
        // modifier-free form is the colon-free back-compat wire
        // (split answers None = bare event).
        let ctrl = Modifiers {
            shift: false,
            ctrl: true,
            alt: false,
            meta: false,
        };
        assert_eq!(
            compose_send_payload(Some("4"), "PointerUp", ctrl, UP),
            "4:PointerUp:c"
        );
        assert_eq!(
            split_send_payload("4:PointerUp:c"),
            Some(decoded("4", "PointerUp", ctrl, UP)),
        );
        assert_eq!(
            compose_send_payload(Some("4"), "PointerUp", NONE, UP),
            "4:PointerUp"
        );
        assert_eq!(
            compose_send_payload(None, "PointerUp", ctrl, UP),
            ":PointerUp:c"
        );
        assert_eq!(
            split_send_payload(":PointerUp:c"),
            Some(decoded("", "PointerUp", ctrl, UP))
        );
        assert_eq!(
            compose_send_payload(None, "PointerUp", NONE, UP),
            "PointerUp"
        );
        assert!(
            split_send_payload("PointerUp").is_none(),
            "bare = no-split contract"
        );
    }

    #[test]
    fn r880_empty_key_is_the_bare_modifier_wire() {
        // R880 — `":<EventName>:<token>"` is the background (bare-target)
        // modifier wire a `wants_bare_send_modifiers` opt-in receives: the
        // empty key segment means "no sub-target". Same `:` grammar SSOT,
        // no special-casing in the splitter.
        assert_eq!(
            split_send_payload(":PointerUp:c"),
            Some(decoded(
                "",
                "PointerUp",
                Modifiers {
                    shift: false,
                    ctrl: true,
                    alt: false,
                    meta: false
                },
                UP
            )),
        );
        // A modifier-free bare event stays the colon-free back-compat wire:
        // no `:` at all, so the splitter answers `None` (= bare event).
        assert!(split_send_payload("PointerUp").is_none());
    }

    #[test]
    fn r659_zero_key_is_valid() {
        // `0` parses to all numeric `K`; the helper does not treat it
        // as a sentinel. Filter mode discriminant 0 (Active) and id 0
        // both land here legitimately.
        let parsed: Option<(usize, SendPayload<'_>)> = parse_send_payload("0:PointerDown");
        assert_eq!(
            parsed,
            Some((0_usize, decoded("0", "PointerDown", NONE, UP)))
        );
        let parsed: Option<(u64, SendPayload<'_>)> = parse_send_payload("0:PointerDown");
        assert_eq!(parsed, Some((0_u64, decoded("0", "PointerDown", NONE, UP))));
    }

    #[test]
    fn r659_negative_key_against_unsigned_returns_none() {
        // FromStr::<u64> rejects `-1`, so the helper surfaces `None`
        // without panic — defensive against a stale composite tag
        // from an older protocol revision.
        let parsed: Option<(u64, SendPayload<'_>)> = parse_send_payload("-1:PointerDown");
        assert!(parsed.is_none());
    }

    #[test]
    fn r913_send_activation_key_returns_the_named_key_on_activation() {
        // The named cousin of send_activation_index: the raw key string
        // on the activation edge (a status-bar segment name, not an
        // index). Gate is shared with send_activation_index.
        assert_eq!(send_activation_key("mode:PointerUp"), Some("mode"));
        assert_eq!(
            send_activation_key("encoding:KeyboardActivate"),
            Some("encoding")
        );
        assert_eq!(
            send_activation_key("mode:PointerEnter"),
            None,
            "hover does not activate"
        );
        assert_eq!(
            send_activation_key("mode:PointerDown"),
            None,
            "press does not activate"
        );
        // The index form is the parsed specialization of the key form.
        assert_eq!(send_activation_index("7:PointerUp"), Some(7));
        assert_eq!(send_activation_key("7:PointerUp"), Some("7"));
    }

    #[test]
    fn r912_send_activation_index_gates_on_the_activation_edge() {
        // Only the PointerUp / KeyboardActivate edge yields the index.
        assert_eq!(send_activation_index("3:PointerUp"), Some(3));
        assert_eq!(send_activation_index("3:KeyboardActivate"), Some(3));
        // Hover / press half of the cycle does not activate.
        assert_eq!(send_activation_index("3:PointerEnter"), None);
        assert_eq!(send_activation_index("3:PointerDown"), None);
        assert_eq!(send_activation_index("3:PointerLeave"), None);
        // Non-numeric key / malformed payload.
        assert_eq!(send_activation_index("xx:PointerUp"), None);
        assert_eq!(send_activation_index("PointerUp"), None);
    }

    #[test]
    fn r1231_prefixed_index_strips_and_parses() {
        assert_eq!(prefixed_index("reset3", "reset"), Some(3));
        assert_eq!(prefixed_index("toggle0", "toggle"), Some(0));
        // Wrong prefix / missing index / non-numeric tail.
        assert_eq!(prefixed_index("toggle0", "reset"), None);
        assert_eq!(prefixed_index("reset", "reset"), None);
        assert_eq!(prefixed_index("resetcol", "reset"), None);
        // Composes into a widget value type via `.map`.
        assert_eq!(prefixed_index("elem2", "elem").map(|i| i + 100), Some(102));
    }
}
