//! ★★★★★ R2121 §5.2 §5.11 — **where a painted mark's address comes from, for
//! the sessions section.**
//!
//! # What was missing
//!
//! Every mark this screen paints is named with a dotted address under `sv.`,
//! and **nothing declared one**. Measured at entry: **seventy-one string
//! literals**, all inside this one crate — the painter, the hit test's inverse,
//! the accessibility roster, the conformance judge, the paint gates and the
//! unit tests, each composing the same addresses again. A wrong letter in any
//! of them compiles, paints, and makes every query that looks for the mark
//! answer nothing.
//!
//! ⚠ The population is unusually clean and that is measured rather than
//! assumed: the shell spells **zero** and the walks spell **zero**, so unlike
//! the four screens converted before this one there is no second or third
//! population to chase. What that buys is the end state in one round — the
//! namespace gate below can assert **zero literals outside this file**, which
//! `hello-packet-view` needed seven instalments to reach.
//!
//! # ★★★★★ The roster was already here, and it was not the addresses
//!
//! [`crate::spec::LIST`] and [`crate::spec::DETAIL`] name every part of both
//! panes — key and title — and the conformance judge and the accessibility
//! roster have both derived from them since R1948. What those tables do NOT
//! publish is the ADDRESS, so each painter composed `sv.<pane>.<key>` by hand
//! from a roster that was sitting right there. That is R2116's finding one turn
//! sharper: *a table that publishes a key and not the address that key names
//! makes every reader assemble it*, and here the readers were in the same
//! crate as the table.
//!
//! ⇒ this module holds the composition, the part rosters ARE the specification's
//! (`spec::LIST` / `spec::DETAIL`, not a second list), and
//! `r2121_no_module_but_the_declaration_spells_this_screens_namespace` is what
//! says nobody re-spells it.
//!
//! # ⚠ Two stems carry two vocabularies each, so the inverses are disjoint
//!
//! `sv.row.` names a ROW (`sv.row.<session>`) and a CELL
//! (`sv.row.<session>.<column>`); `sv.detail.` names a PART (`sv.detail.badge`)
//! and, one level down, that part's own children (`sv.detail.badge.box`) and
//! entries (`sv.detail.timeline.3`). Stripping the prefix cannot tell them
//! apart, which is R2118's measured lesson on the node lab's wire family: the
//! reader that stops at the prefix reads a cell as a row and a child as a part,
//! **in both directions and silently**.
//!
//! So [`row_id`] REFUSES a cell and [`cell_of`] refuses a row; [`detail_part`]
//! and [`list_part`] refuse anything with a further separator in it. A caller
//! that genuinely wants *the row a cell belongs to* — the hit test does, and
//! says why — asks for both and says so at the call site. Order is a property
//! of one caller; refusal is a property of the address.

use crate::spec;

/// The namespace every mark this screen paints begins with.
///
/// Carried with its separator, which is what a membership test wants: a bare
/// `sv` would also claim a mark called `svelte`.
pub const NAMESPACE: &str = "sv.";

/// The root group every mark of this screen hangs under.
pub const ROOT: &str = "sv.root";

/// The list pane's own group.
pub const LIST: &str = "sv.list";

/// The detail pane's own group.
pub const DETAIL: &str = "sv.detail";

/// Where a resting description is painted and announced.
pub const TIP: &str = "sv.tip";

/// The grid of rows — the list's own scrollable body.
///
/// ★ The one PART whose address a caller needs as a `&'static str` rather than
/// a composed `String`: it is an accessibility focus token compared by
/// identity, and two paint gates look it up by name. A `const` cannot call
/// [`list`], so this is a declaration held against the derivation by
/// `r2121_every_sessions_address_is_derived` — the arrangement
/// `address::FORM_CONTROL_TEMPLATE` has had on the node lab since R2050, and
/// for the same reason.
pub const ROWS: &str = "sv.list.rows";

/// [`LIST`] with the separator its parts hang off.
///
/// A `&'static str` because the conformance judge needs one to scan the painted
/// regions with; the gate drives it against [`LIST`], so the two cannot drift.
pub const LIST_SEAT: &str = "sv.list.";

/// [`DETAIL`] with the separator its parts hang off.
pub const DETAIL_SEAT: &str = "sv.detail.";

/// Where a session row is addressed.
pub const ROW_SEAT: &str = "sv.row.";

/// Where a status chip is addressed.
pub const CHIP_SEAT: &str = "sv.chip.";

/// The words a part's own children are addressed under.
///
/// ★ A CLOSED set, and that is the point: `box`, `pill`, `head` and `dot` are
/// the four things a part of this screen puts inside itself, and [`child`]
/// refuses a fifth. An open suffix here would make `detail_part` and its
/// siblings unable to say where a part's address ends — which is the ambiguity
/// this module exists to remove rather than one to introduce.
pub const CHILD_WORDS: &[&str] = &["box", "pill", "head", "dot"];

/// The address the list pane paints `part` under.
///
/// `part` is one of [`spec::LIST`]'s keys — the painter names the part, this
/// composes the address, and nobody does both.
#[must_use]
pub fn list(part: &str) -> String {
    format!("{LIST_SEAT}{part}")
}

/// The address the detail pane paints `part` under.
#[must_use]
pub fn detail(part: &str) -> String {
    format!("{DETAIL_SEAT}{part}")
}

/// The address a session row is painted under.
#[must_use]
pub fn row(session: &str) -> String {
    format!("{ROW_SEAT}{session}")
}

/// The address one CELL of a row is painted under.
///
/// ★★★★★ The key is a JOIN of two vocabularies — the session and the column —
/// and it is composed here rather than at each reader, which is R2111's
/// measured case on the capture viewer's grid: the painter and the population
/// expander each spelled `{row}.{col}` and nothing compared the two.
#[must_use]
pub fn cell(session: &str, column: &str) -> String {
    format!("{ROW_SEAT}{session}.{column}")
}

/// The address a status chip is painted under.
#[must_use]
pub fn chip(key: &str) -> String {
    format!("{CHIP_SEAT}{key}")
}

/// The address one entry of a banded detail part is painted under.
///
/// The timeline's steps and the channel list's rows, whose keys are ordinals
/// rather than words.
#[must_use]
pub fn entry(part: &str, n: usize) -> String {
    format!("{DETAIL_SEAT}{part}.{n}")
}

/// The address a part's own child is painted under.
///
/// # Panics
///
/// If `word` is not one of [`CHILD_WORDS`]. That is a defect in the caller
/// rather than a state the screen reaches, and refusing it here is what keeps
/// the part inverses able to say where a part's address ends.
#[must_use]
pub fn child(seat: &str, word: &str) -> String {
    assert!(
        CHILD_WORDS.contains(&word),
        "`{word}` is not one of this screen's child words {CHILD_WORDS:?} — a \
         new one has to be declared before it can be addressed, or the part \
         inverses stop being able to tell a part from what is inside it"
    );
    format!("{seat}.{word}")
}

/// Whether `tag` is a mark of this screen at all.
#[must_use]
pub fn ours(tag: &str) -> bool {
    tag.starts_with(NAMESPACE)
}

/// The list part `tag` names, or `None`.
///
/// ⚠ Refuses a part's CHILD (`sv.list.filter.box`): the answer to *which part
/// is this* for a child is the child's own business, and a reader handed
/// `filter.box` as a part key would look it up in [`spec::LIST`] and find
/// nothing — quietly.
#[must_use]
pub fn list_part(tag: &str) -> Option<&str> {
    bare(tag.strip_prefix(LIST_SEAT)?)
}

/// The detail part `tag` names, or `None`.
#[must_use]
pub fn detail_part(tag: &str) -> Option<&str> {
    bare(tag.strip_prefix(DETAIL_SEAT)?)
}

/// The session `tag`'s row is about, or `None`.
///
/// ⚠ Refuses a CELL. A caller that means *the row this cell belongs to* asks
/// [`cell_of`] as well and says so — see this module's header.
#[must_use]
pub fn row_id(tag: &str) -> Option<&str> {
    bare(tag.strip_prefix(ROW_SEAT)?)
}

/// The session and column `tag`'s cell is about, or `None`.
#[must_use]
pub fn cell_of(tag: &str) -> Option<(&str, &str)> {
    let rest = tag.strip_prefix(ROW_SEAT)?;
    let (session, column) = rest.split_once('.')?;
    (!session.is_empty() && !column.is_empty() && !column.contains('.'))
        .then_some((session, column))
}

/// The chip key `tag` names, or `None`.
#[must_use]
pub fn chip_key(tag: &str) -> Option<&str> {
    bare(tag.strip_prefix(CHIP_SEAT)?)
}

/// A key with nothing further hanging off it, or `None`.
///
/// The one rule the four inverses above share, written once: a remainder that
/// still holds a separator is not a key of that family, and an EMPTY remainder
/// is the family's own stem rather than a member of it. R2112 paid for the
/// second half on the decode tree — `Some("")` there let a gate count a stem as
/// an unnamed member and report the family fully claimed.
fn bare(rest: &str) -> Option<&str> {
    (!rest.is_empty() && !rest.contains('.')).then_some(rest)
}

/// Every part the two panes declare, beside the address it is painted at.
///
/// Derived from [`spec::LIST`] and [`spec::DETAIL`] rather than listed again:
/// the specification is what says which parts exist, and a second roster here
/// would be exactly the drift this module removes.
///
/// ⚠⚠ Each row carries the [`spec::PartSpec`] ITSELF rather than its key, and
/// that is not tidiness — **the two panes share key words**. Both name a part
/// `title`, and a caller handed `("title", "sv.detail.title")` that went back
/// to the tables to look the title up would find the LIST's row first and name
/// the detail pane's heading "Sessions". The first draft of this function did
/// exactly that; the pairing is kept here so no caller can lose it.
#[must_use]
pub fn parts() -> Vec<(&'static spec::PartSpec, String)> {
    spec::LIST
        .iter()
        .map(|p| (p, list(p.key)))
        .chain(spec::DETAIL.iter().map(|p| (p, detail(p.key))))
        .collect()
}
