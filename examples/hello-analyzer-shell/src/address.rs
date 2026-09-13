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

// --- a collapsed chooser's parts, wherever this screen paints one ----------------
//
// ★★★★★ R2223 §5.2 — **this screen paints collapsed choosers under TWO
// prefixes, and each of them spelled the part words for itself.**
//
// `pinion_widget_paint::chooser` composes an OPEN roster's box and options
// (`chooser::address`), and deliberately does not compose the collapsed
// control's three parts: R1732 measured that its two consumers disagree —
// `config_form` addresses the chevron `pick`, this screen addresses it `arrow`
// — so a rule inside the crate would be a third vocabulary neither caller uses.
// That judgement is still right, and it leaves the part words as **the
// screen's** to own.
//
// This screen did not own them. The preferences page held them as three
// prefixes and three composers; the card configuration panel, which paints the
// same control under `card.{id}.config`, composed `{prefix}.shown.{key}` and
// `{prefix}.arrow.{key}` inline in `main.rs`. Two spellings of one screen's
// vocabulary, and the one that is not a composer is the one no reader can ask.
//
// So the words live here once, and both prefixes ask. `ChooserPart` rather than
// three functions taking a prefix, for [`pinion_widget_paint::stat_tile::Row`]'s
// reason: a caller describing a chooser wants *which parts are there*, and a
// list of them beside this module is what goes stale when a part is added.

/// One part of a collapsed chooser, as **this screen** addresses it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ChooserPart {
    /// The control itself — the box a press lands on.
    Control,
    /// The word the control is showing.
    Shown,
    /// The chevron that says there is a roster behind it.
    Arrow,
}

impl ChooserPart {
    /// Every part, in the order a reader meets them.
    pub const ALL: &'static [Self] = &[Self::Control, Self::Shown, Self::Arrow];

    /// The word this part's addresses carry.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::Control => "choose",
            Self::Shown => "shown",
            Self::Arrow => "arrow",
        }
    }

    /// The stem every one of this part's addresses shares, under `prefix`.
    #[must_use]
    pub fn stem(self, prefix: &str) -> String {
        format!("{prefix}.{}.", self.word())
    }

    /// This part's address for the chooser `key`, under `prefix`.
    #[must_use]
    pub fn tag(self, prefix: &str, key: &str) -> String {
        format!("{}{key}", self.stem(prefix))
    }
}

/// The prefix every collapsed chooser is painted under.
pub const SETTINGS_CHOOSE: &str = "shell.settings.choose.";

/// [`SETTINGS_CHOOSE`] with the population's placeholder — see
/// [`SETTINGS_HEAD_TEMPLATE`].
pub const SETTINGS_CHOOSE_TEMPLATE: &str = "shell.settings.choose.{}";

/// The address of the collapsed control on the value row `key`.
#[must_use]
pub fn settings_choose(key: impl AsRef<str>) -> String {
    ChooserPart::Control.tag(SETTINGS, key.as_ref())
}

/// The prefix the word a collapsed chooser is showing is painted under.
pub const SETTINGS_SHOWN: &str = "shell.settings.shown.";

/// [`SETTINGS_SHOWN`] with the population's placeholder.
pub const SETTINGS_SHOWN_TEMPLATE: &str = "shell.settings.shown.{}";

/// The address of the word the chooser on the value row `key` is showing.
#[must_use]
pub fn settings_shown(key: impl AsRef<str>) -> String {
    ChooserPart::Shown.tag(SETTINGS, key.as_ref())
}

/// The prefix every chooser's chevron is painted under.
pub const SETTINGS_ARROW: &str = "shell.settings.arrow.";

/// [`SETTINGS_ARROW`] with the population's placeholder.
pub const SETTINGS_ARROW_TEMPLATE: &str = "shell.settings.arrow.{}";

/// The address of the chevron on the value row `key`.
#[must_use]
pub fn settings_arrow(key: impl AsRef<str>) -> String {
    ChooserPart::Arrow.tag(SETTINGS, key.as_ref())
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

// --- the card -----------------------------------------------------------------
//
// ★★★★★ R2185 §5.2 — **a placed card's family: its prefix, its id, and the
// chrome every card shares.**
//
// Measured at entry: the census charged the template family `card.{}` 57 Rust
// readers and 20 walk sites (R2184), and grouping those readers by function
// showed R2180's defect across the whole card rather than one feed. The chrome
// was composed by the header painter and then again by the hit test
// (`hit_word`), the accessibility tree (`card_nodes`, `card_chrome_nodes`) and
// the page's descriptions; the card's prefix was declared a second time
// (`judge.rs`'s `BOARD`); and the id grammar was composed by the specification
// and parsed back by hand in `main.rs`.
//
// # Who owns what
//
// The grip and the four affordance controls are the CRATE's composition —
// `card_header::{grip_tag, affordance_tag}` — and this module places them under
// a card without restating them. The card's own address and id, the tab a
// shared place draws for an occupant, the strip holding those tabs, the
// configuration panel, the remedy, the edit bar and its steppers, and the filter
// card's chips are painted by this screen, so they are declared here.
//
// ⚠ Not here yet, and said: the parts a card's BODY paints (`stat.{n}`, `bins`,
// `tree.{n}`, `bytes.{n}`, a table's `grid` and `head`) — later instalments, a
// body at a time — and `widen`, a remedy's wire word whose painter this round
// did not measure.

/// The prefix every placed card is addressed under.
pub const CARD: &str = "card.";

/// A placed card's id: its kind, then its place on the board — `alarms#6`.
///
/// The kind so the definition is recoverable without a side table; the ordinal
/// so one kind can be placed more than once.
#[must_use]
pub fn card_id(kind: &str, ordinal: impl std::fmt::Display) -> String {
    format!("{kind}#{ordinal}")
}

/// The kind a card id names — [`card_id`] read back.
#[must_use]
pub fn card_kind(id: &str) -> &str {
    id.split_once('#').map_or(id, |(kind, _)| kind)
}

/// The card `id`, as painted.
#[must_use]
pub fn card(id: &str) -> String {
    format!("{CARD}{id}")
}

/// The card's drag handle — the header crate's composition, under this card.
#[must_use]
pub fn card_grip(id: &str) -> String {
    pinion_widget_paint::card_header::grip_tag(&card(id))
}

/// One of the card's affordance controls — the header crate's composition.
#[must_use]
pub fn card_affordance(id: &str, affordance: pinion_core::widgets::card::CardAffordance) -> String {
    pinion_widget_paint::card_header::affordance_tag(&card(id), affordance)
}

/// The tab a shared place draws for the occupant `id`.
///
/// Named by the OCCUPANT rather than by the card in front, for the reason
/// `card_header::HeaderTab::tag` gives.
#[must_use]
pub fn card_tab(id: &str) -> String {
    format!("{}.tab", card(id))
}

/// The strip holding a shared place's tabs, on the card in front.
#[must_use]
pub fn card_tabs(id: &str) -> String {
    format!("{}.tabs", card(id))
}

/// The card's own configuration panel.
#[must_use]
pub fn card_config(id: &str) -> String {
    format!("{}.config", card(id))
}

/// The control a card offers for what its state lacks.
#[must_use]
pub fn card_remedy(id: &str) -> String {
    format!("{}.remedy", card(id))
}

/// The bar a card in editing shows.
#[must_use]
pub fn card_edit_bar(id: &str) -> String {
    format!("{}.editbar", card(id))
}

/// One stepper on that bar, by its verb.
#[must_use]
pub fn card_stepper(id: &str, verb: &str) -> String {
    format!("{}.{verb}", card(id))
}

/// The prefix every chip of the card `id` carries — what a press is read back
/// through.
#[must_use]
pub fn card_chip_prefix(id: &str) -> String {
    format!("{}.chip.", card(id))
}

/// The card's chip `n`.
#[must_use]
pub fn card_chip(id: &str, n: impl std::fmt::Display) -> String {
    format!("{}{n}", card_chip_prefix(id))
}

/// The bar the card's chips sit in — the group, not a chip.
#[must_use]
pub fn card_chips(id: &str) -> String {
    format!("{}.chips", card(id))
}

// --- the card's body ------------------------------------------------------------
//
// ★★★★★ R2186 §5.2 — **what a card's BODY paints**, declared where the chrome
// was (R2185).
//
// Measured at entry: `card.{}` kept 43 Rust readers after R2185, and every one
// was in a body painter or its describing twin — `latency_body` /
// `latency_nodes`, `filter_body` and `filter_counts` / `filter_nodes`,
// `health_body` / `health_nodes`, `byte_pane` / `byte_nodes`, `decode_body` /
// `decode_nodes`, the table bodies / `table_nodes` — each writing the same parts
// twice, and the card region's own list of body roots writing them a third time
// as suffix words.
//
// # Who owns what
//
// A stat tile's rows and trail are the CRATE's composition
// (`pinion_widget_paint::stat_tile`); this module places the one a reader names —
// the trail, under which the shell paints its sparkline — through
// `stat_tile::trail_tag`. A table's grid, rows and cells are handed to
// `pinion_a11y::grid` WHOLE, so they are this screen's; the cell and heading
// stems moved here from `main.rs`, where R1873 had given them one home shared
// with a paint test.

/// The heading row of a table card.
#[must_use]
pub fn card_head(id: &str) -> String {
    format!("{}.head", card(id))
}

/// The stem every column heading of a table card is addressed under.
#[must_use]
pub fn card_head_cell_stem(id: &str) -> String {
    format!("{}.", card_head(id))
}

/// One column heading of a table card.
#[must_use]
pub fn card_head_cell(id: &str, column: impl std::fmt::Display) -> String {
    format!("{}{column}", card_head_cell_stem(id))
}

/// The stem every cell of a table card is addressed under.
#[must_use]
pub fn card_cell_stem(id: &str) -> String {
    format!("{}.cell.", card(id))
}

/// One cell of a table card.
#[must_use]
pub fn card_cell(id: &str, row: impl std::fmt::Display, column: impl std::fmt::Display) -> String {
    format!("{}{row}_{column}", card_cell_stem(id))
}

/// A table card's grid, as a reader meets it.
#[must_use]
pub fn card_grid(id: &str) -> String {
    format!("{}.grid", card(id))
}

/// One data row of a packet table.
#[must_use]
pub fn card_row(id: &str, row: impl std::fmt::Display) -> String {
    format!("{}.row.{row}", card(id))
}

/// One data row of the key map.
#[must_use]
pub fn card_map_row(id: &str, row: impl std::fmt::Display) -> String {
    format!("{}.map.{row}", card(id))
}

/// A card's strip of stat tiles.
#[must_use]
pub fn card_tiles(id: &str) -> String {
    format!("{}.tiles", card(id))
}

/// One stat tile of a card.
#[must_use]
pub fn card_stat(id: &str, n: impl std::fmt::Display) -> String {
    format!("{}.stat.{n}", card(id))
}

/// The prefix a tile's trailing sparkline is tagged under, given the TILE's own
/// address — inside the tile's trail, which is the crate's composition.
///
/// ★ R2223 — the tile-relative form is the primitive and [`card_stat_spark`]
/// composes it, because a caller enumerating a tile's parts holds the tile and
/// not the `(card, n)` pair it was built from. Before this the `.spark` word
/// had one home and the enumeration a second one.
#[must_use]
pub fn stat_spark_under(tile: &str) -> String {
    format!(
        "{}.spark",
        pinion_widget_paint::stat_tile::Row::Trail.tag(tile)
    )
}

/// The prefix a tile's trailing sparkline is tagged under — inside the tile's
/// trail, which is the crate's composition.
#[must_use]
pub fn card_stat_spark(id: &str, n: impl std::fmt::Display) -> String {
    stat_spark_under(&card_stat(id, n))
}

/// The latency card's distribution chart, as the chart's tag prefix.
#[must_use]
pub fn card_dist(id: &str) -> String {
    format!("{}.dist", card(id))
}

/// The box that holds that distribution.
#[must_use]
pub fn card_bins(id: &str) -> String {
    format!("{}.bins", card(id))
}

/// The latency card's caption.
#[must_use]
pub fn card_caption(id: &str) -> String {
    format!("{}.caption", card(id))
}

/// The filter card's query field.
#[must_use]
pub fn card_query(id: &str) -> String {
    format!("{}.query", card(id))
}

/// The filter card's match counts.
#[must_use]
pub fn card_counts(id: &str) -> String {
    format!("{}.counts", card(id))
}

/// The filter card's sparkline.
#[must_use]
pub fn card_sparkline(id: &str) -> String {
    format!("{}.sparkline", card(id))
}

/// The decode card's layer tree.
#[must_use]
pub fn card_tree(id: &str) -> String {
    format!("{}.tree", card(id))
}

/// One row of that tree.
#[must_use]
pub fn card_tree_row(id: &str, n: impl std::fmt::Display) -> String {
    format!("{}.{n}", card_tree(id))
}

/// The decode card's byte grid.
#[must_use]
pub fn card_bytegrid(id: &str) -> String {
    format!("{}.bytegrid", card(id))
}

/// One line of that grid.
#[must_use]
pub fn card_bytes(id: &str, line: impl std::fmt::Display) -> String {
    format!("{}.bytes.{line}", card(id))
}

/// One byte of that grid.
#[must_use]
pub fn card_byte(id: &str, index: impl std::fmt::Display) -> String {
    format!("{}.byte.{index}", card(id))
}

/// A placeholder card's code line.
#[must_use]
pub fn card_code(id: &str) -> String {
    format!("{}.code", card(id))
}

/// The body roots a card region names as its children — each one a composer
/// above, so this list cannot spell a part the painter does not.
///
/// ★ R2186 — this was `BODY_ROOTS` in `main.rs`, nine suffix WORDS compared
/// against `format!("card.{id}.{suffix}")`: a third spelling of every root, beside
/// the painter's and the describer's.
pub const CARD_BODY_ROOTS: &[fn(&str) -> String] = &[
    card_grid,
    card_tree,
    card_bytegrid,
    card_query,
    card_chips,
    card_counts,
    card_sparkline,
    card_tiles,
    card_bins,
];

// --- the detached panel ---------------------------------------------------------
//
// ★★★★★ R2225 §5.2 §5.11 — **the family this module's own artifact named as
// next, and the only one in this screen with NO declaring site at all.**
//
// Every other family here arrived the same way: the addresses were spelled at
// each reader, a round gathered them into a composer, and the composer became
// the one place a letter could be wrong. The detached panel never had that
// round. Measured at entry: **7 sites in `main.rs`**, 25 in the paint sweep, 1
// in the crate's tests and 6 across two walks — and the seven in `main.rs` are
// not seven copies of one composition, they are TWO compositions of one
// grammar that a reader has to hold side by side to see agree:
//
//   * the paint tags a control `float.{id}.{}` from `offered.wire()`;
//   * the hit test answers for the same control, and of its three lines ONE
//     called `wire()` while two spelled `redock` and `close` as literals —
//     under a comment stating the invariant the other two break: *the tag is
//     the affordance's own wire word, so what a pointer answers and what the
//     paint tagged are one name.*
//
// ⇒ the comment was the only thing holding it. A letter changed in
// `DetachedAffordance::wire` moves the paint and leaves the hit test behind:
// the control is drawn, the pointer answers a name nothing painted, and every
// later assertion reads that as *the panel did not draw its redock control*.
// Nothing in the tree could have caught it, because the two sites agreeing was
// never something anything compared.

/// The prefix every detached panel's address carries.
pub const FLOAT: &str = "float.";

// ⚠⚠ **Deliberately no `FLOAT_TEMPLATE` and no `float_card_id`**, and the
// absence is a judgement rather than an omission. Both were written this round
// — the siblings above have each — and the workspace's `dead_code` lint refused
// them, because nothing in this binary reads a float address back or lists one
// in a `&'static str` table. R2176's finding is why that refusal is respected
// rather than silenced: a composer nobody calls is published API that no
// consumer holds to its shape, and one `pub` word turns the only thing that
// would have said so off. The parse belongs here the day something parses.

/// The detached panel showing the card `id`.
///
/// ⚠ The id is the CARD's — a float is a card that left the board, so it keeps
/// the id the board gave it and a reader can follow one panel across the move.
#[must_use]
pub fn float(id: &str) -> String {
    format!("{FLOAT}{id}")
}

/// One control in the panel's header, named by the affordance's own wire word.
///
/// ★★★★★ The invariant R1907's comment stated and nothing enforced: the paint
/// and the hit test compose through THIS, so they cannot name a control
/// differently. `wire()` is called here once instead of at each of them.
#[must_use]
pub fn float_affordance(id: &str, affordance: pinion_core::detach::DetachedAffordance) -> String {
    format!("{}.{}", float(id), affordance.wire())
}

/// The badge that says this panel is not on the board.
#[must_use]
pub fn float_badge(id: &str) -> String {
    format!("{}.badge", float(id))
}

/// The corner a person drags to resize the panel.
///
/// ⚠ Its own composer rather than an affordance, and the asymmetry is the
/// panel's rather than this module's: the header controls come from a policy
/// roster that can drop one, and the resize corner is drawn by this screen
/// unconditionally. A reader that treated it as an affordance would count the
/// header's controls wrong — [`rail_account`]'s distinction, one family over.
#[must_use]
pub fn float_resize(id: &str) -> String {
    format!("{}.resize", float(id))
}

// --- the emitted grammar --------------------------------------------------------

/// The committed artifact's body: every CARD address grammar this screen
/// paints, as text.
///
/// ★★★★★ R2217 §5.2 — **a walk is Python and cannot call any of the above.**
/// Everything under this heading composes an address the walks then RETYPE:
/// measured at entry, 85 sites across 12 walks spell `card.{id}.grip` and its
/// siblings as literals, which is the campaign's own defect one language out.
/// `pinion-chart` answered the same question at R2146 by EMITTING its grammar
/// as a tracked file, and `tools/painted_grammar.py` already globs
/// `examples/*/src/painted_grammar.tsv` — a reader waiting for an artifact no
/// example had ever written.
///
/// ⚠ The templates are **DERIVED, not restated**: each row is what the
/// composer itself returns when handed `{id}` (and `{n}`, `{row}`, …) instead
/// of a value. A table of literals beside the composers would be a second
/// spelling that can drift; calling them cannot. That is also why two
/// composers widened to `impl Display` in this round — a `usize` parameter
/// cannot carry a placeholder, and the signature that can is the one its
/// siblings already had.
///
/// The rows are `kind<TAB>name<TAB>value`, sorted, so a diff of the artifact
/// is a diff of the grammar.
///
/// ⚠ `cfg(test)`, unlike `pinion_chart::address::render_grammar`, and the
/// difference is the target: that one is a LIBRARY whose callers are outside
/// it, this one is a BINARY, where a function nobody in the binary calls is
/// dead code the workspace lints refuse. What renders the artifact is the test
/// that regenerates and compares it, so that is where the renderer lives.
/// The placeholder a template carries where a card id goes.
const GRAMMAR_ID: &str = "{id}";

/// The chrome a card paints: its own address and everything in its header.
fn chrome_rows() -> Vec<(&'static str, String, String)> {
    let id = GRAMMAR_ID;
    vec![
        ("const", "CARD".to_owned(), CARD.to_owned()),
        // ⚠ `id`, not `grammar`: this composes an ARGUMENT for an address
        // (`decode#3`), not an address, so the reader's separator rule is
        // right to refuse it as a template — see `painted_grammar.KINDS`.
        ("id", "card_id".to_owned(), card_id("{kind}", "{ordinal}")),
        ("grammar", "card".to_owned(), card(id)),
        ("grammar", "card_grip".to_owned(), card_grip(id)),
        (
            "grammar",
            "card_affordance".to_owned(),
            format!("{}.{{affordance}}", card(id)),
        ),
        ("grammar", "card_tab".to_owned(), card_tab(id)),
        ("grammar", "card_tabs".to_owned(), card_tabs(id)),
        ("grammar", "card_config".to_owned(), card_config(id)),
        ("grammar", "card_remedy".to_owned(), card_remedy(id)),
        ("grammar", "card_edit_bar".to_owned(), card_edit_bar(id)),
        (
            "grammar",
            "card_stepper".to_owned(),
            card_stepper(id, "{verb}"),
        ),
        (
            "grammar",
            "card_chip_prefix".to_owned(),
            card_chip_prefix(id),
        ),
        ("grammar", "card_chip".to_owned(), card_chip(id, "{n}")),
        ("grammar", "card_chips".to_owned(), card_chips(id)),
    ]
}

/// The parts a card's BODY paints — the R2186 half of the declaration.
fn body_rows() -> Vec<(&'static str, String, String)> {
    let id = GRAMMAR_ID;
    vec![
        ("grammar", "card_head".to_owned(), card_head(id)),
        (
            "grammar",
            "card_head_cell_stem".to_owned(),
            card_head_cell_stem(id),
        ),
        (
            "grammar",
            "card_head_cell".to_owned(),
            card_head_cell(id, "{column}"),
        ),
        ("grammar", "card_cell_stem".to_owned(), card_cell_stem(id)),
        (
            "grammar",
            "card_cell".to_owned(),
            card_cell(id, "{row}", "{column}"),
        ),
        ("grammar", "card_grid".to_owned(), card_grid(id)),
        ("grammar", "card_row".to_owned(), card_row(id, "{row}")),
        (
            "grammar",
            "card_map_row".to_owned(),
            card_map_row(id, "{row}"),
        ),
        ("grammar", "card_tiles".to_owned(), card_tiles(id)),
        ("grammar", "card_stat".to_owned(), card_stat(id, "{n}")),
        (
            "grammar",
            "card_stat_spark".to_owned(),
            card_stat_spark(id, "{n}"),
        ),
        ("grammar", "card_dist".to_owned(), card_dist(id)),
        ("grammar", "card_bins".to_owned(), card_bins(id)),
        ("grammar", "card_caption".to_owned(), card_caption(id)),
        ("grammar", "card_query".to_owned(), card_query(id)),
        ("grammar", "card_counts".to_owned(), card_counts(id)),
        ("grammar", "card_sparkline".to_owned(), card_sparkline(id)),
        ("grammar", "card_tree".to_owned(), card_tree(id)),
        (
            "grammar",
            "card_tree_row".to_owned(),
            card_tree_row(id, "{n}"),
        ),
        ("grammar", "card_bytegrid".to_owned(), card_bytegrid(id)),
        ("grammar", "card_bytes".to_owned(), card_bytes(id, "{line}")),
        ("grammar", "card_byte".to_owned(), card_byte(id, "{index}")),
        ("grammar", "card_code".to_owned(), card_code(id)),
    ]
}

/// Every part of a card's configuration chooser, as **one row per part**.
///
/// ★★★★★ R2223 §5.2 — a card offers its own settings under `card.{id}.config`,
/// each one a collapsed chooser, and neither channel said so: the preferences
/// page publishes its three chooser stems on `settings_addresses` and a card's
/// were published nowhere at all. A walk driving a card's configuration panel
/// therefore had to spell `…config.shown.{key}` — the case this round is
/// named for.
///
/// ⚠ **Rows, not `part` rows.** The format's `part` kind means *one word
/// `{affordance}` can be*, and putting `choose` beside `close` there would make
/// a reader asking what an affordance can be receive three words that are not
/// affordances — one name over two populations, which is the shape this
/// project has paid for repeatedly. A part word that composes a whole address
/// IS a grammar; that it happens to be one word is not the reader's problem.
///
/// Generated from [`ChooserPart::ALL`], so a part added to the vocabulary is
/// published by adding it and nothing else.
fn card_setting_rows() -> Vec<(&'static str, String, String)> {
    let under = card_config(GRAMMAR_ID);
    ChooserPart::ALL
        .iter()
        .map(|part| {
            (
                "grammar",
                format!("card_setting_{}", part.word()),
                part.tag(&under, "{key}"),
            )
        })
        .collect()
}

/// Every word a CARD's `{affordance}` can be, as `part` rows — the kind the
/// format already has for "one word a placeholder can be".
///
/// ★★★★★ R2225 — **the row's name carries the family now**, and this is the
/// change that let a second family into this artifact at all. A `part` name is
/// a BARE WORD where a `grammar` name is already family-qualified
/// (`card_grip`, `card_cell`), and a card's `close` and a detached panel's
/// `close` are two addresses under one name. The reader kept whichever sorted
/// last — silently, composing an address for the wrong family, which a walk
/// reads as *the screen did not paint it*. `painted_grammar.table` refuses the
/// collision as of this round; this is the publisher's half, and it is the one
/// that makes the refusal something no artifact ever has to hit.
/// ⚠ R2225.1 — **from `CardAffordance::ALL`, which existed the whole time.**
/// R2225 listed the four by hand here while the float rows beside it derived
/// from `DetachedAffordance::ALL` and said so as a rule — *a word added to that
/// vocabulary is published by adding it and nothing else*. One round, one file,
/// the rule applied to one family. Counted at the repair: this was the ONLY
/// site in the tree enumerating the whole card vocabulary by hand; nine others
/// already read `ALL`, two of them in this same binary.
fn card_affordance_rows() -> Vec<(&'static str, String, String)> {
    use pinion_core::widgets::card::CardAffordance;

    CardAffordance::ALL
        .into_iter()
        .map(|affordance| {
            (
                "part",
                format!("card_{}", affordance.wire()),
                card_affordance(GRAMMAR_ID, affordance),
            )
        })
        .collect()
}

/// Every DETACHED PANEL address grammar this screen paints.
///
/// ★★★★★ R2225 §5.2 — the second family in this artifact. Its `part` rows are
/// derived from [`pinion_core::detach::DetachedAffordance::ALL`] rather than
/// listed, so a word added to that vocabulary is published by adding it and
/// nothing else — and `ALL` is the VOCABULARY rather than a policy's roster,
/// because a walk composes an address for a control before it knows which host
/// will be asked.
fn float_rows() -> Vec<(&'static str, String, String)> {
    let id = GRAMMAR_ID;
    let mut rows = vec![
        ("const", "FLOAT".to_owned(), FLOAT.to_owned()),
        ("grammar", "float".to_owned(), float(id)),
        (
            "grammar",
            "float_affordance".to_owned(),
            format!("{}.{{affordance}}", float(id)),
        ),
        ("grammar", "float_badge".to_owned(), float_badge(id)),
        ("grammar", "float_resize".to_owned(), float_resize(id)),
    ];
    rows.extend(
        pinion_core::detach::DetachedAffordance::ALL
            .iter()
            .map(|affordance| {
                (
                    "part",
                    format!("float_{}", affordance.wire()),
                    float_affordance(id, *affordance),
                )
            }),
    );
    rows
}

/// Every card grammar row this screen publishes, sorted — the ONE table both
/// channels read.
///
/// ★★★★★ R2219 §5.2 — **one fact, two publications, and until this round they
/// could disagree.** A screen here can publish what it paints two ways: over
/// the WIRE, in `spec["declared_addresses"]` (R2171, read by
/// `rpc_verify.declared_address`), and as an EMITTED artifact (R2217, read by
/// `rpc_verify.painted_address`). Nothing joined them, and the gap is not
/// hypothetical: the wire row carried one family (`carry`, nine keys hand-
/// listed in `main.rs`) while the card family — 85 retyped walk sites — was in
/// neither channel until R2217 put it in one.
///
/// So the rows are built ONCE here. The artifact renders them; the wire
/// publishes them; a test asserts the two carry the same table. A hand-written
/// second copy could drift, and the copy that drifts is the one nobody reads.
///
/// ★★★★★ R2225 — **`grammar_rows`, not `card_grammar_rows`.** It held one
/// family when it was named, and the artifact's header said the rest were the
/// next instalments; the detached panel is the first of them. A name that
/// asserts a population is the shape this project keeps paying for — R2129's
/// two checks over one population, R2122's one name over two — so the name
/// widened in the same round the population did rather than one round later.
#[must_use]
pub fn grammar_rows() -> Vec<(&'static str, String, String)> {
    let mut rows = chrome_rows();
    rows.extend(body_rows());
    rows.extend(card_setting_rows());
    rows.extend(card_affordance_rows());
    rows.extend(float_rows());
    rows.sort_unstable();
    rows
}

/// The same table, as the wire publishes it: `kind -> name -> value`.
#[must_use]
pub fn grammar_json() -> serde_json::Value {
    let mut out = serde_json::Map::new();
    for (kind, name, value) in grammar_rows() {
        let entry = out
            .entry(kind.to_owned())
            .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
        if let Some(map) = entry.as_object_mut() {
            map.insert(name, serde_json::Value::String(value));
        }
    }
    serde_json::Value::Object(out)
}

#[cfg(test)]
#[must_use]
fn render_grammar() -> String {
    let rows = grammar_rows();

    let mut out = String::from(
        "# Every address grammar this screen paints for a CARD and for a\n\
         # DETACHED PANEL, emitted from the composers in `src/address.rs` by\n\
         # handing each one a placeholder.\n\
         # Rewritten by setting PINION_REGEN_ADDRESS_PIN and running this\n\
         # example's `the_committed_grammar_is_what_the_composers_compose`\n\
         # test; do not hand-edit.\n\
         #\n\
         # kind<TAB>name<TAB>value -- const | grammar | id | part.\n\
         # A `grammar` value is a template: format it with the placeholders it\n\
         # carries. A `part` row is one word `{affordance}` can be. The `id`\n\
         # row composes an ARGUMENT for an address, not an address.\n\
         #\n\
         # ⚠ A `part` name carries its FAMILY (`card_close`, `float_close`).\n\
         # The word alone is what a card and a detached panel both offer, and\n\
         # a reader handed one for the other composes an address naming\n\
         # nothing — `painted_grammar.table` refuses such a row outright.\n\
         #\n\
         # ⚠ This screen's rail, palette and settings grammars are the next\n\
         # instalments; their absence here is unemitted, not unpainted.\n",
    );
    for (kind, name, value) in rows {
        out.push_str(kind);
        out.push('\t');
        out.push_str(&name);
        out.push('\t');
        out.push_str(&value);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod grammar_tests {
    use super::render_grammar;

    /// ★★★★★ R2217 — the committed artifact is what the composers compose.
    ///
    /// Regenerate with `PINION_REGEN_ADDRESS_PIN=1 cargo test -p
    /// hello-analyzer-shell`, the workspace's one regeneration flag.
    ///
    /// ⚠ **LOCALLY** — R2146 measured that the remote build wrapper forwards
    /// no environment variable of its caller's, so a regeneration through it
    /// does not regenerate, and the variable reaching the far side would write
    /// the artifact where the next sync cannot see it.
    #[test]
    fn the_committed_grammar_is_what_the_composers_compose() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/src/painted_grammar.tsv");
        let rendered = render_grammar();
        // ★ A floor, not decoration: an empty render would agree with an empty
        // artifact, which is a gate green for having asked nothing.
        assert!(
            rendered.lines().filter(|l| !l.starts_with('#')).count() >= 35,
            "the render holds too few rows to be this screen's card grammar"
        );
        if std::env::var_os(pinion_core::REGEN_ADDRESS_PIN).is_some() {
            std::fs::write(path, &rendered).expect("the artifact is writable");
            return;
        }
        let committed = std::fs::read_to_string(path).expect("the artifact is committed");
        assert_eq!(
            committed, rendered,
            "★★★★★ this screen's painted card grammar changed. A reader outside \
             Rust formats these templates instead of spelling an address, so a \
             change here is a PUBLISHED change. If it is intended, set \
             PINION_REGEN_ADDRESS_PIN and re-run this test."
        );
    }

    /// ★★★★★ R2219 — the WIRE and the ARTIFACT carry the same table.
    ///
    /// This screen publishes what it paints twice: `spec["declared_addresses"]`
    /// for a reader driving the live app, and the committed artifact for one
    /// reading the tree. Both are worth having — an agent mid-session cannot
    /// read a file, and a walk composing before launch cannot ask a process —
    /// but two publications of one fact that nothing joins is a divergence
    /// waiting to happen, and this campaign has paid for that shape twice
    /// already (R2129's two populations, R2217's two kind-vocabularies).
    ///
    /// The join is that both render `grammar_rows`. This asserts it in
    /// both directions: every row reaches the JSON under its own kind, the JSON
    /// carries nothing the rows do not, and the artifact's text carries each
    /// row verbatim.
    #[test]
    fn the_wire_and_the_artifact_publish_one_table() {
        let rows = super::grammar_rows();
        // ⚠ The WIRE ROW ITSELF, not this module's view of it. Comparing
        // `grammar_json` with `grammar_rows` would be two readings of
        // one function agreeing with itself; what can actually regress is
        // `declared_addresses_json` going back to a hand-written list, which is
        // the shape it had for the `carry` family. So the assertion reaches for
        // what the screen publishes.
        let published_row = crate::declared_addresses_json();
        let json = published_row
            .get("painted")
            .cloned()
            .expect("the screen publishes its painted grammar on the wire");
        assert_eq!(
            json,
            super::grammar_json(),
            "the published row is not the table this module builds — a second \
             hand-written copy is exactly what R2219 removed"
        );
        let rendered = super::render_grammar();
        assert!(
            rows.len() >= 48,
            "{} rows is not this screen's painted grammar",
            rows.len()
        );
        for (kind, name, value) in &rows {
            let published = json
                .get(kind)
                .and_then(|by_name| by_name.get(name))
                .and_then(serde_json::Value::as_str);
            assert_eq!(
                published,
                Some(value.as_str()),
                "the wire does not publish {kind}/{name}, which the artifact does"
            );
            assert!(
                rendered.contains(&format!("{kind}\t{name}\t{value}\n")),
                "the artifact does not carry {kind}/{name}, which the wire does"
            );
        }
        let published: usize = json
            .as_object()
            .map(|kinds| {
                kinds
                    .values()
                    .filter_map(serde_json::Value::as_object)
                    .map(serde_json::Map::len)
                    .sum()
            })
            .unwrap_or_default();
        assert_eq!(
            published,
            rows.len(),
            "the wire publishes {published} row(s) and the table holds {} — a \
             channel carrying what the other does not is the divergence this \
             round removed",
            rows.len()
        );
    }

    /// Every `pub fn <family>…() -> String` in `source`, in the order written.
    ///
    /// Its own function so the case below can hand it a fixture: held only
    /// against `address.rs`, whether this reads a signature or a line depends
    /// on what rustfmt did to that file this week.
    fn composers_of(source: &str, family: &str) -> Vec<String> {
        let padded = format!("\n{source}");
        let opening = format!("\npub fn {family}");
        let mut out = Vec::new();
        for (start, _) in padded.match_indices(&opening) {
            let head = &padded[start + 1..];
            let Some(body) = head.find(" {") else {
                continue;
            };
            let signature = &head[..body];
            // ★ The return type is the discriminator, not the name: a
            // `-> Option<&str>` beside these is a PARSER and composes nothing,
            // so it is right for the grammar not to carry it.
            if !signature.ends_with("-> String") {
                continue;
            }
            if let Some(name) = signature
                .strip_prefix("pub fn ")
                .and_then(|rest| rest.split('(').next())
            {
                out.push(name.to_owned());
            }
        }
        out
    }

    /// ★★★★★ Every composer of an EMITTED family is IN the grammar.
    ///
    /// Derived from this file's own source rather than from a list beside it:
    /// R2145 measured that a list of needles is what goes stale, and the
    /// composer added tomorrow is exactly the row nobody would remember to
    /// add. The rule is `pub fn <family>…() -> String` — a composer returns an
    /// address; `card_kind` and `float_card_id` return borrowed slices and are
    /// PARSERS, which is why the return type is the discriminator rather than
    /// the name.
    ///
    /// ★★ R2225 — **over the emitted families, not over `card` alone.** The
    /// scan read `pub fn card` while one family was emitted, and a gate keyed
    /// to one member of a growing population keeps passing while covering less
    /// of it: the detached panel's five composers would have been outside it
    /// from the round they were written. The families are listed once, here,
    /// and the floor is per-family so a family that stops being scanned says so
    /// instead of being absorbed by the other's count.
    #[test]
    fn every_composer_appears_in_the_grammar() {
        let source = include_str!("address.rs");
        let rendered = render_grammar();
        let named: Vec<&str> = rendered
            .lines()
            .filter(|line| !line.starts_with('#'))
            .filter_map(|line| line.split('\t').nth(1))
            .collect();
        // ★★★★★ R2225 — the scan reads a WHOLE SIGNATURE, not a line.
        //
        // It was `line.strip_prefix("pub fn card") … line.ends_with("-> String
        // {")`, and every card composer happens to fit one line, so the limit
        // was invisible: this round's `float_affordance` takes a fully-qualified
        // enum, rustfmt broke it over four lines, and the composer left the gate
        // silently — the floor below is the only thing that said so. A gate
        // whose reach depends on a formatter's line budget is one that stops
        // covering whatever grows a longer argument.
        // ★★★★★ R2225 — the SHIPPED half of the file, and the cut is not
        // tidiness. `composers_of` is a text scan, this module's own fixture
        // below spells `pub fn card_one(…) -> String`, and the scan read it and
        // demanded the grammar publish it. The population this gate asks about
        // is what the BINARY carries; everything from the test module on is a
        // second population that happens to share the file.
        let shipped = source
            .split_once("#[cfg(test)]")
            .map_or(source, |(before, _)| before);
        for (family, floor) in [("card", 35usize), ("float", 4)] {
            let composers = composers_of(shipped, family);
            for name in &composers {
                assert!(
                    named.contains(&name.as_str()),
                    "{name} composes an address and is not in the emitted \
                     grammar — a reader outside Rust cannot format what this \
                     does not publish"
                );
            }
            assert!(
                composers.len() >= floor,
                "only {} {family} composer(s) found; this scan is not reading \
                 this module",
                composers.len()
            );
        }
    }

    /// ★★★★★ R2225 — the scan above reads a WHOLE SIGNATURE, not a line, and
    /// THIS is what can fail when it stops doing so.
    ///
    /// It was `line.strip_prefix("pub fn card") … line.ends_with("-> String
    /// {")`, and every card composer happens to fit one line, so the limit was
    /// invisible. This round's `float_affordance` takes a fully-qualified enum;
    /// written before it was formatted it spanned four lines and left the gate
    /// silently, which the per-family floor is the only thing that reported.
    ///
    /// ⚠⚠ **And rustfmt then put it back on one line**, which is exactly why
    /// this case is synthetic. Held against `address.rs` alone the widening has
    /// no failing path today — a formatter's line budget decides whether the
    /// gate is being exercised, and a gate nothing can fail is not a gate. The
    /// fixture below fails the moment the scan goes back to reading lines,
    /// whatever this file happens to look like.
    #[test]
    fn the_composer_scan_reads_a_signature_and_not_a_line() {
        const FIXTURE: &str = "\
pub fn card_one(id: &str) -> String { String::new() }
pub fn card_two(
    id: &str,
    affordance: some::very::long::Path,
) -> String {
    String::new()
}
pub fn card_kind(id: &str) -> &str { id }
pub fn card_three(
    id: &str,
) -> Option<&str> {
    None
}
fn card_private(id: &str) -> String { String::new() }
";
        let found = composers_of(FIXTURE, "card");
        assert_eq!(
            found,
            vec!["card_one".to_owned(), "card_two".to_owned()],
            "the scan reads a one-line composer AND a wrapped one, and takes \
             neither the parser (`-> &str`, `-> Option<&str>`) nor the private \
             function"
        );
        // ★ The floor's other half: a family with no composer answers empty
        // rather than raising, so the per-family floor above is what reports it.
        assert!(composers_of(FIXTURE, "float").is_empty());
    }

    /// ★★★★★ R2225.1 — every word of every affordance vocabulary is published,
    /// and nothing else is.
    ///
    /// **This is the gate whose absence let R2225 hand-list one family.** The
    /// composer scan above asks about `grammar` rows and nothing asked about
    /// `part` ones, so a `part` builder that emitted three of four words — or
    /// that went on emitting a word the vocabulary had dropped — passed every
    /// gate in this file. Deriving the rows from `ALL` fixes today's instance;
    /// this is what makes a hand-written list fail tomorrow.
    ///
    /// ⚠ Asserted in BOTH directions. *Every word is published* alone would
    /// pass a builder that also published surplus rows, and *nothing else* alone
    /// would pass one that published none — and the cheapest wrong repair to a
    /// red here is to delete a row, which only the first direction refuses.
    ///
    /// ⚠ The two vocabularies are named here rather than derived, and that is
    /// the honest place to stop: *which* families this screen paints is this
    /// screen's own fact. What must not be restated is their CONTENTS, which is
    /// what `ALL` answers.
    #[test]
    fn every_affordance_word_is_published_exactly_once() {
        use pinion_core::detach::DetachedAffordance;
        use pinion_core::widgets::card::CardAffordance;

        let published: Vec<String> = super::grammar_rows()
            .into_iter()
            .filter(|(kind, _, _)| *kind == "part")
            .map(|(_, name, _)| name)
            .collect();

        let mut wanted: Vec<String> = CardAffordance::ALL
            .iter()
            .map(|a| format!("card_{}", a.wire()))
            .collect();
        wanted.extend(
            DetachedAffordance::ALL
                .iter()
                .map(|a| format!("float_{}", a.wire())),
        );

        for word in &wanted {
            assert!(
                published.contains(word),
                "{word} is in an affordance vocabulary this screen paints and \
                 the artifact does not publish it — a walk cannot ask for a \
                 part nothing hands it"
            );
        }
        let mut published_sorted = published.clone();
        published_sorted.sort();
        let mut wanted_sorted = wanted.clone();
        wanted_sorted.sort();
        assert_eq!(
            published_sorted, wanted_sorted,
            "the artifact's `part` rows are not exactly the two vocabularies — \
             a surplus row names an address no affordance composes"
        );
    }

    /// ★★★★★ R2225 — no two rows of this artifact claim one name.
    ///
    /// The publisher's half of the refusal `painted_grammar.table` gained this
    /// round. That reader stops a collision from being read wrongly; this stops
    /// one from being WRITTEN, which is the half that matters while families
    /// keep joining — the artifact's header names three more still to come.
    ///
    /// ⚠ Asserted over `(kind, name)` rather than over `name`, because the
    /// kinds are separate namespaces to the reader: `const FLOAT` and a
    /// `grammar` row could legitimately share a name, and demanding otherwise
    /// would refuse an artifact nothing can misread.
    #[test]
    fn no_two_rows_claim_one_name() {
        let mut seen: std::collections::HashMap<(String, String), String> =
            std::collections::HashMap::new();
        for (kind, name, value) in super::grammar_rows() {
            if let Some(held) = seen.insert((kind.to_owned(), name.clone()), value.clone()) {
                assert_eq!(
                    held, value,
                    "{kind} {name} is published twice, as {held} and {value} — \
                     a reader asking for {name} would be handed one of them and \
                     could not tell which. Name the family in the row."
                );
            }
        }
    }
}
