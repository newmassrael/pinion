//! R1691 §5.40 §5.12 §2 #7 — **which painted regions have a voice**, and why
//! the silent ones are silent.
//!
//! ## The question nothing could ask
//!
//! A screen paints regions and publishes an accessibility tree, and the two are
//! built by different code. Nothing in either says whether they *agree*. A
//! region that nobody gave a node to is not an error anywhere: it paints, it
//! takes presses, and a reader is simply never told it exists. The failure is
//! silent in the exact sense that matters — there is no log line, no refusal,
//! and no count.
//!
//! Measured on this tree the day this module was written: the reference
//! analysis tool's first screen painted **166** addressable regions and
//! announced **30** of them (its tree held 35 nodes; five are virtual
//! description regions, which is a distinction nothing had drawn either). So
//! **136 were unclassified.** The gap had never been reported because no
//! surface asked for it, and it was found by hand, one round, because somebody
//! wondered whether one new pill had a name.
//!
//! ## Against the reference toolkit at 6.11 (built and run, not read)
//!
//! Its accessibility layer builds an interface for every widget automatically,
//! which sounds like the stronger position and is the reason the question
//! cannot be asked there. A probe built against 6.11.1 and run:
//!
//! * a window with six children answered **7 nodes, 4 of them with an empty
//!   accessible name** — a button whose name the author forgot, a decorative
//!   rule, a custom painted region, and the window. **The forgotten button and
//!   the decorative rule are the same answer**: a role and no name. One is a
//!   defect and one is correct, and nothing separates them.
//! * clearing every author-settable accessibility slot on the rule (its widget
//!   has exactly three: a name, a description and an identifier) left the tree
//!   at 7 nodes. **There is no way to declare a region deliberately silent.**
//! * hiding it left the tree at 7 nodes too, with one state bit flipped — so
//!   the only act that quiets a region also removes its ink, and does not even
//!   remove the node.
//! * a label bound to a field announced its text **twice**: once as the label's
//!   own name and once as the field's. The duplicate is produced by
//!   construction and there is nothing to suppress it with.
//!
//! So the floor has an accessibility tree and no census of it. This module is
//! the census: every addressable painted region is **classified**, and the
//! classification is checked in both directions.
//!
//! ## The shape
//!
//! A declaration carries a [`Silence`] — a [`kind`](Silence::kind) from a
//! closed vocabulary and one [`detail`](Silence::detail) the kind gives meaning
//! to. It is the same shape as
//! [`Unavailable`](crate::availability::Unavailable), and for the same reason:
//! the arms exist because each one leaves a reader somewhere different, and
//! that difference is derived once as a [`Relay`] rather than restated per
//! site.
//!
//! ```
//! use pinion_core::voice::{Relay, Silence};
//!
//! // A colour swatch: there is nothing for a reader to receive.
//! assert_eq!(Silence::decorative("a colour swatch").relay(), Relay::Nowhere);
//! // A caption inside a button: the button's NAME is this text.
//! let caption = Silence::name_of("lab.toolbar.run");
//! assert_eq!(caption.relay(), Relay::Peer);
//! assert_eq!(caption.relay_target(), Some("lab.toolbar.run"));
//! ```
//!
//! [`voice_census`] then walks a produced paint scene and answers per tag with
//! a [`Voice`]. Five of its seven arms are defects, and each is a *different*
//! defect with a different fix — which is the whole reason the census is not a
//! bool.
//!
//! ## A name is only judgeable where a voice is owed (R1692)
//!
//! Counting nodes is not the question either: `role = Button, name = ""` is a
//! node. [`NameFault`] judges what a region actually says, by three rules with
//! no taste in them — nothing said, the address said instead of a name, or
//! nothing a reader can pronounce.
//!
//! The rules are worth nothing on their own, and the same probe shows why. Run
//! against the floor at 6.11.1, a window of six regions answered **9 nodes, 8
//! of which fail one of these three rules** — five with an empty name, of which
//! exactly *one* is a defect and four are ornament, a layout box and the window
//! itself. The framework had also derived a button's name from its visible text
//! and produced `×` by construction. So there the rules are noise: 8 flags for
//! 1 defect, because nothing separates a region that owes a voice from one that
//! does not. Here the [`Silence`] declarations do that separating, so the rules
//! only ever run where a voice was owed — and a flag is a defect.
//!
//! ## Silence is not invisibility
//!
//! The declaration is independent of the ink, which is the conflation the floor
//! cannot escape. A decorative rule stays painted and stops being announced; a
//! layout box keeps its children's voices while losing its own. Nothing here
//! changes a pixel.
//!
//! ## Why the census is not a set difference
//!
//! It would be one line: painted tags minus announced tags. That answers *how
//! many* and never *which are wrong*, and the difference is the entire value —
//! a screen with two hundred correctly-silent regions and one forgotten button
//! reads as 201 either way. The classification is what makes the number
//! actionable, and the [`Voice::Dangling`] arm is what stops a classification
//! from being a place to hide: a reason that names a node which does not itself
//! speak is a lie the census refuses.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::scene::Scene;

/// Why an addressable painted region has no node in the accessibility tree.
///
/// Closed, and each arm exists because it leaves a reader in a different place
/// — see [`Relay`], which is that difference derived.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SilenceKind {
    /// It carries no information a reader can lose: a rule, a colour swatch, a
    /// shadow, a grid line. [`detail`](Silence::detail) says what it draws.
    ///
    /// Covers the subtree — ornament is ornament all the way down.
    Decorative,
    /// A box whose job is to place its children. The children speak; a node
    /// here would add a level of tree with nothing in it.
    /// [`detail`](Silence::detail) says what it arranges.
    ///
    /// The **one** arm that does not cover its subtree, which is the point of
    /// it: each child is still classified on its own.
    Layout,
    /// Its text is announced as another node's **name**, so a voice here would
    /// say the same thing twice — the duplicate the floor produces by
    /// construction for every label bound to a field.
    /// [`detail`](Silence::detail) names that node.
    NameOf,
    /// Its content is folded into another node's announcement — that node's
    /// value, description or children — so a reader receives it there, whole.
    /// [`detail`](Silence::detail) names that node.
    ///
    /// The named node is a **parent in the accessibility tree**, which need not
    /// be its parent in the paint tree: a switch's state caption is painted
    /// beside the switch and announced as part of it. That the two trees differ
    /// is exactly why the redirect names a tag instead of being inferred from
    /// the scene.
    ///
    /// Distinct from [`NameOf`](Self::NameOf) by what it becomes: a name is
    /// what a control is *called* and is read on every focus move, while a
    /// folded part is read when the reader asks for detail. Announcing a
    /// paragraph as a name is the failure this separation prevents.
    PartOf,
}

impl SilenceKind {
    /// The lowercase wire spelling.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            SilenceKind::Decorative => "decorative",
            SilenceKind::Layout => "layout",
            SilenceKind::NameOf => "name_of",
            SilenceKind::PartOf => "part_of",
        }
    }

    /// Parse a wire spelling back.
    ///
    /// The inverse of [`name`](Self::name), so what a surface publishes is what
    /// it accepts — the symmetry R1616 made a rule after a published vocabulary
    /// turned out not to be a readable one.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|k| k.name() == name)
    }

    /// Every arm, in declaration order.
    ///
    /// A member list rather than a count: proving a set is complete by
    /// searching for what is missing yields zero every time (R1650).
    pub const ALL: [SilenceKind; 4] = [
        SilenceKind::Decorative,
        SilenceKind::Layout,
        SilenceKind::NameOf,
        SilenceKind::PartOf,
    ];

    /// Where a reader receives this region's information instead.
    ///
    /// Derived here, once, rather than restated at each declaration — the same
    /// posture [`Recourse`](crate::availability::Recourse) takes for an
    /// unavailable region.
    #[must_use]
    pub const fn relay(self) -> Relay {
        match self {
            SilenceKind::Decorative => Relay::Nowhere,
            SilenceKind::Layout => Relay::Children,
            SilenceKind::NameOf => Relay::Peer,
            SilenceKind::PartOf => Relay::Ancestor,
        }
    }

    /// Whether the declaration reaches the node's descendants.
    ///
    /// [`Layout`](Self::Layout) is the only arm that does not, and that is what
    /// it means: a box that merely places things has said nothing about what it
    /// placed. Ornament, a borrowed name and a summarised whole all extend
    /// downward, because in each case the *content* is what the reason is about.
    #[must_use]
    pub const fn covers_subtree(self) -> bool {
        !matches!(self, SilenceKind::Layout)
    }

    /// Whether [`detail`](Silence::detail) is the tag of another node rather
    /// than prose — which is what makes it checkable.
    #[must_use]
    pub const fn detail_is_a_tag(self) -> bool {
        matches!(self, SilenceKind::NameOf | SilenceKind::PartOf)
    }
}

/// Where a reader receives a silent region's information — derived from
/// [`SilenceKind`], never declared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Relay {
    /// Nowhere, because there is nothing to receive.
    Nowhere,
    /// From the children, each of which speaks for itself.
    Children,
    /// From a named peer, as that peer's name.
    Peer,
    /// From a named ancestor, which announces the whole.
    Ancestor,
}

impl Relay {
    /// The lowercase wire spelling.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Relay::Nowhere => "nowhere",
            Relay::Children => "children",
            Relay::Peer => "peer",
            Relay::Ancestor => "ancestor",
        }
    }

    /// Parse a wire spelling back.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|r| r.name() == name)
    }

    /// Every arm, in declaration order.
    pub const ALL: [Relay; 4] = [
        Relay::Nowhere,
        Relay::Children,
        Relay::Peer,
        Relay::Ancestor,
    ];

    /// Whether a reader can reach the information at all.
    ///
    /// The one predicate a screen needs that the kind does not answer: it
    /// separates *ornament* — where nothing was lost — from the three arms
    /// where something was moved and can be navigated to.
    #[must_use]
    pub const fn is_reachable(self) -> bool {
        !matches!(self, Relay::Nowhere)
    }

    /// The census arm a region takes when this relay's promise is **not kept**.
    ///
    /// A reachable relay is a claim about somewhere else: that a named peer
    /// says this text, that a named ancestor folds it in, that the children
    /// speak for the box. Each claim can be false, and a false one is worse
    /// than an undeclared silence because it reads as handled — so each gets an
    /// arm rather than being absorbed into [`Voice::Silent`].
    ///
    /// [`Nowhere`](Self::Nowhere) promises nothing, so there is nothing to
    /// break and the region is simply quiet. That is what makes this total: the
    /// mapping is stated once here instead of at the two places the census
    /// checks a promise, and
    /// [`is_reachable`](Self::is_reachable) agreeing with "this arm is a defect"
    /// is an invariant a fifth [`SilenceKind`] has to answer for.
    #[must_use]
    pub const fn unkept(self) -> Voice {
        match self {
            Relay::Nowhere => Voice::Silent,
            Relay::Children => Voice::Hollow,
            Relay::Peer | Relay::Ancestor => Voice::Dangling,
        }
    }
}

/// Why the name a region announces is not one a reader can use.
///
/// Three rules, and the shared property is that **none of them has any taste in
/// it**: each is decidable from the tag and the name alone, and each has been a
/// real defect on this tree. Judging whether a name is a *good* name is not
/// mechanisable and is not attempted — this is a floor, and a region that clears
/// it can still be badly named.
///
/// The rules only run where a voice is **owed**, which is the whole reason they
/// are usable. Measured against the reference toolkit at 6.11.1, where every
/// widget is in the tree and nothing declares a silence, the same three rules
/// flag 8 of 9 nodes to find 1 defect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NameFault {
    /// Nothing is said. The tree has a node, and a reader is told its role and
    /// no more — "button", and nothing about which button.
    ///
    /// The floor's default for any control without visible text, and there
    /// indistinguishable from ornament.
    Absent,
    /// The **address** is said instead of a name: the announced text is this
    /// node's own tag, and the tag is structured rather than a word.
    ///
    /// A tag is how a program addresses a region; it is not how a person is
    /// told what it is. The structure test is what keeps this from firing on a
    /// control whose visible label happens to be its tag spelled as one word —
    /// a name matching the visible label is *correct* (WAI-ARIA's label-in-name),
    /// so only a name that is recognisably an identifier is a fault.
    Address,
    /// Nothing pronounceable is said: the name has no letter or digit anywhere
    /// — `×`, `…`, `+`, `—`.
    ///
    /// Announced verbatim, a screen reader says the symbol's Unicode name or
    /// nothing at all. R1686 fixed exactly this on a remove control; the floor
    /// *manufactures* it, deriving a button's accessible name from its visible
    /// text with no test that the text is a word.
    Wordless,
}

impl NameFault {
    /// The lowercase wire spelling.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            NameFault::Absent => "absent",
            NameFault::Address => "address",
            NameFault::Wordless => "wordless",
        }
    }

    /// Parse a wire spelling back.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|f| f.name() == name)
    }

    /// Every arm, in declaration order.
    pub const ALL: [NameFault; 3] = [NameFault::Absent, NameFault::Address, NameFault::Wordless];

    /// Judge the name a reader hears for the region tagged `tag`, or [`None`]
    /// when it clears all three rules.
    ///
    /// `name_required` is
    /// [`Announcement::name_required`](Announcement::name_required) — the one
    /// rule that a role can excuse, because for a blank table cell an empty name
    /// is the data. The other two are never excused: an address and a symbol are
    /// wrong answers wherever they appear.
    ///
    /// Order matters only in what gets reported: an empty name is [`Absent`]
    /// rather than [`Wordless`], because "nobody wrote one" and "what was
    /// written is unpronounceable" are different repairs.
    ///
    /// [`Absent`]: Self::Absent
    /// [`Wordless`]: Self::Wordless
    #[must_use]
    pub fn judge(tag: &str, name: &str, name_required: bool) -> Option<Self> {
        let said = name.trim();
        if said.is_empty() {
            return name_required.then_some(NameFault::Absent);
        }
        if said == tag && is_address_shaped(tag) {
            return Some(NameFault::Address);
        }
        if !said.chars().any(char::is_alphanumeric) {
            return Some(NameFault::Wordless);
        }
        None
    }
}

/// Whether a tag is recognisably an identifier rather than a word.
///
/// True when it holds anything a written name does not: `lab.toolbar.run`,
/// `method#0`, `grid/row`, `chart-tab`, `scale_group`.
///
/// The carve-out is deliberate and was measured. A tag that reads as a word or
/// words — `notch`, `violin`, `log axis` — is *not* an address here, because
/// those screens paint that word as the control's visible label and an
/// accessible name matching the visible label is correct (WAI-ARIA's
/// label-in-name). Three such tags on this tree would otherwise be flagged, and
/// all three are right.
fn is_address_shaped(tag: &str) -> bool {
    tag.chars().any(|c| !c.is_alphanumeric() && c != ' ')
}

/// A declaration that a region deliberately has no voice, and why.
///
/// The peer of [`Unavailable`](crate::availability::Unavailable): one kind from
/// a closed vocabulary and one detail whose meaning the kind fixes, so the
/// detail never has to be parsed to find out what it is.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Silence {
    kind: SilenceKind,
    detail: Cow<'static, str>,
}

impl Silence {
    /// A silence of `kind`, with the detail that kind gives meaning to.
    #[must_use]
    pub fn new(kind: SilenceKind, detail: impl Into<Cow<'static, str>>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }

    /// It draws ornament; `detail` says what.
    #[must_use]
    pub fn decorative(detail: impl Into<Cow<'static, str>>) -> Self {
        Self::new(SilenceKind::Decorative, detail)
    }

    /// It places its children; `detail` says what it arranges.
    #[must_use]
    pub fn layout(detail: impl Into<Cow<'static, str>>) -> Self {
        Self::new(SilenceKind::Layout, detail)
    }

    /// Its text is the name of the node tagged `tag`.
    #[must_use]
    pub fn name_of(tag: impl Into<Cow<'static, str>>) -> Self {
        Self::new(SilenceKind::NameOf, tag)
    }

    /// Its content is folded into the announcement of the node tagged `tag`.
    #[must_use]
    pub fn part_of(tag: impl Into<Cow<'static, str>>) -> Self {
        Self::new(SilenceKind::PartOf, tag)
    }

    /// Which class of silence this is.
    #[must_use]
    pub const fn kind(&self) -> SilenceKind {
        self.kind
    }

    /// The specific thing [`kind`](Self::kind) points at.
    #[must_use]
    pub fn detail(&self) -> &str {
        &self.detail
    }

    /// Where a reader receives the information instead.
    #[must_use]
    pub const fn relay(&self) -> Relay {
        self.kind.relay()
    }

    /// The tag this silence redirects a reader to, or [`None`] when the detail
    /// is prose.
    ///
    /// This is what [`voice_census`] checks: a redirect to a node that does not
    /// itself speak sends a reader nowhere, which is worse than an undeclared
    /// silence because it reads as handled.
    #[must_use]
    pub fn relay_target(&self) -> Option<&str> {
        self.kind.detail_is_a_tag().then(|| self.detail.as_ref())
    }
}

/// What the accessibility tree says about one addressable region.
///
/// Seven arms, of which [`Announced`](Self::Announced) and [`Silent`](Self::Silent)
/// are the two correct outcomes and the other five are five *different*
/// defects. A census that only counted would merge them, and the fix for each is
/// different: give it a name, say something usable instead of an address, decide
/// why it is quiet, delete the node, point the reason somewhere real, or give
/// the box that promised its children speak a child that does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Voice {
    /// The region is painted and the accessibility tree has a node for it.
    Announced,
    /// The region is painted, has no node, and the scene says why.
    Silent,
    /// The region is painted, has no node, and nobody decided that. **A reader
    /// is never told it exists and no author chose it.**
    Unvoiced,
    /// A node is announced for a tag nothing paints, and no other node refers to
    /// it. A reader can be sent to a name with no region behind it.
    ///
    /// A node referred to by another — a description region, a composite child,
    /// a bounds contributor — is *not* this: it is deliberately virtual, and the
    /// reference it carries is what makes it reachable.
    Ghost,
    /// The region is silent by a reason that names another node, and that node
    /// does not speak either. The redirect goes nowhere.
    Dangling,
    /// The accessibility tree has a node for the region and what it announces is
    /// not usable — see [`NameFault`] for which of the three it is.
    ///
    /// Distinct from [`Unvoiced`](Self::Unvoiced) by who has to act: an unvoiced
    /// region needs a *decision* (does it owe a reader anything?), while this one
    /// was already decided and the answer came out empty. Counting it as
    /// announced is what the floor does, and is how a control can be in the tree
    /// for years saying nothing.
    Mumbled,
    /// The region is silent by a reason that promises **its children speak**,
    /// and nothing under it does.
    ///
    /// The subtree is inaudible whole, and every check passes: each descendant
    /// is correctly quiet on its own terms, and the box that was supposed to
    /// carry them says it is only arranging things. The one promise that cannot
    /// be checked from the declaration — only from below.
    Hollow,
}

impl Voice {
    /// The lowercase wire spelling.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Voice::Announced => "announced",
            Voice::Silent => "silent",
            Voice::Unvoiced => "unvoiced",
            Voice::Ghost => "ghost",
            Voice::Dangling => "dangling",
            Voice::Mumbled => "mumbled",
            Voice::Hollow => "hollow",
        }
    }

    /// Parse a wire spelling back.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|v| v.name() == name)
    }

    /// Every arm, in declaration order.
    pub const ALL: [Voice; 7] = [
        Voice::Announced,
        Voice::Silent,
        Voice::Unvoiced,
        Voice::Ghost,
        Voice::Dangling,
        Voice::Mumbled,
        Voice::Hollow,
    ];

    /// Whether this arm is a defect — five of the seven are.
    #[must_use]
    pub const fn is_defect(self) -> bool {
        matches!(
            self,
            Voice::Unvoiced | Voice::Ghost | Voice::Dangling | Voice::Mumbled | Voice::Hollow
        )
    }

    /// Whether the accessibility tree has a node for this region at all.
    ///
    /// Two arms do — a region that speaks and one that speaks unusably — and
    /// the difference matters exactly once: a box promising that its children
    /// speak has kept that promise the moment one of them is *in the tree*, so
    /// its subtree is not [`Hollow`](Self::Hollow) merely because a child is
    /// [`Mumbled`](Self::Mumbled). Reporting one region's defect twice, under
    /// two tags, would make the census's own numbers wrong.
    #[must_use]
    pub const fn has_node(self) -> bool {
        matches!(self, Voice::Announced | Voice::Mumbled)
    }
}

/// Every wire spelling a [`SilenceKind`] can take, derived from the arms so a
/// published vocabulary cannot lag the enum.
///
/// A client's claim on this axis is that it can match the kind **exhaustively**,
/// and a hand-written list would make that claim wrong at the moment a fifth arm
/// arrived — the failure R1616 made a rule after.
pub const SILENCE_KIND_WIRE_NAMES: [&str; SilenceKind::ALL.len()] = {
    let mut names = [""; SilenceKind::ALL.len()];
    let mut i = 0;
    while i < SilenceKind::ALL.len() {
        names[i] = SilenceKind::ALL[i].name();
        i += 1;
    }
    names
};

/// Every wire spelling a [`Relay`] can take, derived the same way.
pub const RELAY_WIRE_NAMES: [&str; Relay::ALL.len()] = {
    let mut names = [""; Relay::ALL.len()];
    let mut i = 0;
    while i < Relay::ALL.len() {
        names[i] = Relay::ALL[i].name();
        i += 1;
    }
    names
};

/// Every wire spelling a [`Voice`] can take, derived the same way.
pub const VOICE_WIRE_NAMES: [&str; Voice::ALL.len()] = {
    let mut names = [""; Voice::ALL.len()];
    let mut i = 0;
    while i < Voice::ALL.len() {
        names[i] = Voice::ALL[i].name();
        i += 1;
    }
    names
};

/// Every wire spelling a [`NameFault`] can take, derived the same way.
pub const NAME_FAULT_WIRE_NAMES: [&str; NameFault::ALL.len()] = {
    let mut names = [""; NameFault::ALL.len()];
    let mut i = 0;
    while i < NameFault::ALL.len() {
        names[i] = NameFault::ALL[i].name();
        i += 1;
    }
    names
};

/// What the accessibility tree says about one tag, as far as the census needs
/// to know.
///
/// Two fields, and each answers a question a set of tags cannot: **what a
/// reader hears** (a node is not a voice — see [`NameFault`]), and **what the
/// node is made of** (a node with no region of its own is still anchored when
/// its members have one).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Announcement {
    /// The name a reader hears. Empty when the tree gives none, which is an
    /// answer rather than a gap — the floor's default and its commonest defect.
    pub name: String,
    /// Whether an empty [`name`](Self::name) is a defect here.
    ///
    /// A property of the node's **role**, which this crate cannot see: a table
    /// cell with no value is a blank cell and a container with no semantics has
    /// nothing to be called, while a button, a slider or a column header that
    /// says nothing is a region a reader lands on and learns nothing from.
    /// Supplied by whoever builds the tree
    /// (`pinion_a11y::AriaRole::name_required`).
    ///
    /// Defaults to `true`: an exemption is a claim somebody has to make, and a
    /// field that silently defaulted to "no name needed" would let the whole
    /// rule be switched off by forgetting it.
    pub name_required: bool,
    /// The node is a **live region**: it is announced when it changes rather
    /// than reached by navigating, so it owes no painted rectangle.
    ///
    /// This is the WAI-ARIA pattern a screen has whenever it reports something
    /// a sighted reader learns from the change itself — a filtered count, a
    /// panel that rearranged. Without this the census would call such a node a
    /// [`Ghost`](Voice::Ghost), which is the opposite of the truth: nobody can
    /// be sent there, because it is what comes to them.
    pub live: bool,
    /// The tags this node is **composed of**: its members, and the fragments
    /// its own rectangle is the union of.
    ///
    /// A composite container — a legend, a transcript, a radio group — is
    /// announced for a tag the scene need not paint, because what it stands for
    /// is what it holds. A reader landing on it descends to ink. That is a
    /// different anchor from being *pointed at* (the `referenced` argument of
    /// [`voice_census`]) and both are needed: one is "somebody sends a reader to
    /// me", the other is "I can send a reader on".
    ///
    /// References that are **about another node's content** — a description
    /// region, a name source, a controlled target — are deliberately not here.
    /// They say nothing about whether this node has anything behind it.
    pub composes: Vec<String>,
}

impl Default for Announcement {
    fn default() -> Self {
        Self {
            name: String::new(),
            name_required: true,
            live: false,
            composes: Vec::new(),
        }
    }
}

impl Announcement {
    /// A node that announces `name`, composes nothing, and owes a name — the
    /// ordinary case, and the one every test writes.
    #[must_use]
    pub fn named(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Self::default()
        }
    }

    /// Add a tag this node is composed of.
    #[must_use]
    pub fn composing(mut self, tag: impl Into<String>) -> Self {
        self.composes.push(tag.into());
        self
    }

    /// Declare that an empty name is the answer here rather than an omission —
    /// see [`name_required`](Self::name_required).
    #[must_use]
    pub fn optionally_named(mut self) -> Self {
        self.name_required = false;
        self
    }

    /// Declare this a live region — see [`live`](Self::live).
    #[must_use]
    pub fn living(mut self) -> Self {
        self.live = true;
        self
    }
}

/// One addressable region, and what the accessibility tree says about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceNode {
    /// The tag — the same spelling `scene/click`, `scene/invoke` and
    /// `scene/access` address, so a reader of this census can act on a row.
    pub tag: String,
    /// What the tree says.
    pub voice: Voice,
    /// What a reader actually hears, when the tree has a node for this tag.
    ///
    /// Carried rather than judged-and-discarded so a row that fails is one a
    /// person can act on without re-running anything: the census says the name
    /// is unusable *and what it was*.
    pub name: Option<String>,
    /// Why [`name`](Self::name) is unusable. [`Some`] exactly when
    /// [`voice`](Self::voice) is [`Voice::Mumbled`], since that arm is defined
    /// as "there is a fault" and this is which one.
    pub fault: Option<NameFault>,
    /// The declaration that reached this node, when one did — its own or an
    /// ancestor's, following the same precedence the disabled cascade uses.
    pub silence: Option<Silence>,
    /// The node carries its own declaration. True together with
    /// [`declared_by`](Self::declared_by) means a node that declares itself
    /// silent *and* sits inside a silent region.
    pub self_declared: bool,
    /// The nearest **strict ancestor** whose declaration covers this node, or
    /// [`None`].
    pub declared_by: Option<String>,
}

/// Every addressable region of a paint scene, classified — and every announced
/// tag with no region behind it.
///
/// Counts are **derived**, never stored: a coverage figure written down is a
/// figure that stops falling when the thing it measures does (R1690).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct VoiceCensus {
    /// The rows, painted ones in paint order followed by any [`Voice::Ghost`].
    pub nodes: Vec<VoiceNode>,
}

impl VoiceCensus {
    /// How many rows carry `voice`.
    #[must_use]
    pub fn count(&self, voice: Voice) -> usize {
        self.nodes.iter().filter(|n| n.voice == voice).count()
    }

    /// Every row whose arm is a defect, in census order.
    pub fn defects(&self) -> impl Iterator<Item = &VoiceNode> {
        self.nodes.iter().filter(|n| n.voice.is_defect())
    }

    /// Whether every addressable region is either announced or declared silent,
    /// and every announced node has a region or a referrer.
    ///
    /// The one predicate a gate wants, derived from the arms so it cannot
    /// disagree with them.
    #[must_use]
    pub fn is_total(&self) -> bool {
        !self.nodes.iter().any(|n| n.voice.is_defect())
    }
}

/// Classify every addressable region of a produced paint scene against the
/// accessibility tree that was built beside it.
///
/// `announced` maps every tag the tree has a node for to what it
/// [`Announcement`] says. It is that rather than a set of tags because a census
/// of nodes is not a census of voices: a node with nothing usable to say is
/// exactly the floor's failure, and it cannot be seen from the tags alone.
///
/// `referenced` is every tag some node points at — a description region, a
/// composite child, a bounds contributor, a name source — which is one of the
/// three things that separate a deliberately virtual node from a
/// [`Ghost`](Voice::Ghost). The others are [`composes`](Announcement::composes),
/// the same question asked from the other end, and [`live`](Announcement::live),
/// a region a reader never travels to because it comes to them.
///
/// Untagged nodes are not rows. They cannot be addressed, so a row for one
/// would name nothing — the same rule
/// [`disabled_census`](crate::scene_disabled::disabled_census) applies, for the
/// same reason.
#[must_use]
pub fn voice_census(
    scene: &Scene,
    announced: &BTreeMap<String, Announcement>,
    referenced: &BTreeSet<String>,
) -> VoiceCensus {
    let mut nodes = Vec::new();
    let mut painted = BTreeSet::new();
    walk(scene, None, None, announced, &mut painted, &mut nodes);
    // The other direction. A name with no region behind it is reachable only
    // through something: whoever refers to it, or whatever it is made of. With
    // neither, a reader can be sent to it and find nothing.
    for (tag, announcement) in announced {
        if painted.contains(tag) || referenced.contains(tag) || announcement.live {
            continue;
        }
        if announcement
            .composes
            .iter()
            .any(|member| painted.contains(member))
        {
            continue;
        }
        nodes.push(VoiceNode {
            tag: tag.clone(),
            voice: Voice::Ghost,
            name: Some(announcement.name.clone()),
            // Not judged: what is wrong with a ghost is that it has nothing
            // behind it, and the repair is to paint it or delete it. Judging
            // its name too would report one defect as two.
            fault: None,
            silence: None,
            self_declared: false,
            declared_by: None,
        });
    }
    VoiceCensus { nodes }
}

/// Depth-first walk carrying the nearest covering declaration and the tag that
/// made it, mirroring
/// [`disabled_census`](crate::scene_disabled::disabled_census)'s cascade.
///
/// Returns whether anything in this subtree — including the node itself — has a
/// node in the accessibility tree, which is the only way to check the one
/// promise a declaration makes about what is *below* it.
fn walk(
    scene: &Scene,
    declared_by: Option<&str>,
    inherited: Option<&Silence>,
    announced: &BTreeMap<String, Announcement>,
    painted: &mut BTreeSet<String>,
    out: &mut Vec<VoiceNode>,
) -> bool {
    let own = scene
        .layout_style()
        .and_then(|layout| layout.silence.as_ref());
    let self_declared = own.is_some();
    // The node's own reason when it has one, otherwise the region's — the same
    // precedence the disabled cascade applies.
    let reason = own.or(inherited);
    let mut row = None;
    let mut speaks = false;
    if let Some(tag) = scene.tag() {
        painted.insert(tag.to_owned());
        let (voice, fault) = classify(tag, reason, announced);
        speaks = voice.has_node();
        row = Some(out.len());
        out.push(VoiceNode {
            tag: tag.to_owned(),
            voice,
            name: announced.get(tag).map(|a| a.name.clone()),
            fault,
            silence: reason.cloned(),
            self_declared,
            declared_by: declared_by.map(str::to_owned),
        });
    }
    // What reaches the CHILDREN: this node's declaration when it has one that
    // covers a subtree, else whatever was already covering it.
    // A layout box has said nothing about what it placed, so its children
    // inherit whatever was covering the box itself — the same answer as a node
    // that declared nothing at all, which is why the two are one arm.
    let (child_declarer, child_reason) = match own {
        Some(silence) if silence.kind().covers_subtree() => (scene.tag().or(declared_by), own),
        _ => (declared_by, inherited),
    };
    match scene {
        Scene::Container(c) => {
            for child in &c.children {
                speaks |= walk(child, child_declarer, child_reason, announced, painted, out);
            }
        }
        Scene::Scroll(s) => {
            speaks |= walk(
                &s.content,
                child_declarer,
                child_reason,
                announced,
                painted,
                out,
            );
        }
        Scene::Box(_)
        | Scene::Text(_)
        | Scene::Path(_)
        | Scene::Image(_)
        | Scene::External(_)
        | Scene::Effect(_)
        | Scene::ImmediateModeNode(_)
        | Scene::TextGrid(_) => {}
    }
    // R1692 — the promise that can only be checked from below. `Relay::Children`
    // says a reader receives this region's information from what it holds; if
    // nothing under it is in the tree, the subtree is inaudible whole and every
    // per-node check still passes. Expressed as the relay's property rather than
    // as the arm's name, so a fifth kind relaying to its children inherits it.
    if let Some(index) = row {
        if out[index].voice == Voice::Silent
            && own.is_some_and(|silence| silence.relay() == Relay::Children)
            && !speaks
        {
            out[index].voice = Relay::Children.unkept();
        }
    }
    speaks
}

/// The arm for one painted tag, and the name fault when there is one.
///
/// A node that both speaks and declares a reason is [`Announced`](Voice::Announced):
/// the tree is what a reader actually receives, and a stale declaration beside a
/// real voice is a documentation problem rather than an accessibility one. The
/// census still carries the declaration on the row, so it can be found.
fn classify(
    tag: &str,
    reason: Option<&Silence>,
    announced: &BTreeMap<String, Announcement>,
) -> (Voice, Option<NameFault>) {
    if let Some(announcement) = announced.get(tag) {
        return match NameFault::judge(tag, &announcement.name, announcement.name_required) {
            Some(fault) => (Voice::Mumbled, Some(fault)),
            None => (Voice::Announced, None),
        };
    }
    let Some(reason) = reason else {
        return (Voice::Unvoiced, None);
    };
    let voice = match reason.relay_target() {
        // A redirect to a node that is not in the tree at all. A node that IS in
        // the tree and mumbles is a defect of its own, reported on its own row —
        // the redirect did arrive somewhere.
        Some(target) if !announced.contains_key(target) => reason.relay().unkept(),
        _ => Voice::Silent,
    };
    (voice, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{ContainerNode, Rect, TextNode};
    use crate::style::LayoutStyle;

    /// Tags the tree announces, each with a name that clears every rule — so a
    /// test about *classification* never fails for a reason about naming.
    fn tags(names: &[&str]) -> BTreeMap<String, Announcement> {
        names
            .iter()
            .map(|s| ((*s).to_owned(), Announcement::named(format!("the {s}"))))
            .collect()
    }

    fn refs(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|s| (*s).to_owned()).collect()
    }

    fn text(tag: &'static str) -> Scene {
        Scene::Text(TextNode::new(tag, Rect::default()).with_tag(tag))
    }

    fn quiet(tag: &'static str, silence: Silence, children: Vec<Scene>) -> Scene {
        Scene::Container(
            ContainerNode::new(children)
                .with_tag(tag)
                .with_layout(LayoutStyle::new().with_silence(silence)),
        )
    }

    fn group(tag: &'static str, children: Vec<Scene>) -> Scene {
        Scene::Container(ContainerNode::new(children).with_tag(tag))
    }

    fn row<'a>(census: &'a VoiceCensus, tag: &str) -> &'a VoiceNode {
        census
            .nodes
            .iter()
            .find(|n| n.tag == tag)
            .unwrap_or_else(|| panic!("no row for {tag}"))
    }

    #[test]
    fn a_painted_tag_with_a_node_is_announced() {
        let scene = group("root", vec![text("a")]);
        let census = voice_census(&scene, &tags(&["root", "a"]), &BTreeSet::new());
        assert_eq!(census.nodes.len(), 2);
        assert!(census.is_total());
        assert_eq!(row(&census, "a").voice, Voice::Announced);
    }

    /// The defect this module exists for: painted, addressable, and nobody
    /// decided anything about it.
    #[test]
    fn a_painted_tag_with_no_node_and_no_reason_is_unvoiced() {
        let scene = group("root", vec![text("forgotten")]);
        let census = voice_census(&scene, &tags(&["root"]), &BTreeSet::new());
        assert_eq!(row(&census, "forgotten").voice, Voice::Unvoiced);
        assert!(!census.is_total());
        assert_eq!(census.count(Voice::Unvoiced), 1);
    }

    /// The distinction the floor cannot make: the forgotten control and the
    /// ornament are the same absence there, and two different arms here.
    #[test]
    fn a_declared_silence_and_a_forgotten_control_are_different_arms() {
        let scene = group(
            "root",
            vec![
                text("forgotten"),
                quiet("rule", Silence::decorative("a separator"), vec![]),
            ],
        );
        let census = voice_census(&scene, &tags(&["root"]), &BTreeSet::new());
        assert_eq!(row(&census, "forgotten").voice, Voice::Unvoiced);
        assert_eq!(row(&census, "rule").voice, Voice::Silent);
        assert_eq!(
            row(&census, "rule").silence.as_ref().map(Silence::relay),
            Some(Relay::Nowhere),
        );
    }

    #[test]
    fn a_decorative_region_covers_its_subtree_and_names_the_declarer() {
        let scene = group(
            "root",
            vec![quiet(
                "legend",
                Silence::decorative("colour swatches"),
                vec![text("swatch.a"), text("swatch.b")],
            )],
        );
        let census = voice_census(&scene, &tags(&["root"]), &BTreeSet::new());
        assert!(census.is_total(), "the region covered both swatches");
        let inner = row(&census, "swatch.a");
        assert_eq!(inner.voice, Voice::Silent);
        assert!(!inner.self_declared);
        assert_eq!(inner.declared_by.as_deref(), Some("legend"));
        assert_eq!(
            inner.silence.as_ref().map(Silence::kind),
            Some(SilenceKind::Decorative),
        );
    }

    /// The one arm that does not cover its subtree, and the reason it is worth
    /// having: a box that merely places things has said nothing about what it
    /// placed, so a forgotten child inside it is still found.
    #[test]
    fn a_layout_box_does_not_cover_its_children() {
        let scene = group(
            "root",
            vec![quiet(
                "body",
                Silence::layout("stacks the palette"),
                vec![text("role.a"), text("role.b")],
            )],
        );
        // `role.b` speaks, so the box's promise is kept and the box itself is a
        // plain silence — and `role.a` is still found.
        let census = voice_census(&scene, &tags(&["root", "role.b"]), &BTreeSet::new());
        assert_eq!(row(&census, "body").voice, Voice::Silent);
        assert_eq!(
            row(&census, "role.a").voice,
            Voice::Unvoiced,
            "a layout declaration is not a licence for what it holds",
        );
    }

    /// ★★★★★ R1692 — the promise a `layout` silence makes is "the children
    /// speak", and until now nothing asked whether they did. A box over a
    /// subtree where **nothing** is in the tree leaves that whole region
    /// inaudible while every per-node check passes: each descendant is
    /// correctly quiet on its own terms.
    #[test]
    fn a_layout_box_whose_subtree_says_nothing_is_hollow() {
        let scene = group(
            "root",
            vec![quiet(
                "body",
                Silence::layout("stacks the palette"),
                vec![quiet(
                    "swatch",
                    Silence::decorative("a colour chip"),
                    vec![],
                )],
            )],
        );
        let census = voice_census(&scene, &tags(&["root"]), &BTreeSet::new());
        assert_eq!(
            row(&census, "body").voice,
            Voice::Hollow,
            "nothing under the box is announced, so 'the children speak' is false",
        );
        assert_eq!(
            row(&census, "swatch").voice,
            Voice::Silent,
            "and the child is correctly quiet — which is what makes the hole \
             invisible without this arm",
        );
        assert!(!census.is_total());

        // Give one descendant a voice and the promise is kept.
        let census = voice_census(&scene, &tags(&["root", "swatch"]), &BTreeSet::new());
        assert_eq!(row(&census, "body").voice, Voice::Silent);
        assert!(census.is_total());
    }

    /// A box with no addressable children at all promises the same thing about
    /// nothing.
    #[test]
    fn a_layout_box_over_an_empty_subtree_is_hollow() {
        let scene = group(
            "root",
            vec![quiet("body", Silence::layout("stacks nothing"), vec![])],
        );
        let census = voice_census(&scene, &tags(&["root"]), &BTreeSet::new());
        assert_eq!(row(&census, "body").voice, Voice::Hollow);
    }

    /// A descendant that is in the tree keeps the promise even when what it
    /// says is unusable: the box did carry a reader somewhere, and the bad name
    /// is that node's own defect, on that node's own row. Reporting it twice
    /// would make the census's numbers wrong.
    #[test]
    fn a_mumbling_child_still_keeps_the_boxs_promise() {
        let scene = group(
            "root",
            vec![quiet(
                "body",
                Silence::layout("stacks the palette"),
                vec![text("chip")],
            )],
        );
        let mut announced = tags(&["root"]);
        announced.insert("chip".to_owned(), Announcement::named(""));
        let census = voice_census(&scene, &announced, &BTreeSet::new());
        assert_eq!(row(&census, "body").voice, Voice::Silent);
        assert_eq!(row(&census, "chip").voice, Voice::Mumbled);
        assert_eq!(census.count(Voice::Hollow), 0);
    }

    /// ★★★★★ R1692 — a node is not a voice. The floor answers `role = Button,
    /// name = ""` and calls that an accessible control; the census does not.
    #[test]
    fn a_node_with_nothing_usable_to_say_is_not_announced() {
        let scene = group("root", vec![text("save"), text("lab.toolbar.meta")]);
        let mut announced = tags(&["root"]);
        announced.insert("save".to_owned(), Announcement::named("   "));
        announced.insert(
            "lab.toolbar.meta".to_owned(),
            Announcement::named("lab.toolbar.meta"),
        );
        let census = voice_census(&scene, &announced, &BTreeSet::new());

        let save = row(&census, "save");
        assert_eq!(save.voice, Voice::Mumbled);
        assert_eq!(save.fault, Some(NameFault::Absent));
        assert_eq!(
            save.name.as_deref(),
            Some("   "),
            "the row carries what it said"
        );

        let meta = row(&census, "lab.toolbar.meta");
        assert_eq!(meta.voice, Voice::Mumbled);
        assert_eq!(meta.fault, Some(NameFault::Address));

        assert_eq!(census.count(Voice::Mumbled), 2);
        assert!(!census.is_total());
    }

    /// The fault is `Some` exactly when the arm is `Mumbled` — the arm says
    /// there is one and this says which, so a row can never claim both or
    /// neither.
    #[test]
    fn a_fault_is_carried_exactly_by_the_arm_that_means_there_is_one() {
        let scene = group("root", vec![text("good"), text("bad"), text("quiet")]);
        let mut announced = tags(&["root", "good"]);
        announced.insert("bad".to_owned(), Announcement::named("×"));
        let census = voice_census(&scene, &announced, &BTreeSet::new());
        for node in &census.nodes {
            assert_eq!(
                node.fault.is_some(),
                node.voice == Voice::Mumbled,
                "{} is {} and carries {:?}",
                node.tag,
                node.voice.name(),
                node.fault,
            );
        }
        assert_eq!(row(&census, "bad").fault, Some(NameFault::Wordless));
    }

    /// ★★★★ R1692 — a composite container is announced for a tag the scene
    /// need not paint: a legend, a transcript, a radio group. It is anchored by
    /// what it holds, which is the same exemption as being pointed at, asked
    /// from the other end. Measured when this landed: **eight of the tree's
    /// nine ghosts** were exactly this shape and none was a defect.
    #[test]
    fn a_container_is_anchored_by_the_members_that_paint() {
        let scene = group("root", vec![text("legend.0"), text("legend.1")]);
        let mut announced = tags(&["root", "legend.0", "legend.1"]);
        announced.insert(
            "legend".to_owned(),
            Announcement::named("Series")
                .composing("legend.0")
                .composing("legend.1"),
        );
        let census = voice_census(&scene, &announced, &BTreeSet::new());
        assert_eq!(census.count(Voice::Ghost), 0);
        assert!(census.is_total());

        // And the discriminating case: a node that composes nothing painted is
        // still a name a reader can be sent to and never find.
        let mut announced = tags(&["root"]);
        announced.insert("status".to_owned(), Announcement::named("19 properties"));
        let census = voice_census(&scene, &announced, &BTreeSet::new());
        assert_eq!(row(&census, "status").voice, Voice::Ghost);
    }

    /// ★★★ R1692 — the third anchor, and the one that is not about geometry at
    /// all: a live region is announced when it changes rather than reached by
    /// navigating, so a reader can never be sent somewhere and find nothing.
    #[test]
    fn a_live_region_owes_no_rectangle() {
        let scene = group("root", vec![text("a")]);
        let mut announced = tags(&["root", "a"]);
        announced.insert(
            "count".to_owned(),
            Announcement::named("19 properties").living(),
        );
        let census = voice_census(&scene, &announced, &BTreeSet::new());
        assert_eq!(census.count(Voice::Ghost), 0);
        assert!(census.is_total());
        assert!(
            census.nodes.iter().all(|n| n.tag != "count"),
            "and it is not a painted row either",
        );

        // The same node without the declaration is the defect it was.
        let mut announced = tags(&["root", "a"]);
        announced.insert("count".to_owned(), Announcement::named("19 properties"));
        let census = voice_census(&scene, &announced, &BTreeSet::new());
        assert_eq!(row(&census, "count").voice, Voice::Ghost);
    }

    /// A redirect to a node that does not speak sends a reader nowhere, and
    /// reads as handled — which is why it is its own arm and not a `Silent`.
    #[test]
    fn a_reason_naming_a_silent_node_is_dangling() {
        let scene = group(
            "root",
            vec![
                quiet("caption", Silence::name_of("button"), vec![]),
                text("button"),
            ],
        );
        let census = voice_census(&scene, &tags(&["root"]), &BTreeSet::new());
        assert_eq!(row(&census, "caption").voice, Voice::Dangling);
        assert!(!census.is_total());

        // Give the button a voice and the redirect becomes true.
        let census = voice_census(&scene, &tags(&["root", "button"]), &BTreeSet::new());
        assert_eq!(row(&census, "caption").voice, Voice::Silent);
        assert_eq!(row(&census, "button").voice, Voice::Announced);
        assert!(census.is_total());
    }

    #[test]
    fn an_announced_tag_nothing_paints_is_a_ghost_unless_referred_to() {
        let scene = group("root", vec![text("a")]);
        let census = voice_census(&scene, &tags(&["root", "a", "said.a"]), &BTreeSet::new());
        assert_eq!(row(&census, "said.a").voice, Voice::Ghost);

        // A description region another node points at is deliberately virtual.
        let census = voice_census(&scene, &tags(&["root", "a", "said.a"]), &refs(&["said.a"]));
        assert!(
            census.nodes.iter().all(|n| n.voice != Voice::Ghost),
            "a referred-to node is not a ghost",
        );
        assert_eq!(census.nodes.len(), 2, "and it is not a painted row either");
    }

    #[test]
    fn an_untagged_node_is_not_a_row() {
        let scene = Scene::Container(ContainerNode::new(vec![Scene::Text(TextNode::new(
            "loose",
            Rect::default(),
        ))]));
        let census = voice_census(&scene, &BTreeMap::new(), &BTreeSet::new());
        assert!(
            census.nodes.is_empty(),
            "nothing addressable, nothing to say"
        );
        assert!(census.is_total());
    }

    #[test]
    fn a_node_declaring_its_own_silence_inside_a_silent_region_reports_both() {
        let scene = quiet(
            "card",
            Silence::part_of("card"),
            vec![quiet("badge", Silence::decorative("a role chip"), vec![])],
        );
        let census = voice_census(&scene, &tags(&["card"]), &BTreeSet::new());
        let badge = row(&census, "badge");
        assert!(badge.self_declared);
        assert_eq!(badge.declared_by.as_deref(), Some("card"));
        assert_eq!(
            badge.silence.as_ref().map(Silence::kind),
            Some(SilenceKind::Decorative),
            "its own reason wins over the region's",
        );
    }

    #[test]
    fn the_scroll_content_is_walked() {
        use crate::scene::ScrollNode;
        let scene = Scene::Scroll(ScrollNode::new(Rect::default(), text("inner")).with_tag("body"));
        let census = voice_census(&scene, &tags(&["body"]), &BTreeSet::new());
        assert_eq!(row(&census, "inner").voice, Voice::Unvoiced);
    }

    #[test]
    fn every_published_vocabulary_is_a_readable_one() {
        for kind in SilenceKind::ALL {
            assert_eq!(SilenceKind::from_name(kind.name()), Some(kind));
        }
        for relay in Relay::ALL {
            assert_eq!(Relay::from_name(relay.name()), Some(relay));
        }
        for voice in Voice::ALL {
            assert_eq!(Voice::from_name(voice.name()), Some(voice));
        }
        for fault in NameFault::ALL {
            assert_eq!(NameFault::from_name(fault.name()), Some(fault));
        }
        assert_eq!(SilenceKind::from_name("nonsense"), None);
    }

    /// ★★★★★ R1692 — every promise a silence makes has an arm that catches it
    /// broken, and the one arm that promises nothing has none. A fifth
    /// [`SilenceKind`] relaying somewhere new has to answer this.
    #[test]
    fn every_promise_a_silence_makes_has_an_arm_for_it_being_false() {
        for kind in SilenceKind::ALL {
            let relay = kind.relay();
            assert_eq!(
                relay.unkept().is_defect(),
                relay.is_reachable(),
                "{}: a relay that sends a reader somewhere can send them nowhere",
                kind.name(),
            );
        }
        assert_eq!(Relay::Nowhere.unkept(), Voice::Silent);
    }

    /// The three rules, each on the shape it exists for, and each on the shape
    /// it must NOT fire on — the second half is what keeps a gate usable.
    #[test]
    fn the_name_rules_judge_mechanics_and_not_english() {
        assert_eq!(NameFault::judge("t", "", true), Some(NameFault::Absent));
        assert_eq!(NameFault::judge("t", " \t ", true), Some(NameFault::Absent));
        assert_eq!(
            NameFault::judge("lab.toolbar.meta", "lab.toolbar.meta", true),
            Some(NameFault::Address),
        );
        assert_eq!(
            NameFault::judge("chart-tab", "chart-tab", true),
            Some(NameFault::Address),
            "a hyphenated identifier is an identifier",
        );
        for glyph in ["×", "…", "☐", "⠿", "—", "+"] {
            assert_eq!(
                NameFault::judge("t", glyph, true),
                Some(NameFault::Wordless),
                "{glyph} is not something a reader is told",
            );
        }

        // Not faults. A name that matches the visible label is CORRECT, and the
        // tags below are words their screens paint.
        assert_eq!(NameFault::judge("notch", "notch", true), None);
        assert_eq!(NameFault::judge("log axis", "log axis", true), None);
        assert_eq!(NameFault::judge("lab.toolbar.run", "Run", true), None);
        assert_eq!(
            NameFault::judge("page#3", "3", true),
            None,
            "a digit is a word",
        );
        assert_eq!(
            NameFault::judge("t", "닫기", true),
            None,
            "so is a non-Latin one",
        );
        assert_eq!(
            NameFault::judge("t", "× close", true),
            None,
            "a glyph beside a word is a word",
        );
    }

    /// ★★★ R1692 — the one rule a role can excuse, and the two it cannot. A
    /// blank table cell is what the data says; an address or a symbol is a wrong
    /// answer wherever it is given.
    #[test]
    fn only_the_empty_rule_can_be_excused_by_the_role() {
        assert_eq!(NameFault::judge("grid#0_3", "", false), None);
        assert_eq!(
            NameFault::judge("grid#0_3", "grid#0_3", false),
            Some(NameFault::Address),
        );
        assert_eq!(
            NameFault::judge("grid#0_3", "×", false),
            Some(NameFault::Wordless)
        );
    }

    /// And the exemption has to be asked for. A default that excused an empty
    /// name would switch the rule off by omission, which is how the thing this
    /// module measures happened in the first place.
    #[test]
    fn a_name_is_required_unless_something_says_otherwise() {
        assert!(Announcement::default().name_required);
        assert!(Announcement::named("Save").name_required);
        assert!(!Announcement::named("").optionally_named().name_required);

        let scene = group("root", vec![text("blank"), text("forgotten")]);
        let mut announced = tags(&["root"]);
        announced.insert(
            "blank".to_owned(),
            Announcement::named("").optionally_named(),
        );
        announced.insert("forgotten".to_owned(), Announcement::named(""));
        let census = voice_census(&scene, &announced, &BTreeSet::new());
        assert_eq!(row(&census, "blank").voice, Voice::Announced);
        assert_eq!(row(&census, "forgotten").voice, Voice::Mumbled);
    }

    /// Each arm leaves a reader somewhere different — the justification for the
    /// vocabulary being four and not one bool.
    #[test]
    fn no_two_kinds_share_a_relay() {
        let mut seen = BTreeSet::new();
        for kind in SilenceKind::ALL {
            assert!(
                seen.insert(kind.relay()),
                "{} shares a relay with an earlier arm",
                kind.name(),
            );
        }
        assert_eq!(seen.len(), SilenceKind::ALL.len());
    }

    #[test]
    fn only_ornament_is_unreachable() {
        for kind in SilenceKind::ALL {
            assert_eq!(
                kind.relay().is_reachable(),
                kind != SilenceKind::Decorative,
                "{}",
                kind.name(),
            );
        }
    }

    #[test]
    fn a_tag_is_checkable_exactly_for_the_two_arms_that_redirect() {
        assert_eq!(Silence::decorative("x").relay_target(), None);
        assert_eq!(Silence::layout("x").relay_target(), None);
        assert_eq!(Silence::name_of("t").relay_target(), Some("t"));
        assert_eq!(Silence::part_of("t").relay_target(), Some("t"));
    }
}
