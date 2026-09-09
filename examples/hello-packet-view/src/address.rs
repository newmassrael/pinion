//! ★★★★★ R2109 §5.2 §5.11 — **where the filter bar's painted addresses come
//! from.**
//!
//! # What was missing
//!
//! This screen names painted marks with dotted addresses — `pv.filter.query`,
//! `pv.filter.saved.<n>` — and **nothing declared them**. Measured at entry: 38
//! sites in this crate's five modules and 34 across seven walks, 72 in all, and
//! no declaring site anywhere. Every reader re-typed the string: the painter,
//! the accessibility tree, the screen specification, the paint sweep, the
//! judge, and the walks that drive the bar from outside.
//!
//! ⇒ one wrong letter compiles, paints, and makes every query looking for the
//! mark answer **nothing** — quietly. The mark is on the screen and no reader
//! can find it. This is the eleventh family the address campaign has converted
//! and the second (after R2104's toolbar) that had no declaration to ask.
//!
//! # ★ What was already here, and why it was not enough
//!
//! `lib.rs` held `QUERY_TAG` and `SAVED_TAG`, each written where one round
//! happened to need it. That is R2106's finding one screen over: *declaring a
//! corner whenever a round needs one leaves a declared CORNER, not a declared
//! screen* — the other five words went on spelling themselves, and the two
//! consts could not be checked against the parametric chips that hang off one
//! of them.
//!
//! # ★★ The inverse belongs here too
//!
//! A router turning a chip's tag back into its index is a *second* place the
//! prefix is typed, and the one where a mismatch is silent in the other
//! direction: the press lands on nothing and the screen simply does not
//! respond. Address and parse are one pair, so they live together and are
//! tested against each other.
//!
//! # ⚠ What a walk does instead
//!
//! A demo is Python and cannot call this. Its answer is to **read the address
//! off the wire**: the screen's specification publishes the bar's own tag, its
//! fixed seats by word, and the prefix the saved chips hang off, all derived
//! from here, so a walk names a word and is handed the address the paint used.

/// ★★★★★ R2109 — the tag the **filter bar itself** is painted under.
///
/// The bar, not one of its seats. Carried WITHOUT the separator because that is
/// what the mark is called; [`FILTER_SEAT`] is the form a reader composes onto.
///
/// ⚠ Published beside the seat roster rather than as its first row, which is a
/// decision the address recovery forces (R2104): a roster's prefix is recovered
/// by taking a row's own key off the end of its address, and the bar's address
/// is the prefix WITHOUT the separator — a row for it would hand every later
/// reader a prefix that composes `pv.filterquery`.
pub const FILTER: &str = "pv.filter";

/// [`FILTER`] with the separator its seats hang off.
///
/// A `&'static str` because the judge's family scan needs one; the gate drives
/// it against [`FILTER`] so the two forms cannot drift.
pub const FILTER_SEAT: &str = "pv.filter.";

/// The box a person types a filter expression into.
pub const FILTER_QUERY: &str = "pv.filter.query";

/// The text run INSIDE that box — a different mark from the box, which is why
/// the specification names it separately.
pub const FILTER_QUERY_TEXT: &str = "pv.filter.query-text";

/// What the bar says the filter kept.
pub const FILTER_COUNT: &str = "pv.filter.count";

/// What the bar says is wrong with the expression, when something is.
pub const FILTER_FAULT: &str = "pv.filter.fault";

/// The row of saved filters — the CONTAINER, whose members are addressed under
/// [`FILTER_SAVED_SEAT`].
///
/// ⚠ This word is the one place the two shapes meet: `saved` is a fixed seat of
/// the bar AND the stem its chips hang off. The pair is spelled once here and
/// the gate holds `FILTER_SAVED_SEAT` to `FILTER_SAVED` plus a dot, so a reader
/// cannot end up with a container address and a member prefix that disagree.
pub const FILTER_SAVED: &str = "pv.filter.saved";

/// ★★★★★ R2109 — every FIXED word this bar addresses, beside its address.
///
/// The roster a reader classifies by and the wire publishes. The saved chips
/// are NOT here: their population is whatever the person has saved, so they are
/// published as a prefix and a member's key is appended — [`FILTER_SAVED_SEAT`],
/// which is the shape `form_parts` and the palette's four families take.
pub const FILTER_SEATS: &[(&str, &str)] = &[
    ("query", FILTER_QUERY),
    ("query-text", FILTER_QUERY_TEXT),
    ("count", FILTER_COUNT),
    ("fault", FILTER_FAULT),
    ("saved", FILTER_SAVED),
];

/// The address of the fixed seat `word`.
///
/// ⚠ Composed rather than looked up, so a word this bar does not address
/// produces an address nothing carries — a lookup that answers nothing rather
/// than a wrong mark, which is the safe direction. [`filter_word`] is the
/// inverse and it is the one that knows the roster.
#[must_use]
pub fn filter(word: &str) -> String {
    format!("{FILTER_SEAT}{word}")
}

/// The fixed word an address names, or `None` when the tag is not one of them.
///
/// ★ [`filter`]'s inverse. The roster is consulted here and not there, because
/// *what this bar addresses* is a closed question in this direction and an open
/// one in the other.
///
/// ⚠ The bar's own tag answers `None`: it is the container, not a seat, and a
/// prefix that swallowed the separator would resolve a reader asking about the
/// BAR to whichever seat sorted first.
#[must_use]
pub fn filter_word(tag: &str) -> Option<&'static str> {
    FILTER_SEATS
        .iter()
        .find(|(_, seat)| *seat == tag)
        .map(|(word, _)| *word)
}

/// The prefix every SAVED FILTER chip is painted under.
///
/// [`FILTER_SAVED`] with its separator — the parametric half of this family.
pub const FILTER_SAVED_SEAT: &str = "pv.filter.saved.";

/// [`FILTER_SAVED_SEAT`] with the population's placeholder, for a specification
/// table whose rows must be `&'static str`.
///
/// A declaration rather than a derivation because a `const` cannot format; the
/// gate drives it against [`filter_saved`] so a table that stopped agreeing
/// with what the painter composes is a test failure rather than a table
/// pointing at nothing.
pub const FILTER_SAVED_TEMPLATE: &str = "pv.filter.saved.{}";

/// The address of the saved-filter chip at `index`.
#[must_use]
pub fn filter_saved(index: usize) -> String {
    format!("{FILTER_SAVED_SEAT}{index}")
}

/// The index a saved chip's address names, or `None` when the tag is not one.
///
/// ★★★★★ [`filter_saved`]'s inverse, and the reason this module holds a pair
/// rather than a composer: the router reads a press back through it, and a
/// prefix typed there that disagreed by one letter would make every chip
/// pressable-looking and inert. `None` for a non-numeric tail is the honest
/// answer — `pv.filter.saved.all` is not a chip.
#[must_use]
pub fn filter_saved_index(tag: &str) -> Option<usize> {
    tag.strip_prefix(FILTER_SAVED_SEAT)?.parse().ok()
}
