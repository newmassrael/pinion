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
//! a [`Voice`]. Three of its five arms are defects, and each is a *different*
//! defect with a different fix — which is the whole reason the census is not a
//! bool.
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
use std::collections::BTreeSet;

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
/// Five arms, of which [`Announced`](Self::Announced) and [`Silent`](Self::Silent)
/// are the two correct outcomes and the other three are three *different*
/// defects. A census that only counted would merge them, and the fix for each is
/// different: give it a name, decide why it is quiet, delete the node, or point
/// the reason somewhere real.
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
        }
    }

    /// Parse a wire spelling back.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|v| v.name() == name)
    }

    /// Every arm, in declaration order.
    pub const ALL: [Voice; 5] = [
        Voice::Announced,
        Voice::Silent,
        Voice::Unvoiced,
        Voice::Ghost,
        Voice::Dangling,
    ];

    /// Whether this arm is a defect — three of the five are.
    #[must_use]
    pub const fn is_defect(self) -> bool {
        matches!(self, Voice::Unvoiced | Voice::Ghost | Voice::Dangling)
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

/// One addressable region, and what the accessibility tree says about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceNode {
    /// The tag — the same spelling `scene/click`, `scene/invoke` and
    /// `scene/access` address, so a reader of this census can act on a row.
    pub tag: String,
    /// What the tree says.
    pub voice: Voice,
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
/// `announced` is every tag the tree has a node for. `referenced` is every tag
/// some node points at — a description region, a composite child, a bounds
/// contributor, a name source — which is what separates a deliberately virtual
/// node from a [`Ghost`](Voice::Ghost).
///
/// Untagged nodes are not rows. They cannot be addressed, so a row for one
/// would name nothing — the same rule
/// [`disabled_census`](crate::scene_disabled::disabled_census) applies, for the
/// same reason.
#[must_use]
pub fn voice_census(
    scene: &Scene,
    announced: &BTreeSet<String>,
    referenced: &BTreeSet<String>,
) -> VoiceCensus {
    let mut nodes = Vec::new();
    let mut painted = BTreeSet::new();
    walk(scene, None, None, announced, &mut painted, &mut nodes);
    // The other direction. A name with no region behind it is only reachable
    // through whoever refers to it; with nobody referring, a reader can be sent
    // to it and find nothing.
    for tag in announced.difference(&painted) {
        if referenced.contains(tag) {
            continue;
        }
        nodes.push(VoiceNode {
            tag: tag.clone(),
            voice: Voice::Ghost,
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
fn walk(
    scene: &Scene,
    declared_by: Option<&str>,
    inherited: Option<&Silence>,
    announced: &BTreeSet<String>,
    painted: &mut BTreeSet<String>,
    out: &mut Vec<VoiceNode>,
) {
    let own = scene
        .layout_style()
        .and_then(|layout| layout.silence.as_ref());
    let self_declared = own.is_some();
    // The node's own reason when it has one, otherwise the region's — the same
    // precedence the disabled cascade applies.
    let reason = own.or(inherited);
    if let Some(tag) = scene.tag() {
        painted.insert(tag.to_owned());
        let voice = classify(tag, reason, announced);
        out.push(VoiceNode {
            tag: tag.to_owned(),
            voice,
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
                walk(child, child_declarer, child_reason, announced, painted, out);
            }
        }
        Scene::Scroll(s) => walk(
            &s.content,
            child_declarer,
            child_reason,
            announced,
            painted,
            out,
        ),
        Scene::Box(_)
        | Scene::Text(_)
        | Scene::Path(_)
        | Scene::Image(_)
        | Scene::External(_)
        | Scene::Effect(_)
        | Scene::ImmediateModeNode(_)
        | Scene::TextGrid(_) => {}
    }
}

/// The arm for one painted tag.
///
/// A node that both speaks and declares a reason is [`Announced`](Voice::Announced):
/// the tree is what a reader actually receives, and a stale declaration beside a
/// real voice is a documentation problem rather than an accessibility one. The
/// census still carries the declaration on the row, so it can be found.
fn classify(tag: &str, reason: Option<&Silence>, announced: &BTreeSet<String>) -> Voice {
    if announced.contains(tag) {
        return Voice::Announced;
    }
    let Some(reason) = reason else {
        return Voice::Unvoiced;
    };
    match reason.relay_target() {
        Some(target) if !announced.contains(target) => Voice::Dangling,
        _ => Voice::Silent,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{ContainerNode, Rect, TextNode};
    use crate::style::LayoutStyle;

    fn tags(names: &[&str]) -> BTreeSet<String> {
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
                vec![text("role.a")],
            )],
        );
        let census = voice_census(&scene, &tags(&["root"]), &BTreeSet::new());
        assert_eq!(row(&census, "body").voice, Voice::Silent);
        assert_eq!(
            row(&census, "role.a").voice,
            Voice::Unvoiced,
            "a layout declaration is not a licence for what it holds",
        );
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
        let census = voice_census(&scene, &tags(&["root", "a", "said.a"]), &tags(&["said.a"]));
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
        let census = voice_census(&scene, &BTreeSet::new(), &BTreeSet::new());
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
        assert_eq!(SilenceKind::from_name("nonsense"), None);
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
