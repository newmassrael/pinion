//! ★★★★★ R2109 §5.2 §5.11 — **where this screen's painted addresses come
//! from.**
//!
//! R2109 opened this module for the filter bar; R2111 gave it the message
//! grid, which is the largest family of the capture viewer and the largest this
//! address campaign has converted; R2112 gave it the decode tree, R2113 the byte
//! grid, R2114 the two strips, and R2115 the application bar and the three lone
//! marks that CLOSE the screen. Each has its own entry further down.
//!
//! ★★★★★ **This module is now the only place in this crate where a literal in
//! this screen's namespace may appear**, and that is a gate rather than a
//! convention: `r2115_no_module_but_the_declaration_spells_this_screens_namespace`
//! reads every module's source and refuses one. Its sibling in the shell refuses
//! the same thing of the HOST that mounts this screen as a page. The two
//! together are what the seventeen instalments were for — before them, each gate
//! could only see the families already declared, so a screen could grow a new
//! region tomorrow and spell it everywhere with nothing saying a word.
//!
//! ★★★★★ FOUR ARRANGEMENTS sit side by side here, and the contrast is worth
//! reading before adding a fifth, because which one a family has is decided by
//! its key and its shape rather than by its size:
//!
//! * keys that are NUMBERS — the message grid, the byte grid, the reassembly
//!   lanes. `parse()` writes every refusal: a tail that is not a number, the
//!   family's own stem, and a sibling's prefix all fall out as `None`.
//! * keys drawn from an OPEN VOCABULARY — the decode tree's field paths and
//!   layer identifiers. Nothing is inherited, so each inverse says for itself
//!   that the stem is not a member and that a sibling's prefix is not its own.
//! * keys under the SEAT PREFIX ITSELF — the session-context strip, where a
//!   negotiated value and the strip's fixed seat share one prefix. There is no
//!   word between the stem and the key, so the seat ROSTER is the discriminator
//!   and the two rosters have to be proven disjoint.
//! * NO KEYS AT ALL — the application bar, three fixed seats and nothing
//!   parametric. Nothing has to be decided: there is no key vocabulary, so the
//!   roster IS the whole declaration and the inverse is one equality. It was
//!   left for last because it is the one arrangement that teaches nothing, and
//!   the three lone marks below are its degenerate case — one mark each, so not
//!   even a roster.
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

// ─── the decode tree ────────────────────────────────────────────────────────
//
// ★★★★★ R2112 — the capture viewer's second pane, and the first family this
// campaign has converted whose parametric keys are an OPEN VOCABULARY.
//
// Measured at entry: **56 sites in this crate's five modules**, 19 across four
// walks and three in the shell that mounts this screen as a page — 78 in all,
// with nothing declaring any of them.
//
// ⚠ What is new here, said rather than left to be found. Every parametric
// family this campaign has converted so far keyed on a NUMBER, and `parse()`
// was doing quiet work in each inverse: it refused a tail that was not this
// screen's. The decode tree keys on a field PATH and a layer identifier, so
// there is no such refusal to inherit, and each inverse has to say for itself
// what it will not answer for:
//
// * an EMPTY tail. `pv.tree.field.` is the family's stem and not a member of
//   it, and an inverse that answered `Some("")` would let a reader classify the
//   stem itself as a field with no name.
// * the OTHER two prefixes. The three hang off one stem and are told apart by
//   one word, so a reader that stripped only [`TREE_SEAT`] and took the rest
//   would read a chevron as a field whose path began `layer.`.
//
// ⚠⚠ And a field path is HIERARCHICAL — `l0` is a layer heading and `l0.link`
// is a field under it — so a field's own address is a PREFIX of its children's.
// That is why the whole tail is the path and nothing here splits on the
// separator: `pv.tree.field.l0.link` names one field, not a field `l0` with
// something after it. [`list_row_annotation_of`] one family over splits for
// exactly the opposite reason, and the two must not be made to look alike.

/// ★★★★★ R2112 — the tag the **decode tree itself** is painted under.
///
/// The tree, not one of its seats, and carried WITHOUT the separator for
/// [`FILTER`]'s reason: [`TREE_SEAT`] is the form a reader composes onto.
pub const TREE: &str = "pv.tree";

/// [`TREE`] with the separator every member of this family hangs off.
///
/// The whole family's stem, which is what a reader asking *is this mark part of
/// the decode tree* needs — the integrated gate asks exactly that, across the
/// tree's seats, rows, chevrons and badges at once.
pub const TREE_SEAT: &str = "pv.tree.";

/// The scrolling body the decode rows are laid out inside.
///
/// A seat rather than the tree itself: the viewport clips, and what a reader
/// lands on is what is inside it.
pub const TREE_BODY: &str = "pv.tree.body";

/// The run in the head strip that names this pane.
///
/// Declared as the tree's NAME rather than as a stop of its own: the pane's
/// accessible name redirects here, so the title a sighted reader sees and the
/// name an agent is told are one run.
pub const TREE_TITLE: &str = "pv.tree.title";

/// The band painted behind the field a reader has open.
///
/// Decorative: the item itself announces that it is selected, and this is how a
/// sighted reader is told the same fact.
pub const TREE_SELECTED: &str = "pv.tree.selected";

/// ★★★★★ R2112 — every FIXED word this tree addresses, beside its address.
///
/// The roster a reader classifies by and the wire publishes. The fields, the
/// chevrons and the derived badges are NOT here: their populations are the
/// decode's, so each is published as a prefix and a member's key is appended.
pub const TREE_SEATS: &[(&str, &str)] = &[
    ("body", TREE_BODY),
    ("title", TREE_TITLE),
    ("selected", TREE_SELECTED),
];

/// The address of the fixed seat `word`.
///
/// ⚠ Composed rather than looked up, for [`filter`]'s reason. Its readers are
/// the gate — which drives every const above through it, so the three are
/// shadows of one rule rather than three spellings — and [`tree_word`], which
/// is the direction where *what this tree addresses* is a closed question.
#[must_use]
pub fn tree(word: &str) -> String {
    format!("{TREE_SEAT}{word}")
}

/// The fixed word an address names, or `None` when the tag is not one of them.
///
/// ★ [`tree`]'s inverse, and the classifier the integrated gate reaches for
/// first. Exact equality against the roster rather than a prefix strip, which
/// is what keeps `pv.tree.field.l0` from reading as a seat called `field.l0`.
///
/// ⚠ The tree's own tag answers `None` — container and content, [`list_word`]'s
/// argument verbatim.
#[must_use]
pub fn tree_word(tag: &str) -> Option<&'static str> {
    TREE_SEATS
        .iter()
        .find(|(_, seat)| *seat == tag)
        .map(|(word, _)| *word)
}

/// The prefix every DECODE FIELD's row is painted under.
pub const TREE_FIELD_SEAT: &str = "pv.tree.field.";

/// [`TREE_FIELD_SEAT`] with the population's placeholder, for a specification
/// table whose rows must be `&'static str`.
pub const TREE_FIELD_TEMPLATE: &str = "pv.tree.field.{}";

/// The address of the decode row for the field at `path`.
///
/// ⚠ `path` is the field's identity in the decode, which is hierarchical: a
/// layer heading is `l1` and a field under it is `l1.sn`. The address of the
/// heading is therefore a prefix of its children's, and a reader wanting *this
/// field and no other* compares whole addresses rather than prefixes.
#[must_use]
pub fn tree_field(path: &str) -> String {
    format!("{TREE_FIELD_SEAT}{path}")
}

/// The field path an address names, or `None` when the tag is not a decode row.
///
/// ★★★★★ [`tree_field`]'s inverse, and the hit router's: a press on a decode
/// row selects that field. The whole tail is the path — see the note at the top
/// of this section for why nothing here splits on the separator.
///
/// ⚠ An empty tail is `None`. `pv.tree.field.` is this family's stem, not a
/// member of it, and answering `Some("")` would let a classifier count the stem
/// as a nameless field.
#[must_use]
pub fn tree_field_path(tag: &str) -> Option<&str> {
    non_empty(tag.strip_prefix(TREE_FIELD_SEAT)?)
}

/// The prefix every FOLD CHEVRON is painted under.
pub const TREE_LAYER_SEAT: &str = "pv.tree.layer.";

/// [`TREE_LAYER_SEAT`] with the population's placeholder.
pub const TREE_LAYER_TEMPLATE: &str = "pv.tree.layer.{}";

/// The address of the fold chevron for the layer called `id`.
#[must_use]
pub fn tree_layer(id: &str) -> String {
    format!("{TREE_LAYER_SEAT}{id}")
}

/// The layer an address names, or `None` when the tag is not a chevron.
///
/// ★★★★★ [`tree_layer`]'s inverse, and the one R1815 is a monument to: the
/// chevron carried this address from R1693 and no arm of the hit router matched
/// it, so the mark was addressable in the paint and inert to every press for
/// 122 rounds. With the prefix declared once, an arm that stopped matching is a
/// test failure rather than a control that silently does nothing.
///
/// ⚠ A layer's identifier is ALSO a field path — `l1` names the heading row and
/// the chevron drawn on it — so these two inverses answer about the same layer
/// from two different marks. They are told apart by the word in the address and
/// by nothing else, which is why each strips its own whole prefix.
#[must_use]
pub fn tree_layer_id(tag: &str) -> Option<&str> {
    non_empty(tag.strip_prefix(TREE_LAYER_SEAT)?)
}

/// The prefix every DERIVED badge is painted under.
pub const TREE_DERIVED_SEAT: &str = "pv.tree.derived.";

/// [`TREE_DERIVED_SEAT`] with the population's placeholder.
pub const TREE_DERIVED_TEMPLATE: &str = "pv.tree.derived.{}";

/// The address of the derived badge on the row for `path`.
///
/// The badge says the value beside it came from no bytes. Its population is the
/// fields the decode computes rather than reads, which is a subset of the
/// fields — so this composes on the same key as [`tree_field`] and the two
/// addresses differ only in their word.
#[must_use]
pub fn tree_derived(path: &str) -> String {
    format!("{TREE_DERIVED_SEAT}{path}")
}

/// The field a derived badge's address names, or `None` when the tag is not a
/// badge.
///
/// ★★★★★ [`tree_derived`]'s inverse, and the reader that makes it earn its
/// place is the INTEGRATED gate: with the tree mounted as a page of the
/// application, every painted mark under this family has to be one some declared
/// reader can recover, and the badge is the only member no other reader claims.
/// Without this, the claim that a mark's address is findable would have a hole
/// exactly where the family's third axis is.
#[must_use]
pub fn tree_derived_path(tag: &str) -> Option<&str> {
    non_empty(tag.strip_prefix(TREE_DERIVED_SEAT)?)
}

// ─── the byte grid ──────────────────────────────────────────────────────────
//
// ★★★★★ R2113 — the capture viewer's third pane, and the family that makes the
// campaign's two key shapes sit side by side in one module.
//
// Measured at entry: **39 sites in this crate's five modules**, 6 across four
// walks and 2 in the shell that mounts this screen as a page — 47 in all.
//
// ⚠ Every key here is a NUMBER, so `parse()` writes the refusals R2112 had to
// write by hand one pane over: a tail that is not a number, a family's own stem,
// and another family's prefix all fall out as `None` without an inverse saying
// so. The decode tree's [`non_empty`] has no counterpart in this section, and
// that absence is the point rather than an omission — **which shape a family has
// is decided by its key's vocabulary, and this module now holds one of each.**
//
// ⚠⚠ TWO PAIRS OF FAMILIES SHARE A KEY SPACE, which is what a reader here has
// to get right:
//
// * `cell.<byte>` and `lit.<byte>` are both keyed by the byte's index — one is
//   the digit pair a reader sees, the other the highlight painted behind it.
// * `offset.<row>` and `row.<row>` are both keyed by the row's index — one is
//   the hex offset painted at the left, the other the accessibility row.
//
// They are told apart by the word before the key and by nothing else, so each
// inverse strips its own whole prefix.
//
// ⚠⚠⚠ **`row` PAINTS NOTHING**, and that is a fact a gate has to be told rather
// than discover. A byte row *is* its eight cells and the offset beside them; the
// row is what a reader descends through in the accessibility tree, and it is
// anchored there by the members it composes. So the integrated gate below floors
// the readers that appear in the PAINT and checks this one against the
// accessibility tree instead — see
// `r2113_every_mark_the_mounted_byte_grid_paints_has_an_address_a_reader_recovers`.
//
// ★ Two of this family's seven addresses were ALREADY composed in one place
// before this round — `bytes_offset_tag` and `bytes_row_tag`, each written in
// the round that happened to need it. That is R2110's finding a third time:
// declaring a corner whenever a round needs one leaves a declared CORNER, not a
// declared screen, and the two corners could not be checked against the five
// spellings that hung off the same stem.

/// ★★★★★ R2113 — the tag the **byte grid itself** is painted under.
///
/// The grid, not one of its seats, and carried WITHOUT the separator for
/// [`FILTER`]'s reason: [`BYTES_SEAT`] is the form a reader composes onto.
pub const BYTES: &str = "pv.bytes";

/// [`BYTES`] with the separator every member of this family hangs off.
pub const BYTES_SEAT: &str = "pv.bytes.";

/// The scrolling body the byte rows are laid out inside.
pub const BYTES_BODY: &str = "pv.bytes.body";

/// The run in the head strip that names this pane.
pub const BYTES_TITLE: &str = "pv.bytes.title";

/// The pane's readout: which decode row is open and which bytes it was read
/// from.
///
/// A seat rather than a decoration: the grid declares this run as its
/// description, so what it says is part of what the grid announces.
pub const BYTES_SPAN: &str = "pv.bytes.span";

/// ★★★★★ R2113 — every FIXED word this grid addresses, beside its address.
///
/// The roster a reader classifies by and the wire publishes. The cells, the
/// highlights, the offsets and the accessibility rows are NOT here: their
/// populations are the frame's.
pub const BYTES_SEATS: &[(&str, &str)] = &[
    ("body", BYTES_BODY),
    ("title", BYTES_TITLE),
    ("span", BYTES_SPAN),
];

/// The address of the fixed seat `word`.
#[must_use]
pub fn bytes(word: &str) -> String {
    format!("{BYTES_SEAT}{word}")
}

/// The fixed word an address names, or `None` when the tag is not one of them.
///
/// ★ [`bytes`]'s inverse. Exact equality against the roster, for
/// [`tree_word`]'s reason.
#[must_use]
pub fn bytes_word(tag: &str) -> Option<&'static str> {
    BYTES_SEATS
        .iter()
        .find(|(_, seat)| *seat == tag)
        .map(|(word, _)| *word)
}

/// The prefix every BYTE CELL is painted under.
pub const BYTES_CELL_SEAT: &str = "pv.bytes.cell.";

/// [`BYTES_CELL_SEAT`] with the population's placeholder.
pub const BYTES_CELL_TEMPLATE: &str = "pv.bytes.cell.{}";

/// The address of the cell showing the byte at `index`.
#[must_use]
pub fn bytes_cell(index: usize) -> String {
    format!("{BYTES_CELL_SEAT}{index}")
}

/// The byte a cell's address names, or `None` when the tag is not a cell.
///
/// ★★★★★ [`bytes_cell`]'s inverse, and the hit router's — a press on a cell
/// selects the field that byte belongs to. `parse()` is what refuses the stem,
/// a non-numeric tail and the three sibling prefixes; the decode tree next door
/// had to write all three refusals by hand because its keys are paths.
#[must_use]
pub fn bytes_cell_index(tag: &str) -> Option<usize> {
    tag.strip_prefix(BYTES_CELL_SEAT)?.parse().ok()
}

/// The prefix every LIT highlight is painted under.
pub const BYTES_LIT_SEAT: &str = "pv.bytes.lit.";

/// [`BYTES_LIT_SEAT`] with the population's placeholder.
pub const BYTES_LIT_TEMPLATE: &str = "pv.bytes.lit.{}";

/// The address of the highlight behind the byte at `index`.
#[must_use]
pub fn bytes_lit(index: usize) -> String {
    format!("{BYTES_LIT_SEAT}{index}")
}

/// The byte a highlight's address names, or `None` when the tag is not one.
///
/// ⚠ Shares its key space with [`bytes_cell_index`]: both are keyed by the byte,
/// and a reader that stripped only [`BYTES_SEAT`] would answer for either. The
/// judge reads the lit set back through this, so a prefix that drifted would
/// make the pane look as though it lit nothing.
#[must_use]
pub fn bytes_lit_index(tag: &str) -> Option<usize> {
    tag.strip_prefix(BYTES_LIT_SEAT)?.parse().ok()
}

/// The prefix every ROW OFFSET is painted under.
pub const BYTES_OFFSET_SEAT: &str = "pv.bytes.offset.";

/// [`BYTES_OFFSET_SEAT`] with the population's placeholder.
pub const BYTES_OFFSET_TEMPLATE: &str = "pv.bytes.offset.{}";

/// The address of the hex offset painted at the left of `row`.
///
/// ★ The offset is that row's HEADER, which is why it carries a tag of its own
/// rather than being part of the row: a grid whose rows had no header would be
/// less locatable than the floor this project measures against.
#[must_use]
pub fn bytes_offset(row: usize) -> String {
    format!("{BYTES_OFFSET_SEAT}{row}")
}

/// The row an offset's address names, or `None` when the tag is not one.
#[must_use]
pub fn bytes_offset_row(tag: &str) -> Option<usize> {
    tag.strip_prefix(BYTES_OFFSET_SEAT)?.parse().ok()
}

/// The prefix every ACCESSIBILITY ROW of the grid is addressed under.
pub const BYTES_ROW_SEAT: &str = "pv.bytes.row.";

/// [`BYTES_ROW_SEAT`] with the population's placeholder.
pub const BYTES_ROW_TEMPLATE: &str = "pv.bytes.row.{}";

/// The address of the accessibility row at `row`.
///
/// ★★★★★ **NOTHING PAINTS THIS.** A byte row *is* the eight cells and the offset
/// beside them — there is no rectangle of its own — and the row exists so a
/// reader can descend through it. It is anchored in the census by the members it
/// composes, which is an exemption the census checks for itself rather than one
/// this screen declares.
///
/// ⚠ Said here because the fact is invisible from the address: every other
/// member of this family is a mark, and a gate that counted painted marks and
/// floored all five readers would be red for a reason that is correct behaviour.
#[must_use]
pub fn bytes_row(row: usize) -> String {
    format!("{BYTES_ROW_SEAT}{row}")
}

/// The row an accessibility row's address names, or `None` when the tag is not
/// one.
///
/// ⚠ Shares its key space with [`bytes_offset_row`] — one row index, two
/// addresses, told apart by the word before it.
#[must_use]
pub fn bytes_row_index(tag: &str) -> Option<usize> {
    tag.strip_prefix(BYTES_ROW_SEAT)?.parse().ok()
}

// ─── the session-context strip ──────────────────────────────────────────────
//
// ★★★★★ R2114 — the band above the three panes, and the family that makes the
// campaign meet a THIRD arrangement: **the parametric members hang off the same
// prefix as the fixed seats.**
//
// Measured at entry: **17 sites in this crate's five modules**, 4 across two
// walks and none in the shell — 21 in all, with nothing declaring any of them.
//
// ⚠ Every family before this one put a WORD between the stem and the key —
// `saved.<n>`, `row.<n>`, `field.<path>`, `lane.<n>` — so an inverse could strip
// its own prefix and be sure of what it had. Here a negotiated value is painted
// at `pv.context.<slug>` and the strip's one fixed seat at `pv.context.session`,
// which is the SAME shape. There is no word to tell them apart, so the roster
// itself is the discriminator:
//
// * [`context_word`] matches a seat by EQUALITY against the roster;
// * [`context_value_slug`] strips [`CONTEXT_SEAT`] and then refuses a tail the
//   roster claims, and refuses an empty tail for [`non_empty`]'s reason.
//
// ⇒ the two rosters must stay DISJOINT or one address means two things, and
// that is not a property either function can hold on its own — it is a property
// of the two tables, so `r2114_every_context_address_is_derived` holds it.
//
// ⚠⚠ THE SLUG RULE WAS SPELLED IN FIVE PLACES, and this is the narrowest
// instance of this debt the campaign has measured — narrower than R2111's join
// character, because it is a rule rather than a character and it was copied
// four times. Before this round `key.replace(' ', "_")` stood in the
// specification's population expander, in the painter, in the accessibility
// tree, and twice in the paint sweep, with **no assertion anywhere between
// them**. The direction of a drift is the quiet one: the expander and the
// painter disagreeing makes the census enumerate an address nobody paints while
// the screen paints the right mark, so the screen looks broken and is not.
// [`context_slug`] is that rule's one home now.

/// ★★★★★ R2114 — the tag the **session-context strip itself** is painted under.
///
/// The strip, not one of its seats, and carried WITHOUT the separator for
/// [`FILTER`]'s reason: [`CONTEXT_SEAT`] is the form a reader composes onto.
pub const CONTEXT: &str = "pv.context";

/// [`CONTEXT`] with the separator every member of this family hangs off.
///
/// ⚠ BOTH kinds of member: the fixed seat below and every negotiated value.
/// This is the one family of this screen where the parametric prefix and the
/// seat prefix are the same string, which is why there is no
/// `CONTEXT_VALUE_SEAT` beside it — a second const holding the same value would
/// be two homes for the fact rather than one.
pub const CONTEXT_SEAT: &str = "pv.context.";

/// The run that says which session these values were negotiated for, and how
/// much of the capture that premise covers.
pub const CONTEXT_SESSION: &str = "pv.context.session";

/// ★★★★★ R2114 — every FIXED word this strip addresses, beside its address.
///
/// The roster a reader classifies by and the wire publishes — and here it is
/// also what [`context_value_slug`] refuses, because a negotiated value is
/// addressed under the same prefix.
pub const CONTEXT_SEATS: &[(&str, &str)] = &[("session", CONTEXT_SESSION)];

/// The address of the fixed seat `word`.
#[must_use]
pub fn context(word: &str) -> String {
    format!("{CONTEXT_SEAT}{word}")
}

/// The fixed word an address names, or `None` when the tag is not one of them.
///
/// ★ [`context`]'s inverse. Exact equality against the roster, for
/// [`tree_word`]'s reason — and here equality is load-bearing twice over, since
/// a prefix match would claim every negotiated value as well.
#[must_use]
pub fn context_word(tag: &str) -> Option<&'static str> {
    CONTEXT_SEATS
        .iter()
        .find(|(_, seat)| *seat == tag)
        .map(|(word, _)| *word)
}

/// [`CONTEXT_SEAT`] with the population's placeholder.
///
/// ⚠ The template of the VALUES, spelled from the seat prefix rather than
/// written out, because they are the same prefix. The specification names this
/// family by this template and expands it by [`context_slug`].
pub const CONTEXT_VALUE_TEMPLATE: &str = "pv.context.{}";

/// ★★★★★ R2114 — **the slug rule, and its only home.**
///
/// A negotiated value's key is what a reader sees — `id width`, `low latency` —
/// and its address cannot carry the space, so the address uses an underscore.
/// That substitution is the whole rule, and until this round it was written out
/// in five places with nothing comparing them.
///
/// ⚠ Not `const fn`: [`str::replace`] allocates. The gate drives the painter,
/// the accessibility tree, the paint sweep and the specification's expander
/// through this one function, so a sixth spelling is a thing that cannot be
/// written rather than a thing nobody noticed.
#[must_use]
pub fn context_slug(key: &str) -> String {
    key.replace(' ', "_")
}

/// The address of the negotiated value whose key is `key`.
///
/// ★ Takes the KEY a reader sees, not the slug, so a caller never holds the
/// intermediate form — the one place [`context_slug`] is applied on the way to
/// an address is here.
#[must_use]
pub fn context_value(key: &str) -> String {
    format!("{CONTEXT_SEAT}{}", context_slug(key))
}

/// The SLUG a negotiated value's address names, or `None` when the tag is not
/// one.
///
/// ★★★★★ [`context_value`]'s inverse, and the one that carries this family's
/// hand-written refusals. `parse()` refuses nothing here — the keys are an open
/// vocabulary, like the decode tree's one pane down — and unlike that pane there
/// is no word between the stem and the key, so the seat roster is what the
/// refusal is made of.
///
/// ⚠ Answers the SLUG and not the key: the substitution is not invertible (a key
/// holding an underscore would slug to itself), so a reader wanting the key
/// compares slugs. The gate holds the specification's keys to slugging
/// distinctly, which is what makes that comparison total.
#[must_use]
pub fn context_value_slug(tag: &str) -> Option<&str> {
    let tail = non_empty(tag.strip_prefix(CONTEXT_SEAT)?)?;
    if CONTEXT_SEATS.iter().any(|(word, _)| *word == tail) {
        return None;
    }
    Some(tail)
}

// ─── the reassembly strip ───────────────────────────────────────────────────
//
// ★★★★★ R2114 — the band along the bottom, taken in the same round as the strip
// above because the two are the last families of this screen and each is small.
//
// Measured at entry: **23 sites in this crate's five modules**, 2 across two
// walks and 1 in the shell that mounts this screen as a page — 26 in all.
//
// ⚠ Its one parametric family keys on a NUMBER, so it is R2113's shape and
// `parse()` writes its refusals — while the strip above is a third shape again.
// **Two arrangements arrive in one round, and which one a family has is decided
// by its key's vocabulary and by whether a word stands between the stem and the
// key.** That contrast is the reason these two are declared side by side rather
// than in the order the campaign's budget would have taken them.

/// ★★★★★ R2114 — the tag the **reassembly strip itself** is painted under.
pub const REASSEMBLY: &str = "pv.reassembly";

/// [`REASSEMBLY`] with the separator every member of this family hangs off.
pub const REASSEMBLY_SEAT: &str = "pv.reassembly.";

/// The run in the strip that names it.
///
/// ⚠ Painted, and deliberately SILENT: it names the strip, so the strip
/// announces it and a second stop reading the same words would be the band
/// announced twice. A mark all the same, which is why it is a seat here.
pub const REASSEMBLY_TITLE: &str = "pv.reassembly.title";

/// The strip's readout: how many channels carry traffic and how the
/// reassemblies are doing.
pub const REASSEMBLY_COUNTS: &str = "pv.reassembly.counts";

/// ★★★★★ R2114 — every FIXED word this strip addresses, beside its address.
pub const REASSEMBLY_SEATS: &[(&str, &str)] =
    &[("title", REASSEMBLY_TITLE), ("counts", REASSEMBLY_COUNTS)];

/// The address of the fixed seat `word`.
#[must_use]
pub fn reassembly(word: &str) -> String {
    format!("{REASSEMBLY_SEAT}{word}")
}

/// The fixed word an address names, or `None` when the tag is not one of them.
///
/// ★ [`reassembly`]'s inverse. Exact equality against the roster, for
/// [`bytes_word`]'s reason.
#[must_use]
pub fn reassembly_word(tag: &str) -> Option<&'static str> {
    REASSEMBLY_SEATS
        .iter()
        .find(|(_, seat)| *seat == tag)
        .map(|(word, _)| *word)
}

/// The prefix every LANE box is painted under.
pub const REASSEMBLY_LANE_SEAT: &str = "pv.reassembly.lane.";

/// [`REASSEMBLY_LANE_SEAT`] with the population's placeholder.
pub const REASSEMBLY_LANE_TEMPLATE: &str = "pv.reassembly.lane.{}";

/// The address of the lane at `index`.
#[must_use]
pub fn reassembly_lane(index: usize) -> String {
    format!("{REASSEMBLY_LANE_SEAT}{index}")
}

/// The lane a box's address names, or `None` when the tag is not a lane.
///
/// ★ [`reassembly_lane`]'s inverse. `parse()` refuses the stem, a non-numeric
/// tail and the strip's two seats, which is the whole difference between this
/// family and the context strip's — one word between the stem and the key.
#[must_use]
pub fn reassembly_lane_index(tag: &str) -> Option<usize> {
    tag.strip_prefix(REASSEMBLY_LANE_SEAT)?.parse().ok()
}

// ─── the application bar ────────────────────────────────────────────────────
//
// ★★★★★ R2115 — the band across the top, and the LAST family of this screen
// that a reader had to spell.
//
// Measured at entry: **20 sites in this crate's five modules** and 2 across two
// walks — 22 in all, and none in the shell.
//
// ⚠ Three fixed seats and NO parametric family, which makes this the simplest
// arrangement the campaign has met and the reason it was left for last: there is
// no key vocabulary to decide anything, so the roster is the whole declaration.

/// ★★★★★ R2115 — the tag the **application bar itself** is painted under.
pub const APPBAR: &str = "pv.appbar";

/// [`APPBAR`] with the separator its seats hang off.
pub const APPBAR_SEAT: &str = "pv.appbar.";

/// What the bar says is being captured.
pub const APPBAR_INTERFACE: &str = "pv.appbar.interface";

/// The rate readout beside it.
pub const APPBAR_RATE: &str = "pv.appbar.rate";

/// The running commentary — a LIVE region that opens empty and fills as the
/// screen is driven.
///
/// ⚠ This one is also an ANSWER address: `Announced::at` names it when a refusal
/// has to reach a person, so a wrong letter here loses the refusal rather than a
/// mark. Two readers, one string, and until this round neither could be compared
/// with the other.
pub const APPBAR_SAID: &str = "pv.appbar.said";

/// ★★★★★ R2115 — every FIXED word this bar addresses, beside its address.
///
/// The whole family: this bar has no parametric half. The title run is NOT here
/// because nothing tags it — it is painted as a bare label and named by the
/// group, which is a decision `VOICES` carries rather than this roster.
pub const APPBAR_SEATS: &[(&str, &str)] = &[
    ("interface", APPBAR_INTERFACE),
    ("rate", APPBAR_RATE),
    ("said", APPBAR_SAID),
];

/// The address of the fixed seat `word`.
#[must_use]
pub fn appbar(word: &str) -> String {
    format!("{APPBAR_SEAT}{word}")
}

/// The fixed word an address names, or `None` when the tag is not one of them.
///
/// ★ [`appbar`]'s inverse. Exact equality against the roster, for
/// [`bytes_word`]'s reason.
#[must_use]
pub fn appbar_word(tag: &str) -> Option<&'static str> {
    APPBAR_SEATS
        .iter()
        .find(|(_, seat)| *seat == tag)
        .map(|(word, _)| *word)
}

// ─── the three lone marks ───────────────────────────────────────────────────
//
// ★★★★★ R2115 — what is left once the six families are declared: three tags
// that are NOT families. Each is one mark with no members, so none of them gets
// a seat roster, a template or an inverse — a family shape here would be five
// declarations describing a set of one.
//
// ⚠ Two of them were ALREADY composed in one place before this round —
// `TOOLTIP_TAG` and `MAP_TAG`, each written in the round that happened to need
// it. That is R2110's finding a fourth time, and it is why they move here rather
// than being left alone: a corner declared where a round needed it cannot be
// checked against the namespace it belongs to, and the gate below is exactly
// that check.

/// The screen's root panel — an address for the sweep and the receiver a press
/// falls through to. Not a place a reader travels, which is what `SILENCES`
/// says about it.
pub const ROOT: &str = "pv.root";

/// The region a description is painted in.
pub const TIP: &str = "pv.tip";

/// The overview map.
pub const MAP: &str = "pv.map";

/// ★★★★★ R2115 — **the namespace this screen's every painted address begins
/// with**, and the needle the gate uses.
///
/// Declared rather than spelled because the gate that keeps this module the only
/// speller has to name the thing it forbids, and a gate spelling its own needle
/// is the defect one level up (R2053). The trailing separator is part of it: a
/// reader classifying a snapshot by family compares against this, and `pv` alone
/// would also claim a screen called `pvsomething`.
pub const NAMESPACE: &str = "pv.";

/// `tail` unless it is empty.
///
/// ★ Shared by the four inverses above rather than written into each, because
/// *the stem is not a member of its own family* is one rule and four copies of
/// it is this module's own defect one level down. A `const fn` cannot do this,
/// so it is a plain one; the gate drives all four doors through it.
fn non_empty(tail: &str) -> Option<&str> {
    (!tail.is_empty()).then_some(tail)
}
