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

// --- the preferences page ------------------------------------------------
//
// ★★★★★ R2119 §5.2 §5.11 — **the twenty-second instalment, and the campaign's
// SIXTH arrangement: a family with eleven stems, three of them two-headed.**
//
// Measured at entry, before a letter moved: **100 sites across seven files** —
// 84 in this crate's five modules and 16 across two walks — against a rail
// whose conversion at R2051 was 28, a palette's at R2110 124, and a link's at
// R2118 56. It is the largest family this shell has left and the head of
// `tools/painted_addresses.py --owed`.
//
// # ★ Why the arrangement is new
//
// The five before this had ONE key vocabulary under a stem — a seat roster, an
// open vocabulary re-parsed out of the tag, a count, an enum's arms, or (R2118)
// two heads on one stem told apart by a classifier. This page has eleven stems
// and its ambiguity is one level deeper: THREE of those stems each carry two
// populations.
//
// * `head.` holds the page's own two words AND one heading per group;
// * `row.` holds a row's own address AND that row's chip strip, one deeper;
// * `option.` holds a SWITCH's address AND, one deeper, a word inside an open
//   roster — which is a different control on a different row entirely.
//
// ⇒ a classifier over the whole family would be one function with eleven arms
// answering questions no reader asks together. What each reader needs is its
// own family's inverse *that refuses the other head*.
//
// # ★★★★★ THREE inverses, not eleven, and the compiler is what said so
//
// The first draft of this module declared an inverse per stem — eleven of them,
// each with a refusal argued in its own doc comment. `cargo check` answered
// with **eleven dead-code errors**: this application ROUTES a press on a
// switch, a booked button and an appearance choice, and nothing else here ever
// turns one of this page's addresses back into a key. The rest of the page's
// marks are read by their family PREFIX (`judge.rs` asks the paint for a
// surface) or composed and never parsed.
//
// R2110 recorded the same measurement one family over and this is it again:
// **an inverse with no reader is not a declaration, it is a second thing to
// drift** — it cannot be wrong in a way anyone sees, so it rots in silence
// while reading as coverage. The three that survive are the three the router
// calls, and [`settings_option_key`]'s refusal is the one this round's finding
// is about.
//
// # ★★ Two of this family's addresses are NOT this screen's to compose
//
// R2050's finding, a page over: an open roster's box and the words inside it
// are painted by `pinion_widget_paint::chooser`, from a prefix its caller
// brings. So [`settings_roster`] and [`settings_choice`] delegate there rather
// than spelling `.roster.` and `.option.` a second time — and the same round
// found what that second spelling costs, because the chooser was recovering an
// option's word by taking the LAST segment of the address and the analysis
// tool's own capture sources are `lo · 127.0.0.1:7447`.
//
// # ⚠ What a walk does instead
//
// A walk is Python and cannot call any of this, so the page publishes its own
// tag, its fixed seats by word, and every parametric family as a prefix, under
// `settings_addresses` on the wire.

/// ★★★★★ R2119 — the tag the preferences PAGE itself is painted under.
///
/// The page, not one of its seats, so it carries no separator — and it is also
/// the prefix `pinion_widget_paint::chooser` composes this page's roster
/// addresses from, which is why it is a `&'static str` a caller can hand over
/// whole.
pub const SETTINGS: &str = "shell.settings";

/// [`SETTINGS`] with the separator everything under it hangs off.
pub const SETTINGS_SEAT: &str = "shell.settings.";

/// The page's own scrolling viewport.
pub const SETTINGS_BODY: &str = "shell.settings.body";

/// The strip the page closes with, which is the one place either screen says
/// which build a reader is looking at.
pub const SETTINGS_BUILD: &str = "shell.settings.build";

/// The appearance segment as a whole — the group a reader chooses *within*.
///
/// ⚠ A fixed word that is ALSO a stem: the choices hang off
/// [`SETTINGS_THEME_SEAT`]. The two are told apart by the separator, which is
/// why this one must never be written with a trailing dot.
pub const SETTINGS_THEME: &str = "shell.settings.theme";

/// What the page calls itself.
pub const SETTINGS_HEAD_TITLE: &str = "shell.settings.head.title";

/// The sentence under that heading.
pub const SETTINGS_HEAD_GIST: &str = "shell.settings.head.gist";

/// ★★★★★ R2119 — every FIXED word this page addresses, beside its address.
///
/// The roster a reader classifies by and the wire publishes. The groups, the
/// rows, the switches, the bookings, the choosers and the choices are NOT here:
/// their populations are the specification's own tables, so each is published
/// as a prefix and reached through its composer.
///
/// ⚠ `head.title` and `head.gist` carry their own separator because that is
/// what the address says — [`PALETTE_SEATS`]'s rule, and for its reason: a word
/// list that flattened them would be a second naming scheme.
///
/// ⚠⚠ `theme` is in this roster AND is a stem. That is the page's own fact,
/// not a modelling choice: the segment is a `radiogroup` a reader is told
/// about, and its two radios are addressed under it.
pub const SETTINGS_SEATS: &[(&str, &str)] = &[
    ("body", SETTINGS_BODY),
    ("build", SETTINGS_BUILD),
    ("theme", SETTINGS_THEME),
    ("head.title", SETTINGS_HEAD_TITLE),
    ("head.gist", SETTINGS_HEAD_GIST),
];

/// The prefix the page's own headings and each group's heading are painted
/// under.
pub const SETTINGS_HEAD: &str = "shell.settings.head.";

/// [`SETTINGS_HEAD`] with the population's placeholder, for a specification
/// table whose rows must be `&'static str`.
pub const SETTINGS_HEAD_TEMPLATE: &str = "shell.settings.head.{}";

/// The address of the heading a reader sees for the group `key`.
///
/// ★ Takes anything that reads as a string, for [`rail_seat`]'s reason: the
/// rosters this is called over hold their keys differently.
#[must_use]
pub fn settings_head(key: impl AsRef<str>) -> String {
    format!("{SETTINGS_HEAD}{}", key.as_ref())
}

/// The prefix every group's own region is painted under.
pub const SETTINGS_GROUP: &str = "shell.settings.group.";

/// [`SETTINGS_GROUP`] with the population's placeholder — see
/// [`SETTINGS_HEAD_TEMPLATE`].
pub const SETTINGS_GROUP_TEMPLATE: &str = "shell.settings.group.{}";

/// The address of the region holding the group `key`.
#[must_use]
pub fn settings_group(key: impl AsRef<str>) -> String {
    format!("{SETTINGS_GROUP}{}", key.as_ref())
}

/// The prefix every row's title and sentence are painted under.
pub const SETTINGS_ROW: &str = "shell.settings.row.";

/// [`SETTINGS_ROW`] with the population's placeholder — see
/// [`SETTINGS_HEAD_TEMPLATE`].
pub const SETTINGS_ROW_TEMPLATE: &str = "shell.settings.row.{}";

/// The appearance row, as a `&'static str` for a specification table.
pub const SETTINGS_ROW_THEME: &str = "shell.settings.row.theme";

/// The payload-format row, as a `&'static str` for a specification table.
pub const SETTINGS_ROW_PLUGINS: &str = "shell.settings.row.plugins";

/// That row's chip strip, likewise.
pub const SETTINGS_ROW_PLUGINS_CHIPS: &str = "shell.settings.row.plugins.chips";

/// The address of the row `key`.
#[must_use]
pub fn settings_row(key: impl AsRef<str>) -> String {
    format!("{SETTINGS_ROW}{}", key.as_ref())
}

/// The address of the chip strip on the row `key`.
///
/// ★ One level deeper than the row, which is what keeps `parts_under` from
/// reading it as a row: the framework takes the tags whose remainder holds no
/// further separator, so a chip strip addressed flat would be counted as a
/// row the specification never declared.
#[must_use]
pub fn settings_row_chips(key: impl AsRef<str>) -> String {
    format!("{SETTINGS_ROW}{}.chips", key.as_ref())
}

/// The prefix every switch — and every word of every open roster — is painted
/// under.
///
/// ⚠ Two populations, one stem. See [`settings_option_key`].
pub const SETTINGS_OPTION: &str = "shell.settings.option.";

/// [`SETTINGS_OPTION`] with the population's placeholder — see
/// [`SETTINGS_HEAD_TEMPLATE`].
pub const SETTINGS_OPTION_TEMPLATE: &str = "shell.settings.option.{}";

/// The address of the switch `key`.
#[must_use]
pub fn settings_option(key: impl AsRef<str>) -> String {
    format!("{SETTINGS_OPTION}{}", key.as_ref())
}

/// The switch an address names, or `None` when it is not one.
///
/// ★★★★★ The refusal is the whole function. A word inside an open roster is
/// addressed `option.<row>.<word>` — one level deeper, on a different control,
/// on a different row — so a bare `strip_prefix` answers `Some("interface.lo ·
/// 127.0.0.1:7447")` for it. Until R2119 nothing refused: the router's next
/// step happened to find no switch by that name and fell through, which made
/// the correctness a property of the CALLER's ordering rather than of the
/// address. R2117's rule: order is a property of one call site, refusal a
/// property of the address.
#[must_use]
pub fn settings_option_key(tag: &str) -> Option<&str> {
    let key = tag.strip_prefix(SETTINGS_OPTION)?;
    (!key.contains('.')).then_some(key)
}

/// The address of the word `word` inside the roster the row `key` opens.
///
/// ★★ Delegated to the painter. `pinion_widget_paint::chooser` composes this
/// from the prefix it is painted under, and this page's prefix is [`SETTINGS`].
#[must_use]
pub fn settings_choice(key: &str, word: &str) -> String {
    pinion_widget_paint::chooser::address::option(SETTINGS, key, word)
}

/// The prefix every word of the roster the row `key` opens is painted under.
#[must_use]
pub fn settings_choice_prefix(key: &str) -> String {
    pinion_widget_paint::chooser::address::option_prefix(SETTINGS, key)
}

/// The prefix every booked row's button is painted under.
pub const SETTINGS_KEY: &str = "shell.settings.key.";

/// [`SETTINGS_KEY`] with the population's placeholder — see
/// [`SETTINGS_HEAD_TEMPLATE`].
pub const SETTINGS_KEY_TEMPLATE: &str = "shell.settings.key.{}";

/// The address of the booked button on the row `key`.
#[must_use]
pub fn settings_key(key: impl AsRef<str>) -> String {
    format!("{SETTINGS_KEY}{}", key.as_ref())
}

/// The booked row an address names, or `None` when it is not one.
#[must_use]
pub fn settings_key_row(tag: &str) -> Option<&str> {
    tag.strip_prefix(SETTINGS_KEY)
}

/// The prefix every collapsed chooser is painted under.
pub const SETTINGS_CHOOSE: &str = "shell.settings.choose.";

/// [`SETTINGS_CHOOSE`] with the population's placeholder — see
/// [`SETTINGS_HEAD_TEMPLATE`].
pub const SETTINGS_CHOOSE_TEMPLATE: &str = "shell.settings.choose.{}";

/// The address of the collapsed control on the value row `key`.
#[must_use]
pub fn settings_choose(key: impl AsRef<str>) -> String {
    format!("{SETTINGS_CHOOSE}{}", key.as_ref())
}

/// The prefix the word a collapsed chooser is showing is painted under.
pub const SETTINGS_SHOWN: &str = "shell.settings.shown.";

/// [`SETTINGS_SHOWN`] with the population's placeholder.
pub const SETTINGS_SHOWN_TEMPLATE: &str = "shell.settings.shown.{}";

/// The address of the word the chooser on the value row `key` is showing.
#[must_use]
pub fn settings_shown(key: impl AsRef<str>) -> String {
    format!("{SETTINGS_SHOWN}{}", key.as_ref())
}

/// The prefix every chooser's chevron is painted under.
pub const SETTINGS_ARROW: &str = "shell.settings.arrow.";

/// [`SETTINGS_ARROW`] with the population's placeholder.
pub const SETTINGS_ARROW_TEMPLATE: &str = "shell.settings.arrow.{}";

/// The address of the chevron on the value row `key`.
#[must_use]
pub fn settings_arrow(key: impl AsRef<str>) -> String {
    format!("{SETTINGS_ARROW}{}", key.as_ref())
}

/// The address of the roster the value row `key` opens onto.
///
/// ★★ Delegated to the painter, for [`settings_choice`]'s reason.
#[must_use]
pub fn settings_roster(key: &str) -> String {
    pinion_widget_paint::chooser::address::roster(SETTINGS, key)
}

/// The prefix every open roster's own box is painted under.
///
/// ⚠ Declared here rather than derived, because the painter composes a whole
/// address and a `const` cannot format; the gate drives it against
/// [`settings_roster`] so the two cannot drift.
pub const SETTINGS_ROSTER: &str = "shell.settings.roster.";

/// The prefix every payload-format chip is painted under.
pub const SETTINGS_PLUGIN: &str = "shell.settings.plugin.";

/// The chip for the record format.
pub const SETTINGS_PLUGIN_RECORDS: &str = "shell.settings.plugin.records";

/// The chip for the schema format.
pub const SETTINGS_PLUGIN_SCHEMA: &str = "shell.settings.plugin.schema";

/// The address of the chip for the payload format `word`.
#[must_use]
pub fn settings_plugin(word: impl AsRef<str>) -> String {
    format!("{SETTINGS_PLUGIN}{}", word.as_ref())
}

/// The prefix every appearance choice is painted under.
///
/// ⚠ [`SETTINGS_THEME`] with a separator, and the two mean different marks —
/// the segment as a whole, and one radio in it.
pub const SETTINGS_THEME_SEAT: &str = "shell.settings.theme.";

/// [`SETTINGS_THEME_SEAT`] with the population's placeholder.
pub const SETTINGS_THEME_TEMPLATE: &str = "shell.settings.theme.{}";

/// The address of the appearance choice at `n`.
///
/// ★ Indexed, not worded: the choices are the reference's own two and the
/// screen addresses them by position, which is the vocabulary the segment's
/// press already carries.
#[must_use]
pub fn settings_theme(n: usize) -> String {
    format!("{SETTINGS_THEME_SEAT}{n}")
}

/// The appearance choice an address names, or `None` when it is not one.
///
/// ★★ Refuses a tail that is not a number, which is what tells this family from
/// a fixed word under the same stem — R2118's disjointness, one page over.
#[must_use]
pub fn settings_theme_index(tag: &str) -> Option<usize> {
    tag.strip_prefix(SETTINGS_THEME_SEAT)?.parse().ok()
}

// --- the header chrome --------------------------------------------------
//
// ★★★★★ R2158 §5.2 §5.11 — **the shell's two bars and the menu one of them
// opens**, the largest cluster this campaign had left in Rust.
//
// Measured at entry: **40 sites in this crate and 10 across two walks, 50 in
// all** — `shell.appbar` 15+3, `shell.subbar` 17+6, `shell.preset` 8+1.
//
// ⚠⚠ **This family had a composer already, and that is why it looks converted
// and is not.** `AppChip::tag()` and `SubChip::tag()` are `match`es over closed
// enums, each returning the address for one seat — a perfectly good mapping
// from seat to address, held in one place. What no one could reach through it
// is the PREFIX and the FAMILY: `judge.rs` wanted "everything under the sub
// bar" and a method returning one seat's whole address cannot answer that, so
// it declared `const LAYOUT_BAR: &str = "shell.subbar."` of its own. The
// specification re-listed all six seat addresses because a `const` table cannot
// call a method. The paint sweep spelled them a third time.
//
// ⇒ ★★★★★ **a composer is not a declaration.** A composer answers *what is
// this one seat's address*; a declaration also answers *what is this family's
// prefix*, *which seats are there* and *which seat is this tag* — and a reader
// denied those three writes the prefix out, which is where the second speller
// comes from every time.

/// The application bar itself — the toolbar the seats sit in.
///
/// ⚠⚠ **The census cannot see this one, and it was spelled eight times.** A
/// bar's own tag is two segments with nothing after it, and the address needle
/// requires a dot AFTER the second word, so `"shell.appbar"` matched nothing
/// and was reported as no sites at all. R2125 met the same blind spot on
/// `lab.canvas` and R2156 met it on a two-segment configuration key. ⇒ a family
/// the instrument scores at zero is not a family that is finished, and the
/// declaration is what finds these because it is derived from the SEATS rather
/// than from what a regex can match.
pub const APPBAR_ROOT: &str = "shell.appbar";

// ⚠ There is deliberately NO `APPBAR` prefix const to match [`SUBBAR`], and the
// asymmetry is driven by readers rather than by tidiness: `judge.rs` asks *what
// is under the sub bar* and nothing asks that of the application bar. A const
// with no reader is not a declaration — it is a second spelling waiting for
// somebody to disagree with, which is this module's whole subject.

/// The tab strip's own container, which is not a seat.
///
/// Its own const for [`RAIL_ACCOUNT`]'s reason: it takes no press and the seat
/// roster does not hold it, so a reader that treated it as a seat would count
/// the bar's seats wrong.
pub const APPBAR_TABS: &str = "shell.appbar.tabs";

/// The prefix every application-bar TAB carries — deeper than the bar's own
/// family, because a tab is addressed by the key its specification row
/// declares.
pub const APPBAR_TAB: &str = "shell.appbar.tab.";

/// [`APPBAR_TAB`] with the population's placeholder, for a specification table
/// whose rows must be `&'static str`. A test holds it against the derivation.
pub const APPBAR_TAB_TEMPLATE: &str = "shell.appbar.tab.{}";

/// The source picker.
pub const APPBAR_SOURCE: &str = "shell.appbar.source";

/// The capture readout — a live region rather than a control.
pub const APPBAR_CAPTURE: &str = "shell.appbar.capture";

/// The search box.
pub const APPBAR_SEARCH: &str = "shell.appbar.search";

/// The application bar's fixed seats, by the word each is known by.
///
/// ⚠ The tabs are NOT here and must not be: their population is
/// `spec::VIEW_TABS`, which is where the keys live, and a second list of them
/// here is exactly the drift this module exists to remove.
///
/// ★ Built from the consts above rather than holding the literals, for
/// [`PALETTE_SEATS`]' reason: a specification table's rows must be
/// `&'static str`, so each seat needs a name a `const` can take, and the roster
/// is then the same strings rather than a second set of them.
pub const APPBAR_SEATS: &[(&str, &str)] = &[
    ("source", APPBAR_SOURCE),
    ("capture", APPBAR_CAPTURE),
    ("search", APPBAR_SEARCH),
];

/// The address of the application-bar seat known by `word`.
#[must_use]
pub fn appbar_seat(word: &str) -> Option<&'static str> {
    APPBAR_SEATS
        .iter()
        .find(|(known, _)| *known == word)
        .map(|(_, tag)| *tag)
}

/// The address of the application-bar tab for `key`.
///
/// ★ Takes anything that reads as a string, for [`rail_seat`]'s reason: the
/// rosters this is called over hold their keys differently.
#[must_use]
pub fn appbar_tab(key: impl AsRef<str>) -> String {
    format!("{APPBAR_TAB}{}", key.as_ref())
}

// ⚠ And no `appbar_tab_key` inverse. R2049's rule is that an address and its
// parse are one pair — but the pair exists because somebody PARSES. This shell
// resolves a press by comparing against the closed enum's own `tag()`
// (`BarChip::all().find(|c| c.tag() == tag)`), so there is no parse here to be
// the second speller. Writing one anyway would add an item whose only caller is
// the test that checks it, which is a check of itself.

/// The sub bar itself — see [`APPBAR_ROOT`] for why this is its own const and
/// why the census reported it as nothing.
pub const SUBBAR_ROOT: &str = "shell.subbar";

/// The prefix every sub-bar seat address carries.
///
/// ⚠ This is the const `judge.rs` used to declare for itself as `LAYOUT_BAR`,
/// and the reason it did is recorded above: the bar had a composer and no
/// declaration, so "everything under the sub bar" was unaskable.
pub const SUBBAR: &str = "shell.subbar.";

/// The sub bar's live widget count, which is a readout rather than a seat.
pub const SUBBAR_COUNT: &str = "shell.subbar.count";

/// The chip that opens the preset menu.
pub const SUBBAR_PRESET: &str = "shell.subbar.preset";

/// The chip that enters layout-edit mode.
pub const SUBBAR_EDIT: &str = "shell.subbar.edit";

/// The chip that adds a widget.
pub const SUBBAR_ADD: &str = "shell.subbar.add";

/// The sub bar's pressable seats, by the word each is known by — built from the
/// consts above for [`APPBAR_SEATS`]' reason.
pub const SUBBAR_SEATS: &[(&str, &str)] = &[
    ("preset", SUBBAR_PRESET),
    ("edit", SUBBAR_EDIT),
    ("add", SUBBAR_ADD),
];

/// The address of the sub-bar seat known by `word`.
#[must_use]
pub fn subbar_seat(word: &str) -> Option<&'static str> {
    SUBBAR_SEATS
        .iter()
        .find(|(known, _)| *known == word)
        .map(|(_, tag)| *tag)
}

// ⚠ No `subbar_word` inverse either, for the reason above. If one is ever
// needed it must consult [`SUBBAR_SEATS`] rather than strip [`SUBBAR`]:
// [`SUBBAR_COUNT`] carries that prefix and is a readout, so a bare
// `strip_prefix` would answer `Some("count")` — a plausible wrong answer rather
// than a loud one.

/// The preset menu itself.
pub const PRESET_MENU: &str = "shell.preset.menu";

/// The prefix every preset menu ITEM carries.
pub const PRESET_ITEM: &str = "shell.preset.item.";

// ⚠ No `PRESET_ITEM_TEMPLATE`. The other families carry one because a
// specification table's rows must be `&'static str` and those families HAVE a
// row; the preset menu is built from whatever is saved and no table declares
// it. A template here would be a `const` whose only reader is the test holding
// it against the composer.

/// The address of the preset menu item at `n`.
///
/// ★ Indexed, not worded: the menu is built from whatever presets are saved, so
/// position is the only vocabulary its press carries — [`settings_theme`]'s
/// case, one page over.
#[must_use]
pub fn preset_item(n: usize) -> String {
    format!("{PRESET_ITEM}{n}")
}

/// The preset an address names, or `None` when the tag is not an item.
///
/// ★★ Refuses a tail that is not a number, which is what tells an item from
/// [`PRESET_MENU`] under the same stem.
#[must_use]
pub fn preset_item_index(tag: &str) -> Option<usize> {
    tag.strip_prefix(PRESET_ITEM)?.parse().ok()
}

// ── the carried footprint (R2171) ───────────────────────────────────────────
//
// ★★★★★ The largest family this screen had not declared: seven paint sites in
// `main.rs`, three family prefixes in the pin generator, six assertions, and
// THIRTEEN walk sites across three walks — all spelling `shell.carry.…` by
// hand. A drag paints it, so the family is only on screen for the length of a
// gesture, which is exactly when a wrong letter is hardest to notice: the mark
// is *supposed* to be absent most of the time.

/// The prefix every mark of the carried footprint hangs off.
pub const CARRY: &str = "shell.carry.";

/// The chip that follows the pointer while a widget is being carried.
pub const CARRY_CHIP: &str = "shell.carry.chip";

/// The board's grid, drawn only while something is being carried over it.
pub const CARRY_GRID: &str = "shell.carry.grid";

/// The preview of where the carried widget would land.
pub const CARRY_SLOT: &str = "shell.carry.slot";

/// [`CARRY_SLOT`] with the separator its own parts hang off.
pub const CARRY_SLOT_SEAT: &str = "shell.carry.slot.";

/// The grip glyph drawn on that preview.
pub const CARRY_SLOT_GRIP: &str = "shell.carry.slot.grip";

/// One cell of the preview's own footprint.
pub const CARRY_SLOT_CELL: &str = "shell.carry.slot.cell";

/// The mark drawn where a carried widget would JOIN one already placed.
pub const CARRY_JOIN: &str = "shell.carry.join";

/// The line that invites the release in words.
pub const CARRY_BANNER: &str = "shell.carry.banner";

// ⚠ No `carry_part(key)` / `carry_slot_part(key)` composers. They were written
// and the compiler refused them as dead code, which is the right answer and the
// one R2158 recorded: a composer with no production consumer is not a
// declaration, it is a rotting thing this campaign creates. The keyed members
// are composed by WALKS, from the keys `spec["carry"]` already publishes — so
// what they needed was the PREFIX, and [`CARRY`] and [`CARRY_SLOT_SEAT`] are
// published beside the members for exactly that.

// --- the alarm feed ----------------------------------------------------------
//
// ★★★★★ R2180 §5.2 §5.16 — **the alarm card's feed, composed in FOUR places and
// counted in one.**
//
// Measured at entry. The painter composed the feed and its rows and cells
// (`alarms_body`); the accessibility builder composed all of them again
// (`alarms_nodes`), and with them the HEADING address that
// `pinion_widget_paint::header_feed` paints — a copy of a crate's composition
// across a crate boundary, which must agree with the paint to the letter or a
// reader is told about a heading nothing draws; the specification composed all
// of them a third time (`alarm_members`); and two geometry-only builders passed
// `"card.alarms.feed"`, a name nothing paints — the feed is painted under its
// card's id, `card.alarms#6.feed`.
//
// The census counted only the specification's copies, because every other one
// begins `card.{id}` or `{tag}` and its needle reads no address in a literal
// that does not open with one (R2174's blind spot, now in its third screen).
//
// # Who owns what
//
// The heading row, the headings and their parts are the CRATE's composition —
// `header_feed::{head_tag, body_tag, column_tag, column_label_tag,
// column_sort_tag}` — and this module does not restate them. What the crate's
// own documentation leaves to the caller is the ROW, so the rows and their
// cells are declared here, together with where the feed sits inside a card.

/// The alarm feed of the card `card_id`, relative to `card.` — the form a
/// specification population member takes.
#[must_use]
pub fn alarm_feed_under(card_id: &str) -> String {
    format!("{card_id}.feed")
}

/// The alarm feed painted inside the card `card_id`.
#[must_use]
pub fn alarm_feed(card_id: &str) -> String {
    format!("card.{}", alarm_feed_under(card_id))
}

/// One built row of the feed addressed `feed`, by its place in the window.
///
/// ★ `.row.{slot}` and not `#{slot}`: `#` is the router's composite-subindex
/// convention, and a feed row is not a router target — see the painter.
#[must_use]
pub fn alarm_row(feed: &str, slot: usize) -> String {
    format!("{feed}.row.{slot}")
}

/// One word of that row, in the column the heading above it names.
#[must_use]
pub fn alarm_cell(feed: &str, slot: usize, column: usize) -> String {
    format!("{}.cell.{column}", alarm_row(feed, slot))
}
