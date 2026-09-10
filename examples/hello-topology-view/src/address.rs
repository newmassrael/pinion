//! ★★★★★ R2122 §5.2 §5.11 — **where a painted mark's address comes from, for
//! the topology section.**
//!
//! Public because the ASSEMBLED shell needs it: the analyzer shell mounts this
//! section and its integration gate has to recover a mark's key from the paint,
//! and the only alternative is for the shell to carry a second copy of this
//! screen's composition — the class this module exists to remove.
//!
//! ⚠ The paragraph above lives HERE rather than on the `pub mod` line, and that
//! is not a matter of taste: a module carrying both an outer doc at its
//! declaration and an inner `//!` header has the two MERGED and resolved in the
//! DECLARING file's scope, so every link to this module's own items stops
//! resolving while links into the private `spec` module start resolving and are
//! then refused for being private. Measured at R2121's push gate on the sibling
//! section. One module, one documentation site.
//!
//! # What was missing
//!
//! Every mark this screen paints is named with a dotted address under `tv.`,
//! and **nothing declared one**. Measured at entry: **ninety-four string
//! literals** over ninety-four lines, all inside this one crate — the painter,
//! the hit test's inverse, the accessibility roster, the conformance judge, the
//! paint gates and the unit tests, each composing the same addresses again. A
//! wrong letter in any of them compiles, paints, and makes every query that
//! looks for the mark answer nothing.
//!
//! ⚠ **That is two different numbers and both are right.** The paint-address
//! census charges **eighty-three** of them: its needle wants three segments, or
//! two with the separator, so it does not charge the bare stem, the bare
//! namespace, or the two-segment lone marks below — a FAMILY-WIDE QUESTION is
//! not an address, which is R2119's measured rule. The budget moved by exactly
//! the charged number. A count of literals and a count of charged sites are
//! answers to different questions, and this file states which is which because
//! the first draft of this paragraph stated a third number that was neither.
//!
//! ⚠ The population is measured rather than assumed: the shell spells **zero**
//! and the walks spell **zero**, so — as on the sessions section — there is no
//! second or third population to chase, and the namespace gate can assert
//! **zero literals outside this file** instead of ratcheting toward it.
//!
//! # ★★★★★ Three panes name three parts `title`, so a bare key is ambiguous
//!
//! `spec::FILTERS`, `spec::GRAPH` and `spec::INSPECTOR` each declare a part
//! keyed `title`, and the conformance judge and the accessibility roster have
//! derived from those tables since R1947. What the tables do NOT publish is the
//! ADDRESS, so each painter composed `tv.<pane>.<key>` by hand from a roster
//! that was sitting right there — and a caller handed the bare key `title`
//! cannot say which pane it belongs to. [`parts`] therefore hands out the
//! [`crate::PartSpec`] ITSELF beside the address, never the key.
//!
//! # ⚠ Four stems carry two vocabularies each, so the inverses are disjoint
//!
//! `tv.node.` names a NODE (`tv.node.P-01`) and that node's selection RING
//! (`tv.node.P-01.ring`); `tv.toggle.` names a SWITCH and its KNOB;
//! `tv.inspector.` names a PART, that part's own child (`…badge.box`,
//! `…status.pill`) and a banded part's ENTRIES (`…keys.1`); `tv.link.` names
//! nothing but a label. Stripping a prefix cannot tell those apart, which is
//! R2118's measured lesson on the node lab's wire family: the reader that stops
//! at the prefix reads a ring as a node and a child as a part, **in both
//! directions and silently**.
//!
//! So [`node_id`] refuses a ring, [`toggle_key`] refuses a knob, the three
//! part inverses refuse anything with a further separator in it, and what is
//! INSIDE a mark is recovered by [`child_of`] and [`entry_of`] — which name
//! the owner rather than pretending the child is one.
//!
//! # ⚠⚠ Two addresses of this screen are deliberately not where they look
//!
//! [`HIGHLIGHT`] is the key-pattern field drawn inside the rail's `highlight`
//! group, and it is addressed `tv.highlight` rather than under that group's
//! part. [`link_label`] addresses a LABEL and there is no `tv.link.<n>` for it
//! to hang under, because the link itself is a stroked line and a line carries
//! no tag. Both are facts about the paint; declaring them here is what stops
//! the next reader from inventing the address they expected instead.

use crate::spec;

/// The stem this screen's marks hang under, without a separator.
///
/// ★ Wanted bare by `WidgetPaint::paint_stems`, which is asked for the stems a
/// screen's marks live under rather than for a prefix to match — so the
/// separator is added by whoever is matching, not carried into the roster.
pub const STEM: &str = "tv";

/// The namespace every mark this screen paints begins with.
///
/// Carried WITH its separator, which is what a membership test wants: a bare
/// `tv` would also claim a mark called `tvchannel`. Held equal to [`STEM`] plus
/// the separator by this crate's own gate, so the two spellings cannot drift.
pub const NAMESPACE: &str = "tv.";

/// The root group every mark of this screen hangs under.
pub const ROOT: &str = "tv.root";

/// The filter rail's own group.
pub const FILTERS: &str = "tv.filters";

/// The graph column's own group.
pub const GRAPH: &str = "tv.graph";

/// The inspector's own group.
pub const INSPECTOR: &str = "tv.inspector";

/// Where a resting description is painted and announced.
pub const TIP: &str = "tv.tip";

/// The key-pattern field in the rail's highlight group.
///
/// See this module's header for why it is not addressed under that group.
pub const HIGHLIGHT: &str = "tv.highlight";

/// [`FILTERS`] with the separator its parts hang off.
///
/// A `&'static str` because the conformance judge needs one to scan the painted
/// regions with; the gate drives it against [`FILTERS`], so the two cannot
/// drift.
pub const FILTERS_SEAT: &str = "tv.filters.";

/// [`GRAPH`] with the separator its parts hang off.
pub const GRAPH_SEAT: &str = "tv.graph.";

/// [`INSPECTOR`] with the separator its parts hang off.
pub const INSPECTOR_SEAT: &str = "tv.inspector.";

/// Where an observed node is addressed.
pub const NODE_SEAT: &str = "tv.node.";

/// Where a layout button is addressed.
pub const LAYOUT_SEAT: &str = "tv.layout.";

/// Where a switch is addressed.
pub const TOGGLE_SEAT: &str = "tv.toggle.";

/// Where a highlight chip is addressed.
pub const CHIP_SEAT: &str = "tv.chip.";

/// Where an observed link's label is addressed.
pub const LINK_SEAT: &str = "tv.link.";

/// The plot — the graph column's pressable body, and one of its parts.
///
/// ★ One of the four parts a caller needs as a `&'static str` rather than a
/// composed `String`: it is an accessibility focus token compared by identity,
/// and the three below are matched on by the hit test. A `const` cannot call
/// [`graph`], so these are declarations held against the derivation by
/// `r2122_every_topology_address_is_derived` — the arrangement
/// `hello_node_lab`'s `address::FORM_CONTROL_TEMPLATE` has had since R2050, and
/// for the same reason.
pub const CANVAS: &str = "tv.graph.canvas";

/// The control that returns the plot to the zoom it opened at.
///
/// A `const` so the hit test can MATCH on it. See [`CANVAS`].
pub const FIT: &str = "tv.graph.fit";

/// The control that draws the plot closer.
pub const ZOOM_IN: &str = "tv.graph.zoom_in";

/// The control that draws the plot further away.
pub const ZOOM_OUT: &str = "tv.graph.zoom_out";

/// The words a mark's own child is addressed under.
///
/// ★ A CLOSED set, and that is the point: `box`, `pill`, `ring` and `knob` are
/// the four things a mark of this screen puts inside itself, and [`child`]
/// refuses a fifth. An open suffix here would make the part inverses unable to
/// say where a mark's address ends — which is the ambiguity this module exists
/// to remove rather than one to introduce.
pub const CHILD_WORDS: &[&str] = &["box", "pill", "ring", "knob"];

/// The address the filter rail paints `part` under.
///
/// `part` is one of `spec::FILTERS`'s keys — the painter names the part, this
/// composes the address, and nobody does both.
#[must_use]
pub fn filters(part: &str) -> String {
    format!("{FILTERS_SEAT}{part}")
}

/// The address the graph column paints `part` under.
#[must_use]
pub fn graph(part: &str) -> String {
    format!("{GRAPH_SEAT}{part}")
}

/// The address the inspector paints `part` under.
#[must_use]
pub fn inspector(part: &str) -> String {
    format!("{INSPECTOR_SEAT}{part}")
}

/// The address an observed node is painted under.
#[must_use]
pub fn node(id: &str) -> String {
    format!("{NODE_SEAT}{id}")
}

/// The address the `n`th layout button is painted under.
#[must_use]
pub fn layout(n: usize) -> String {
    format!("{LAYOUT_SEAT}{n}")
}

/// The address a switch is painted under.
#[must_use]
pub fn toggle(key: &str) -> String {
    format!("{TOGGLE_SEAT}{key}")
}

/// The address the `n`th highlight chip is painted under.
#[must_use]
pub fn chip(n: usize) -> String {
    format!("{CHIP_SEAT}{n}")
}

/// The address the `n`th observed link's label is painted under.
///
/// ⚠ There is no address for the link itself; see this module's header.
#[must_use]
pub fn link_label(n: usize) -> String {
    format!("{LINK_SEAT}{n}.label")
}

/// The address one entry of a banded inspector part is painted under.
///
/// The key-pattern rows, whose keys are ordinals rather than words.
#[must_use]
pub fn entry(part: &str, n: usize) -> String {
    format!("{INSPECTOR_SEAT}{part}.{n}")
}

/// The address a mark's own child is painted under.
///
/// # Panics
///
/// If `word` is not one of [`CHILD_WORDS`]. That is a defect in the caller
/// rather than a state the screen reaches, and refusing it here is what keeps
/// the inverses able to say where a mark's address ends.
#[must_use]
pub fn child(seat: &str, word: &str) -> String {
    assert!(
        CHILD_WORDS.contains(&word),
        "`{word}` is not one of this screen's child words {CHILD_WORDS:?} — a \
         new one has to be declared before it can be addressed, or the \
         inverses stop being able to tell a mark from what is inside it"
    );
    format!("{seat}.{word}")
}

/// Whether `tag` is a mark of this screen at all.
#[must_use]
pub fn ours(tag: &str) -> bool {
    tag.starts_with(NAMESPACE)
}

/// The filter-rail part `tag` names, or `None`.
///
/// ⚠ Refuses a part's CHILD: the answer to *which part is this* for a child is
/// the child's own business, and a reader handed one as a part key would look
/// it up in `spec::FILTERS` and find nothing — quietly.
#[must_use]
pub fn filters_part(tag: &str) -> Option<&str> {
    bare(tag.strip_prefix(FILTERS_SEAT)?)
}

/// The graph-column part `tag` names, or `None`.
#[must_use]
pub fn graph_part(tag: &str) -> Option<&str> {
    bare(tag.strip_prefix(GRAPH_SEAT)?)
}

/// The inspector part `tag` names, or `None`.
#[must_use]
pub fn inspector_part(tag: &str) -> Option<&str> {
    bare(tag.strip_prefix(INSPECTOR_SEAT)?)
}

/// The node `tag` names, or `None`.
///
/// ⚠ Refuses a node's selection RING. A caller that means *the node this ring
/// belongs to* asks [`child_of`] and says so at the call site — order is a
/// property of one caller, refusal is a property of the address.
#[must_use]
pub fn node_id(tag: &str) -> Option<&str> {
    bare(tag.strip_prefix(NODE_SEAT)?)
}

/// The layout button `tag` names, or `None`.
#[must_use]
pub fn layout_index(tag: &str) -> Option<usize> {
    bare(tag.strip_prefix(LAYOUT_SEAT)?)?.parse().ok()
}

/// The switch `tag` names, or `None`.
///
/// ⚠ Refuses a switch's KNOB, for the reason [`node_id`] refuses a ring.
#[must_use]
pub fn toggle_key(tag: &str) -> Option<&str> {
    bare(tag.strip_prefix(TOGGLE_SEAT)?)
}

/// The highlight chip `tag` names, or `None`.
#[must_use]
pub fn chip_index(tag: &str) -> Option<usize> {
    bare(tag.strip_prefix(CHIP_SEAT)?)?.parse().ok()
}

/// The observed link whose LABEL `tag` names, or `None`.
///
/// ⚠ Refuses a bare `tv.link.<n>`, which this screen never paints — a reader
/// handed one would go looking for a mark that is not there.
#[must_use]
pub fn link_label_index(tag: &str) -> Option<usize> {
    let (n, word) = tag.strip_prefix(LINK_SEAT)?.split_once('.')?;
    (word == "label").then(|| n.parse().ok())?
}

/// The mark `tag` is a CHILD of, with the child word, or `None`.
///
/// The inverse of [`child`], and the reason a ring, a knob, a box and a pill do
/// not each need a family of their own: they are recovered by naming the mark
/// that owns them.
#[must_use]
pub fn child_of(tag: &str) -> Option<(&str, &str)> {
    let (owner, word) = tag.rsplit_once('.')?;
    (!owner.is_empty() && CHILD_WORDS.contains(&word)).then_some((owner, word))
}

/// The banded inspector part `tag` is an ENTRY of, with its ordinal, or `None`.
#[must_use]
pub fn entry_of(tag: &str) -> Option<(&str, usize)> {
    let (part, n) = tag.strip_prefix(INSPECTOR_SEAT)?.split_once('.')?;
    let n: usize = n.parse().ok()?;
    (!part.is_empty() && !part.contains('.')).then_some((part, n))
}

/// A key with nothing further hanging off it, or `None`.
///
/// The one rule the inverses above share, written once: a remainder that still
/// holds a separator is not a key of that family, and an EMPTY remainder is the
/// family's own stem rather than a member of it. R2112 paid for the second half
/// on the capture viewer's decode tree — `Some("")` there let a gate count a
/// stem as an unnamed member and report the family fully claimed.
fn bare(rest: &str) -> Option<&str> {
    (!rest.is_empty() && !rest.contains('.')).then_some(rest)
}

/// Every part the three panes declare, beside the address it is painted at.
///
/// Derived from `spec::FILTERS`, `spec::GRAPH` and `spec::INSPECTOR` rather
/// than listed again: the specification is what says which parts exist, and a
/// second roster here would be exactly the drift this module removes.
///
/// ⚠⚠ Each row carries the [`crate::PartSpec`] ITSELF rather than its key, and
/// that is not tidiness — **all three panes name a part `title`**. A caller
/// handed `("title", "tv.inspector.title")` that went back to the tables to
/// look the title up would find the RAIL's row first and name the inspector's
/// heading "Filters and layers". The pairing is kept here so no caller can lose
/// it.
#[must_use]
pub fn parts() -> Vec<(&'static spec::PartSpec, String)> {
    spec::FILTERS
        .iter()
        .map(|p| (p, filters(p.key)))
        .chain(spec::GRAPH.iter().map(|p| (p, graph(p.key))))
        .chain(spec::INSPECTOR.iter().map(|p| (p, inspector(p.key))))
        .collect()
}
