//! R1615 §5.12 §2 #7 — the `scene/marks` wire shape: **why the thing tagged
//! `tag` looks the way it does**.
//!
//! ## Why this is a scene method and not a widget's own introspection
//!
//! A widget's appearance is routinely decided by more than one piece of state,
//! and those pieces live in different places. A byte dump lights a byte because
//! an overview brush selected the field it is in *and* because a drag selection
//! covers it — two sibling externals, neither of which owns the assembled fact.
//! Before this method the assembly happened in the view, was folded into cell
//! colours, and was thrown away; the only oracle that could be asked took the
//! other half of the question **as an argument**, so the client had to query one
//! external, feed the answer into another, and reproduce the framework's
//! composition rule on its own side.
//!
//! The view is where the assembly happens, so the view publishes it and the
//! question is asked of the picture. That is the same principle
//! `scene/snapshot` rests on — the truth about what was drawn is in the paint
//! tree — extended from *what* was drawn to *why*.
//!
//! ## What the answer distinguishes
//!
//! Four outcomes, because they are four different facts and a client acts
//! differently on each:
//!
//! | outcome | `published` | meaning |
//! |---|---|---|
//! | runs | `true` | the node named the declarations behind its appearance |
//! | silent | `false`, `channel: "carries"` | the node could and did not |
//! | no channel | `false`, `channel: "uniform"` / `"structural"` / `"opaque"` | the node's *kind* has nothing to attribute, and this says which reason |
//! | no such tag | (error) | nothing in the scene carries that tag |
//!
//! Collapsing the middle two into "no marks" is what makes an agent unable to
//! tell a widget that forgot to publish from a box that has nothing to publish.
//!
//! `at` is the fifth distinction and it is about the *question*, not the node:
//! present only when the caller named a position, and present-with-empty-names
//! when it named one nothing covers. "I did not ask" and "nothing is there" are
//! not the same answer either.
//!
//! ## Addressed by tag, not by path
//!
//! `tag` is how the framework's own marked nodes are found
//! (`Scene::find_with_tag`), and it is stable across the layout that decides a
//! node's position in the tree. Path addressing here would want `path::resolve`
//! against the *paint* scene, which is a separate open question about which
//! scene the spatial methods read; see `debt-locate-and-bbox-are-state-only`.

use pinion_core::marks::{MarkedRuns, MarksLookup};
use pinion_core::scene::Scene;

/// R1615 — one published run: what it is, and where.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct MarkRunWire {
    /// What the run is called — the identity a colour cannot carry.
    pub name: String,
    /// First covered index, in the enclosing outcome's `domain`.
    pub start: usize,
    /// One past the last covered index.
    pub end: usize,
}

/// R1615 — the stack covering one position, present only when the request
/// named a position.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct MarkStackWire {
    /// The position asked about, echoed so an answer is self-describing.
    pub index: usize,
    /// Every name covering it, in declaration order — innermost last.
    pub names: Vec<String>,
    /// The one a painter obeys, or `null` where nothing covers the position.
    /// Null and an empty `names` say the same thing two ways on purpose: a
    /// client keying off either reads the same fact.
    pub top: Option<String>,
}

/// R1615 — the `scene/marks` answer.
///
/// `published` is the discriminator, and `channel` is what makes a `false`
/// worth reading: a node that could have named its runs and did not is a
/// different fact from a node whose kind has nothing to name.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct MarksOutcome {
    /// The tag asked about, echoed.
    pub tag: String,
    /// The node kind carrying it — the same spelling `scene/snapshot` uses
    /// for a node's `type`.
    pub kind: String,
    /// Whether that kind can name the declarations behind its appearance, and
    /// when it cannot, why: `carries` / `uniform` / `structural` / `opaque`.
    pub channel: String,
    /// Whether this node named any.
    pub published: bool,
    /// What the run indices count. Absent exactly when nothing was published,
    /// because an index space with no runs in it is not a fact about anything.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    /// The runs, in declaration order — which is the order overlap resolves
    /// in, so a client redrawing from this list front to back gets the same
    /// picture.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runs: Option<Vec<MarkRunWire>>,
    /// The stack at the requested position. Absent when the request named no
    /// position; present with an empty `names` when it named one nothing
    /// covers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at: Option<MarkStackWire>,
}

/// The `scene/marks` result for `tag`, or `None` when no node carries it.
///
/// `index` is optional: without it the answer is the whole run list, with it
/// the answer also carries the stack covering that position. The index counts
/// in the reported `domain`, which is why the domain is always reported when
/// there is one — a hex dump's runs are over *bytes*, and the cells that show
/// them are three times as many.
#[must_use]
pub fn marks_outcome(scene: &Scene, tag: &str, index: Option<usize>) -> Option<MarksOutcome> {
    let node = scene.find_with_tag(tag)?;
    let kind = node.node_kind();
    let mut outcome = MarksOutcome {
        tag: tag.to_owned(),
        kind: kind.name().to_owned(),
        channel: kind.marks_channel().wire_name().to_owned(),
        published: false,
        domain: None,
        runs: None,
        at: None,
    };
    match node.marks_for_tag(tag) {
        MarksLookup::Published(runs) => {
            outcome.published = true;
            outcome.domain = Some(runs.domain().to_owned());
            outcome.runs = Some(run_list(&runs));
            outcome.at = index.map(|at| stack_at(&runs, at));
        }
        MarksLookup::Silent | MarksLookup::NoChannel(_) => {
            outcome.at = index.map(|at| MarkStackWire {
                index: at,
                names: Vec::new(),
                top: None,
            });
        }
        // `find_with_tag` just answered with this node, so the tag resolves.
        MarksLookup::NoSuchTag => unreachable!("the node answering was found by this tag"),
    }
    Some(outcome)
}

/// The runs, in declaration order.
fn run_list(runs: &MarkedRuns<'_>) -> Vec<MarkRunWire> {
    runs.runs()
        .iter()
        .map(|run| MarkRunWire {
            name: run.name.to_owned(),
            start: run.start,
            end: run.end,
        })
        .collect()
}

/// The stack covering `at`: every name in declaration order, plus the one a
/// painter obeys.
fn stack_at(runs: &MarkedRuns<'_>, at: usize) -> MarkStackWire {
    let names: Vec<String> = runs
        .names_at(at)
        .into_iter()
        .map(ToOwned::to_owned)
        .collect();
    MarkStackWire {
        top: names.last().cloned(),
        index: at,
        names,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::cell_metric::CellMetric;
    use pinion_core::marks::{MarkSet, domain};
    use pinion_core::scene::{BoxNode, ContainerNode, Rect, StyleRun, TextGridNode, TextNode};
    use pinion_core::style::{BoxStyle, TextStyle};
    use serde_json::Value;

    fn dump_scene() -> Scene {
        let marks = MarkSet::over(domain::BYTE)
            .marking("frame", 0, 64)
            .marking("header", 0, 16)
            .marking("length", 4, 8);
        Scene::Container(ContainerNode::new(vec![
            Scene::Box(
                BoxNode::new(Rect::new(0, 0, 10, 10), BoxStyle::default()).with_tag("chrome"),
            ),
            Scene::TextGrid(
                TextGridNode::new(CellMetric::DEFAULT)
                    .with_tag("dump")
                    .with_marks(marks),
            ),
            Scene::TextGrid(TextGridNode::new(CellMetric::DEFAULT).with_tag("quiet")),
        ]))
    }

    fn json(scene: &Scene, tag: &str, index: Option<usize>) -> Value {
        serde_json::to_value(marks_outcome(scene, tag, index).expect("tag resolves"))
            .expect("the outcome serializes")
    }

    #[test]
    fn r1615_a_byte_alone_gets_the_whole_stack() {
        let value = json(&dump_scene(), "dump", Some(5));
        assert_eq!(value["published"], Value::Bool(true));
        assert_eq!(value["kind"], "TextGrid");
        assert_eq!(value["channel"], "carries");
        assert_eq!(value["domain"], "byte");
        assert_eq!(value["at"]["index"], 5);
        assert_eq!(
            value["at"]["names"],
            Value::Array(vec!["frame".into(), "header".into(), "length".into()])
        );
        assert_eq!(value["at"]["top"], "length");
        assert_eq!(value["runs"].as_array().expect("runs").len(), 3);
        assert_eq!(value["runs"][2]["name"], "length");
        assert_eq!(value["runs"][2]["start"], 4);
        assert_eq!(value["runs"][2]["end"], 8);
    }

    #[test]
    fn r1615_without_an_index_the_answer_is_the_run_list_and_no_stack() {
        let value = json(&dump_scene(), "dump", None);
        assert_eq!(value["runs"].as_array().expect("runs").len(), 3);
        assert!(
            value.get("at").is_none(),
            "no position was asked about, so there is no stack"
        );
    }

    #[test]
    fn r1615_an_uncovered_position_says_so_without_pretending_to_be_missing() {
        let value = json(&dump_scene(), "dump", Some(999));
        assert_eq!(value["at"]["names"], Value::Array(vec![]));
        assert_eq!(value["at"]["top"], Value::Null, "nothing covers it");
        assert_eq!(value["at"]["index"], 999);
        assert_eq!(
            value["published"],
            Value::Bool(true),
            "the node did publish"
        );
    }

    #[test]
    fn r1615_the_three_ways_of_having_no_answer_are_three_answers() {
        let scene = dump_scene();
        // Silent: a real grid, carrying the channel, declaring nothing.
        let silent = json(&scene, "quiet", Some(0));
        assert_eq!(silent["published"], Value::Bool(false));
        assert_eq!(silent["channel"], "carries");
        assert!(silent.get("runs").is_none());
        assert!(silent.get("domain").is_none());

        // No channel: a real node whose KIND has nothing to attribute, and the
        // reason is on the wire rather than in this crate's rustdoc.
        let uniform = json(&scene, "chrome", Some(0));
        assert_eq!(uniform["published"], Value::Bool(false));
        assert_eq!(uniform["channel"], "uniform");
        assert_eq!(uniform["kind"], "Box");

        // No such tag: not an answer at all.
        assert!(marks_outcome(&scene, "nobody", Some(0)).is_none());

        assert_ne!(silent["channel"], uniform["channel"]);
    }

    #[test]
    fn r1615_a_text_node_reports_the_character_domain_and_its_named_runs() {
        let scene = Scene::Text(
            TextNode::new("let x = 1;".to_string(), Rect::new(0, 0, 80, 20))
                .with_tag("code")
                .with_runs(vec![
                    StyleRun::new(0, 3, TextStyle::new()).named("keyword"),
                    StyleRun::new(8, 9, TextStyle::new()).named("number"),
                ]),
        );
        let value = json(&scene, "code", Some(1));
        assert_eq!(value["kind"], "Text");
        assert_eq!(
            value["domain"], "utf8_byte",
            "a text run indexes UTF-8 bytes, and the wire says which"
        );
        assert_eq!(value["at"]["names"], Value::Array(vec!["keyword".into()]));
        assert_eq!(value["at"]["top"], "keyword");
    }

    #[test]
    fn r1615_the_domain_is_never_left_for_the_client_to_guess() {
        // The two framework domains differ, and a client that assumed either
        // one would misread the other. Whenever runs are published, the domain
        // is published with them.
        let value = json(&dump_scene(), "dump", None);
        assert_eq!(value["domain"], "byte");
        let text = Scene::Text(
            TextNode::new("ab".to_string(), Rect::new(0, 0, 8, 8))
                .with_tag("t")
                .with_runs(vec![StyleRun::new(0, 1, TextStyle::new()).named("n")]),
        );
        let value = json(&text, "t", None);
        assert_ne!(value["domain"], "byte");
        assert_eq!(value["domain"], "utf8_byte");
    }

    #[test]
    fn r1615_top_and_the_last_name_are_one_fact() {
        // Two spellings of the same answer, so a client keying off either
        // reads the same thing. They are derived from one list here; the test
        // is what keeps that true if they stop being.
        let scene = dump_scene();
        for index in 0..70 {
            let outcome = marks_outcome(&scene, "dump", Some(index)).expect("tag resolves");
            let at = outcome.at.expect("a position was named");
            assert_eq!(
                at.top.as_deref(),
                at.names.last().map(String::as_str),
                "top and the last name disagree at {index}"
            );
        }
    }
}
