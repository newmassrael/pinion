//! ★★★★★ R2049 §5.2 §5.11 — **where a painted mark's address comes from.**
//!
//! # What was missing
//!
//! The screens in this workspace name painted marks with dotted addresses —
//! `lab.palette.role.Router`, `lab.form.control.<path>` — and **nothing
//! declared them**. Every reader that needed one re-typed a `format!` string:
//! the painter, the accessibility roster, the screen specification, the family
//! census, the gates, and the walks that check the frame from outside.
//!
//! ⇒ one wrong letter compiles, paints, and makes every query that looks for
//! the mark answer **nothing** — quietly. The mark is on the screen and no
//! reader can find it, which is the shape this repository has already paid for
//! twice (a prefix that swallowed chrome, and a family census that counted a
//! caption as a member).
//!
//! # ★ One family at a time, and counted
//!
//! The debt that opened this asked for exactly that, because *"change them
//! all"* has stopped half-way here before. This module holds the **palette
//! role row** and its swatch and nothing else yet, and
//! `r2049_a_role_address_is_typed_in_one_place` is what says so — it reads the
//! sources and refuses a second speller.
//!
//! # ★★ The inverse belongs here too
//!
//! A router turning a tag back into a role was a *second* place the prefix was
//! typed, and the one where a mismatch is silent in the other direction: the
//! press lands on nothing and the screen simply does not respond. Address and
//! parse are one pair, so they live together and are tested against each other.
//!
//! # ⚠ What a walk does instead
//!
//! A demo is Python and cannot call this. Its answer is to **read the address
//! off the wire** rather than to spell it: the screen's specification publishes
//! each role's row and swatch addresses, derived from here, so a walk names a
//! role and is handed the address the paint used.

use crate::graph::Role;
use pinion_node_graph::LinkId;

/// ★★★★★ R2050 — the tag this screen paints its settings form under.
///
/// The form's PARTS are addressed by the framework
/// ([`pinion_widget_paint::config_form::address`]) from this prefix and a row's
/// key; what this screen owns is the prefix, so it is declared here and
/// everything that composes a form address — the painter, the router, the
/// specification, the gates and the wire — is handed it.
pub const FORM: &str = "lab.form";

/// [`FORM`] with the separator its parts hang off — the stem a specification
/// table names when it says which region a pane holds.
///
/// A `&'static str` because that table needs one; the gate drives it against
/// [`FORM`].
pub const FORM_STEM: &str = "lab.form.";

/// [`form_control`]'s address with the population's placeholder, for a
/// specification table whose rows must be `&'static str`.
///
/// A test holds it against the derivation, the same way [`ROLE_ROW_TEMPLATE`]
/// is held against [`ROLE_ROW`].
pub const FORM_CONTROL_TEMPLATE: &str = "lab.form.control.{}";

/// ★★★★★ R2053 — a row's removal seat, with the population's placeholder.
///
/// A specification row must be `&'static str` and a derivation cannot make one,
/// so the templates below are declarations — but they are declarations in ONE
/// file, and the address gate drives every one of them against
/// [`form_part_prefix`], so a template that stopped agreeing with what the
/// painter composes is a build failure rather than a table pointing at nothing.
pub const FORM_REMOVE_TEMPLATE: &str = "lab.form.remove.{}";

/// The seat a derived row offers instead of a removal — see
/// [`FORM_PART_TEMPLATES`].
pub const FORM_AUTHOR_TEMPLATE: &str = "lab.form.author.{}";

/// A row's applies badge — see [`FORM_PART_TEMPLATES`].
pub const FORM_APPLIES_TEMPLATE: &str = "lab.form.applies.{}";

/// A derived row's source badge — see [`FORM_PART_TEMPLATES`].
pub const FORM_SOURCE_TEMPLATE: &str = "lab.form.source.{}";

/// A row that goes aside, saying what it is instead — see
/// [`FORM_PART_TEMPLATES`].
pub const FORM_ASIDE_TEMPLATE: &str = "lab.form.aside.{}";

/// ★★★★★ R2053 — every part of a form row this screen ADDRESSES.
///
/// The painter paints more than a reader here reaches for; this is the roster
/// of the ones that are read, pressed or published, and it is what the wire
/// hands a walk. A part word missing from it is not a defect in the painter —
/// it is a part nothing here asks about, and adding it is one line.
///
/// ⚠ Not a list of what EXISTS: only the painter knows that, and this file
/// cannot ask it without re-spelling its vocabulary. What the gate holds is
/// that every template below names a part in this roster, so the two cannot
/// disagree about a word.
/// ⚠ `toggle` is in this roster and the painter no longer paints it — R1837
/// took the part with the affordance it named. It stays because the screen's
/// own reference walk still DECLARES it, and this roster is what is addressed
/// rather than what is drawn; dropping it would leave that walk spelling a
/// prefix nothing hands it, which is the defect this list exists to remove.
pub const FORM_PARTS: &[&str] = &[
    "control", "row", "key", "type", "said", "applies", "source", "aside", "defect", "remove",
    "author", "disown", "add", "item", "option", "step", "shown", "pick", "switch", "roster",
    "toggle",
];

/// Every declared template beside the part word it is for.
///
/// Built FROM the consts above rather than beside them, so the gate drives what
/// the tables actually use. The part word is carried because the gate needs it
/// to re-derive; a table of templates alone would be a list nothing could
/// check.
pub const FORM_PART_TEMPLATES: &[(&str, &str)] = &[
    ("control", FORM_CONTROL_TEMPLATE),
    ("remove", FORM_REMOVE_TEMPLATE),
    ("author", FORM_AUTHOR_TEMPLATE),
    ("applies", FORM_APPLIES_TEMPLATE),
    ("source", FORM_SOURCE_TEMPLATE),
    ("aside", FORM_ASIDE_TEMPLATE),
];

/// The address of the control a form row's value is edited through.
///
/// The framework's derivation with this screen's prefix already applied, so a
/// caller here never spells either half.
#[must_use]
pub fn form_control(key: &str) -> String {
    pinion_widget_paint::config_form::address::control(FORM, key)
}

/// ★★★★★ R2053 — the address of ANY part of a form row, for this screen.
///
/// R2052 made the framework's side of this total: every part it paints is
/// composed in one place. This is the other side — a reader here asks for a
/// part by name and never spells the prefix, the separator, or the key's
/// position in the address.
///
/// The part words are the painter's own (`add`, `remove`, `applies`, `source`,
/// `aside`, `defect`, `item`, `option`, `step`, `shown`, `pick`, `key`,
/// `type`, `said`, `switch`, `author`, `disown`, `row`, `roster`); passing one
/// it does not paint composes an address nothing carries, which is a lookup
/// that answers nothing rather than a wrong mark — the safe direction.
#[must_use]
pub fn form_part(part: &str, key: &str) -> String {
    pinion_widget_paint::config_form::address::child(FORM, part, key)
}

/// The prefix every address of that part carries.
///
/// ★ DERIVED — the address with an empty key — rather than a second literal,
/// for the reason [`form_control_prefix`] is.
#[must_use]
pub fn form_part_prefix(part: &str) -> String {
    form_part(part, "")
}

/// The row an address of that part names, or `None` when it is not one.
#[must_use]
pub fn form_part_key<'a>(part: &str, tag: &'a str) -> Option<&'a str> {
    pinion_widget_paint::config_form::address::key_of(FORM, part, tag)
}

/// The prefix every form control address carries.
///
/// ★ DERIVED — the address with an empty key — rather than a second literal, so
/// a reader that classifies by prefix and one that builds a whole address
/// cannot drift apart.
#[must_use]
pub fn form_control_prefix() -> String {
    form_control("")
}

/// The row a control address names, or `None` when the tag is not one.
#[must_use]
pub fn form_control_key(tag: &str) -> Option<&str> {
    pinion_widget_paint::config_form::address::control_key(FORM, tag)
}

/// The prefix every palette role row carries.
///
/// Declared once. [`ROLE_ROW_TEMPLATE`] is the same address in the form the
/// voice specification's population expansion takes, and a test holds the two
/// together so they cannot drift.
/// ★★★★★ R2082 — the address a **card** is painted under, as a template.
///
/// Declared for the reason [`ROLE_ROW_TEMPLATE`] was (R2049) and the group
/// heading's was (R2078): a walk is Python and cannot call the declaration, so
/// every walk that wants a card's rectangle re-types this prefix, and a wrong
/// letter reads as *the screen did not paint it* rather than as a typo. R2078's
/// own walk recorded the absence in place — "the card family has no declaration
/// to derive from either, spelled inline in three places" — and this is that
/// declaration, one of the remainders
/// [[debt-a-paint-address-is-retyped-at-every-reader]] names.
///
/// ⚠ `lab.node.build.<word>` is a DIFFERENT family that shares this prefix (the
/// build seat, R1885), which is why the screen's own router reads that one
/// first and why a reader must not treat the prefix as "cards".
pub const CARD_TEMPLATE: &str = "lab.node.{}";

/// The address a **host frame** is painted under, as a template.
pub const FRAME_TEMPLATE: &str = "lab.frame.{}";

/// ★★★★★ R2117 — [`CARD_TEMPLATE`]'s prefix, with the separator every reader
/// either composes onto or strips off.
///
/// Declared because thirteen readers held this string themselves, and one of
/// them held its LENGTH: `tag[9..].contains('.')` is what "the card itself, not
/// one of its parts" looked like before this round. Nine is `"lab.node."`
/// counted by hand, so renaming the prefix would have left that reader slicing
/// a string at the wrong place — in the middle of a UTF-8 character if the new
/// prefix were shorter, and silently at the wrong boundary if it were longer.
pub const CARD: &str = "lab.node.";

/// ★★★★★ R2117 — the ANOTHER FAMILY that shares [`CARD`]'s prefix.
///
/// The build seat (R1885). A card is `lab.node.<name>`; this is
/// `lab.node.build.<word>`, so a reader stripping [`CARD`] and stopping there
/// reads `build` as a card called "build". The screen's router reads this one
/// FIRST for exactly that reason, and [`card_of`] refuses it rather than
/// leaving the ordering as the only thing that keeps them apart — an order is a
/// property of one call site, and a refusal is a property of the address.
pub const CARD_BUILD: &str = "lab.node.build.";

/// ★★★★★ R2117 — every part of a card that carries an address of its own.
///
/// A card is drawn as a box with things inside it, and three of those things
/// are addressed: the identifier it shows, the role badge beside it, and the
/// problem marker it grows when the gate has something to say about it. The
/// rest of a card's contents are painted into the card's own rectangle and are
/// found through it.
///
/// ⚠ This is what is ADDRESSED, not what is drawn — [`FORM_PARTS`] draws the
/// same distinction one family over, and for the same reason: only the painter
/// knows what exists, and a roster here that tried to say so would be a second
/// copy of its vocabulary.
pub const CARD_PARTS: &[&str] = &["id", "badge", "issue"];

/// The identifier a card shows, with the population's placeholder.
pub const CARD_ID_TEMPLATE: &str = "lab.node.{}.id";

/// The role badge beside it.
pub const CARD_BADGE_TEMPLATE: &str = "lab.node.{}.badge";

/// The problem marker a card grows when the gate has something to say.
pub const CARD_ISSUE_TEMPLATE: &str = "lab.node.{}.issue";

/// ★★★★★ R2117 — every declared card-part template beside the part word it is
/// for.
///
/// A specification row must be `&'static str` and a derivation cannot make one,
/// so these are declarations — but they are declarations in ONE file, and the
/// address gate drives every one of them against [`card_part`], so a template
/// that stopped agreeing with what the painter composes is a test failure
/// rather than a table pointing at a mark nobody paints. [`FORM_PART_TEMPLATES`]
/// is the same arrangement one family over; R2053 built it for that reason.
pub const CARD_PART_TEMPLATES: &[(&str, &str)] = &[
    ("id", CARD_ID_TEMPLATE),
    ("badge", CARD_BADGE_TEMPLATE),
    ("issue", CARD_ISSUE_TEMPLATE),
];

/// That card's address.
#[must_use]
pub fn card(name: &str) -> String {
    CARD_TEMPLATE.replace("{}", name)
}

/// The address of one PART of that card.
///
/// `part` is one of [`CARD_PARTS`]; a word that is not composes an address
/// nothing paints, which is a lookup answering nothing rather than a wrong
/// mark — the safe direction, and the same one [`form_part`] takes.
#[must_use]
pub fn card_part(name: &str, part: &str) -> String {
    format!("{CARD}{name}.{part}")
}

/// The card a tag names, or `None` when the tag is not a card's own address.
///
/// ★★★★★ [`card`]'s inverse, and it refuses THREE things that all begin with
/// this family's prefix:
///
/// * a tag of the build family ([`CARD_BUILD`]) — `build` is not a card name;
/// * a tag naming one of a card's PARTS, because the part's address and the
///   card's are different marks and a reader asking "which card is this" of
///   `lab.node.R-01.badge` wants [`card_part_of`];
/// * the bare prefix, which names no card at all.
///
/// ⚠ A card's name cannot contain a dot — the screen's own identifier rule —
/// which is what makes "no dot in the tail" a sound test for *this is the card
/// itself*. R1915 measured what the other reading costs one family over: a pin
/// reader that split at the last dot left every member pin drawn, announced and
/// unreachable.
#[must_use]
pub fn card_of(tag: &str) -> Option<&str> {
    let tail = tag.strip_prefix(CARD)?;
    if tail.contains('.') || tail.is_empty() {
        return None;
    }
    Some(tail)
}

/// The card and the part a tag names, or `None` when it is not a card part.
///
/// ★ [`card_part`]'s inverse. The part must be one this screen ADDRESSES, so a
/// tag whose tail is a word the roster does not carry is refused rather than
/// answered — otherwise a future `lab.node.<name>.<anything>` would read as a
/// part that nothing paints.
#[must_use]
pub fn card_part_of(tag: &str) -> Option<(&str, &'static str)> {
    let tail = tag.strip_prefix(CARD)?;
    let (name, word) = tail.split_once('.')?;
    if name.is_empty() {
        return None;
    }
    let part = CARD_PARTS.iter().find(|known| **known == word)?;
    Some((name, *part))
}

/// That frame's address.
#[must_use]
pub fn frame(name: &str) -> String {
    FRAME_TEMPLATE.replace("{}", name)
}

pub const ROLE_ROW: &str = "lab.palette.role.";

/// The prefix every palette role swatch carries.
pub const ROLE_SWATCH: &str = "lab.palette.swatch.";

/// ★★★★★ R2078 — the prefix every palette GROUP HEADING carries.
///
/// Declared for [`ROLE_ROW`]'s reason, and measured before declaring: the
/// address was spelled **five times** across this screen's own source — the
/// painter, the paint census's demanded list, its family table, the voice
/// roster and the heading gate — and a walk of this round made six. That is the
/// shape `debt-a-paint-address-is-retyped-at-every-reader` names, and the
/// roster expansion is what made it matter: the palette had TWO headings and
/// now has the behaviour canon's SEVEN, so a family nobody could address is a
/// family five times the size it was.
///
/// ⚠ Its own prefix under `lab.palette.` and NOT a suffix on the role row's,
/// which is this screen's own rule about prefixes speaking: `lab.palette.role.`
/// would swallow a heading whose label happened to match a role's name and
/// resolve it to the wrong mark. The heading and the rows are siblings.
pub const GROUP_HEAD: &str = "lab.palette.group.";

/// [`GROUP_HEAD`] with the population's placeholder, for a specification table
/// whose rows must be `&'static str`.
///
/// ⚠ R2078 — the voice specification's heading row spelled
/// `"lab.palette.group.{}"` itself, one row above a sibling already taking
/// [`ROLE_ROW_TEMPLATE`] — which is the drift that constant exists to prevent.
/// A test holds the two forms together, so a table taking the template cannot
/// part company with the runtime derivation.
pub const GROUP_HEAD_TEMPLATE: &str = "lab.palette.group.{}";

/// The address of the palette heading that reads `label`.
///
/// ★ Takes the group's own word, which is what the roster declares and what the
/// wire publishes — so the address a client presses and the address the paint
/// used are one spelling by construction.
#[must_use]
pub fn group_head(label: &str) -> String {
    format!("{GROUP_HEAD}{label}")
}

/// [`ROLE_ROW`] with the population's placeholder, for a specification table
/// whose rows must be `&'static str`.
pub const ROLE_ROW_TEMPLATE: &str = "lab.palette.role.{}";

/// [`ROLE_SWATCH`] with the population's placeholder.
pub const ROLE_SWATCH_TEMPLATE: &str = "lab.palette.swatch.{}";

/// The address of the palette row a person presses to add this role.
#[must_use]
pub fn role_row(role: Role) -> String {
    role_row_named(role.name())
}

/// The same address for a role held as a NAME.
///
/// The specification tables and the wire carry a role by its name rather than
/// as a value, and a caller holding one should not have to find the value again
/// just to spell an address it is about to hand back.
#[must_use]
pub fn role_row_named(name: &str) -> String {
    format!("{ROLE_ROW}{name}")
}

/// The address of the colour band on that row.
#[must_use]
pub fn role_swatch(role: Role) -> String {
    role_swatch_named(role.name())
}

/// The same address for a role held as a name.
#[must_use]
pub fn role_swatch_named(name: &str) -> String {
    format!("{ROLE_SWATCH}{name}")
}

/// ★★★★★ R2085 — the prefix the control that COLOURS a palette group carries.
///
/// ⚠ Its own prefix and **not** a suffix under [`GROUP_HEAD`], which is this
/// screen's own rule about prefixes speaking, recorded twice already (R1885's
/// build seats, R1915's member pin, and the note on [`WAY_IN`]): an arm for
/// `lab.palette.group.<label>` would swallow `<label>.ink`, look for a heading
/// called that and answer nothing. The heading and its control are siblings.
pub const GROUP_INK: &str = "lab.palette.ink.";

/// [`GROUP_INK`] with the population's placeholder, for the specification table
/// whose rows must be `&'static str`.
///
/// ⚠ Declared with the prefix rather than beside the voice row, which is
/// R2078's finding: a table that spells its own address is a table free to
/// drift from the paint, and `r1653_the_painted_screen_invented_nothing` is
/// what turns that drift into a red — it did, on this family's first paint,
/// naming all seven chips as marks no family accounts for.
pub const GROUP_INK_TEMPLATE: &str = "lab.palette.ink.{}";

/// The address of the control that colours the group headed `label`.
#[must_use]
pub fn group_ink(label: &str) -> String {
    format!("{GROUP_INK}{label}")
}

/// The group a colour control's address names, or `None` when the tag is not
/// one.
///
/// ★ The inverse of [`group_ink`], here rather than at the router, for
/// [`role_of_row`]'s reason — and the label is checked against the declared
/// partition rather than trusted as text, so a tag naming no heading resolves
/// to nothing instead of to whichever heading sorted first.
#[must_use]
pub fn group_of_ink(tag: &str) -> Option<&'static str> {
    let label = tag.strip_prefix(GROUP_INK)?;
    crate::spec::palette_groups()
        .iter()
        .map(|run| run.label)
        .find(|declared| *declared == label)
}

/// The role a palette row address names, or `None` when the tag is not one.
///
/// ★ The inverse of [`role_row`], here rather than at the router, so the two
/// cannot be written against different prefixes. `None` is a real answer — most
/// tags are not palette rows — and a name no role answers to is also `None`,
/// which is what stops a press on a tag this screen does not own from
/// resolving to whichever role happened to sort first.
#[must_use]
pub fn role_of_row(tag: &str) -> Option<Role> {
    let name = tag.strip_prefix(ROLE_ROW)?;
    Role::ALL.into_iter().find(|role| role.name() == name)
}

/// ★★★★★ R2067 — the prefix a card's **way in** carries.
///
/// A family DECLARED on the round that invents it, which is the one thing the
/// address debt's instalments could not do for the families they converted:
/// each of those had to find every reader that had already spelled the prefix.
/// This one has five readers on its first day — the painter, the router's
/// family filter, the router's parse, the accessibility roster and the paint
/// census — and they are handed the address instead.
///
/// ⚠ Its own prefix rather than a suffix under the card's, and the router's own
/// rule is what says so: an arm for `lab.node.<name>` would swallow
/// `<name>.inside`, look for a card called that and answer nothing — the shape
/// R1885 recorded for the build seats and R1915 for a member pin.
pub const WAY_IN: &str = "lab.inside.";

/// The address of the control a person presses to go inside the graph `card`
/// stands for.
///
/// Takes the card's NAME because that is what this screen addresses a card by,
/// and what the wire and the accessibility tree both carry.
#[must_use]
pub fn way_in(card: &str) -> String {
    format!("{WAY_IN}{card}")
}

/// The card a way-in address names, or `None` when the tag is not one.
///
/// ★ The inverse of [`way_in`], here rather than at the router, for
/// [`role_of_row`]'s reason: written against a different prefix it becomes a
/// press that lands on nothing and a screen that does not respond. Whether a
/// card of that name EXISTS is the router's question, not this one's — this
/// answers what the address says.
#[must_use]
pub fn card_of_way_in(tag: &str) -> Option<&str> {
    tag.strip_prefix(WAY_IN)
}

/// ★★★★★ R2104 — the tag the canvas **toolbar itself** is painted under.
///
/// Declared for [`ROLE_ROW`]'s reason, and measured before declaring: this
/// family was the largest remaining entry in
/// `python3 tools/painted_addresses.py --owed` — **73 walk sites across eight
/// files**, and 106 more in Rust across two binaries, none of them derived from
/// anything. A toolbar seat is the most pressed thing on this screen, so a
/// wrong letter here is not only a mark nobody finds; it is a press that lands
/// on nothing and a screen that does not respond.
///
/// ⚠ The bar's OWN tag, and not a prefix. Its seats hang off
/// [`TOOLBAR_SEAT`], which carries the separator — this screen's standing rule
/// about prefixes speaking ([`WAY_IN`], [`GROUP_INK`]): a prefix of
/// `lab.toolbar` alone would swallow every seat, and a reader classifying a
/// snapshot by it would call the whole cluster the bar.
pub const TOOLBAR: &str = "lab.toolbar";

/// [`TOOLBAR`] with the separator its seats hang off.
///
/// A `&'static str` because the seat consts below are `&'static str` too and a
/// derivation cannot make one; the gate drives it against [`TOOLBAR`].
pub const TOOLBAR_SEAT: &str = "lab.toolbar.";

/// The graph's name, painted on the bar.
pub const TOOLBAR_TITLE: &str = "lab.toolbar.title";

/// The bar's status line — what the graph is, beside its name.
pub const TOOLBAR_META: &str = "lab.toolbar.meta";

/// The launch verdict chip, in the left cluster.
pub const TOOLBAR_GATE: &str = "lab.toolbar.gate";

/// The focus chip — see `ToolGroup::Focus`.
pub const TOOLBAR_FOCUS: &str = "lab.toolbar.focus";

/// The focus chip's caption, painted inside it by `caption::captioned`.
pub const TOOLBAR_FOCUS_CAPTION: &str = "lab.toolbar.focus.caption";

/// The zoom read-out.
///
/// ⚠ Not a seat: it is a caption painted inside `lab.reset.view`, which is a
/// seat of ANOTHER family sitting on this row. That is why `ToolGroup::labels`
/// carries this word and `ToolGroup::seats` does not, and why a reader must not
/// take it for a control.
pub const TOOLBAR_ZOOM: &str = "lab.toolbar.zoom";

/// The zoom-in stepper.
pub const TOOLBAR_ZOOM_IN: &str = "lab.toolbar.zoom.in";

/// The zoom-out stepper.
pub const TOOLBAR_ZOOM_OUT: &str = "lab.toolbar.zoom.out";

/// Fit the graph to the view.
pub const TOOLBAR_FIT: &str = "lab.toolbar.fit";

/// Go to where the graph ends up — see `ToolGroup::Home`.
pub const TOOLBAR_HOME: &str = "lab.toolbar.home";

/// Export the configuration.
pub const TOOLBAR_CONFIG: &str = "lab.toolbar.config";

/// Produce the launch script.
pub const TOOLBAR_SCRIPT: &str = "lab.toolbar.script";

/// Save the graph.
pub const TOOLBAR_SAVE: &str = "lab.toolbar.save";

/// Open the saved graph.
pub const TOOLBAR_OPEN: &str = "lab.toolbar.open";

/// Clear back to the graph this screen opens with.
pub const TOOLBAR_CLEAR: &str = "lab.toolbar.clear";

/// The launch seat.
pub const TOOLBAR_RUN: &str = "lab.toolbar.run";

/// The launch seat's caption, which says what is running.
pub const TOOLBAR_RUN_LABEL: &str = "lab.toolbar.run.label";

/// The overflow control — the seat that holds the groups the row gave up.
pub const TOOLBAR_MORE: &str = "lab.toolbar.more";

/// The overflow control's caption, which names what it is holding.
pub const TOOLBAR_MORE_LABEL: &str = "lab.toolbar.more.label";

/// ★★★★★ R2104 — every word this screen ADDRESSES under [`TOOLBAR_SEAT`],
/// beside the address declared for it.
///
/// [`FORM_PARTS`]'s shape, for the same three readers: the crate's own const
/// sites, the gate that refuses a second speller, and the wire that hands a
/// walk the address instead of letting it type one.
///
/// ⚠ Not a list of what EXISTS — only the painter knows that, and a caption
/// nothing here reaches for is not a defect in the painter. What the gate holds
/// is two things: every entry's declared address is what [`toolbar`] derives
/// from its word, and every tag `ToolGroup` claims to paint under this prefix
/// is in this roster. The second is the half that catches a seat joining a
/// group without joining the wire.
pub const TOOLBAR_SEATS: &[(&str, &str)] = &[
    ("title", TOOLBAR_TITLE),
    ("meta", TOOLBAR_META),
    ("gate", TOOLBAR_GATE),
    ("focus", TOOLBAR_FOCUS),
    ("focus.caption", TOOLBAR_FOCUS_CAPTION),
    ("zoom", TOOLBAR_ZOOM),
    ("zoom.in", TOOLBAR_ZOOM_IN),
    ("zoom.out", TOOLBAR_ZOOM_OUT),
    ("fit", TOOLBAR_FIT),
    ("home", TOOLBAR_HOME),
    ("config", TOOLBAR_CONFIG),
    ("script", TOOLBAR_SCRIPT),
    ("save", TOOLBAR_SAVE),
    ("open", TOOLBAR_OPEN),
    ("clear", TOOLBAR_CLEAR),
    ("run", TOOLBAR_RUN),
    ("run.label", TOOLBAR_RUN_LABEL),
    ("more", TOOLBAR_MORE),
    ("more.label", TOOLBAR_MORE_LABEL),
];

/// The address of the toolbar seat called `word`.
///
/// ★ The runtime half of the consts above, for a caller holding a word rather
/// than a name. The wire's roster is built through this, so the address a
/// client presses and the address the paint used are one spelling by
/// construction.
#[must_use]
pub fn toolbar(word: &str) -> String {
    format!("{TOOLBAR_SEAT}{word}")
}

/// The seat word a toolbar address names, or `None` when the tag is not one.
///
/// ★ The inverse of [`toolbar`], here rather than at the router, for
/// [`role_of_row`]'s reason. `None` is a real answer twice over: the bar's own
/// tag is not a seat, and neither is `lab.reset.view`, which sits on this row
/// and belongs to another family.
#[must_use]
pub fn toolbar_word(tag: &str) -> Option<&str> {
    tag.strip_prefix(TOOLBAR_SEAT)
}

/// ★★★★★ R2105 — the tag the card inspector is painted under.
///
/// [`TOOLBAR`]'s shape and its reason, measured the same way before declaring:
/// this family was the head of `python3 tools/painted_addresses.py --owed` —
/// **41 walk sites across twelve files**, and 85 more in this crate's own Rust
/// (73 of them a seat), none of them derived from anything.
///
/// ⚠ What makes the inspector different from the bar is what a wrong letter
/// COSTS. The toolbar's seats are pressed; the inspector's are mostly READ —
/// the identifier, the role chip, the degree, how many cards are picked, what
/// cannot be reached. A press that lands on nothing is at least a walk that
/// fails. A read that lands on nothing is a walk asserting about an address
/// that was never painted, and the assertion it writes is *the screen did not
/// paint it* — a report that accuses the screen of the walk's own typo.
///
/// ⚠⚠ The pane's OWN tag, and not a prefix. Its seats hang off
/// [`INSPECTOR_SEAT`], which carries the separator, for [`TOOLBAR`]'s reason:
/// a prefix of `lab.inspector` alone swallows every seat, so a reader
/// classifying a snapshot by it would call the whole panel the pane. This
/// screen already relies on that distinction — `spec::PANES` names the pane and
/// `SILENCES` names `lab.inspector.body` separately, and they are container and
/// content.
pub const INSPECTOR: &str = "lab.inspector";

/// [`INSPECTOR`] with the separator its seats hang off.
///
/// A `&'static str` because the seat consts below are `&'static str` too and a
/// derivation cannot make one; the gate drives it against [`INSPECTOR`].
pub const INSPECTOR_SEAT: &str = "lab.inspector.";

/// The scrolling body the panel's content sits in — the pane a reader lands on.
///
/// ⚠ A layout mark rather than a control: `spec::SILENCES` carries it for that
/// reason, and it is in this roster because walks reach for it, not because it
/// speaks.
pub const INSPECTOR_BODY: &str = "lab.inspector.body";

/// The panel's flip control — see the pair it makes with [`INSPECTOR_FOLD`].
pub const INSPECTOR_FLIP: &str = "lab.inspector.flip";

/// The panel's fold control, which leaves the strip a hand grabs to open it.
pub const INSPECTOR_FOLD: &str = "lab.inspector.fold";

/// The selected card's identifier, the panel's heading.
pub const INSPECTOR_ID: &str = "lab.inspector.id";

/// The selected card's role chip.
pub const INSPECTOR_ROLE: &str = "lab.inspector.role";

/// How many wires the selected card carries.
pub const INSPECTOR_DEGREE: &str = "lab.inspector.degree";

/// [`INSPECTOR_DEGREE`]'s caption, painted inside it.
pub const INSPECTOR_DEGREE_TEXT: &str = "lab.inspector.degree.text";

/// How many cards are picked, and which one the panel is showing.
pub const INSPECTOR_SELCOUNT: &str = "lab.inspector.selcount";

/// [`INSPECTOR_SELCOUNT`]'s caption, painted inside it.
pub const INSPECTOR_SELCOUNT_TEXT: &str = "lab.inspector.selcount.text";

/// What the panel holds that no scrolling can reach.
pub const INSPECTOR_REACH: &str = "lab.inspector.reach";

/// [`INSPECTOR_REACH`]'s caption, painted inside it.
pub const INSPECTOR_REACH_TEXT: &str = "lab.inspector.reach.text";

/// The panel's note — what it has to say about the selection.
pub const INSPECTOR_NOTE: &str = "lab.inspector.note";

/// [`INSPECTOR_NOTE`]'s caption, painted inside it.
pub const INSPECTOR_NOTE_TEXT: &str = "lab.inspector.note.text";

/// The field the selected card's name is typed into.
pub const INSPECTOR_NAME: &str = "lab.inspector.name";

/// The seat that starts a rename.
pub const INSPECTOR_RENAME: &str = "lab.inspector.rename";

/// The seat that adds a key to the selected card.
pub const INSPECTOR_ADDKEY: &str = "lab.inspector.addkey";

/// Fold the card down, or open it again — `NodeAct::Collapse`.
pub const INSPECTOR_COLLAPSE: &str = "lab.inspector.collapse";

/// Take the card out of the run without deleting it — `NodeAct::Disable`.
pub const INSPECTOR_DISABLE: &str = "lab.inspector.disable";

/// Take the card off the canvas — `NodeAct::Delete`.
pub const INSPECTOR_DELETE: &str = "lab.inspector.delete";

/// Put the card's unwired pins away, or bring them back — `NodeAct::Pins`.
pub const INSPECTOR_PINS: &str = "lab.inspector.pins";

/// ★★★★★ R2105 — every word this screen ADDRESSES under [`INSPECTOR_SEAT`],
/// beside the address declared for it.
///
/// [`TOOLBAR_SEATS`]'s shape, for the same three readers: the crate's own const
/// sites, the gate that refuses a second speller, and the wire that hands a
/// walk the address instead of letting it type one.
///
/// ⚠ Not a list of what EXISTS — only the painter knows that. What the gate
/// holds is two things: every entry's declared address is what [`inspector`]
/// derives from its word, and every `lab.inspector.*` tag the SPECIFICATION
/// declares — `spec::VOICES`, `spec::SILENCES`, and the seats `NodeAct` claims
/// — is in this roster. The second half is the one that catches a seat reaching
/// the paint tree and the announcement without reaching the wire, which is the
/// direction a walk cannot detect: it would report that the screen did not
/// paint a mark the screen paints.
pub const INSPECTOR_SEATS: &[(&str, &str)] = &[
    ("body", INSPECTOR_BODY),
    ("flip", INSPECTOR_FLIP),
    ("fold", INSPECTOR_FOLD),
    ("id", INSPECTOR_ID),
    ("role", INSPECTOR_ROLE),
    ("degree", INSPECTOR_DEGREE),
    ("degree.text", INSPECTOR_DEGREE_TEXT),
    ("selcount", INSPECTOR_SELCOUNT),
    ("selcount.text", INSPECTOR_SELCOUNT_TEXT),
    ("reach", INSPECTOR_REACH),
    ("reach.text", INSPECTOR_REACH_TEXT),
    ("note", INSPECTOR_NOTE),
    ("note.text", INSPECTOR_NOTE_TEXT),
    ("name", INSPECTOR_NAME),
    ("rename", INSPECTOR_RENAME),
    ("addkey", INSPECTOR_ADDKEY),
    ("collapse", INSPECTOR_COLLAPSE),
    ("disable", INSPECTOR_DISABLE),
    ("delete", INSPECTOR_DELETE),
    ("pins", INSPECTOR_PINS),
];

/// The address of the inspector seat called `word`.
///
/// ★ The runtime half of the consts above, for a caller holding a word rather
/// than a name — the painter composing a seat from `NodeAct`'s wire word, and
/// the wire's roster, are both built through this, so the address a client
/// reads and the address the paint used are one spelling by construction.
#[must_use]
pub fn inspector(word: &str) -> String {
    format!("{INSPECTOR_SEAT}{word}")
}

/// The seat word an inspector address names, or `None` when the tag is not one.
///
/// ★ The inverse of [`inspector`], here rather than at the router, for
/// [`role_of_row`]'s reason. `None` is a real answer: the pane's own tag is not
/// one of its seats, and a shrink policy naming the pane is asking about the
/// container.
#[must_use]
pub fn inspector_word(tag: &str) -> Option<&str> {
    tag.strip_prefix(INSPECTOR_SEAT)
}

/// ★★★★★ R2106 — the tag the **node palette** is painted under.
///
/// [`TOOLBAR`]'s shape and its reason, measured the same way before declaring:
/// this family was the head of `python3 tools/painted_addresses.py --owed` once
/// the inspector left it — **41 walk sites across nine files**, and 77 more in
/// this crate's own Rust, none of them derived from anything.
///
/// ⚠ What makes the palette different from the bar and the panel is that it is
/// not ONE roster. Four of its families were already declared here — the role
/// row (R2049), its swatch, the group heading (R2078) and the heading's colour
/// control (R2085) — each on the round that needed it, and the rest of the pane
/// was left spelling itself. So this is the round that finishes a family the
/// project has been converting a corner at a time, and what it adds is the
/// FIXED seats (below) beside four PARAMETRIC prefixes ([`PALETTE_PIN`],
/// [`PALETTE_PROTOCOL`], [`PALETTE_PART`], [`PALETTE_VERB`]).
///
/// ⚠⚠ The pane's OWN tag, and not a prefix. Its seats hang off
/// [`PALETTE_SEAT`], which carries the separator, for [`TOOLBAR`]'s reason: a
/// prefix of the bare tag swallows every seat, so a reader classifying a
/// snapshot by it would call the whole pane the container. This screen already
/// relies on that distinction — `spec::PANES` names the pane and `SILENCES`
/// names the body separately, and they are container and content.
pub const PALETTE: &str = "lab.palette";

/// [`PALETTE`] with the separator its seats hang off.
///
/// A `&'static str` because the seat consts below are `&'static str` too and a
/// derivation cannot make one; the gate drives it against [`PALETTE`].
pub const PALETTE_SEAT: &str = "lab.palette.";

/// The scrolling body the pane's content sits in — the region a reader lands
/// on, and the one a walk scrolls.
///
/// ⚠ A layout mark rather than a control: `spec::SILENCES` carries it for that
/// reason, and it is in this roster because walks reach for it, not because it
/// speaks.
pub const PALETTE_BODY: &str = "lab.palette.body";

/// The pane's flip control — see the pair it makes with [`PALETTE_FOLD`].
pub const PALETTE_FLIP: &str = "lab.palette.flip";

/// The pane's fold control, which leaves the strip a hand grabs to open it.
pub const PALETTE_FOLD: &str = "lab.palette.fold";

/// The heading over the pin legend — what an appearance means.
pub const PALETTE_LEGEND: &str = "lab.palette.legend";

/// The switch that turns discovery on.
pub const PALETTE_DISCOVERY: &str = "lab.palette.discovery";

/// The heading over that switch.
pub const PALETTE_DISCOVERY_HEAD: &str = "lab.palette.discovery.head";

/// The switch's caption, painted inside it by `caption::captioned`.
///
/// ⚠ Declared as well as derived. The painter composes it through the
/// framework's own name for the suffix, which must stay the composition; this
/// const is what the specification table — whose rows must be `&'static str` —
/// takes, and the gate drives the two against each other so a table cannot part
/// company with the paint.
pub const PALETTE_DISCOVERY_CAPTION: &str = "lab.palette.discovery.caption";

/// The rail the switch's knob slides along.
pub const PALETTE_DISCOVERY_TRACK: &str = "lab.palette.discovery.track";

/// The heading over the definitions register.
pub const PALETTE_PARTS: &str = "lab.palette.parts";

/// ★★★★★ R2106 — every word this screen ADDRESSES under [`PALETTE_SEAT`] as a
/// FIXED seat, beside the address declared for it.
///
/// [`TOOLBAR_SEATS`]'s shape, for the same three readers: the crate's own const
/// sites, the gate that refuses a second speller, and the wire that hands a
/// walk the address instead of letting it type one.
///
/// ⚠ Fixed only. The pane's parametric families expand over a population — the
/// pin legend, the transports, the definitions and their verbs — and a roster
/// of them would be a table the size of the document. Those are published as
/// PREFIXES instead, the way [`FORM_PARTS`] is, and a walk names the member and
/// appends it.
///
/// ⚠⚠ Not a list of what EXISTS — only the painter knows that. What the gate
/// holds is two things: every entry's declared address is what [`palette`]
/// derives from its word, and every fixed tag under this prefix that the
/// SPECIFICATION declares is in this roster. The second half catches a seat
/// reaching the paint tree and the announcement without reaching the wire,
/// which is the direction a walk cannot detect: it would report that the screen
/// did not paint a mark the screen paints.
pub const PALETTE_SEATS: &[(&str, &str)] = &[
    ("body", PALETTE_BODY),
    ("flip", PALETTE_FLIP),
    ("fold", PALETTE_FOLD),
    ("legend", PALETTE_LEGEND),
    ("discovery", PALETTE_DISCOVERY),
    ("discovery.head", PALETTE_DISCOVERY_HEAD),
    ("discovery.caption", PALETTE_DISCOVERY_CAPTION),
    ("discovery.track", PALETTE_DISCOVERY_TRACK),
    ("parts", PALETTE_PARTS),
];

/// The address of the palette seat called `word`.
///
/// ★ The runtime half of the consts above, for a caller holding a word rather
/// than a name. The wire's roster is built through this, so the address a
/// client reads and the address the paint used are one spelling by
/// construction.
#[must_use]
pub fn palette(word: &str) -> String {
    format!("{PALETTE_SEAT}{word}")
}

/// The seat word a palette address names, or `None` when the tag is not one.
///
/// ★ The inverse of [`palette`], here rather than at the router, for
/// [`role_of_row`]'s reason. `None` is a real answer: the pane's own tag is not
/// one of its seats, and a shrink policy naming the pane is asking about the
/// container.
///
/// ⚠ It answers for the parametric families too — a legend entry's key is a
/// word under this prefix — because what it reports is what the ADDRESS says,
/// not which roster the word is in. A caller that wants a fixed seat checks
/// [`PALETTE_SEATS`]; a caller that wants a member uses the family's own
/// prefix.
#[must_use]
pub fn palette_word(tag: &str) -> Option<&str> {
    tag.strip_prefix(PALETTE_SEAT)
}

/// The prefix every entry of the **pin legend** carries.
pub const PALETTE_PIN: &str = "lab.palette.pin.";

/// [`PALETTE_PIN`] with the population's placeholder, for a specification table
/// whose rows must be `&'static str`.
pub const PALETTE_PIN_TEMPLATE: &str = "lab.palette.pin.{}";

/// The address of the legend entry for a pin that appears as `kind`.
#[must_use]
pub fn palette_pin(kind: &str) -> String {
    format!("{PALETTE_PIN}{kind}")
}

/// The prefix every **transport chip** carries.
pub const PALETTE_PROTOCOL: &str = "lab.palette.protocol.";

/// [`PALETTE_PROTOCOL`] with the population's placeholder.
pub const PALETTE_PROTOCOL_TEMPLATE: &str = "lab.palette.protocol.{}";

/// The address of the colour-key chip for the transport called `word`.
#[must_use]
pub fn palette_protocol(word: &str) -> String {
    format!("{PALETTE_PROTOCOL}{word}")
}

/// The prefix every row of the **definitions register** carries.
///
/// ⚠ Keyed by the definition's ID and not its name, which is R2048's finding
/// and not a preference: two definitions may answer to one name on purpose, so
/// a name-keyed address cannot say which row a press was about — precisely when
/// the screen is telling a person the name reaches neither.
pub const PALETTE_PART: &str = "lab.palette.part.";

/// The words a register row is addressed by, under [`PALETTE_PART`] and the
/// row's id.
///
/// The row's own band takes no word; these are the two runs painted inside it.
pub const PALETTE_PART_WORDS: &[&str] = &["name", "line"];

/// The address of the register row for the definition `id`.
#[must_use]
pub fn palette_part(id: u32) -> String {
    format!("{PALETTE_PART}{id}")
}

/// The address of the run called `word` inside that row — see
/// [`PALETTE_PART_WORDS`].
#[must_use]
pub fn palette_part_word(id: u32, word: &str) -> String {
    format!("{PALETTE_PART}{id}.{word}")
}

/// The prefix every **register control** carries — a verb over a definition.
pub const PALETTE_VERB: &str = "lab.palette.verb.";

/// The address of the control that applies `verb` to the definition `id`.
#[must_use]
pub fn palette_verb(verb: &str, id: u32) -> String {
    format!("{PALETTE_VERB}{verb}.{id}")
}

/// The verb and the definition a register control's address names, or `None`
/// when the tag is not one.
///
/// ★ The inverse of [`palette_verb`], here rather than at the router, for
/// [`role_of_row`]'s reason — and the verb is matched against a caller-supplied
/// vocabulary rather than split on the first dot, because a definition's id is
/// numeric and a verb that gained a dot would otherwise be read as an id.
///
/// ⚠ Whether the definition EXISTS is the router's question, not this one's:
/// this answers what the address says.
#[must_use]
pub fn palette_verb_of<'a>(tag: &'a str, verbs: &[&'a str]) -> Option<(&'a str, u32)> {
    let rest = tag.strip_prefix(PALETTE_VERB)?;
    verbs.iter().find_map(|verb| {
        let id = rest.strip_prefix(&format!("{verb}."))?;
        Some((*verb, id.parse().ok()?))
    })
}

/// ★★★★★ R2108 — the prefix every **pin of a card** is painted under.
///
/// A card draws two pins and a split puts members under them, so this family's
/// address is `<prefix><card>.<word>` — two parametric levels where every
/// family before it had one. The card's name comes from the graph and the word
/// from `pin_word`, which is why neither is declared here: this module owns the
/// SHAPE of an address, not the vocabularies its levels are drawn from.
///
/// ⚠ Carried with its separator, because every reader of this family either
/// composes onto it or strips it off. There is no mark at the bare `lab.pin`:
/// this family has no container of its own — a pin is drawn on the card that
/// owns it, and the card's own address is where a reader asks about that.
///
/// ⚠⚠ Named `PIN` and NOT after the diameter constant this crate also calls
/// `PIN` — that one was renamed `PIN_D` (a private const in `lib.rs`, so it is
/// named here rather than linked) by the round that declared this. The two are
/// a FOURTH namespace this campaign has had to check (R2106
/// counted three: a specification table's keys, a module's own function names,
/// and the introspection path surface), and it is the silent one: a `const` in
/// another module is not something the compiler refuses, so `PIN / 2` and
/// `address::PIN` would have sat in one file as one word meaning a length on
/// one line and a prefix on the next. Every declaration here is named for the
/// segment it addresses, so the address keeps the word and the geometry
/// constant says what it measures.
pub const PIN: &str = "lab.pin.";

/// The address of the pin that `word` names on the card called `card`.
///
/// `word` is `pin_word`'s output — `dial`, `accept`, or one of those with a
/// member's name behind a dot — so the tag a client presses, the address
/// `split_pin` accepts and the name the accessibility tree carries are one
/// spelling by construction.
#[must_use]
pub fn pin(card: &str, word: &str) -> String {
    format!("{PIN}{card}.{word}")
}

/// The card and the pin word an address names, or `None` when the tag is not
/// one of this family.
///
/// ★★★★★ [`pin`]'s inverse, and it splits at the FIRST dot. R1915 measured what
/// the other choice costs: a reader splitting at the last one read
/// `accept.host` as a member word `host` belonging to a card called
/// `<name>.accept`, matched nothing, and answered *nothing* — a pin that was
/// drawn, announced, and unreachable by any press. A card's name cannot carry a
/// dot, and the caller asks the graph whether the name it is handed is a card,
/// so splitting first is correct rather than merely different.
///
/// ⚠ Whether the card EXISTS is the caller's question, not this one's: this
/// answers what the address says. [`palette_verb_of`] draws the same line.
#[must_use]
pub fn pin_of(tag: &str) -> Option<(&str, &str)> {
    tag.strip_prefix(PIN)?.split_once('.')
}

// ─── the reset affordances ──────────────────────────────────────────────────
//
// ★★★★★ R2116 — the five "put it back" seats, and the first family this
// campaign has met whose KEY VOCABULARY IS A TYPE.
//
// Measured at entry: **22 sites across four walks** and **20 in this crate's
// four modules**, none in the shell — 42 in all.
//
// ⚠ Every arrangement before this drew its keys from a roster written beside
// the addresses (a seat list) or from an open vocabulary parsed back out of the
// tag (a field path, a layer id, a row number). This family's keys are
// `ResetScope::WIRE_NAMES`, which is itself derived from that enum's
// arms — so the roster below is not the vocabulary's source, it is a SECOND
// statement of it, and the gate's job is to hold the two equal. That is a
// stronger property than the earlier families have: a scope added to the enum
// and not addressed here fails, and an address here for a scope the enum does
// not have fails too.
//
// ⚠⚠ There is NO mark at the bare `lab.reset`. The five seats are drawn in two
// different places — four on the floating gate panel, one in the toolbar's zoom
// cluster — so nothing contains them and there is no container to address. That
// is why this family has a seat prefix and no root const, unlike the bar and
// the two panels above.

/// The prefix every reset affordance hangs off.
///
/// ★ Carried WITH its separator, for [`PIN`]'s reason: every reader of this
/// family either composes onto it or strips it off, and there is no mark at the
/// bare stem to want the other form for.
pub const RESET_SEAT: &str = "lab.reset.";

/// Put back which cards exist.
pub const RESET_NODES: &str = "lab.reset.nodes";

/// Put back where the cards sit, and which host each starts on.
pub const RESET_LAYOUT: &str = "lab.reset.layout";

/// Put back every form's values and rows.
pub const RESET_FIELDS: &str = "lab.reset.fields";

/// Put back the authored links.
pub const RESET_LINKS: &str = "lab.reset.links";

/// Put back pan and zoom.
///
/// ⚠ The one seat that is NOT on the gate panel: it sits in the toolbar's zoom
/// cluster and is drawn unconditionally, because pan and zoom always have a
/// home to go to. A reader classifying this address by *where it is painted*
/// would put it with the bar; it is addressed here, with the other four,
/// because what it does is what names it. `ResetScope::gated` is the fact, and
/// this comment is not a second copy of it — `ResetScope::gated` holds it.
///
/// ⚠ NAMED, not linked. `ResetScope` is private to this crate, so a rustdoc
/// intra-doc link from a `pub` item resolves only under
/// `--document-private-items` and is refused by `rustdoc::private_intra_doc_links`
/// everywhere else. The push gate caught exactly that here.
pub const RESET_VIEW: &str = "lab.reset.view";

/// ★★★★★ R2116 — every scope's word beside its address.
///
/// The whole family: this prefix carries nothing parametric, because the
/// vocabulary is closed by the type it comes from. The gate holds this roster
/// against `ResetScope::WIRE_NAMES` in BOTH directions — named rather than
/// linked, because that type is private to this crate.
pub const RESET_SEATS: &[(&str, &str)] = &[
    ("nodes", RESET_NODES),
    ("layout", RESET_LAYOUT),
    ("fields", RESET_FIELDS),
    ("links", RESET_LINKS),
    ("view", RESET_VIEW),
];

/// The address of the reset seat for the scope called `word`.
#[must_use]
pub fn reset(word: &str) -> String {
    format!("{RESET_SEAT}{word}")
}

/// The scope word an address names, or `None` when the tag is not one of them.
///
/// ★ [`reset`]'s inverse. Exact equality against the roster rather than a
/// prefix strip, so a tag that merely *begins* like one of these — a caption or
/// a descendant a future round hangs underneath a seat — is not read as the
/// seat itself.
#[must_use]
pub fn reset_word(tag: &str) -> Option<&'static str> {
    RESET_SEATS
        .iter()
        .find(|(_, seat)| *seat == tag)
        .map(|(word, _)| *word)
}

// ─── the wires, and the picked wire's own chrome ────────────────────────────
//
// ★★★★★ R2118 — **the first family with TWO HEADS on one stem.**
//
// Measured at entry: **12 sites across three walks** and **44 in this crate's
// three modules** (`lib.rs` 23, `painted.rs` 20, `spec.rs` 1 — `tests.rs`
// spelled this family zero times), none in the shell — 56 in all, and the
// largest remainder this screen had left.
//
// Every family declared above carries ONE vocabulary under its stem: a bar seat
// is a word, a card is a name, a form row is a config path, a reset is an arm of
// an enum. This one carries two, and they are not the same kind of thing:
//
// * the **census** head — `lab.link.<id>`, one mark per wire the document holds,
//   keyed by the document's own [`LinkId`], so the vocabulary is open and
//   numeric and only the document can enumerate it;
// * the **roster** head — the fixed words this family also addresses: the picked
//   wire's caption, its one act, the turn seat, the wire a hand is still
//   holding, and the endpoint chips indexed by position.
//
// ⚠ **A stem with two heads cannot be taken apart by stripping it.**
// `lab.link.7` and `lab.link.act` both survive `strip_prefix("lab.link.")`, so a
// reader that stops there reads `act` as a wire the document does not have and
// `7` as a seat the screen does not paint — in BOTH directions, and both
// silently. That is why the inverse here is a CLASSIFIER ([`link_mark_of`])
// rather than a strip, and why the gate holds the two heads DISJOINT rather than
// merely non-empty: every mark this family paints is recovered by exactly one of
// them, and a roster word that came to parse as a number would be a failure
// rather than a mark two readers each claim.
//
// ⚠⚠ The two heads are why the prefix on its own is a bad question, and this
// crate was asking it SIX times (measured: five in the paint sweeps, one in the
// press filter). The bare prefix was standing for four different meanings —
// *is this drawn on the canvas*, *is this pressable*, *is this the chrome the
// card sweeps excuse*, *is this a wire whose rhythm says which layer it is in* —
// and the first three are genuinely about the whole family, chrome included.
// EXACTLY ONE wanted the census head alone: the rhythm census, which would have
// called a chrome panel a link somebody drew the first time it ran with a wire
// picked. So the count of wrong readers is one, not six — what the other five
// were missing is not correctness, it is that nothing SAID which question they
// were asking, and a reader cannot tell a deliberate family-wide test from a
// census head somebody spelled too widely.
//
// ⚠ That paragraph said "eight times" and "three of those four" in its first
// draft, and both were written from memory. Measured against `HEAD` they are six
// and one. The round whose whole subject is *a number that names one population
// and is read as another* wrote two such numbers into its own declaration — the
// fourth time in this campaign that the subject landed on its own draft.

/// The prefix every mark of this family hangs off.
///
/// ★ Carried WITH its separator, for [`PIN`]'s and [`RESET_SEAT`]'s reason:
/// every reader either composes onto it or strips it off, and there is no mark
/// at the bare stem to want the other form for. A wire is drawn on the canvas
/// and found through the canvas; nothing draws a box around the wires as a
/// group, so there is no container here to address.
pub const LINK: &str = "lab.link.";

/// The address a **drawn wire** is painted under, as a template.
///
/// [`CARD_TEMPLATE`]'s shape, over a different population: the placeholder is
/// the document's own link id, so the specification row that carries this
/// template stands over `Population::Links` the way the card's stands over
/// `Population::Nodes`.
pub const LINK_TEMPLATE: &str = "lab.link.{}";

/// The caption the picked wire carries.
pub const LINK_LABEL: &str = "lab.link.label";

/// The word inside that caption.
pub const LINK_LABEL_TEXT: &str = "lab.link.label.text";

/// The one act offered on the picked wire — `delete` on a drawn one, `adopt` on
/// one a source merely reported.
pub const LINK_ACT: &str = "lab.link.act";

/// The word inside the act seat.
pub const LINK_ACT_TEXT: &str = "lab.link.act.text";

/// The seat that turns the picked wire around (R2000).
pub const LINK_TURN: &str = "lab.link.turn";

/// The word inside the turn seat.
pub const LINK_TURN_TEXT: &str = "lab.link.turn.text";

/// The wire a hand is still holding — drawn from the pin the drag started at to
/// wherever the cursor is now, and belonging to no link because the document
/// does not have one yet.
///
/// ⚠ This is the mark that makes the roster head more than "the picked wire's
/// chrome": it is drawn when NOTHING is picked, so a reader that described this
/// family as *the wires plus what a selection adds* would be wrong about it.
pub const LINK_PREVIEW: &str = "lab.link.preview";

/// The prefix an endpoint chip of the picked wire hangs off.
///
/// ★ Its own stem inside the roster head, because its key is a POSITION rather
/// than a word: the chips are one per endpoint the target offers, so the
/// vocabulary is open the way the census head's is while the family it belongs
/// to is the closed one. [`PALETTE_PART`] is the same arrangement one screen
/// region over.
pub const LINK_ENDPOINT: &str = "lab.link.endpoint.";

/// The suffix a seat's word-bearing child carries.
///
/// ⚠ Not the framework's `caption` suffix, and the difference is load-bearing:
/// a caption is part of its box and is subtracted from a family's member count
/// by every census in this workspace (R1792/R1794), while these runs are members
/// in their own right — the picked wire's column is four marks and the sweep
/// that keeps it inside the canvas measures all four (R1704).
pub const LINK_TEXT: &str = ".text";

/// ★★★★★ R2118 — every FIXED word this family addresses, beside the address
/// declared for it.
///
/// [`RESET_SEATS`]'s shape. What makes this roster different from the bar's and
/// the panel's is what sits beside it: this is not the whole family, it is the
/// half of it whose vocabulary is closed, and the gate's job is to hold it
/// disjoint from the half that is not ([`link`]).
///
/// ⚠ Not a list of what is PAINTED. Four of these are drawn only while a wire is
/// picked, two more only when the picked wire is one the document holds, and the
/// preview only while a hand is dragging. Only the painter knows that; what this
/// roster says is what the screen is entitled to address, which is the question a
/// reader classifying a tag is asking.
pub const LINK_SEATS: &[(&str, &str)] = &[
    ("label", LINK_LABEL),
    ("label.text", LINK_LABEL_TEXT),
    ("act", LINK_ACT),
    ("act.text", LINK_ACT_TEXT),
    ("turn", LINK_TURN),
    ("turn.text", LINK_TURN_TEXT),
    ("preview", LINK_PREVIEW),
];

/// The address of the wire the document calls `id`.
///
/// ★ Takes the document's own [`LinkId`] rather than a number, so a caller
/// holding some other integer — an endpoint position, a row index, a count —
/// cannot compose an address that looks right and names nothing. The card's
/// declaration cannot do this (a card's key is a name the person typed); this
/// family's can, because the id is a type.
#[must_use]
pub fn link(id: LinkId) -> String {
    format!("{LINK}{}", id.0)
}

/// The address of the fixed seat called `word`.
///
/// `word` is one of [`LINK_SEATS`]; a word that is not composes an address
/// nothing paints, which is a lookup answering nothing rather than a wrong mark
/// — [`card_part`]'s direction, and the safe one.
///
/// ⚠ **ITS ONLY CALLER IS THE GATE THAT CHECKS IT, AND THAT IS MEASURED RATHER
/// THAN OVERLOOKED.** Every caller in this crate arrives holding the seat CONST,
/// because this roster's vocabulary is closed AND fixed — nothing expands over
/// it — where [`reset`] next door is genuinely composed, its words coming from
/// an enum a caller iterates.
///
/// ⚠⚠ AND WHICH CRATE KIND IT LIVES IN DECIDES WHO NOTICES. R2110's shell
/// equivalent was refused by the COMPILER as dead code — `hello-analyzer-shell`
/// is a BIN, where `pub` is not an exemption — and a count then confirmed it had
/// no reader. THIS crate is a lib, so nothing refuses a `pub fn` nobody calls,
/// and nothing refuses one whose only caller is its own gate in either kind. So
/// the count here is by hand, and this round nearly deleted on it: [`palette`]
/// next door has exactly this shape. Deleting would move the composition INTO
/// the gate — the general form R2053 measured, though its instance was a gate's
/// own POPULATION (a hand-written module list) rather than a composition. The
/// composer stays and the roster is held against it.
#[must_use]
pub fn link_seat(word: &str) -> String {
    format!("{LINK}{word}")
}

/// The address of the endpoint chip at position `at`.
#[must_use]
pub fn link_endpoint(at: usize) -> String {
    format!("{LINK_ENDPOINT}{at}")
}

/// The address of the word inside that chip.
#[must_use]
pub fn link_endpoint_text(at: usize) -> String {
    format!("{LINK_ENDPOINT}{at}{LINK_TEXT}")
}

/// Which of this family's two vocabularies an address belongs to.
///
/// ★★★★★ R2118 — the answer a `strip_prefix` cannot give. See the section
/// comment above [`LINK`]: the two heads share a stem, so *what is left after
/// the prefix* is not a key until something has said WHICH key it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkMark {
    /// A wire the document holds — the **census** head.
    Wire(LinkId),
    /// One of the fixed words of [`LINK_SEATS`] — the **roster** head.
    Seat(&'static str),
    /// An endpoint chip of the picked wire, or the word inside one — the roster
    /// head's parametric part.
    Endpoint {
        /// Which endpoint the target offers, by position.
        at: usize,
        /// Whether this is the chip's word rather than the chip itself.
        text: bool,
    },
}

/// What a tag of this family names, or `None` when it is not one of them.
///
/// ★★★★★ The inverse of [`link`], [`link_seat`] and [`link_endpoint`] at once,
/// and it has to be one function rather than three: three separate inverses over
/// one stem would each answer about their own head and none of them could say
/// that a tag belongs to *the other* one, which is the failure the section
/// comment above [`LINK`] describes. Asked once, the answer is a classification.
///
/// The census head is tried LAST, and that ordering is not what keeps the heads
/// apart — [`card_of`]'s comment records the lesson that an order is a property
/// of one call site while a refusal is a property of the address. What keeps
/// them apart is that a roster word never parses as a `u32`, which
/// `r2118_the_two_heads_of_the_link_family_are_disjoint` asserts over the whole
/// roster rather than leaving to a reading of these seven words.
#[must_use]
pub fn link_mark_of(tag: &str) -> Option<LinkMark> {
    let tail = tag.strip_prefix(LINK)?;
    if let Some(rest) = tail.strip_prefix("endpoint.") {
        let (digits, text) = match rest.strip_suffix(LINK_TEXT) {
            Some(head) => (head, true),
            None => (rest, false),
        };
        return digits
            .parse::<usize>()
            .ok()
            .map(|at| LinkMark::Endpoint { at, text });
    }
    if let Some((word, _)) = LINK_SEATS.iter().find(|(word, _)| *word == tail) {
        return Some(LinkMark::Seat(word));
    }
    tail.parse::<u32>()
        .ok()
        .map(|id| LinkMark::Wire(LinkId(id)))
}

/// The wire a tag names, or `None` when it names anything else.
///
/// ★ [`link`]'s inverse for a caller that wants the census head alone — a hit
/// test asking *which wire did the pointer land on*, a rhythm census asking
/// *which of these paths is a drawn wire*. Written through [`link_mark_of`] so
/// there is one statement of what separates the heads.
#[must_use]
pub fn link_of(tag: &str) -> Option<LinkId> {
    match link_mark_of(tag)? {
        LinkMark::Wire(id) => Some(id),
        LinkMark::Seat(_) | LinkMark::Endpoint { .. } => None,
    }
}

// ─── what this module does NOT declare yet ──────────────────────────────────

/// ★★★★★ R2116 — **the families this screen paints that nothing here
/// addresses**, named so that a gate can tell them from a family nobody thought
/// of.
///
/// Every address gate this campaign has written asks *does any reader re-spell
/// a family the declaration HOLDS*, and that question has a floor built into it:
/// a family the declaration does not hold is invisible to it. Seventeen
/// instalments each closed one family and left that floor exactly where it was,
/// so this screen could grow an eighteenth region tomorrow, spell it in five
/// modules, and nothing would say a word.
///
/// `hello-packet-view` closed that floor by reaching zero and asserting it
/// (R2115). This screen cannot: it is four times the size and the remainder is
/// families, not sites. So the remainder is DECLARED instead — and the gate
/// that reads it asserts it in **both** directions:
///
/// * every family the assembled screen paints is either recovered by a reader
///   above or named here — so a NEW undeclared family fails on the day it is
///   painted, which is what the seventeen instalments could not do;
/// * every name here is still unaddressed — so a round that declares one and
///   forgets to strike it out fails too, which is what stops this list becoming
///   the hand-written thing it is standing in for.
///
/// ⚠ These are STEMS, two segments, not addresses. Nothing composes onto them
/// and no reader takes them apart; they exist to be counted and struck out. The
/// day this slice is empty, the gate that reads it becomes
/// `r2115_no_module_but_the_declaration_spells_this_screens_namespace` — the
/// same assertion screen B already carries — and this const goes with it.
/// ⚠⚠ THE POPULATION IS THE SCREEN'S OWN SWEEP, not the assembly's. Four of
/// these — the application bar, the rail, the toast and the fault panel — are
/// painted by the STANDALONE binary and not by the screen mounted as a page,
/// because the shell draws its own bar and its own rail. So a reader who
/// measured this list against the shell would find four names owning nothing
/// and conclude the list had rotted. That is why exactness is asserted by
/// `r2116_every_family_this_screen_paints_is_declared_or_owed` in this crate,
/// over `STATES`, and the assembled gate next door only refuses a family that
/// is in neither place.
pub const UNADDRESSED_FAMILIES: &[&str] = &[
    "lab.appbar",
    "lab.canvas",
    "lab.crumb",
    "lab.faults",
    "lab.gate",
    "lab.hint",
    "lab.observed",
    "lab.rail",
    "lab.toast",
];

/// ★★★★★ R2116 — the namespace every address of this screen begins with, and
/// the needle a gate over the whole screen uses.
///
/// Declared rather than spelled because a gate that spells its own needle is
/// this debt one level up (R2053). The trailing separator is part of it: a
/// reader classifying a snapshot by family compares against this, and `lab`
/// alone would also claim a screen called `labsomething`.
pub const NAMESPACE: &str = "lab.";
