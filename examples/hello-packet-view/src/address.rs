//! ★★★★★ R2109 §5.2 §5.11 — **where this screen's painted addresses come
//! from.**
//!
//! R2109 opened this module for the filter bar; R2111 gave it the message
//! grid, which is the largest family of the capture viewer and the largest this
//! address campaign has converted. The grid's own entry is further down, under
//! *the message grid*.
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

// ─── the message grid ───────────────────────────────────────────────────────
//
// ★★★★★ R2111 — the capture viewer's largest family, and the first this
// campaign has converted that is a GRID rather than a bar: three fixed seats,
// two parametric rosters (columns, rows), a parametric family over the PRODUCT
// of two of them (cells), and a third axis hanging off a row's own address (the
// annotations that share the name column).
//
// Measured at entry: **78 sites in this crate's five modules**, 32 across ten
// walks and one in the shell that mounts this screen as a page — 111 in all,
// with nothing declaring any of them. `pv.list.cell.*` alone is the largest
// single painted site in the whole application.
//
// ⚠ Two shapes meet here that the filter bar did not have:
//
// * a cell's key is a JOIN of two indexes, so the separator between them is a
//   second thing that can be typed differently in two places — and it was, in
//   the painter and in the specification's population expander.
// * an annotation hangs off a ROW's address rather than the grid's, which makes
//   the grid's own inverse answerable wrongly: `pv.list.row.2.linked` must not
//   read back as row 2, or a press on an annotation would select a message.
//   The gate pins both directions.

/// ★★★★★ R2111 — the tag the **message grid itself** is painted under.
///
/// The grid, not one of its seats, and carried WITHOUT the separator for
/// [`FILTER`]'s reason: [`LIST_SEAT`] is the form a reader composes onto.
pub const LIST: &str = "pv.list";

/// [`LIST`] with the separator every member of this family hangs off.
///
/// The whole family's stem, which is what a reader asking *is this mark part of
/// the message grid* needs — the shell's own run-geometry gate asks exactly
/// that, across the grid's headings, cells and annotations at once.
pub const LIST_SEAT: &str = "pv.list.";

/// The scrolling body the grid's rows are laid out inside.
///
/// A seat rather than the grid itself: the viewport clips, and what a reader
/// lands on is what is inside it.
pub const LIST_BODY: &str = "pv.list.body";

/// The column-heading ROW — a pointer-transparent container over the head
/// strip, focusable, so the accessibility tree's header row and the paint's are
/// one thing.
///
/// ⚠ This carried, where it was declared before R2111, the note that its doc
/// once said *"nothing paints it"* — and that sentence was the defect: a row a
/// reader is told to descend through, with no painted node to stand on, is a row
/// no keyboard can enter. R2061 grew the container. Kept here because the note
/// belongs with the address, and the address is here now.
pub const LIST_HEADER: &str = "pv.list.header";

/// The band painted behind the message a reader has open.
///
/// Decorative: the row itself announces that it is selected, and this is how a
/// sighted reader is told the same fact.
pub const LIST_SELECTED: &str = "pv.list.selected";

/// ★★★★★ R2111 — every FIXED word this grid addresses, beside its address.
///
/// The roster a reader classifies by and the wire publishes. The headings, rows
/// and cells are NOT here: their populations are the capture's, so each is
/// published as a prefix and a member's key is appended.
pub const LIST_SEATS: &[(&str, &str)] = &[
    ("body", LIST_BODY),
    ("header", LIST_HEADER),
    ("selected", LIST_SELECTED),
];

/// The address of the fixed seat `word`.
///
/// ⚠ Composed rather than looked up, for [`filter`]'s reason. Its readers are
/// the gate — which drives every const above through it, so the three are
/// shadows of one rule rather than three spellings — and [`list_word`], which
/// is the direction where *what this grid addresses* is a closed question.
#[must_use]
pub fn list(word: &str) -> String {
    format!("{LIST_SEAT}{word}")
}

/// The fixed word an address names, or `None` when the tag is not one of them.
///
/// ★ [`list`]'s inverse. The round trip through the pair is what detects a
/// roster carrying one tag under two words, which neither direction finds
/// alone: a duplicate resolves to whichever word sorted first and the second
/// word's trip comes back wrong.
///
/// ⚠ The grid's own tag answers `None` — container and content, [`filter_word`]'s
/// argument verbatim.
#[must_use]
pub fn list_word(tag: &str) -> Option<&'static str> {
    LIST_SEATS
        .iter()
        .find(|(_, seat)| *seat == tag)
        .map(|(word, _)| *word)
}

/// The prefix every COLUMN HEADING is painted under.
pub const LIST_HEAD_SEAT: &str = "pv.list.head.";

/// [`LIST_HEAD_SEAT`] with the population's placeholder, for a specification
/// table whose rows must be `&'static str`.
pub const LIST_HEAD_TEMPLATE: &str = "pv.list.head.{}";

/// The address of the column heading at `n`.
#[must_use]
pub fn list_head(n: usize) -> String {
    format!("{LIST_HEAD_SEAT}{n}")
}

/// The column a heading's address names, or `None` when the tag is not one.
///
/// ★ [`list_head`]'s inverse, and the one the hit router reaches for first:
/// R1829 found that a heading and a row share their address up to the family
/// and diverge only after it, so the order of the arms there is load-bearing.
/// With each prefix declared once, *which* arm matches is decided by the
/// declaration rather than by how carefully two strings were typed.
#[must_use]
pub fn list_head_index(tag: &str) -> Option<usize> {
    tag.strip_prefix(LIST_HEAD_SEAT)?.parse().ok()
}

/// The prefix every MESSAGE ROW is painted under.
pub const LIST_ROW_SEAT: &str = "pv.list.row.";

/// [`LIST_ROW_SEAT`] with the population's placeholder.
pub const LIST_ROW_TEMPLATE: &str = "pv.list.row.{}";

/// The address of the message row whose index in the capture is `n`.
///
/// ⚠ `n` is the row's IDENTITY, not where it currently sits: a query hides
/// rows, so the row drawn third is not row three. The screen's own `list_row`
/// is the other fact — the RECTANGLE a visual position occupies — and the two
/// take different numbers. They return different types, so the compiler refuses
/// the confusion rather than leaving it to a reader.
#[must_use]
pub fn list_row(n: usize) -> String {
    format!("{LIST_ROW_SEAT}{n}")
}

/// The message a row's address names, or `None` when the tag is not one.
///
/// ★★★★★ [`list_row`]'s inverse, and the one with a second refusal to make: an
/// ANNOTATION hangs off a row's own address (`…row.2.linked`), so a parse that
/// only stripped the prefix and took what it could would read an annotation as
/// its row. A press on the exchange marker would then select a message. `None`
/// is the honest answer because the whole tail has to be the index.
#[must_use]
pub fn list_row_index(tag: &str) -> Option<usize> {
    tag.strip_prefix(LIST_ROW_SEAT)?.parse().ok()
}

/// The prefix every GRID CELL is painted under.
pub const LIST_CELL_SEAT: &str = "pv.list.cell.";

/// [`LIST_CELL_SEAT`] with the population's placeholder.
///
/// The placeholder stands for the whole KEY — a cell's key is two indexes
/// joined, not one number — which is why [`list_cell_key`] exists beside it.
pub const LIST_CELL_TEMPLATE: &str = "pv.list.cell.{}";

/// What joins a cell's two indexes.
///
/// ★★★★★ Declared because it was typed twice and nothing compared them: the
/// painter composed a cell's address and the specification's population
/// expander composed the same key to enumerate the family. Two spellings of one
/// separator is this debt at the narrowest it gets — and the narrowest is the
/// one no reader would think to check.
pub const LIST_CELL_JOIN: &str = "_";

/// The KEY of the cell at `row`, `column` — the part a population expands to,
/// without the family prefix.
///
/// Separate from [`list_cell`] because the specification names the family by a
/// template and enumerates it by key, so the key is a fact on its own.
#[must_use]
pub fn list_cell_key(row: usize, column: usize) -> String {
    format!("{row}{LIST_CELL_JOIN}{column}")
}

/// The address of the cell at `row`, `column`.
#[must_use]
pub fn list_cell(row: usize, column: usize) -> String {
    format!("{LIST_CELL_SEAT}{}", list_cell_key(row, column))
}

/// The row and column a cell's address names, or `None` when the tag is not a
/// cell of this grid.
///
/// ★ [`list_cell`]'s inverse, and the hit router's: a press on a cell is a
/// press on its row, so this is read for the row and the column is the half
/// this screen cannot yet act on — registered there rather than invented.
#[must_use]
pub fn list_cell_at(tag: &str) -> Option<(usize, usize)> {
    let (row, column) = tag
        .strip_prefix(LIST_CELL_SEAT)?
        .split_once(LIST_CELL_JOIN)?;
    Some((row.parse().ok()?, column.parse().ok()?))
}

/// The exchange this message is half of.
pub const LIST_ROW_LINKED: &str = "linked";

/// What a reader is told is out of band about this message.
pub const LIST_ROW_NOTE: &str = "note";

/// That this message is one piece of a larger one.
pub const LIST_ROW_FRAGMENT: &str = "fragment";

/// [`LIST_ROW_LINKED`]'s family, as the specification's template.
pub const LIST_ROW_LINKED_TEMPLATE: &str = "pv.list.row.{}.linked";

/// [`LIST_ROW_NOTE`]'s family, as the specification's template.
pub const LIST_ROW_NOTE_TEMPLATE: &str = "pv.list.row.{}.note";

/// [`LIST_ROW_FRAGMENT`]'s family, as the specification's template.
pub const LIST_ROW_FRAGMENT_TEMPLATE: &str = "pv.list.row.{}.fragment";

/// ★★★★★ R2111 — every annotation word a message row addresses, beside the
/// specification template that names its family.
///
/// ⚠ NOT the placement order. The name column places these right to left with a
/// gap and an ink per annotation, and that order is the painter's decision —
/// recorded where it is made. A roster that implied it owned the order would be
/// a fact nobody checks, which is the class this module exists to remove. The
/// specification takes each template BY NAME for the same reason: an index into
/// this table would make the order load-bearing after all.
pub const LIST_ROW_ANNOTATIONS: &[(&str, &str)] = &[
    (LIST_ROW_LINKED, LIST_ROW_LINKED_TEMPLATE),
    (LIST_ROW_NOTE, LIST_ROW_NOTE_TEMPLATE),
    (LIST_ROW_FRAGMENT, LIST_ROW_FRAGMENT_TEMPLATE),
];

/// What separates one segment of an address from the next.
///
/// ★★★★★ Declared for the same reason [`LIST_CELL_JOIN`] is. Every seat const
/// above carries the separator inside its own literal, which is fine while only
/// one side composes — but the annotations are composed AND parsed, in two
/// functions, and two spellings of a separator is the narrowest form of the
/// defect this module exists to remove. The pair below shares this one.
pub const SEPARATOR: &str = ".";

/// The address of the annotation `word` painted inside row `n`'s name column.
///
/// ⚠ The third axis of this family, and the one that makes [`list_row_index`]'s
/// refusal necessary: this address is a ROW's address with a word after it, so
/// the row's own parse has to reject it.
#[must_use]
pub fn list_row_annotation(n: usize, word: &str) -> String {
    format!("{}{SEPARATOR}{word}", list_row(n))
}

/// The row and the annotation word an address names, or `None` when the tag is
/// not one of a row's annotations.
///
/// ★★★★★ [`list_row_annotation`]'s inverse, and the reader that makes it earn
/// its place is the INTEGRATED gate: with the whole grid mounted as a page of
/// the application, every painted mark under this family has to be one some
/// declared reader can recover, and an annotation is the only member no other
/// reader claims. Without this, the claim that a mark's address is findable
/// would have a hole exactly where the family's third axis is.
///
/// ⚠ The word is looked up in the roster rather than returned as given: a tail
/// this screen does not paint is not an annotation, and answering `Some` for one
/// would let the gate above accept any mark at all under a row's address.
#[must_use]
pub fn list_row_annotation_of(tag: &str) -> Option<(usize, &'static str)> {
    let (index, tail) = tag.strip_prefix(LIST_ROW_SEAT)?.split_once(SEPARATOR)?;
    let word = LIST_ROW_ANNOTATIONS
        .iter()
        .find(|(word, _)| *word == tail)
        .map(|(word, _)| *word)?;
    Some((index.parse().ok()?, word))
}
