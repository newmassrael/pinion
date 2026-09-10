//! ★★★★★ R2051 §5.2 §5.11 — **where this application's own painted addresses
//! come from.**
//!
//! The third instalment of an address debt. R2049 gave a screen's own family a
//! declaring site; R2050 found the next family's owner was the FRAMEWORK, not
//! the screen, and put the declaration where the composition happens. This one
//! is the application's: the rail is chrome this binary paints, and its seats
//! are addressed by a prefix that was typed at every reader — the painter, the
//! router, the description index, the accessibility roster, the specification
//! tables, the gates, and thirteen walks.
//!
//! ⇒ one wrong letter compiles, paints, and makes every query looking for the
//! seat answer nothing, which reads as *the rail did not paint it*.
//!
//! # ★ Why this family is the one the debt singled out
//!
//! Its clause tying this debt to the structural gap is that a rail seat is ONE
//! screen's address and it was being spelled by more than one binary. Measured
//! at R2049 that had already stopped being true — the integration campaign took
//! the other three — so what is left is one application spelling its own
//! address 28 times in its own source. That is the ordinary case, and it is
//! what this closes.
//!
//! # ⚠ What a walk does instead
//!
//! A walk is Python and cannot call this, so the application publishes each
//! seat's address beside the seat and a walk is handed it.

/// The prefix every rail seat address carries.
pub const RAIL: &str = "shell.rail.";

/// [`RAIL`] with the population's placeholder, for a specification table whose
/// rows must be `&'static str`. A test holds it against the derivation.
pub const RAIL_TEMPLATE: &str = "shell.rail.{}";

/// The rail's account block, as a `&'static str` for a specification table.
///
/// Held against [`rail_account`] by a test, the same way [`RAIL_TEMPLATE`] is
/// held against [`RAIL`].
pub const RAIL_ACCOUNT: &str = "shell.rail.account";

/// The address of the rail seat for `key`.
/// ★ Takes anything that reads as a string, because the seat rosters this is
/// called over hold their keys differently — the specification's are `&'static
/// str` and the canon's arrive as `Cow` — and a caller should not have to know
/// which it is holding to spell an address.
#[must_use]
pub fn rail_seat(key: impl AsRef<str>) -> String {
    format!("{RAIL}{}", key.as_ref())
}

/// The seat a rail address names, or `None` when the tag is not one.
///
/// ★★ The inverse, here rather than at the router. R2049's lesson: a parse
/// written against a separately-typed prefix is the second speller, and its
/// mismatch is silent the other way round — the press lands on nothing and the
/// application simply does not navigate.
#[must_use]
pub fn rail_seat_key(tag: &str) -> Option<&str> {
    tag.strip_prefix(RAIL)
}

/// The rail's account block, which belongs to no seat.
///
/// Its own function rather than a caller writing `rail_seat("account")`,
/// because it is NOT a seat: it takes no navigation and the roster does not
/// hold it. A reader that treated it as one would count the rail's seats wrong.
#[must_use]
pub fn rail_account() -> String {
    RAIL_ACCOUNT.to_owned()
}

// --- the palette --------------------------------------------------------
//
// ★★★★★ R2110 §5.2 §5.11 — **the thirteenth instalment, and the largest
// family this campaign has converted.**
//
// Measured at entry, before a letter was moved: **91 sites in this crate's
// five modules and 33 across nine walks, 124 in all**, against a rail whose
// conversion at R2051 was 28 and a filter bar whose conversion at R2109 was
// 72. Nothing declared any of it. `main.rs` held four consts — the part stem,
// the head stem and the head's two words — each written where one round
// happened to need it, which is R2106's finding said again a screen over: *a
// declared CORNER is not a declared screen*. The other nine words went on
// spelling themselves, and the four could not be checked against the three
// PARAMETRIC families that hang off the same stem.
//
// # ★ Why this family is shaped differently from the two before it
//
// The rail is one roster. The filter bar is a roster plus one prefix. The
// palette is **seven fixed seats and three families**: the catalogue entries,
// the group headings, and a row's four parts — and the third is parametric in
// TWO axes, the part word and the kind. So the declaration here is a roster
// beside three composers rather than a roster with a tail.
//
// # ★★ The entries hang off the panel's OWN stem, and that is the sharp edge
//
// A catalogue entry is the panel's stem with the kind and nothing between. So
// the fixed seats, the sections, the parts and the entries all sit in one flat
// namespace, and the inverse [`palette_entry_kind`] is the only reader that can
// tell them apart. It refuses a tail carrying a separator and it refuses the
// fixed words, because a section heading read as *the entry whose kind is
// `section.<something>`* is a plausible wrong answer, and this debt is made of
// those.
//
// # ⚠ What a walk does instead
//
// A walk is Python and cannot call any of this, so the panel publishes its own
// tag, its fixed seats by word, and the three families' prefixes, and the two
// rosters that already cross the wire — the catalogue and the sections — each
// gained the address of its own row. R2109.1's rule holds: what a walk is
// handed for a member is a **composer**, never a stem it glues onto.

/// ★★★★★ R2110 — the tag the palette **panel itself** is painted under.
///
/// The panel, not one of its seats, so it carries no separator; [`PALETTE_SEAT`]
/// is the form a reader composes onto. Published beside the seat roster rather
/// than as its first row, for R2104's reason: a roster's prefix is recovered by
/// taking a row's key off the end of its address, and a row for the panel would
/// hand every later reader a prefix with no separator in it at all.
pub const PALETTE: &str = "shell.palette";

/// [`PALETTE`] with the separator everything under it hangs off.
///
/// A `&'static str` because the judge's family scan and the paint sweep's
/// family filter both need one; the gate drives it against [`PALETTE`] so the
/// two forms cannot drift.
pub const PALETTE_SEAT: &str = "shell.palette.";

/// The stem the panel's own heading is tagged under.
///
/// Its own stem rather than a flat suffix, because `PaintedRegions::parts_under`
/// takes the tags whose remainder holds no further separator — so the heading
/// would sit beside the entries in one flat family unless it is given a stem of
/// its own, and a specification of the catalogue would then have to name lines
/// that are not catalogue entries.
pub const PALETTE_HEAD: &str = "shell.palette.head.";

/// The control that puts the palette away.
pub const PALETTE_HEAD_FOLD: &str = "shell.palette.head.fold";

/// What the panel calls itself.
pub const PALETTE_HEAD_TITLE: &str = "shell.palette.head.title";

/// The one line under it saying how a widget gets onto the board.
pub const PALETTE_HEAD_HINT: &str = "shell.palette.head.hint";

/// What the panel says is already on the board.
pub const PALETTE_PLACED: &str = "shell.palette.placed";

/// What the panel says is booked for a later release.
pub const PALETTE_RESERVED: &str = "shell.palette.reserved";

/// The strip that stands where the panel was, once it has been put away.
pub const PALETTE_STRIP: &str = "shell.palette.strip";

/// The grip glyph drawn on that strip.
pub const PALETTE_STRIP_GRIP: &str = "shell.palette.strip.grip";

/// ★★★★★ R2110 — every FIXED word this panel addresses, beside its address.
///
/// The roster a reader classifies by and the wire publishes. The entries, the
/// group headings and a row's parts are NOT here: their populations are the
/// catalogue, the section list and the product of a row's four parts with the
/// catalogue, so each is published as a prefix and reached through its composer.
///
/// ⚠ The words carry their own separators (`head.fold`, `strip.grip`) because
/// that is what the address says. A word list that flattened them would be a
/// second naming scheme, and a reader recovering the address by appending would
/// then have to know which words were flattened and which were not.
///
/// ⚠⚠ **No composer for this half, and that is measured rather than an
/// omission.** Every caller in this crate arrives holding the SEAT — the
/// painter, the accessibility roster and the hit router each name one — so a
/// `word -> address` function would have no reader here at all. The one place a
/// word arrives from elsewhere is the paint sweep, walking the dashboard
/// specification's own heading roster, and it composes onto [`PALETTE_HEAD`],
/// which carries its own separator. The half that genuinely needs composers is
/// the WALKS', and they get them on the wire.
pub const PALETTE_SEATS: &[(&str, &str)] = &[
    ("head.fold", PALETTE_HEAD_FOLD),
    ("head.title", PALETTE_HEAD_TITLE),
    ("head.hint", PALETTE_HEAD_HINT),
    ("placed", PALETTE_PLACED),
    ("reserved", PALETTE_RESERVED),
    ("strip", PALETTE_STRIP),
    ("strip.grip", PALETTE_STRIP_GRIP),
];

/// The fixed word an address names, or `None` when the tag is not one of them.
///
/// ★ The inverse of the roster, and [`palette_entry_kind`]'s first refusal.
/// The roster is consulted here because *what this panel addresses by a fixed
/// word* is a closed question in this direction and an open one in the other.
#[must_use]
pub fn palette_word(tag: &str) -> Option<&'static str> {
    PALETTE_SEATS
        .iter()
        .find(|(_, seat)| *seat == tag)
        .map(|(word, _)| *word)
}

/// [`PALETTE_SEAT`] with the population's placeholder, for a specification
/// table whose rows must be `&'static str`.
///
/// A declaration rather than a derivation because a `const` cannot format; the
/// gate drives it against [`palette_entry`] so a table that stopped agreeing
/// with what the painter composes is a test failure rather than a table
/// pointing at nothing.
pub const PALETTE_ENTRY_TEMPLATE: &str = "shell.palette.{}";

/// The address of the catalogue row for `kind`.
///
/// ★ Takes anything that reads as a string, for [`rail_seat`]'s reason: the
/// rosters this is called over hold their keys differently — the specification's
/// are `&'static str` and the canon's arrive as `Cow` — and a caller should not
/// have to know which it is holding to spell an address.
#[must_use]
pub fn palette_entry(kind: impl AsRef<str>) -> String {
    format!("{PALETTE_SEAT}{}", kind.as_ref())
}

/// The catalogue kind an address names, or `None` when the tag is not an entry.
///
/// ★★★★★ [`palette_entry`]'s inverse, and the reader that makes this family
/// legible at all. Every fixed seat, every section heading and every row part
/// also begins with [`PALETTE_SEAT`], so a bare `strip_prefix` answers `Some`
/// for all of them — a heading read as a KIND, which is a plausible wrong
/// answer rather than a loud one.
///
/// Two refusals, and both are needed: a tail carrying a separator is a member
/// of one of the three deeper families, and a tail that is a fixed word is a
/// seat. Neither is a catalogue entry.
#[must_use]
pub fn palette_entry_kind(tag: &str) -> Option<&str> {
    let kind = tag.strip_prefix(PALETTE_SEAT)?;
    (!kind.contains('.') && palette_word(tag).is_none()).then_some(kind)
}

/// The prefix every group heading is painted under.
pub const PALETTE_SECTION: &str = "shell.palette.section.";

/// [`PALETTE_SECTION`] with the population's placeholder — see
/// [`PALETTE_ENTRY_TEMPLATE`].
pub const PALETTE_SECTION_TEMPLATE: &str = "shell.palette.section.{}";

/// The address of the heading for the group `key`.
///
/// ⚠ **No inverse beside it, and that is measured rather than forgotten.** The
/// rail has one because a press on a seat is routed back through it; nothing
/// routes a press on a group heading — it is a grouping a reader descends
/// through, not a control. The parse this family does need is the framework's
/// own `parts_as_read`, which takes [`PALETTE_SECTION`] and strips it, so the
/// prefix a reader hands it is still declared in one place.
#[must_use]
pub fn palette_section(key: impl AsRef<str>) -> String {
    format!("{PALETTE_SECTION}{}", key.as_ref())
}

/// The prefix every part of every palette row is painted under.
pub const PALETTE_PART: &str = "shell.palette.part.";

/// The prefix the parts called `word` are painted under, across the catalogue.
///
/// ★★ Part first — the word, then the kind — because the kind is the address
/// and the part is what is drawn of it, which is the convention
/// `painted_surface_of` reads. The row itself keeps the entry address, so a
/// row's own tag and its parts' cannot be confused for each other by a
/// separator count.
#[must_use]
pub fn palette_part_prefix(word: &str) -> String {
    format!("{PALETTE_PART}{word}.")
}

/// The address of the `word` part of the catalogue row for `kind`.
#[must_use]
pub fn palette_part(word: &str, kind: &str) -> String {
    format!("{PALETTE_PART}{word}.{kind}")
}

/// The swatch part's specification template — see [`PALETTE_PART_TEMPLATES`].
pub const PALETTE_PART_SWATCH_TEMPLATE: &str = "shell.palette.part.swatch.{}";

/// The name part's specification template.
pub const PALETTE_PART_NAME_TEMPLATE: &str = "shell.palette.part.name.{}";

/// The one-line part's specification template.
pub const PALETTE_PART_GIST_TEMPLATE: &str = "shell.palette.part.gist.{}";

/// The trailing seat's specification template.
pub const PALETTE_PART_VERB_TEMPLATE: &str = "shell.palette.part.verb.{}";

/// ★★★★★ R2110 — the four part templates beside the words they are for.
///
/// ⚠ This is the one roster in this module that is a SECOND copy of something:
/// the parts a palette row carries are the behaviour reference's, read out of
/// `docs/analyzer-board-spec.json`, and a `const` cannot read a file. So the
/// gate holds these four words against that document's own list — the copy is
/// declared, checked, and cannot drift silently, which is the most a
/// specification table of `&'static str` allows.
pub const PALETTE_PART_TEMPLATES: &[(&str, &str)] = &[
    ("swatch", PALETTE_PART_SWATCH_TEMPLATE),
    ("name", PALETTE_PART_NAME_TEMPLATE),
    ("gist", PALETTE_PART_GIST_TEMPLATE),
    ("verb", PALETTE_PART_VERB_TEMPLATE),
];
