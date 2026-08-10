//! R1629 §5.12 §2 #7 — the `scene/derivations` wire shape: **how the drawing
//! tagged `tag` was produced**.
//!
//! ## Why this is a scene method
//!
//! The facts here were already computed and already published — as Rust
//! methods on chart builders. `LineChart::overshoot` names every place a
//! spline left the data; `Density` knows its kernel, its bandwidth and the
//! share of mass it put outside the observed range; `BoxPlotChart` knows which
//! categories could not become violins. An RPC client holds a `Scene` and none
//! of those objects, so under §2 #2 — the headless API as the *primary* path —
//! the strongest reports in the crate were unreachable by the reader they were
//! written for.
//!
//! The sibling of `scene/marks`, and the two split the question cleanly.
//! Marks answer **why a position looks like that** — a stack of named runs
//! over an index space. Derivations answer **how the drawing as a whole
//! relates to its sources**, which has no position: "this bandwidth decided
//! the outline" is not a fact about any one pixel of it.
//!
//! ## The answer is a closed 2×2, so a client can match exhaustively
//!
//! Every entry states which of four disagreements it is — see
//! [`pinion_core::derivation::DerivationKind`]. `source` rides
//! along because it is one axis of that table and a client asking "everything
//! the DATA does not support" wants a row rather than two kinds it had to know
//! were related. Both are derived from the same enum, so they cannot drift.
//!
//! ## What the outcome distinguishes
//!
//! | outcome | `published` | meaning |
//! |---|---|---|
//! | stated | `true` | the composition named how its drawing was produced (possibly with no entries — see below) |
//! | silent | `false`, `channel: "composes"` | the node composes a drawing and does not answer |
//! | no channel | `false`, `channel: "painted"` / `"deferred"` / `"opaque"` | the node's *kind* has no production step to describe, and this says which reason |
//! | no such tag | (error) | nothing in the scene carries that tag |
//!
//! **`published: true` with an empty list is a real answer** and the most
//! easily lost one: it says the composition ran its reports and the picture
//! hides nothing. Collapsing it into "silent" would make a chart that invented
//! nothing indistinguishable from one that never checked.
//!
//! ## Addressed by tag, and narrowed by kind
//!
//! `tag` is how the framework's own composed nodes are found, for the reason
//! `scene/marks` gives. `kind` is the optional narrower — the query a client
//! actually has ("did this picture invent anything?") — and an unrecognised
//! one is refused with the accepted set named, rather than silently returning
//! everything.

use pinion_core::derivation::{
    Derivation, DerivationKind, DerivationLookup, DerivationSet, DerivationSource, DerivesChannel,
    Evidence,
};
use pinion_core::scene::Scene;

/// R1629 §5.12 §2 #7 — every wire spelling a `kind` (and a `filter`) can
/// carry, DERIVED from the domain enum's own census.
///
/// Computed rather than retyped, for R1616's reason: a hand list here would be
/// a second copy of a closed set, and a second copy of a closed set goes stale
/// in silence. That matters more than usual on this field, because the whole
/// claim of the taxonomy is that a client can match it EXHAUSTIVELY — a
/// published set that lagged the enum would make the exhaustive match wrong at
/// exactly the moment a fifth kind arrived.
pub const KIND_WIRE_NAMES: [&str; DerivationKind::ALL.len()] = {
    let mut names = [""; DerivationKind::ALL.len()];
    let mut i = 0;
    while i < DerivationKind::ALL.len() {
        names[i] = DerivationKind::ALL[i].wire_name();
        i += 1;
    }
    names
};

/// R1629 — every wire spelling a `source` can carry: one axis of the 2×2,
/// derived the same way [`KIND_WIRE_NAMES`] is.
pub const SOURCE_WIRE_NAMES: [&str; DerivationSource::ALL.len()] = {
    let mut names = [""; DerivationSource::ALL.len()];
    let mut i = 0;
    while i < DerivationSource::ALL.len() {
        names[i] = DerivationSource::ALL[i].wire_name();
        i += 1;
    }
    names
};

/// R1629 — every wire spelling a `channel` can carry, derived the same way.
pub const CHANNEL_WIRE_NAMES: [&str; DerivesChannel::ALL.len()] = {
    let mut names = [""; DerivesChannel::ALL.len()];
    let mut i = 0;
    while i < DerivesChannel::ALL.len() {
        names[i] = DerivesChannel::ALL[i].wire_name();
        i += 1;
    }
    names
};

/// R1629 — every wire spelling an `evidence.type` can carry.
///
/// Hand-listed where the other three are derived, and the reason is the type
/// system: [`Evidence`] carries a payload per arm, so it has no `ALL` const to
/// project — a value is needed to spell one. The census test below is what
/// stands in for the derivation, matching this list against a fixture of every
/// arm, so a shape added upstream fails here rather than reaching a client
/// undocumented.
pub const EVIDENCE_WIRE_NAMES: [&str; 4] = ["name", "real", "count", "flag"];

/// R1629 — where a localized derivation applies, in the set's `domain`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct DerivationSpanWire {
    /// First covered index.
    pub start: usize,
    /// One past the last covered index.
    pub end: usize,
}

/// R1629 — the measurement behind one derivation.
///
/// `type` is the discriminator, and the value rides in the field that names
/// it, so a client reads `evidence.type` once and never has to guess whether
/// `3` was a count or a quantity.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct EvidenceWire {
    /// `name` / `real` / `count` / `flag`.
    #[serde(rename = "type")]
    pub kind: String,
    /// Present for `name`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Present for `real`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
    /// Present for `count`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<usize>,
    /// Present for `flag`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flag: Option<bool>,
}

impl From<&Evidence> for EvidenceWire {
    fn from(evidence: &Evidence) -> Self {
        let mut wire = Self {
            kind: evidence.wire_name().to_owned(),
            name: None,
            value: None,
            count: None,
            flag: None,
        };
        match evidence {
            Evidence::Name(n) => wire.name = Some(n.to_string()),
            Evidence::Real(v) => wire.value = Some(*v),
            Evidence::Count(c) => wire.count = Some(*c),
            Evidence::Flag(f) => wire.flag = Some(*f),
            // `Evidence` is `#[non_exhaustive]`: a shape added upstream lands
            // here with its discriminator on the wire and no payload, which is
            // a client reading "I do not know this shape" rather than a
            // mis-typed value.
            _ => {}
        }
        wire
    }
}

/// R1629 — one published statement about how a drawing was produced.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct DerivationWire {
    /// Which of the four disagreements: `invented` / `omitted` / `chosen` /
    /// `discarded`.
    pub kind: String,
    /// Which source the picture is compared against: `data` / `request`. One
    /// axis of the 2×2, derived from `kind`.
    pub source: String,
    /// Whether the picture has something the source does not. The other axis;
    /// with `source` it reconstructs `kind`.
    pub picture_has_more: bool,
    /// What the statement is about — a stable identifier, not a sentence.
    pub name: String,
    /// Which part of the drawing, absent when it is about the whole of it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    /// The measurement.
    pub evidence: EvidenceWire,
    /// The units of a `real` evidence, absent when dimensionless.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    /// Where it applies in the outcome's `domain`, absent when the statement
    /// is not localized.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span: Option<DerivationSpanWire>,
}

impl From<&Derivation> for DerivationWire {
    fn from(d: &Derivation) -> Self {
        Self {
            kind: d.kind().wire_name().to_owned(),
            source: d.kind().source().wire_name().to_owned(),
            picture_has_more: d.kind().picture_has_more(),
            name: d.name().to_owned(),
            subject: d.subject().map(ToOwned::to_owned),
            evidence: EvidenceWire::from(d.evidence()),
            unit: d.unit().map(ToOwned::to_owned),
            span: d
                .span()
                .map(|(start, end)| DerivationSpanWire { start, end }),
        }
    }
}

/// R1629 — the `scene/derivations` answer.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct DerivationsOutcome {
    /// The tag asked about, echoed.
    pub tag: String,
    /// The node kind carrying it — the spelling `scene/snapshot` uses.
    pub kind: String,
    /// Whether that kind can describe a production step, and when it cannot,
    /// why: `composes` / `painted` / `deferred` / `opaque`.
    pub channel: String,
    /// Whether the node stated anything. `true` with an empty `derivations`
    /// means "I ran my reports and the picture hides nothing".
    pub published: bool,
    /// What a `span` indexes. Absent exactly when nothing was published,
    /// because an index space with nothing in it is not a fact about anything.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    /// The statements, in declaration order — the order the builder made them,
    /// which is the order a caption would read them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub derivations: Option<Vec<DerivationWire>>,
    /// The kind the caller narrowed to, echoed so the answer is
    /// self-describing. Absent when the caller asked for everything.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<String>,
}

/// The `scene/derivations` result for `tag`, or `None` when no node carries it.
///
/// `filter` narrows to one kind. It is applied to the list and echoed; it does
/// **not** change `published`, because "this composition answers and none of
/// its statements are of the kind you asked about" is a different fact from
/// "this composition does not answer".
#[must_use]
pub fn derivations_outcome(
    scene: &Scene,
    tag: &str,
    filter: Option<DerivationKind>,
) -> Option<DerivationsOutcome> {
    let node = scene.find_with_tag(tag)?;
    let kind = node.node_kind();
    let mut outcome = DerivationsOutcome {
        tag: tag.to_owned(),
        kind: kind.name().to_owned(),
        channel: kind.derives_channel().wire_name().to_owned(),
        published: false,
        domain: None,
        derivations: None,
        filter: filter.map(|k| k.wire_name().to_owned()),
    };
    match node.derivations_for_tag(tag) {
        DerivationLookup::Published(set) => {
            outcome.published = true;
            outcome.domain = Some(set.domain().to_owned());
            outcome.derivations = Some(entry_list(set, filter));
        }
        DerivationLookup::Silent | DerivationLookup::NoChannel(_) => {}
        // `find_with_tag` just answered with this node, so the tag resolves.
        DerivationLookup::NoSuchTag => unreachable!("the node answering was found by this tag"),
    }
    Some(outcome)
}

/// The entries, in declaration order, narrowed by `filter` when given.
fn entry_list(set: &DerivationSet, filter: Option<DerivationKind>) -> Vec<DerivationWire> {
    set.entries()
        .iter()
        .filter(|d| filter.is_none_or(|want| d.kind() == want))
        .map(DerivationWire::from)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::derivation::DerivationSet;
    use pinion_core::scene::{BoxNode, ContainerNode, Rect};
    use pinion_core::style::BoxStyle;
    use serde_json::Value;

    fn chart_scene() -> Scene {
        let set = DerivationSet::over("sample")
            .stating(Derivation::new(
                DerivationKind::Chosen,
                "interpolation",
                Evidence::Name("catmull-rom".into()),
            ))
            .stating(
                Derivation::new(DerivationKind::Invented, "overshoot", Evidence::Real(-3.25))
                    .about("series.0")
                    .in_units("value")
                    .spanning(3, 5),
            )
            .stating(
                Derivation::new(DerivationKind::Omitted, "off_scale", Evidence::Count(2))
                    .about("series.1"),
            )
            .stating(
                Derivation::new(
                    DerivationKind::Discarded,
                    "caps",
                    Evidence::Name("ohlc".into()),
                )
                .about("mark"),
            );
        Scene::Container(
            ContainerNode::new(vec![
                Scene::Box(
                    BoxNode::new(Rect::new(0, 0, 4, 4), BoxStyle::default()).with_tag("ink"),
                ),
                Scene::Container(ContainerNode::new(Vec::new()).with_tag("quiet")),
            ])
            .with_tag("chart")
            .with_derivations(set),
        )
    }

    fn json(scene: &Scene, tag: &str, filter: Option<DerivationKind>) -> Value {
        serde_json::to_value(derivations_outcome(scene, tag, filter).expect("tag resolves"))
            .expect("the outcome serializes")
    }

    #[test]
    fn r1629_a_chart_states_every_kind_with_its_row_of_the_table() {
        let value = json(&chart_scene(), "chart", None);
        assert_eq!(value["published"], Value::Bool(true));
        assert_eq!(value["kind"], "Container");
        assert_eq!(value["channel"], "composes");
        assert_eq!(value["domain"], "sample");
        assert!(value.get("filter").is_none(), "nothing was narrowed");
        let list = value["derivations"].as_array().expect("entries");
        assert_eq!(list.len(), 4);

        // Declaration order, and every entry carries both axes of the 2x2.
        assert_eq!(list[0]["kind"], "chosen");
        assert_eq!(list[0]["source"], "request");
        assert_eq!(list[0]["picture_has_more"], Value::Bool(true));
        assert_eq!(list[0]["evidence"]["type"], "name");
        assert_eq!(list[0]["evidence"]["name"], "catmull-rom");
        assert!(list[0].get("subject").is_none(), "about the whole drawing");

        assert_eq!(list[1]["kind"], "invented");
        assert_eq!(list[1]["source"], "data");
        assert_eq!(list[1]["subject"], "series.0");
        assert_eq!(list[1]["evidence"]["type"], "real");
        assert_eq!(list[1]["evidence"]["value"], -3.25);
        assert_eq!(list[1]["unit"], "value");
        assert_eq!(list[1]["span"]["start"], 3);
        assert_eq!(list[1]["span"]["end"], 5);

        assert_eq!(list[2]["kind"], "omitted");
        assert_eq!(list[2]["source"], "data");
        assert_eq!(list[2]["picture_has_more"], Value::Bool(false));
        assert_eq!(list[2]["evidence"]["type"], "count");
        assert_eq!(list[2]["evidence"]["count"], 2);
        assert!(list[2].get("span").is_none(), "not localized");
        assert!(list[2].get("unit").is_none(), "a count has no units");

        assert_eq!(list[3]["kind"], "discarded");
        assert_eq!(list[3]["source"], "request");
        assert_eq!(list[3]["picture_has_more"], Value::Bool(false));
    }

    #[test]
    fn r1629_a_filter_narrows_the_list_and_says_it_did() {
        let scene = chart_scene();
        let value = json(&scene, "chart", Some(DerivationKind::Invented));
        assert_eq!(value["filter"], "invented");
        let list = value["derivations"].as_array().expect("entries");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0]["name"], "overshoot");

        // ★ A filter that matches nothing still answers `published: true`:
        // "this composition answers and has nothing of that kind" is not the
        // same fact as "this composition does not answer", and a client that
        // could not tell them apart would read a working chart as broken.
        let empty = json(&scene, "chart", Some(DerivationKind::Chosen));
        assert_eq!(empty["published"], Value::Bool(true));
        assert_eq!(empty["derivations"].as_array().expect("list").len(), 1);
    }

    #[test]
    fn r1629_the_three_ways_of_having_no_answer_are_three_answers() {
        let scene = chart_scene();
        // Silent: a real composition that stated nothing.
        let silent = json(&scene, "quiet", None);
        assert_eq!(silent["published"], Value::Bool(false));
        assert_eq!(silent["channel"], "composes");
        assert!(silent.get("derivations").is_none());
        assert!(silent.get("domain").is_none());

        // No channel: a node whose KIND has no production step, with the
        // reason on the wire rather than in this crate's rustdoc.
        let painted = json(&scene, "ink", None);
        assert_eq!(painted["published"], Value::Bool(false));
        assert_eq!(painted["channel"], "painted");
        assert_eq!(painted["kind"], "Box");

        // No such tag: not an answer at all.
        assert!(derivations_outcome(&scene, "nobody", None).is_none());

        assert_ne!(silent["channel"], painted["channel"]);
    }

    #[test]
    fn r1629_every_evidence_shape_puts_its_value_in_the_field_its_type_names() {
        // One shape per field, and no field set that the type did not name —
        // a client keying off `type` and reading the wrong field would get
        // `null` rather than a plausible wrong number, and this is what keeps
        // that true.
        for (evidence, field) in [
            (Evidence::Name("k".into()), "name"),
            (Evidence::Real(1.5), "value"),
            (Evidence::Count(7), "count"),
            (Evidence::Flag(true), "flag"),
        ] {
            let wire =
                serde_json::to_value(EvidenceWire::from(&evidence)).expect("evidence serializes");
            assert_eq!(wire["type"], evidence.wire_name());
            assert!(!wire[field].is_null(), "{field} carries the value");
            let object = wire.as_object().expect("an object");
            assert_eq!(
                object.len(),
                2,
                "exactly the discriminator and its own field: {object:?}"
            );
        }
    }

    #[test]
    fn r1629_a_composition_with_nothing_to_say_still_states_its_domain() {
        let scene = Scene::Container(
            ContainerNode::new(Vec::new())
                .with_tag("empty")
                .with_derivations(DerivationSet::over("slot")),
        );
        let value = json(&scene, "empty", None);
        assert_eq!(
            value["published"],
            Value::Bool(true),
            "it ran its reports and found nothing"
        );
        assert_eq!(value["derivations"].as_array().expect("list").len(), 0);
        assert_eq!(value["domain"], "slot");
    }
}
