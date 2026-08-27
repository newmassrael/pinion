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

/// The same run, at an address — which is what decides its **site**.
///
/// ★★★★★ R1870 — and the absence of this is why the fixtures below could not
/// see the defect that round repaid. An untagged run is addressed by its path,
/// whose segments are positions, so forty of them fold to ONE site: the file's
/// bound test was building forty runs at one site and reading them as forty
/// distinct defects. That is exactly the shape a real screen has — a table's
/// cells — and asserting against the disguise is how a population hides.
fn run_at(tag: &str, content: &str, px: u32, short: u32) -> Scene {
    let needs = pinion_core::containment::line_box(px);
    let rect = Rect::new(0, 0, 200, needs.saturating_sub(short));
    Scene::Text(
        TextNode::styled(content, rect, face(px))
            .with_tag(tag.to_string())
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(rect.x, rect.y)
                    .with_size(Size::px(rect.w, rect.h)),
            ),
    )
}

/// A distinct NAME per index — never a number, which would fold to one site.
fn distinct_name(n: usize) -> String {
    let letter = |v: usize| char::from(b'a' + u8::try_from(v).expect("a value below 26"));
    format!("{}{}", letter(n / 26), letter(n % 26))
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
    // ⚠ R1870 — forty SITES, and the names have to be NAMES. Written as forty
    // untagged runs this asserted nothing about the bound (they were one site,
    // and the emitter says one correct line about that); written as `row.{n}`
    // it would assert nothing either, because a numbered segment is a position
    // and folds the same way.
    let many: Vec<Scene> = (0..40)
        .map(|n| {
            run_at(
                &format!("field.{}", distinct_name(n)),
                &format!("paging {n}"),
                10,
                5,
            )
        })
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
    // ⚠ R1870 — twelve SITES declared before it, so this holds the ordering
    // BETWEEN sites. At one site the same property is a different mechanism
    // (which run speaks for the site) and has its own test below.
    let mut runs: Vec<Scene> = (0..12)
        .map(|n| {
            run_at(
                &format!("field.{}", distinct_name(n)),
                &format!("SIZE {n}"),
                10,
                5,
            )
        })
        .collect();
    runs.push(run_at("field.zz", "paging", 10, 5));
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
    assert_eq!(
        said[0].field("content"),
        Some("paging"),
        "and it is FIRST, not merely present"
    );
}

/// ★★★★★ R1870 — the budget buys KINDS of mistake, not repetitions of one.
///
/// Measured R1870 on the analysis-tool shell's dashboard, eight of the ten
/// lines went to one table's cells, so every other kind of short box on that
/// frame reached the reader only inside a count. This fixture is that shape, in
/// the small.
///
/// ⚠ The quantities are a command — `cargo test -p hello-analyzer-shell
/// r1870_the_short_box_census -- --nocapture` — and the reason they are is that
/// R1870's first draft wrote hand-read log figures into this comment and the
/// same round's re-measurement found every one of them wrong.
#[test]
fn r1870_one_repeated_site_cannot_spend_the_whole_budget() {
    let mut seen = std::collections::HashSet::new();
    // A table's cells: twenty runs, one authoring mistake.
    let mut runs: Vec<Scene> = (0..20)
        .map(|n| {
            run_at(
                &format!("card.packet#0.cell.{n}_1"),
                &format!("store/{n}"),
                10,
                4,
            )
        })
        .collect();
    // And three other kinds, declared after them — which is where walk order
    // put them out of reach.
    for (i, tag) in ["shell.status", "rail.item.label", "chart.axis.tick"]
        .into_iter()
        .enumerate()
    {
        runs.push(run_at(tag, &format!("SIZE {i}"), 10, 4));
    }
    let scene = screen(runs);
    let said = heard(|| pinion_shell::warn_about_short_boxes_in(&scene, &mut seen));

    let spelled: Vec<&Said> = said
        .iter()
        .filter(|s| s.field("content").is_some())
        .collect();
    assert_eq!(
        spelled.len(),
        4,
        "twenty-three runs at four sites is four lines, not twenty-three: {said:#?}"
    );
    let sites: std::collections::HashSet<&str> =
        spelled.iter().filter_map(|s| s.field("site")).collect();
    assert_eq!(sites.len(), 4, "each line speaks for a different site");
    for tag in ["shell.status", "rail.item.label", "chart.axis.tick"] {
        assert!(
            spelled.iter().any(|s| s.field("at") == Some(tag)),
            "{tag} was declared after twenty cells and must still be heard"
        );
    }
    // ★ And the line for the repeated site says what repairing it retires.
    let cells = spelled
        .iter()
        .find(|s| s.field("site") == Some("card.packet#*.cell.*"))
        .expect("the table's cells are one site");
    assert_eq!(
        cells.field("at_this_site"),
        Some("20"),
        "a reader cannot act on a site without knowing how much sits there"
    );
    assert!(
        said.iter().all(|s| s.field("counted").is_none()),
        "nothing was past the bound here, so nothing should be counted: {said:#?}"
    );
}

/// ★ Inside one site, the run a reader could SEE cut is the one that speaks.
#[test]
fn r1870_the_run_that_speaks_for_a_site_is_the_one_a_reader_can_see() {
    let mut seen = std::collections::HashSet::new();
    let mut runs: Vec<Scene> = (0..6)
        .map(|n| run_at(&format!("row.{n}.name"), &format!("SIZE {n}"), 10, 4))
        .collect();
    runs.push(run_at("row.9.name", "paging", 10, 4));
    let scene = screen(runs);
    let said = heard(|| pinion_shell::warn_about_short_boxes_in(&scene, &mut seen));

    let spelled: Vec<&Said> = said
        .iter()
        .filter(|s| s.field("content").is_some())
        .collect();
    assert_eq!(spelled.len(), 1, "seven runs at one site is one line");
    assert_eq!(spelled[0].field("site"), Some("row.*.name"));
    assert_eq!(
        spelled[0].field("content"),
        Some("paging"),
        "six runs with no descender were declared before the one with it"
    );
    assert_eq!(spelled[0].field("at_this_site"), Some("7"));
}

/// ★★★★★ R1870 — the summary counts SITES it did not spell, not only runs.
///
/// A count of runs past the bound cannot tell a reader whether the tail is one
/// more mistake repeated a hundred times or a hundred more mistakes.
#[test]
fn r1870_the_summary_says_how_many_kinds_it_did_not_spell() {
    let mut seen = std::collections::HashSet::new();
    let runs: Vec<Scene> = (0..14)
        .map(|n| {
            run_at(
                &format!("field.{}", distinct_name(n)),
                &format!("paging {n}"),
                10,
                4,
            )
        })
        .collect();
    let scene = screen(runs);
    let said = heard(|| pinion_shell::warn_about_short_boxes_in(&scene, &mut seen));

    let tail = said.last().expect("fourteen sites overrun a bound of ten");
    assert_eq!(tail.field("spelled"), Some("10"));
    assert_eq!(tail.field("counted"), Some("14"));
    assert_eq!(tail.field("sites"), Some("14"));
    assert_eq!(
        tail.field("sites_unspelled"),
        Some("4"),
        "the tail must say how many KINDS went unsaid: {tail:#?}"
    );
}

/// ★★★★★ R1870 — the tail counts **the lines it did not print**, and every
/// other fixture in this file is blind to whether it does.
///
/// The first draft of this round subtracted a count of LINES from a count of
/// RUNS (`fresh - spelled`) and fired whenever a frame held more runs than
/// lines. Both are inherited from before grouping, when a line *was* a run, and
/// both agree with the truth while every site holds exactly one run — which
/// every fixture above happens to arrange. So this one does not: thirty-five
/// runs at ten spelled sites and three unspelled ones holding two runs each.
/// The honest answer is **6 at 3**; the old arithmetic says 25.
#[test]
fn r1870_the_tail_counts_the_runs_at_the_sites_it_did_not_spell() {
    let mut seen = std::collections::HashSet::new();
    // One table, twenty runs, one site — and visible, so it is spelled.
    let mut runs: Vec<Scene> = (0..20)
        .map(|n| {
            run_at(
                &format!("card.packet#0.cell.{n}_1"),
                &format!("paging {n}"),
                10,
                4,
            )
        })
        .collect();
    // Nine more visible sites, one run each: ten spelled in all.
    for n in 0..9 {
        runs.push(run_at(
            &format!("field.{}", distinct_name(n)),
            "paging",
            10,
            4,
        ));
    }
    // And three sites of two runs each, whose cut a reader could not see, so
    // they sort last and fall past the bound.
    for n in 0..3 {
        for i in 0..2 {
            runs.push(run_at(
                &format!("tail.{}.row.{i}", distinct_name(n)),
                "SIZE",
                10,
                4,
            ));
        }
    }
    let scene = screen(runs);
    let said = heard(|| pinion_shell::warn_about_short_boxes_in(&scene, &mut seen));

    let spelled = said.iter().filter(|s| s.field("content").is_some()).count();
    assert_eq!(spelled, 10, "the bound is a bound: {said:#?}");
    let tail = said.last().expect("three sites went unspelled");
    assert_eq!(tail.field("sites_unspelled"), Some("3"));
    assert_eq!(
        tail.field("runs_unspelled"),
        Some("6"),
        "the tail must count the runs at the sites it did not spell, not \
         subtract a line count from a run count: {tail:#?}"
    );
    assert!(
        tail.message.contains("6 more box(es)"),
        "and the sentence a reader reads must carry that number: {:?}",
        tail.message
    );
    // The identity the line can be checked on by itself.
    assert_eq!(tail.field("sites"), Some("13"));
    assert_eq!(tail.field("spelled"), Some("10"));
    assert_eq!(
        tail.field("counted"),
        Some("35"),
        "`counted` stays every fresh run on the frame — the R1863 contract"
    );
}

/// ★★★★★ R1870 — the prescription is a whole sentence, and it is actionable.
///
/// The sentence shipped from R1863 to R1869 reading `` `containment::line_rect_in`
/// is a box that cannot be`` — it stopped mid-clause, so the one line telling a
/// reader what to DO about a short box never said it. A person reading a real
/// boot log found it; nothing in the tree could, because every assertion about
/// this message asked whether a substring was present.
#[test]
fn r1870_the_line_finishes_the_sentence_it_starts() {
    let mut seen = std::collections::HashSet::new();
    let scene = screen(vec![run_at("shell.status", "paging", 10, 5)]);
    let said = heard(|| pinion_shell::warn_about_short_boxes_in(&scene, &mut seen));
    let one = &said[0];

    assert!(
        !one.message.trim_end().ends_with("cannot be"),
        "the prescription is cut off mid-clause: {:?}",
        one.message
    );
    // What a reader must be able to act on WITHOUT another tool: the height to
    // give the box, and the constructor that cannot get it wrong.
    assert!(
        one.message.contains("give it 17px"),
        "the line does not say what height would hold it: {:?}",
        one.message
    );
    assert!(
        one.message.contains("line_rect_in"),
        "the line does not name the constructor: {:?}",
        one.message
    );
    // ★ And the address must RESOLVE. The real boot's first line named its
    // subject `2` — the run's position among its siblings.
    assert_eq!(one.field("at"), Some("shell.status"));
}

/// ★ R1870 — a site speaks for a run that is NEW, never for one already said.
#[test]
fn r1870_a_site_speaks_for_a_run_the_reader_has_not_heard() {
    let mut seen = std::collections::HashSet::new();
    let first = screen(vec![run_at("row.0.name", "paging", 10, 4)]);
    let heard_first = heard(|| pinion_shell::warn_about_short_boxes_in(&first, &mut seen));
    assert_eq!(heard_first[0].field("content"), Some("paging"));

    // The same site, with the run already said still present and a new one
    // beside it. The old run sorts FIRST (it has the descender), so a site that
    // spoke through its best run rather than its best NEW run would repeat
    // itself and never mention the arrival.
    let then = screen(vec![
        run_at("row.0.name", "paging", 10, 4),
        run_at("row.1.name", "SIZE", 10, 4),
    ]);
    let heard_then = heard(|| pinion_shell::warn_about_short_boxes_in(&then, &mut seen));
    assert_eq!(
        heard_then.len(),
        1,
        "one new run, one line: {heard_then:#?}"
    );
    assert_eq!(
        heard_then[0].field("content"),
        Some("SIZE"),
        "the site repeated the run it had already said instead of the new one"
    );
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

/// ★★★★★ R1871 — **the report is a function of the frame, not of the order the
/// walk met it.**
///
/// R1863 spelled the ten runs it met FIRST: `visible.chain(rest).take(10)` over
/// a scene walk, so which ten a reader was shown depended on where in the tree
/// somebody had declared them. R1870 replaced that with a comparator ending in
/// the site's own name, which is total across distinct sites — so the SET of
/// sites stopped depending on walk order. Nothing asserted it, and the half one
/// level in was left: inside a site the rows sort on visibility then shortfall,
/// a stable sort, so two rows tied on both fall back to the order they were
/// met, and **which run speaks for the site** flips with the declaration order.
///
/// A report two runs of one screen can disagree about is one nobody can quote,
/// and the disagreement is invisible precisely because both runs are correct.
/// So this asserts the whole report — message and every field — is identical
/// when the same frame is built in the opposite order.
#[test]
fn r1871_the_same_frame_reports_the_same_thing_in_any_declaration_order() {
    // Two rows of one site, tied on everything the ordering looks at: same
    // site, same visibility, same shortfall. Only their addresses differ.
    let rows = || {
        vec![
            run_at("row.0.name", "paging", 10, 4),
            run_at("row.1.name", "paging", 10, 4),
            // And a second site, so the BETWEEN-site order is exercised too.
            run_at("card.aa.total", "SIZE", 10, 4),
            run_at("card.bb.total", "SIZE", 10, 4),
        ]
    };
    let forward = screen(rows());
    let mut backward_rows = rows();
    backward_rows.reverse();
    let backward = screen(backward_rows);

    // A fresh `seen` for each, or the second frame is deduped into silence.
    let mut heard_once = std::collections::HashSet::new();
    let mut heard_twice = std::collections::HashSet::new();
    let as_declared = heard(|| pinion_shell::warn_about_short_boxes_in(&forward, &mut heard_once));
    let reversed = heard(|| pinion_shell::warn_about_short_boxes_in(&backward, &mut heard_twice));

    assert!(
        !as_declared.is_empty(),
        "the fixture must actually produce a report, or this asserts nothing",
    );
    let render = |said: &[Said]| -> Vec<(String, Vec<(String, String)>)> {
        said.iter()
            .map(|s| {
                let mut fields = s.fields.clone();
                fields.sort();
                (s.message.clone(), fields)
            })
            .collect()
    };
    assert_eq!(
        render(&as_declared),
        render(&reversed),
        "the same frame reported differently depending on the order its runs \
         were declared in\nforward: {as_declared:#?}\nbackward: {reversed:#?}",
    );
}
