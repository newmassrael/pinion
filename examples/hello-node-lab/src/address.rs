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

/// That card's address.
#[must_use]
pub fn card(name: &str) -> String {
    CARD_TEMPLATE.replace("{}", name)
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
