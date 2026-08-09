//! R1615 §5.36 §2 #7 — **why a painted thing looks the way it does**, as data.
//!
//! Every framework that draws styled content keeps a list of ranges: "these
//! bytes take this format". The list decides the picture, and then the picture
//! is all that is left — the run that produced it has no identity apart from
//! its ink. Ask "why is this word blue" and the honest answer a toolkit can
//! give is "because something set it blue".
//!
//! That is not a hypothetical loss. A syntax highlighter classifies a token as
//! a keyword, a string, a comment, or a number, and then **discards the
//! classification** and keeps a colour; the same class paints two different
//! colours on a light and a dark scheme, so the colour is not even a stable
//! name for it. A hex dump lights a byte because it is inside the length field
//! *and* inside the header *and* inside the frame, and one background colour
//! survives all three.
//!
//! A [`Mark`] is that missing half: a **named** run over a content index
//! space. A [`MarkSet`] is the runs of one node, in declaration order, and it
//! answers [`names_at`](MarkSet::names_at) — the whole stack covering a
//! position, innermost last.
//!
//! ## Past the reference
//!
//! A mature toolkit's text layout carries range decorations as
//! `(start, length, format)`, and a text block hands back the whole list to be
//! re-scanned by whoever wants to know. Three things are absent there and
//! present here:
//!
//! * **A name.** The reference's run *is* its format, so two runs that resolve
//!   to the same ink are indistinguishable once drawn. A name can be attached
//!   — the format type carries an open, integer-keyed user-property space —
//!   but nothing in the framework declares one, so no reader can expect it and
//!   no surface reports it.
//! * **A positional query.** The list is handed over whole; "which runs cover
//!   offset N" is the caller's loop, written again per caller.
//! * **One index space, spelled.** The reference's runs are always over
//!   characters. A dump's runs are over *bytes of the inspected buffer*, which
//!   is not the same as the cells they light, so a set states its
//!   [`domain`](MarkSet::domain) and a reader is never left to guess what the
//!   index counts.
//!
//! ## Overlap resolves in ONE direction, and the direction is queryable
//!
//! Marks overlap constantly — a packet's length field is inside its header is
//! inside the frame — so a byte can carry several, and something has to decide
//! which one a painter obeys.
//!
//! **Declaration order, later wins, for every visual channel alike.** A caller
//! that declares the frame, then the header, then the field gets the field on
//! top, which is the order it wrote them in. The reference splits this *by
//! channel*: a later range's background overpaints an earlier one, while a
//! later range's foreground is suppressed wherever an earlier one already drew
//! text — background last-wins and foreground first-wins **in the same loop**.
//! Two directions in one list is a rule nobody can hold, and it is not written
//! down anywhere in that interface. One direction, stated, is the choice here.

use std::borrow::Cow;
use std::fmt;

use crate::term_grid::GridBuffer;

/// The index spaces the framework's own marked nodes count in.
///
/// A domain is a plain string so an application can name its own (a sample
/// index, a row, a timestamp bucket) without this module knowing about it.
/// These are the ones the framework itself publishes, named once so a client
/// matches a constant instead of a spelling.
pub mod domain {
    /// Offsets into the buffer a [`TextGrid`](crate::scene::Scene::TextGrid)
    /// dump displays — **not** the grid's cells. One byte occupies three
    /// cells in a hex dump (two digits and one glyph), so the two counts
    /// differ and the distinction is the reason a domain is stated at all.
    pub const BYTE: &str = "byte";

    /// UTF-8 byte offsets into a [`Text`](crate::scene::Scene::Text) node's
    /// content — the same units [`StyleRun`](crate::scene::StyleRun) indexes,
    /// so a named run and the mark derived from it address identically.
    pub const UTF8_BYTE: &str = "utf8_byte";

    /// R1618 — **the node itself**, of which there is exactly one, so the only
    /// index is `0`.
    ///
    /// A node that paints from one declaration has no interior to attribute:
    /// its
    /// [`MarksChannel`](crate::marks::MarksChannel::Uniform) doc puts it as
    /// "the node itself *is* the run". That is not the same as having nothing
    /// to say. A grid row is a filled rectangle, and *why* it is that colour —
    /// selected, and hovered, and inside a group the user collapsed — is a
    /// composition of facts living in different externals, which is exactly the
    /// question no single oracle can answer.
    ///
    /// Stating it as a domain rather than as a separate list is deliberate: the
    /// declaration order, the last-wins overlap rule, the positional query and
    /// the wire shape are then **one** mechanism rather than two that could
    /// disagree. A caller uses [`MarkSet::whole`](super::MarkSet::whole) and never
    /// writes an index.
    pub const NODE: &str = "node";
}

/// A named run of positions, `[start, end)`.
///
/// The thing an inspector actually has: not "these bytes are blue" but "these
/// bytes are the length field". The name is the point — it is what a caller
/// keys colour off, what an assistant reads back over the wire, and what
/// [`MarkSet::names_at`] answers with when someone asks *why* a position is
/// lit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mark {
    name: String,
    start: usize,
    end: usize,
}

impl Mark {
    /// A mark called `name` over `[start, end)`. An inverted or empty range is
    /// stored as it was ordered, then reports [`Mark::is_empty`].
    #[must_use]
    pub fn new(name: impl Into<String>, start: usize, end: usize) -> Self {
        let (start, end) = if start <= end {
            (start, end)
        } else {
            (end, start)
        };
        Self {
            name: name.into(),
            start,
            end,
        }
    }

    /// What this run is called.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// First marked position.
    #[must_use]
    pub const fn start(&self) -> usize {
        self.start
    }

    /// One past the last marked position.
    #[must_use]
    pub const fn end(&self) -> usize {
        self.end
    }

    /// How many positions the run covers.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.end - self.start
    }

    /// Whether the run covers nothing.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.start >= self.end
    }

    /// Whether `index` is in the run.
    #[must_use]
    pub const fn contains(&self, index: usize) -> bool {
        self.start <= index && index < self.end
    }
}

/// The marks over one node's content, in declaration order, plus the
/// [`domain`](Self::domain) their indices count in.
///
/// See the module header for the overlap rule and for what the reference
/// interface this replaces does not have.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkSet {
    domain: Cow<'static, str>,
    marks: Vec<Mark>,
}

impl MarkSet {
    /// An empty set whose indices count in `domain` — one of the
    /// [`domain`] constants, or an application's own name for
    /// its index space.
    ///
    /// There is no domain-less constructor. A set of runs whose index space is
    /// unstated is a set no reader can use without guessing, and the guess
    /// fails quietly: a hex dump's marks are over bytes while the cells that
    /// show them are three times as many, so a client that assumed cells would
    /// read a plausible wrong answer rather than an error.
    #[must_use]
    pub fn over(domain: impl Into<Cow<'static, str>>) -> Self {
        Self {
            domain: domain.into(),
            marks: Vec::new(),
        }
    }

    /// The set of `marks` over `domain`, in the order the iterator yields
    /// them — which is the order overlap resolves in.
    #[must_use]
    pub fn from_marks(
        domain: impl Into<Cow<'static, str>>,
        marks: impl IntoIterator<Item = Mark>,
    ) -> Self {
        Self {
            domain: domain.into(),
            marks: marks.into_iter().collect(),
        }
    }

    /// What these runs' indices count.
    #[must_use]
    pub fn domain(&self) -> &str {
        &self.domain
    }

    /// The same set with `mark` declared after everything already in it — so
    /// it wins wherever it overlaps.
    #[must_use]
    pub fn with(mut self, mark: Mark) -> Self {
        self.marks.push(mark);
        self
    }

    /// The same set with a mark called `name` over `[start, end)` declared
    /// last. The builder for the common case.
    #[must_use]
    pub fn marking(self, name: impl Into<String>, start: usize, end: usize) -> Self {
        self.with(Mark::new(name, start, end))
    }

    /// R1618 — an empty set over [`domain::NODE`]: the index space of a node
    /// that paints from one declaration, where the only position is the node.
    ///
    /// Build it up with [`because`](Self::because) and never write an index.
    #[must_use]
    pub fn whole() -> Self {
        Self::over(domain::NODE)
    }

    /// R1618 — declare that this node looks the way it does because of `name`,
    /// after everything already declared.
    ///
    /// The [`domain::NODE`] spelling of [`marking`](Self::marking), and the
    /// only builder a uniform node's caller should need: a run over a
    /// one-position space is `[0, 1)` and writing that out at each call site
    /// would be three chances to write `[0, 0)` — an empty run, which covers
    /// nothing and would silently publish a reason that answers no question.
    ///
    /// Overlap resolves exactly as it does everywhere else: everything
    /// declared covers the one position, in declaration order, and the last one
    /// is what a painter obeyed. So [`names_at(0)`](Self::names_at) is the
    /// whole stack of reasons, innermost last.
    #[must_use]
    pub fn because(self, name: impl Into<String>) -> Self {
        self.marking(name, 0, 1)
    }

    /// R1618 — the reasons this node looks the way it does, in declaration
    /// order, for a set over [`domain::NODE`].
    ///
    /// `names_at(0)` spelled once, so a caller of a uniform node's marks never
    /// has to know that the index it is not allowed to choose is `0`.
    /// Answers empty for a set over any other domain, which is the honest
    /// reading: those indices count something else, and position `0` of that
    /// something is not "the node".
    #[must_use]
    pub fn reasons(&self) -> Vec<&str> {
        if self.domain() == domain::NODE {
            self.names_at(0)
        } else {
            Vec::new()
        }
    }

    /// Declare `mark` last, in place.
    pub fn push(&mut self, mark: Mark) {
        self.marks.push(mark);
    }

    /// The marks, in declaration order.
    pub fn iter(&self) -> impl Iterator<Item = &Mark> {
        self.marks.iter()
    }

    /// How many marks are declared.
    #[must_use]
    pub fn len(&self) -> usize {
        self.marks.len()
    }

    /// Whether nothing is marked.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.marks.is_empty()
    }

    /// The mark by that name, if one is declared. The last, if a name is
    /// declared twice — the same rule overlap follows.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Mark> {
        self.marks.iter().rev().find(|mark| mark.name == name)
    }

    /// Every mark covering `index`, in declaration order.
    ///
    /// **Why a position looks the way it does.** A painter obeys the last of
    /// these; a reader — a person, or an assistant over the wire — gets the
    /// whole stack, so "the length field, inside the header, inside the frame"
    /// is answerable rather than inferred from a colour.
    #[must_use]
    pub fn at(&self, index: usize) -> impl DoubleEndedIterator<Item = &Mark> {
        self.marks.iter().filter(move |mark| mark.contains(index))
    }

    /// The names covering `index`, in declaration order.
    #[must_use]
    pub fn names_at(&self, index: usize) -> Vec<&str> {
        self.at(index).map(Mark::name).collect()
    }

    /// The mark a painter obeys at `index` — the last declared one covering
    /// it.
    #[must_use]
    pub fn top_at(&self, index: usize) -> Option<&Mark> {
        self.at(index).next_back()
    }
}

/// R1615 §2 #7 — whether a [`Scene`](crate::scene::Scene) node kind can name
/// the declarations that produced its appearance.
///
/// Every kind answers, because the answer is an exhaustive match on
/// [`SceneNodeKind`](crate::scene::SceneNodeKind)
/// ([`marks_channel`](crate::scene::SceneNodeKind::marks_channel)): a node kind
/// added later cannot quietly default to "no marks", it has to say which of
/// these it is. That is the difference between a channel that is absent and a
/// channel that was forgotten, and only one of them is a decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MarksChannel {
    /// The kind paints content made of many parts, and can name the runs that
    /// decided how each part looks.
    Carries,
    /// The kind paints from exactly one declaration, so there is nothing to
    /// attribute: the node itself *is* the run.
    Uniform,
    /// The kind paints nothing of its own — attribution belongs to its
    /// children.
    Structural,
    /// A §3 escape hatch. What it draws is opaque to the framework, so the
    /// framework cannot say why any of it looks the way it does; only the
    /// escape's own introspection can.
    Opaque,
}

impl MarksChannel {
    /// The wire spelling, and the word a client matches on.
    #[must_use]
    pub const fn wire_name(self) -> &'static str {
        match self {
            Self::Carries => "carries",
            Self::Uniform => "uniform",
            Self::Structural => "structural",
            Self::Opaque => "opaque",
        }
    }
}

impl fmt::Display for MarksChannel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.wire_name())
    }
}

/// One named run a node published, borrowed from whatever the node stores it
/// in.
///
/// A [`Scene::Text`](crate::scene::Scene::Text) node keeps its runs as
/// [`StyleRun`](crate::scene::StyleRun)s that carry both a name and a resolved
/// style; a [`Scene::TextGrid`](crate::scene::Scene::TextGrid) keeps a
/// [`MarkSet`]. Both project to this, so one question has one answer shape
/// regardless of which kind was asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarkRun<'a> {
    /// What the run is called.
    pub name: &'a str,
    /// First covered index, in the owning set's domain.
    pub start: usize,
    /// One past the last covered index.
    pub end: usize,
}

impl MarkRun<'_> {
    /// Whether `index` is in this run.
    #[must_use]
    pub const fn contains(&self, index: usize) -> bool {
        self.start <= index && index < self.end
    }
}

/// The named runs one node published, with the index space they count in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkedRuns<'a> {
    domain: &'a str,
    runs: Vec<MarkRun<'a>>,
}

impl<'a> MarkedRuns<'a> {
    /// Assemble from a domain and the runs in declaration order.
    #[must_use]
    pub const fn new(domain: &'a str, runs: Vec<MarkRun<'a>>) -> Self {
        Self { domain, runs }
    }

    /// What the run indices count.
    #[must_use]
    pub const fn domain(&self) -> &'a str {
        self.domain
    }

    /// The runs, in declaration order.
    #[must_use]
    pub fn runs(&self) -> &[MarkRun<'a>] {
        &self.runs
    }

    /// Whether the node published no runs at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.runs.is_empty()
    }

    /// The names covering `index`, in declaration order — the whole stack,
    /// innermost last.
    #[must_use]
    pub fn names_at(&self, index: usize) -> Vec<&'a str> {
        self.runs
            .iter()
            .filter(|run| run.contains(index))
            .map(|run| run.name)
            .collect()
    }

    /// The name a painter obeys at `index` — the last declared run covering
    /// it.
    #[must_use]
    pub fn top_at(&self, index: usize) -> Option<&'a str> {
        self.runs
            .iter()
            .filter(|run| run.contains(index))
            .next_back()
            .map(|run| run.name)
    }
}

impl<'a> From<&'a MarkSet> for MarkedRuns<'a> {
    fn from(set: &'a MarkSet) -> Self {
        Self::new(
            set.domain(),
            set.iter()
                .map(|mark| MarkRun {
                    name: mark.name(),
                    start: mark.start(),
                    end: mark.end(),
                })
                .collect(),
        )
    }
}

/// R1615 — a cell projection **and** the named runs that decided how it looks,
/// produced together.
///
/// The pair exists as a type so the two cannot arrive from different places. A
/// grid's cells carry resolved colours; the marks carry the reasons. Set them
/// separately and nothing stops a node from publishing last frame's reasons
/// beside this frame's picture — the picture would be right, the explanation
/// wrong, and no test that checks either one alone would see it.
///
/// A painter that folds marks into cells returns this; a
/// [`TextGridNode`](crate::scene::TextGridNode) takes it whole through
/// [`with_marked_grid`](crate::scene::TextGridNode::with_marked_grid).
#[derive(Debug, Clone, PartialEq)]
pub struct MarkedGrid {
    cells: GridBuffer,
    marks: MarkSet,
}

impl MarkedGrid {
    /// Pair `cells` with the `marks` that produced them.
    ///
    /// The honest limit: this cannot *verify* that it did. What it removes is
    /// the accident — a call site that updates one and forgets the other — by
    /// making the pair the unit that travels.
    #[must_use]
    pub const fn new(cells: GridBuffer, marks: MarkSet) -> Self {
        Self { cells, marks }
    }

    /// The cells.
    #[must_use]
    pub const fn cells(&self) -> &GridBuffer {
        &self.cells
    }

    /// The named runs that decided them.
    #[must_use]
    pub const fn marks(&self) -> &MarkSet {
        &self.marks
    }

    /// The same pair with `f` applied to the cells — for the overlays a
    /// producer adds after folding (a cursor, a damage stamp) that do not
    /// change *why* anything is coloured.
    #[must_use]
    pub fn map_cells(mut self, f: impl FnOnce(GridBuffer) -> GridBuffer) -> Self {
        self.cells = f(self.cells);
        self
    }

    /// Split into the two halves, for a consumer that genuinely wants them
    /// apart.
    #[must_use]
    pub fn into_parts(self) -> (GridBuffer, MarkSet) {
        (self.cells, self.marks)
    }
}

/// What a tagged node answers when asked why it painted the way it did.
///
/// Four outcomes, and the three that are not an answer are each a different
/// fact. Collapsing them — the shape this replaced, where a caller supplied
/// half the question and got a bare list back — is what makes a client unable
/// to tell "there is no such node" from "that node declared nothing" from
/// "that kind of node has nothing to declare".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarksLookup<'a> {
    /// The node carries a marks channel and declared runs on it.
    Published(MarkedRuns<'a>),
    /// The node carries a marks channel and declared nothing on it this frame.
    /// A real node, honestly silent.
    Silent,
    /// The node's kind has no marks channel, and this is why.
    NoChannel(MarksChannel),
    /// No node in the scene carries that tag.
    NoSuchTag,
}

impl<'a> MarksLookup<'a> {
    /// The published runs, or `None` for every other outcome. For callers that
    /// only want the answer and treat the three absences alike.
    #[must_use]
    pub const fn published(&self) -> Option<&MarkedRuns<'a>> {
        match self {
            Self::Published(runs) => Some(runs),
            _ => None,
        }
    }

    /// The names covering `index`, or an empty vec when nothing was published.
    #[must_use]
    pub fn names_at(&self, index: usize) -> Vec<&'a str> {
        self.published()
            .map_or_else(Vec::new, |runs| runs.names_at(index))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlapping_marks_resolve_in_declaration_order_last_wins() {
        // One direction, unlike the split the reference interface has.
        let marks = MarkSet::over(domain::BYTE)
            .marking("frame", 0, 64)
            .marking("header", 0, 16)
            .marking("length", 4, 8);
        assert_eq!(marks.len(), 3);
        assert_eq!(marks.domain(), "byte");
        assert_eq!(marks.names_at(5), vec!["frame", "header", "length"]);
        assert_eq!(marks.top_at(5).map(Mark::name), Some("length"));
        assert_eq!(marks.names_at(12), vec!["frame", "header"]);
        assert_eq!(marks.top_at(12).map(Mark::name), Some("header"));
        assert_eq!(marks.names_at(40), vec!["frame"]);
        assert_eq!(marks.top_at(40).map(Mark::name), Some("frame"));
        assert!(marks.names_at(100).is_empty());
        assert_eq!(marks.top_at(100), None);
    }

    #[test]
    fn a_position_says_why_it_is_lit() {
        // The query the reference's range-decoration list has no form of: the
        // whole stack covering a position, in the order that decided it. A
        // colour cannot answer this -- two marks that resolve to the same ink
        // are indistinguishable once drawn.
        let marks = MarkSet::over(domain::BYTE)
            .marking("frame", 0, 64)
            .marking("header", 0, 16)
            .marking("length", 4, 8);
        let why = marks.names_at(6);
        assert_eq!(why, vec!["frame", "header", "length"]);
        assert_eq!(
            why.last().copied(),
            Some("length"),
            "the last of the stack is the one paint obeys"
        );

        // Model of the split-direction rule: background takes the LAST range
        // covering a position, foreground the FIRST. Two marks, two answers,
        // and no way to ask which run a position belongs to.
        let ranges = [("frame", 0usize, 64usize), ("length", 4, 8)];
        let covering = |index: usize| -> Vec<&str> {
            ranges
                .iter()
                .filter(|(_, s, e)| (*s..*e).contains(&index))
                .map(|(n, _, _)| *n)
                .collect()
        };
        let split = covering(6);
        assert_eq!(split.first().copied(), Some("frame"), "foreground: first");
        assert_eq!(split.last().copied(), Some("length"), "background: last");
        assert_ne!(
            split.first(),
            split.last(),
            "the two channels of one position come from DIFFERENT runs -- \
             which is the rule this module refuses"
        );
        let ours = MarkSet::over(domain::BYTE)
            .marking("frame", 0, 64)
            .marking("length", 4, 8);
        assert_eq!(
            ours.top_at(6).map(Mark::name),
            Some("length"),
            "here both channels come from the same run, and it is nameable"
        );
    }

    #[test]
    fn two_runs_that_paint_alike_are_still_two_runs() {
        // ★ The reference's range decoration IS its format, so this is the
        // question it structurally cannot answer: two runs that resolve to the
        // same ink are one indistinguishable smear once drawn. Model it, then
        // show the difference.
        //
        // A protocol inspector meets this immediately -- a checksum field and
        // a reserved field can be tinted the same and are not the same field.
        #[derive(PartialEq, Debug)]
        struct FormatRange {
            start: usize,
            length: usize,
            format: u32, // stands in for a resolved character format
        }
        let reference = [
            FormatRange {
                start: 0,
                length: 4,
                format: 0xAA,
            },
            FormatRange {
                start: 8,
                length: 4,
                format: 0xAA,
            },
        ];
        // Everything a reader can recover from the reference at a position is
        // the format, and both positions answer identically.
        let format_at = |index: usize| {
            reference
                .iter()
                .find(|r| (r.start..r.start + r.length).contains(&index))
                .map(|r| r.format)
        };
        assert_eq!(format_at(1), format_at(9));
        assert_eq!(format_at(1), Some(0xAA), "the ink is all there is");

        let ours = MarkSet::over(domain::BYTE)
            .marking("checksum", 0, 4)
            .marking("reserved", 8, 12);
        assert_ne!(
            ours.names_at(1),
            ours.names_at(9),
            "same ink, different runs, and the difference is recoverable"
        );
        assert_eq!(ours.names_at(1), vec!["checksum"]);
        assert_eq!(ours.names_at(9), vec!["reserved"]);
    }

    #[test]
    fn a_mark_is_a_run_and_says_so() {
        let mark = Mark::new("payload", 8, 24);
        assert_eq!(mark.name(), "payload");
        assert_eq!((mark.start(), mark.end(), mark.len()), (8, 24, 16));
        assert!(mark.contains(8));
        assert!(mark.contains(23));
        assert!(!mark.contains(24));
        assert!(!mark.is_empty());
        // An inverted range is ordered rather than rejected -- a drag runs
        // either way and the run is the same run.
        assert_eq!(Mark::new("x", 9, 3), Mark::new("x", 3, 9));
        assert!(Mark::new("empty", 5, 5).is_empty());
    }

    #[test]
    fn a_mark_set_is_queryable_by_name_and_states_its_domain() {
        let marks = MarkSet::from_marks(
            domain::UTF8_BYTE,
            [Mark::new("a", 0, 4), Mark::new("b", 4, 8)],
        );
        assert_eq!(marks.domain(), "utf8_byte");
        assert_eq!(marks.get("b").map(Mark::start), Some(4));
        assert_eq!(marks.get("missing"), None);
        assert!(MarkSet::over(domain::BYTE).is_empty());
        // A name declared twice resolves the way overlap does: the last one.
        let shadowed = MarkSet::over(domain::BYTE)
            .marking("f", 0, 4)
            .marking("f", 8, 12);
        assert_eq!(shadowed.get("f").map(Mark::start), Some(8));
        assert_eq!(shadowed.iter().count(), 2);
    }

    #[test]
    fn an_application_names_its_own_domain() {
        // The framework's two constants are not a closed set: a domain is a
        // string precisely so a consumer whose content is indexed by something
        // else can say so rather than misreport one of ours.
        let marks = MarkSet::over("sample").marking("burst", 1_000, 2_000);
        assert_eq!(marks.domain(), "sample");
        assert_eq!(marks.names_at(1_500), vec!["burst"]);
    }

    #[test]
    fn marked_runs_project_a_mark_set_without_changing_the_answers() {
        let marks = MarkSet::over(domain::BYTE)
            .marking("frame", 0, 64)
            .marking("length", 4, 8);
        let runs = MarkedRuns::from(&marks);
        assert_eq!(runs.domain(), marks.domain());
        assert_eq!(runs.runs().len(), 2);
        for index in 0..70 {
            assert_eq!(
                runs.names_at(index),
                marks.names_at(index),
                "projection changed the stack at {index}"
            );
            assert_eq!(runs.top_at(index), marks.top_at(index).map(Mark::name));
        }
    }

    #[test]
    fn the_three_absences_are_three_different_facts() {
        let silent: MarksLookup<'_> = MarksLookup::Silent;
        let no_channel = MarksLookup::NoChannel(MarksChannel::Uniform);
        let no_tag: MarksLookup<'_> = MarksLookup::NoSuchTag;
        assert_ne!(silent, no_channel);
        assert_ne!(no_channel, no_tag);
        assert_ne!(silent, no_tag);
        // ...and none of them is an empty answer masquerading as one.
        assert!(silent.published().is_none());
        assert!(no_channel.published().is_none());
        assert!(no_tag.published().is_none());
        assert!(silent.names_at(0).is_empty());

        // An empty PUBLISHED set is a fourth thing again: the node carries the
        // channel, declared it this frame, and it happens to hold no runs.
        let empty = MarkSet::over(domain::BYTE);
        let published = MarksLookup::Published(MarkedRuns::from(&empty));
        assert!(published.published().is_some());
        assert_ne!(published, silent);
    }

    #[test]
    fn every_channel_has_a_distinct_wire_word() {
        let all = [
            MarksChannel::Carries,
            MarksChannel::Uniform,
            MarksChannel::Structural,
            MarksChannel::Opaque,
        ];
        let mut seen = Vec::new();
        for channel in all {
            let word = channel.wire_name();
            assert!(!word.is_empty());
            assert!(!seen.contains(&word), "{word} is spelled twice");
            seen.push(word);
        }
        assert_eq!(seen.len(), 4);
    }

    // --- R1618: the node itself is the run --------------------------------

    #[test]
    fn r1618_a_uniform_node_stacks_its_reasons_in_declaration_order() {
        // A grid row is one rectangle whose colour is a composition. The
        // reasons stack exactly as positional marks do, and the LAST is the
        // one a painter obeyed — one rule for both shapes, which is the whole
        // argument for reusing the set rather than inventing a second list.
        let set = MarkSet::whole()
            .because("group_expanded")
            .because("selected")
            .because("hovered");
        assert_eq!(set.domain(), domain::NODE);
        assert_eq!(
            set.reasons(),
            vec!["group_expanded", "selected", "hovered"],
            "declaration order, innermost last",
        );
        assert_eq!(set.names_at(0), set.reasons(), "reasons IS names_at(0)");
        assert_eq!(
            set.at(0).next_back().map(Mark::name),
            Some("hovered"),
            "the last declared is what the painter obeyed",
        );
    }

    #[test]
    fn r1618_a_reason_covers_the_node_and_nothing_else() {
        // `because` writes [0, 1). A `[0, 0)` run would cover nothing and
        // publish a reason that answers no question — which is the mistake the
        // builder exists to make unwritable.
        let set = MarkSet::whole().because("selected");
        assert_eq!(set.names_at(0), vec!["selected"]);
        assert!(
            set.names_at(1).is_empty(),
            "there is no position 1: a uniform node is one place",
        );
        let mark = set.get("selected").expect("declared");
        assert_eq!((mark.start(), mark.end()), (0, 1));
    }

    #[test]
    fn r1618_reasons_are_empty_for_a_set_that_counts_something_else() {
        // A byte-domain set's position 0 is the first BYTE, not "the node".
        // Answering with it would hand a caller a plausible wrong answer, the
        // exact failure the domain exists to prevent.
        let bytes = MarkSet::over(domain::BYTE).marking("header", 0, 4);
        assert_eq!(
            bytes.names_at(0),
            vec!["header"],
            "as a byte set, it answers"
        );
        assert!(
            bytes.reasons().is_empty(),
            "but it is not a node set, so it has no reasons",
        );
    }

    #[test]
    fn r1618_an_empty_whole_set_is_a_declaration_not_a_shrug() {
        // "Somebody looked and there was nothing" — distinct from a node that
        // was never asked, which is what the node's `Option` carries.
        let set = MarkSet::whole();
        assert_eq!(set.domain(), domain::NODE);
        assert!(set.is_empty());
        assert!(set.reasons().is_empty());
    }
}
