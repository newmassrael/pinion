//! ★★★★★ R1863 §5.32 §2 #7 — **the paint-time warning about a box too short
//! for its own face actually fires, and says what a reader would need.**
//!
//! # Why this file exists at all
//!
//! `debt-the-paint-time-warning-has-no-test` has been open since R1656, and its
//! sentence is: *R1656 put in a `tracing::warn!` because a person asked to be
//! told immediately, and nothing checks that it fires.* The screens are mostly
//! clean, so the warning is silent in normal running — which means it could be
//! dead in either direction (a flipped condition, a dedup key that never
//! changes, an emptied `cfg` arm) and the tree would look exactly the same.
//!
//! R1863 added a second warning of the same shape, for the defect a reader
//! reported three times in a fortnight. Shipping a second unchecked warning
//! beside an unchecked one would be the debt twice, so this drives the emitter
//! against a scene built to be wrong and asserts what comes out — including the
//! two properties that make it usable rather than merely present: it says the
//! same run **once**, and it never falls silent about the runs it does not
//! spell out.

#![cfg(debug_assertions)]

use std::sync::{Arc, Mutex};

use pinion_core::scene::{ContainerNode, Rect, Scene, TextNode};
use pinion_core::style::{LayoutStyle, Size, TextStyle};
use tracing::field::{Field, Visit};
use tracing::subscriber::with_default;
use tracing_subscriber::Layer;
use tracing_subscriber::layer::SubscriberExt;

/// One warning as a reader would receive it: the message and every field.
#[derive(Debug, Clone, Default)]
struct Said {
    message: String,
    fields: Vec<(String, String)>,
}

impl Said {
    fn field(&self, name: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

#[derive(Default)]
struct Collect(Vec<(String, String)>, String);

impl Visit for Collect {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        let rendered = format!("{value:?}");
        if field.name() == "message" {
            self.1 = rendered;
        } else {
            self.0.push((field.name().to_owned(), rendered));
        }
    }
}

/// A layer that keeps every event on one target, in order.
struct Heard {
    target: &'static str,
    said: Arc<Mutex<Vec<Said>>>,
}

impl<S: tracing::Subscriber> Layer<S> for Heard {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        if event.metadata().target() != self.target {
            return;
        }
        let mut visitor = Collect::default();
        event.record(&mut visitor);
        self.said.lock().expect("not poisoned").push(Said {
            message: visitor.1,
            fields: visitor.0,
        });
    }
}

/// Run `body` with a collector on `pinion::containment` and return what it said.
fn heard(body: impl FnOnce()) -> Vec<Said> {
    let said = Arc::new(Mutex::new(Vec::new()));
    let layer = Heard {
        target: "pinion::containment",
        said: Arc::clone(&said),
    };
    let subscriber = tracing_subscriber::registry().with(layer);
    with_default(subscriber, body);
    said.lock().expect("not poisoned").clone()
}

/// A text style at `px`, which is the only field any of this depends on.
fn face(px: u32) -> TextStyle {
    TextStyle::new().with_size_px(px)
}

/// A run in a box `short` pixels too short for its face.
fn run(content: &str, px: u32, short: u32) -> Scene {
    let needs = pinion_core::containment::line_box(px);
    let rect = Rect::new(0, 0, 200, needs.saturating_sub(short));
    Scene::Text(
        TextNode::styled(content, rect, face(px)).with_layout(
            LayoutStyle::new()
                .with_absolute_position(rect.x, rect.y)
                .with_size(Size::px(rect.w, rect.h)),
        ),
    )
}

fn screen(runs: Vec<Scene>) -> Scene {
    Scene::Container(ContainerNode::new(runs).with_tag("a.box"))
}

#[test]
fn r1863_a_box_too_short_for_its_face_says_so_on_the_frame() {
    let mut seen = std::collections::HashSet::new();
    let scene = screen(vec![run("paging", 10, 5)]);
    let said = heard(|| pinion_shell::warn_about_short_boxes_in(&scene, &mut seen));

    assert_eq!(said.len(), 1, "one short run, one warning: {said:#?}");
    let one = &said[0];
    assert!(
        one.message.contains("too short"),
        "the message does not say what happened: {one:#?}"
    );
    // ★ The fields a reader needs to find it: what it says, how short, and the
    // face — the reader who reported this named the words and the letter.
    assert_eq!(one.field("content"), Some("paging"));
    assert_eq!(one.field("short_by"), Some("5"));
    assert_eq!(one.field("px"), Some("10"));
    assert_eq!(
        one.field("visible"),
        Some("true"),
        "`paging` has a descender, so the cut is one a reader can see"
    );
}

#[test]
fn r1863_the_same_run_is_said_once_however_many_frames_pass() {
    let mut seen = std::collections::HashSet::new();
    let scene = screen(vec![run("paging", 10, 5)]);
    let first = heard(|| pinion_shell::warn_about_short_boxes_in(&scene, &mut seen));
    let again = heard(|| {
        for _ in 0..60 {
            pinion_shell::warn_about_short_boxes_in(&scene, &mut seen);
        }
    });
    assert_eq!(first.len(), 1);
    assert!(
        again.is_empty(),
        "sixty more frames of the same defect said {} more thing(s)",
        again.len()
    );
}

#[test]
fn r1863_a_run_that_gets_worse_is_said_again() {
    let mut seen = std::collections::HashSet::new();
    let mild = screen(vec![run("paging", 10, 2)]);
    let worse = screen(vec![run("paging", 10, 7)]);
    let first = heard(|| pinion_shell::warn_about_short_boxes_in(&mild, &mut seen));
    let second = heard(|| pinion_shell::warn_about_short_boxes_in(&worse, &mut seen));
    assert_eq!(first.len(), 1);
    assert_eq!(
        second.len(),
        1,
        "a box that starts falling FURTHER short is a new fact"
    );
    assert_eq!(second[0].field("short_by"), Some("7"));
}

/// ★★★★★ The property that makes the bound a bound on LINES and not on FACTS.
#[test]
fn r1863_more_short_boxes_than_it_spells_out_are_still_counted() {
    let mut seen = std::collections::HashSet::new();
    let many: Vec<Scene> = (0..40)
        .map(|n| run(&format!("paging {n}"), 10, 5))
        .collect();
    let scene = screen(many);
    let said = heard(|| pinion_shell::warn_about_short_boxes_in(&scene, &mut seen));

    // ⚠ Counted by a FIELD, not by a substring of the message: the summary line
    // also says "too short", so the first spelling of this counted eleven for
    // ten spelled runs and reported the bound broken. A spelled line names the
    // run it is about; the summary names a number.
    let spelled = said.iter().filter(|s| s.field("content").is_some()).count();
    assert!(
        spelled <= 10,
        "{spelled} lines for one frame — the bound is not holding"
    );
    assert_eq!(spelled, 10, "the bound is a bound, not an accident");
    let tail = said
        .last()
        .expect("something was said about forty short boxes");
    assert!(
        tail.field("content").is_none() && tail.field("counted").is_some(),
        "the runs past the bound were dropped instead of counted: {tail:#?}"
    );
    assert_eq!(
        tail.field("counted"),
        Some("40"),
        "the count must be of every fresh run, not of the ones spelled out"
    );
}

/// ★ An ordering, not a permission: a run with no descender is still reported.
#[test]
fn r1863_a_cut_a_reader_could_not_see_is_reported_too() {
    let mut seen = std::collections::HashSet::new();
    let scene = screen(vec![run("SIZE", 10, 5)]);
    let said = heard(|| pinion_shell::warn_about_short_boxes_in(&scene, &mut seen));
    assert_eq!(
        said.len(),
        1,
        "a short box is short whatever letters it holds"
    );
    assert_eq!(said[0].field("visible"), Some("false"));
}

/// ★ And the ones a reader can see come FIRST, which is what the ordering is for.
#[test]
fn r1863_the_runs_a_reader_can_see_are_said_first() {
    let mut seen = std::collections::HashSet::new();
    let mut runs: Vec<Scene> = (0..12).map(|n| run(&format!("SIZE {n}"), 10, 5)).collect();
    runs.push(run("paging", 10, 5));
    let scene = screen(runs);
    let said = heard(|| pinion_shell::warn_about_short_boxes_in(&scene, &mut seen));

    let visible: Vec<&Said> = said
        .iter()
        .filter(|s| s.field("visible") == Some("true"))
        .collect();
    assert_eq!(
        visible.len(),
        1,
        "the one run with a descender must be among the spelled lines even \
         though twelve without one were declared before it"
    );
    assert_eq!(visible[0].field("content"), Some("paging"));
}

/// ★★★★★ R1863 — **and the SIBLING warning fires too**, which is the half of
/// `debt-the-paint-time-warning-has-no-test` that has been open since R1656.
///
/// `warn_about_escaped_marks` needs the font — it shapes each run to ask whether
/// ink left its box — so it cannot be driven the way the one above is. What CAN
/// be driven without a shell is the predicate it stands on
/// (`containment::escapes`, with the ink measurement as a closure), and this
/// asserts that the escape it would report is there to report: a mark outside
/// the box that owns it comes back named, with the edge and the amount.
///
/// ⚠ Stated rather than implied: this holds the PREDICATE, not the emitter. The
/// emitter's own arm still has no test, and this file's other seven hold the
/// pattern that would give it one — a shell-free body plus a collector. That is
/// what the debt's remainder now is, and it is smaller than it was.
#[test]
fn r1863_the_escape_predicate_the_sibling_warning_stands_on_still_reports() {
    let owner = Rect::new(0, 0, 100, 20);
    let inside = Rect::new(10, 4, 40, 12);
    let out = Rect::new(80, 4, 40, 12);
    let mut holder = ContainerNode::new(vec![
        Scene::Text(
            TextNode::styled("in", inside, face(10)).with_layout(
                LayoutStyle::new()
                    .with_absolute_position(inside.x, inside.y)
                    .with_size(Size::px(inside.w, inside.h)),
            ),
        ),
        Scene::Text(
            TextNode::styled("out", out, face(10)).with_layout(
                LayoutStyle::new()
                    .with_absolute_position(out.x, out.y)
                    .with_size(Size::px(out.w, out.h)),
            ),
        ),
    ])
    .with_tag("a.box")
    .with_layout(
        LayoutStyle::new()
            .with_absolute_position(owner.x, owner.y)
            .with_size(Size::px(owner.w, owner.h)),
    );
    // ⚠ The owner's RECT, which is what `escapes` reads — a container with a
    // zero rectangle is a grouping node by that predicate's own rule and makes
    // no promise about where its children go, so it can never be broken. The
    // first spelling of this fixture left it zero and got an empty answer,
    // which is the same failure mode as the missing layout pass above: the
    // check ran and asked nothing.
    holder.rect = owner;
    let scene = Scene::Container(holder);
    // ⚠ No layout pass, deliberately, and the reason is worth the line: this
    // predicate reads `Scene::rect` and the scroll offset rather than
    // `NodeVisit::absolute_rect`, so a layout pass would RECOMPUTE the
    // container's rectangle from its style and lose the one set above. Its
    // sibling in `pinion-screen` needs the pass for the opposite reason. Two
    // predicates about the same seam, two different things to prepare — which
    // is why each fixture asserts that its population is non-empty rather than
    // trusting the shape.
    // The ink measurement the shell supplies from its font cache; here the box
    // itself, so the question is purely about geometry.
    let escapes = pinion_core::containment::escapes(&scene, &mut |t| (t.rect.w, t.rect.h));
    assert!(
        escapes.iter().any(|e| e.content.as_deref() == Some("out")),
        "the mark reaching past its owner was not reported: {escapes:#?}"
    );
    assert!(
        !escapes.iter().any(|e| e.content.as_deref() == Some("in")),
        "a mark inside its owner was reported as an escape: {escapes:#?}"
    );
    let reported = escapes
        .iter()
        .find(|e| e.content.as_deref() == Some("out"))
        .expect("just asserted present");
    assert!(
        reported.over.right > 0,
        "the report names no edge, so a reader cannot act on it: {reported:#?}"
    );
}

#[test]
fn r1863_a_clean_frame_says_nothing() {
    let mut seen = std::collections::HashSet::new();
    let needs = pinion_core::containment::line_box(10);
    let rect = Rect::new(0, 0, 200, needs);
    let scene = screen(vec![Scene::Text(
        TextNode::styled("paging", rect, face(10)).with_layout(
            LayoutStyle::new()
                .with_absolute_position(rect.x, rect.y)
                .with_size(Size::px(rect.w, rect.h)),
        ),
    )]);
    let said = heard(|| pinion_shell::warn_about_short_boxes_in(&scene, &mut seen));
    assert!(said.is_empty(), "a frame with nothing wrong said {said:#?}");
}
