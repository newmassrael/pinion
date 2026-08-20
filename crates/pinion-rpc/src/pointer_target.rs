//! `scene/pointer_target` — [`crate::pointer_reach`] for the things painted
//! INSIDE a surface.
//!
//! ★★★★★ R1700 §5.15 §5.35 §2 #7.
//!
//! # The hole this closes
//!
//! [`crate::pointer_reach`] answers *can a press land on this widget*, over the
//! **registered widgets** — the things the router resolves by name. §2 #7 makes
//! a pinion screen ONE `External` so an agent can query it, so the router's
//! roster for a whole screen has one entry and the hit test lives in the
//! screen's own code.
//!
//! The framework's pointer guarantee therefore stops at the surface boundary.
//! Measured at R1700 on the analyser's capture viewer: the screen paints **291**
//! tagged rectangles and `scene/pointer_reach` vouches for **one** of them. The
//! other 290 were on the screen's honour, and the honour was not kept — a resize
//! reflowed the paint while the hit test went on resolving against the size the
//! screen was designed at, so every one of the **166** rectangles that moved
//! stopped being pressable where it was drawn. Reported by a person twice. Green
//! in every gate both times, because an in-process fixture paints and hit-tests
//! inside one owner scope, where the two halves cannot disagree.
//!
//! # What this reads
//!
//! Three derivations, and the framework holds all three:
//!
//! * the **paint** — every tagged rectangle, absolute and clipped, from the last
//!   rendered frame ([`Scene::absolute_rects_by_tag`]);
//! * [`pinion_core::external::External::target_at`] at the centre of each of
//!   those rectangles, in the surface's own coordinates — what a PRESS there
//!   addresses;
//! * [`pinion_core::external::External::target_of_tag`] for the same tag —
//!   what that rectangle addresses BY NAME.
//!
//! The paint decides where to ask; the two answers decide the verdict. Neither
//! question is the framework's opinion about the screen, and neither is derived
//! from the other, which is what keeps the check from comparing a screen with
//! itself.
//!
//! # The verdicts — `pointer_reach`'s vocabulary, one level down
//!
//! | by name | inside its own rectangle | verdict | |
//! |---|---|---|---|
//! | a word | the same word at the CENTRE, and no probe astray | [`Deliverable`] | pressable where it is drawn |
//! | a word | the same word somewhere else inside | [`Handle`] | it is gripped by a strip |
//! | nothing | something, at the centre | [`Covering`] | decoration over what it decorates |
//! | nothing | nothing | [`Inert`] | a caption, a rule, a badge |
//! | **a word** | **some probe resolves where nothing is drawn** | [`Astray`] | **the defect** |
//! | **a word** | **nowhere inside** | [`Unreachable`] | **the defect** |
//!
//! [`Deliverable`]: TargetVerdict::Deliverable
//! [`Handle`]: TargetVerdict::Handle
//! [`Inert`]: TargetVerdict::Inert
//! [`Covering`]: TargetVerdict::Covering
//! [`Astray`]: TargetVerdict::Astray
//! [`Unreachable`]: TargetVerdict::Unreachable
//!
//! [`Unreachable`] is what the capture viewer's resize produced: 166 rectangles
//! that answered by name and were addressable at no point inside themselves.
//!
//! ## Why the question is "somewhere inside" and not "at the centre"
//!
//! ★★★★★ The centre was the first draft, and this gate's FIRST RUN refuted it
//! on the node lab. A host frame paints one rectangle around the whole group,
//! and its grip is the tab strip along the top — deliberately, because a group
//! that took presses over its own members would make a card undraggable the
//! moment it joined one. So the frame's centre addresses a card, correctly, and
//! the centre rule called that a defect.
//!
//! The honest guarantee for a reader is *if you can see it, you can press it*,
//! and that is satisfied by a grip. So a rectangle whose centre does not
//! address it is probed at eight more points — the four edge midpoints and the
//! four corners, each inset — and answers [`Handle`] with the point that
//! worked.
//!
//! ## ★★★★★ R1736 — and why the OTHER eight are now asked even when the centre
//! agrees
//!
//! Until R1736 the eight extra probes only ever RESCUED a rectangle: they ran
//! when the centre disagreed and could turn `Unreachable` into `Handle`. A
//! rectangle whose centre agreed was never asked anything else, so this gate
//! sampled one point per rectangle in the direction that convicts.
//!
//! A person found the hole by hand. On the node lab, at the zoom the screen
//! opens at, a card's centre addressed the card and **all four of its inset
//! corners addressed the canvas behind it** — so a press six pixels inside a
//! painted card panned the diagram instead of picking the card up. Every gate
//! in this tree was green, and every automatic observer in it — this one, the
//! screen's own sweep, and a demo's `click(path = ...)` — aims at the centre,
//! which was the one point that agreed.
//!
//! So the nine are all asked, always, and each answer is judged against the
//! paint rather than against the centre alone: an answer naming something else
//! is sound exactly when the paint puts that something else at that very point.
//! A card's corner answering the pin that overhangs it is the paint agreeing
//! with itself. The same corner answering the canvas behind it is
//! [`Astray`] — the screen resolved a press somewhere it drew nothing.
//!
//! ★ Cost: nine `target_at` calls per painted rectangle instead of one, on a
//! method whose whole job is a point lookup, in a report nothing renders per
//! frame.
//!
//! ### The floor, measured rather than asserted
//!
//! Built and run against the 6.11.1 release. A canvas item that paints exactly
//! the region it declares is pickable at **100% of the pixels it was drawn in**,
//! under four different camera transforms including fractional zooms — measured
//! over 13,500, 9,450, 11,718 and 9,919 points. That is the floor, and it is
//! **above** where this tree stood: the defect above is a screen of ours failing
//! a property a mature toolkit keeps.
//!
//! What the floor cannot do is notice when the two come apart. An item there
//! declares its draw box and its pick shape **separately**, and the same probe
//! measured an item whose paint reaches six pixels past its pick shape: 15.4% of
//! its painted pixels at zoom 100 and 84, and 30.1% at 135, resolve to *nothing
//! at all*, while the framework holds both rectangles, hands out the larger one
//! as the item's bounds, and never compares them. There is no verdict, no
//! count, and nothing to ask. That is the half this module is.
//!
//! # ★★★ What this census stops being able to see, on a screen that resolves
//! from its paint
//!
//! Stated because a later reader will otherwise take a clean report as evidence
//! of something it no longer tests. This census compares two answers —
//! [`target_of_tag`] by name and [`target_at`] by point — and it is only a
//! COMPARISON while the screen derives them separately. R1736 made the node
//! lab's diagram resolve a press by looking up what it painted and mapping that
//! tag through the very function `target_of_tag` calls, so for those families
//! the two answers now come from one derivation, and geometry drift there is
//! unrepresentable rather than merely unreported.
//!
//! That is the better state — this whole module exists because a repository
//! that repaired the drift by hand four times kept the structure that produced
//! it — but it changes what a green means. On such a screen this census
//! measures **layering**: whether the thing a reader sees on top is the thing a
//! press reaches. What it can no longer measure there is whether the screen
//! knows where it drew something, because the screen no longer has an opinion
//! about that. The screens that still hit-test from a model of their own —
//! which is all the others — are where the original question still bites.
//!
//! [`target_of_tag`]: pinion_core::external::External::target_of_tag
//! [`target_at`]: pinion_core::external::External::target_at
//!
//! # `unanswered` is a third state on purpose
//!
//! [`PointerTarget::Unanswered`] is the default and is not
//! [`PointerTarget::Nothing`]. A surface that does not resolve presses is not a
//! surface that resolved every press to nothing; collapsing them would let a
//! screen nobody checked read as a screen that checked out — the shape R1691
//! names "a total is satisfied by declaring everything silent". `unanswered`
//! names those surfaces, and `deliverable` is published per surface so a
//! surface that answers `Nothing` for everything reports zero rather than
//! passing.
//!
//! # The floor
//!
//! The mature retained-mode toolkits this project is judged against cannot
//! answer any of this. Measured at 6.11, offscreen: a self-painting widget's
//! eight painted marks are invisible to the framework's point lookup, which
//! answers null; the scene-graph point lookup trusts an item's *declared* shape
//! and finds nothing where a paint drew outside it; and no member enumerates
//! what a widget painted, because the only framework-held record of a paint
//! there is pixels, which carry no identity. Introspection-from-paint is what
//! makes the comparison possible here at all.
//!
//! # Wire form
//!
//! ```json
//! { "jsonrpc": "2.0", "method": "scene/pointer_target", "id": 1 }
//! ```

use std::collections::BTreeMap;

use pinion_core::scene::Rect;
use pinion_core::{Scene, external::PointerTarget};
use serde::Serialize;
use serde_json::Value;

use crate::RpcError;
use crate::resolve::painted_surfaces;

/// How a painted rectangle's two answers stand against each other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetVerdict {
    /// Addressable by name, and a press at its own centre addresses the same
    /// thing. The strong form.
    Deliverable,
    /// Addressable by name, and by a press somewhere inside it but not at its
    /// centre — a group whose grip is its tab strip. `at` names the point that
    /// worked.
    Handle,
    /// Addressable by neither — a caption, a rule, a badge.
    Inert,
    /// Not addressable by name, and a press at its centre reaches something
    /// that is: decoration over what it decorates.
    Covering,
    /// ★★★★★ R1736 — addressable by name, and some point inside it resolves to
    /// something **the paint does not put there**. The defect a centre sample
    /// cannot see.
    ///
    /// Not the same failure as [`Self::Unreachable`], and worth its own word:
    /// an unreachable rectangle is one nobody can press, which a reader
    /// discovers immediately. An astray one presses *sometimes*, depending on
    /// where inside it the pointer landed, which is what a person reports as
    /// "it works and then it doesn't".
    Astray,
    /// Addressable by name, and addressable at **no point inside its own
    /// painted rectangle**. The defect.
    Unreachable,
}

impl TargetVerdict {
    /// Whether this verdict is a defect — published so the rule has one version
    /// rather than one per consumer.
    #[must_use]
    pub fn is_defect(self) -> bool {
        matches!(self, Self::Unreachable | Self::Astray)
    }
}

/// One painted rectangle, and the two things its surface says about it.
#[derive(Debug, Clone, Serialize)]
pub struct TargetRow {
    /// The painted tag.
    pub tag: String,
    /// The point the verdict is about, in window coordinates — the rectangle's
    /// centre, or for a [`TargetVerdict::Handle`] the probe that reached it. A
    /// caller can press exactly here.
    pub x: u32,
    /// The point the verdict is about, in window coordinates.
    pub y: u32,
    /// What the surface says this tag addresses, or `null` for nothing.
    pub by_name: Option<String>,
    /// What the surface says a press at the rectangle's CENTRE addresses, or
    /// `null`. Left at the centre even for a `Handle` row, because what the
    /// middle of a group answers is the fact that made it a handle.
    pub at_centre: Option<String>,
    /// ★★★★★ R1736 — for a [`TargetVerdict::Astray`] row, what the point at
    /// (`x`, `y`) resolved to. `null` on every other verdict.
    ///
    /// Published because a defect a caller cannot act on is a number rather
    /// than a report: this names the point AND the wrong answer, so the repair
    /// starts from a fact instead of a search.
    pub astray_to: Option<String>,
    /// The verdict.
    pub verdict: TargetVerdict,
}

/// One surface, and the census over the rectangles painted inside it.
#[derive(Debug, Clone, Serialize)]
pub struct SurfaceTargets {
    /// The surface's own tag.
    pub surface: String,
    /// The size this surface was PAINTED at, from the last rendered frame.
    pub painted_size: (u32, u32),
    /// The size the framework last told it and still answers for it
    /// (`pinion_core::external::surface_size`), or `null` where nothing has
    /// been recorded.
    ///
    /// ★★★★★ R1700 — published because a counterfactual PASSED without it.
    /// Falsifying the recorded height left every check on all three screens
    /// green, and the reason is a real gap rather than a weak assertion: none
    /// of these screens has an addressable rectangle whose position depends on
    /// the window's HEIGHT, so the vertical axis of the size question has no
    /// consumer to disagree through. The invariant `announce_external_sizes`
    /// states in its own comment — the size announced and the size a pointer
    /// fraction is a fraction OF are one derivation — was true, claimed, and
    /// checked by nothing.
    pub announced: Option<(u32, u32)>,
    /// Whether it resolves presses to names at all. `false` means every count
    /// below is zero because nobody was asked, which is a different fact from
    /// clean.
    pub answers: bool,
    /// Painted rectangles attributed to this surface.
    pub painted: usize,
    /// … addressable, and pressable at their own centre.
    pub deliverable: usize,
    /// … addressable inside themselves but not at their centre — gripped.
    pub handle: usize,
    /// … addressable by neither name nor press.
    pub inert: usize,
    /// … decoration over something addressable.
    pub covering: usize,
    /// ★★★★★ R1736 — … addressable by name, and resolving somewhere the paint
    /// put nothing at some point inside themselves. A defect.
    pub astray: usize,
    /// … addressable by name and at no point inside themselves. A defect.
    pub unreachable: usize,
    /// Every row, in tag order, so a caller comparing two window sizes reads
    /// one classification rather than re-deriving it.
    pub rows: Vec<TargetRow>,
}

/// The `scene/pointer_target` result.
#[derive(Debug, Clone, Serialize)]
pub struct PointerTargetReport {
    /// One entry per painted `External`, in scene order.
    pub surfaces: Vec<SurfaceTargets>,
    /// Surfaces that do not resolve presses — the part of the screen the
    /// framework still cannot vouch for, named rather than counted as clean.
    pub unanswered: Vec<String>,
    /// Painted rectangles whose two answers disagree, over all surfaces. A pure
    /// function of `surfaces`, published so the rule deciding "is this screen
    /// defective" has one version rather than one per consumer.
    pub defects: usize,
}

/// ★ R1736 — the framework's own predicate, lifted at its third copy: this
/// module, a screen's paint sweep and the paint-region store all asked "does
/// this rectangle hold this pixel", and a hit test is where two copies
/// disagreeing puts a press somewhere nobody aimed.
fn contains(rect: Rect, x: u32, y: u32) -> bool {
    rect.holds(x, y)
}

fn centre(rect: Rect) -> (u32, u32) {
    (rect.x + rect.w / 2, rect.y + rect.h / 2)
}

/// R1700 §5.35 — compute the report from the painted scene and the state scene.
///
/// A screen that has not painted answers with an empty report rather than an
/// error, for the same reason [`crate::pointer_reach`] does: "nothing
/// disagrees" is true of a screen that does not exist yet, and an error here
/// would make the boot-time check impossible to run before the first frame.
///
/// # Errors
///
/// Only [`RpcError::internal_error`], when the report fails to serialise —
/// propagated rather than unwrapped because a panic in a dispatcher takes the
/// whole surface down.
pub fn handle_scene_pointer_target(
    last_paint_scene: Option<&Scene>,
    state_scene: &Scene,
) -> Result<Value, RpcError> {
    let report = last_paint_scene.map_or_else(
        || PointerTargetReport {
            surfaces: Vec::new(),
            unanswered: Vec::new(),
            defects: 0,
        },
        |paint| build(paint, state_scene),
    );
    serde_json::to_value(&report).map_err(|e| RpcError::internal_error(e.to_string()))
}

/// How the answers stand, given what the nine probes found.
///
/// A pure function of the four facts, so the classification can be exercised
/// without a scene — the rule is what the census IS, and a module whose only
/// verification is that its callers happen to be green is the shape this
/// project keeps recording as a debt.
///
/// ★★★★★ R1736 — `astray` outranks everything, including a centre that agrees.
/// That order is the whole point: the defect a person found was a rectangle
/// whose centre agreed and whose corners did not, and any rule that decided on
/// the centre first would have gone on calling it deliverable.
fn verdict_of(
    by_name: &PointerTarget,
    at_centre: &PointerTarget,
    gripped: bool,
    astray: bool,
) -> TargetVerdict {
    match (by_name.word(), at_centre.word()) {
        (None, None) => TargetVerdict::Inert,
        (None, Some(_)) => TargetVerdict::Covering,
        (Some(_), _) if astray => TargetVerdict::Astray,
        (Some(named), Some(hit)) if named == hit => TargetVerdict::Deliverable,
        (Some(_), _) if gripped => TargetVerdict::Handle,
        (Some(_), _) => TargetVerdict::Unreachable,
    }
}

/// ★★★★★ R1736 — the nine points a rectangle is probed at, and the rule is the
/// **framework's** rather than this module's.
///
/// A screen's own paint sweep asks the same question about the same
/// rectangles, and two copies of it would be two definitions of what "the whole
/// rectangle" means — with the screen that disagreed being the one nobody
/// re-derived. Lifted to [`pinion_core::painted::probe_points`], where the
/// reasoning for the nine and for what they still do not cover lives.
///
/// ★ The edge midpoints are what make a group whose grip is a tab strip
/// answerable at all — the first draft probed only the centre and this gate's
/// first run reported the node lab's two host frames as defects, when what it
/// had actually found is that a group is gripped by its top strip and holds its
/// members everywhere else.
use pinion_core::painted::probe_points as probes;

/// ★★★★★ R1736 — one painted rectangle that answers BY NAME, with the surface
/// it was attributed to.
///
/// The census is two passes over these because judging any one probe needs to
/// know where every ANSWER is painted: a single pass could only ever compare a
/// probe with the tag it was probing, which is what made "the answer names
/// something the paint does not put here" unaskable.
struct Candidate<'a> {
    tag: &'a str,
    rect: Rect,
    surface_tag: &'a String,
    surface_rect: Rect,
    by_name: PointerTarget,
}

/// Per surface, the rectangles each addressable WORD is painted in.
///
/// A word and not a tag, because `target_at` answers in the surface's own
/// vocabulary and several painted rectangles can name one thing.
type PaintedWords<'a> = BTreeMap<&'a String, BTreeMap<String, Vec<Rect>>>;

/// FIRST PASS — attribute each painted rectangle to its surface and ask what it
/// addresses by name.
fn candidates_of<'a>(
    painted: &'a std::collections::HashMap<String, Rect>,
    surfaces: &'a [(String, Rect)],
    state_scene: &Scene,
) -> (Vec<Candidate<'a>>, PaintedWords<'a>) {
    // A rectangle belongs to the SMALLEST surface whose own rectangle contains
    // its centre. Stated as a rule rather than left to walk order because a
    // screen with a surface inside a surface would otherwise be attributed by
    // whichever came first, and two orders would give two censuses.
    let owner_of = |x: u32, y: u32| -> Option<&(String, Rect)> {
        surfaces
            .iter()
            .filter(|(_, rect)| contains(*rect, x, y))
            .min_by_key(|(_, rect)| u64::from(rect.w) * u64::from(rect.h))
    };
    let mut ordered: Vec<(&String, &Rect)> = painted.iter().collect();
    ordered.sort_by(|a, b| a.0.cmp(b.0));

    let mut candidates: Vec<Candidate> = Vec::new();
    let mut words: PaintedWords = BTreeMap::new();
    for (tag, rect) in ordered {
        if rect.w == 0 || rect.h == 0 {
            continue;
        }
        let (cx, cy) = centre(*rect);
        let Some((surface_tag, surface_rect)) = owner_of(cx, cy) else {
            continue;
        };
        if surface_tag == tag {
            continue; // a surface is not a thing painted inside itself
        }
        let Some(node) = state_scene.find_external_with_tag(surface_tag) else {
            continue;
        };
        let by_name = node.handle.target_of_tag(tag);
        if !by_name.answered() {
            continue;
        }
        if let Some(word) = by_name.word() {
            words
                .entry(surface_tag)
                .or_default()
                .entry(word.to_owned())
                .or_default()
                .push(*rect);
        }
        candidates.push(Candidate {
            tag,
            rect: *rect,
            surface_tag,
            surface_rect: *surface_rect,
            by_name,
        });
    }
    (candidates, words)
}

/// SECOND PASS — probe all nine points of one rectangle and judge each answer
/// against the paint.
fn judge(
    cand: &Candidate,
    elsewhere: Option<&BTreeMap<String, Vec<Rect>>>,
    state_scene: &Scene,
) -> Option<TargetRow> {
    let node = state_scene.find_external_with_tag(cand.surface_tag)?;
    let surface_rect = cand.surface_rect;
    // ★ `saturating_sub`, because only the tag's CENTRE is known to be inside
    // the surface — a rectangle straddling the surface's left or top edge has
    // probes outside it, and a bare subtraction there is a panic in a
    // dispatcher rather than a wrong answer.
    let ask = |x: u32, y: u32| {
        node.handle.target_at(
            x.saturating_sub(surface_rect.x),
            y.saturating_sub(surface_rect.y),
        )
    };
    let (cx, cy) = centre(cand.rect);
    let at_centre = ask(cx, cy);
    // A grip is a point OTHER than the centre that reaches the tag, and it is
    // only interesting when the centre does not — a rectangle whose centre
    // answers is deliverable, and reporting one of its edges as the point to
    // press would be a worse answer than its middle.
    let centre_agrees = cand.by_name.word().is_some() && at_centre.word() == cand.by_name.word();
    let mut grip = None;
    let mut astray: Option<(u32, u32, Option<String>)> = None;
    for (n, (px, py)) in probes(cand.rect).into_iter().enumerate() {
        // A probe outside the surface would be asked about a point the surface
        // was never told it owns, and a wrong answer there is the gate's fault
        // rather than the screen's.
        if !contains(surface_rect, px, py) {
            continue;
        }
        let got = if n == 0 {
            at_centre.clone()
        } else {
            ask(px, py)
        };
        if got.word().is_some() && got.word() == cand.by_name.word() {
            if grip.is_none() && n > 0 && !centre_agrees {
                grip = Some((px, py));
            }
            continue;
        }
        if astray.is_some() {
            continue;
        }
        // The answer names something else. Sound exactly when the paint puts
        // that something else AT THIS POINT — a card's corner answering the pin
        // that overhangs it is the paint agreeing with itself; the same corner
        // answering the canvas behind it is not.
        let elsewhere_here = got.word().is_some_and(|word| {
            elsewhere.is_some_and(|by_word| {
                by_word
                    .get(word)
                    .is_some_and(|rects| rects.iter().any(|r| contains(*r, px, py)))
            })
        });
        if !elsewhere_here {
            astray = Some((px, py, got.word().map(ToOwned::to_owned)));
        }
    }
    let verdict = verdict_of(&cand.by_name, &at_centre, grip.is_some(), astray.is_some());
    let (at, astray_to) = match &astray {
        Some((px, py, to)) => ((*px, *py), to.clone()),
        None => (grip.unwrap_or((cx, cy)), None),
    };
    Some(TargetRow {
        tag: cand.tag.to_string(),
        x: at.0,
        y: at.1,
        verdict,
        by_name: cand.by_name.word().map(ToOwned::to_owned),
        at_centre: at_centre.word().map(ToOwned::to_owned),
        astray_to,
    })
}

fn build(paint: &Scene, state_scene: &Scene) -> PointerTargetReport {
    let painted = paint.absolute_rects_by_tag();
    let surfaces = painted_surfaces(paint, state_scene);
    let (candidates, painted_words) = candidates_of(&painted, &surfaces, state_scene);

    let mut per_surface: BTreeMap<String, Vec<TargetRow>> = BTreeMap::new();
    for cand in &candidates {
        if let Some(row) = judge(cand, painted_words.get(cand.surface_tag), state_scene) {
            per_surface
                .entry(cand.surface_tag.clone())
                .or_default()
                .push(row);
        }
    }

    let mut unanswered = Vec::new();
    let mut out = Vec::new();
    let mut defects = 0;
    for (surface, rect) in &surfaces {
        let rows = per_surface.remove(surface).unwrap_or_default();
        // A surface with nothing painted inside it would otherwise be
        // indistinguishable from one that declines to answer, so it is asked
        // once at its own centre.
        let answers = !rows.is_empty()
            || state_scene
                .find_external_with_tag(surface)
                .is_some_and(|node| {
                    let (cx, cy) = centre(*rect);
                    node.handle
                        .target_at(cx.saturating_sub(rect.x), cy.saturating_sub(rect.y))
                        .answered()
                });
        if !answers {
            unanswered.push(surface.clone());
        }
        let count = |want: TargetVerdict| rows.iter().filter(|r| r.verdict == want).count();
        let unreachable = count(TargetVerdict::Unreachable);
        let astray = count(TargetVerdict::Astray);
        // ★ Derived from the verdict's own rule rather than re-listed here, so
        // a sixth verdict cannot be a defect in one place and not the other.
        defects += rows.iter().filter(|r| r.verdict.is_defect()).count();
        out.push(SurfaceTargets {
            surface: surface.clone(),
            painted_size: (rect.w, rect.h),
            announced: pinion_core::external::surface_size(surface),
            answers,
            painted: rows.len(),
            deliverable: count(TargetVerdict::Deliverable),
            handle: count(TargetVerdict::Handle),
            inert: count(TargetVerdict::Inert),
            covering: count(TargetVerdict::Covering),
            astray,
            unreachable,
            rows,
        });
    }

    PointerTargetReport {
        surfaces: out,
        unanswered,
        defects,
    }
}

#[cfg(test)]
mod tests {
    use super::{PointerTarget, Rect, TargetVerdict, centre, contains, probes, verdict_of};

    fn word(w: &str) -> PointerTarget {
        PointerTarget::Word(w.to_owned())
    }

    #[test]
    fn r1700_the_six_verdicts_are_the_six_ways_the_answers_can_stand() {
        let nothing = PointerTarget::Nothing;
        // A control: named, and the middle of it addresses the same thing.
        assert_eq!(
            verdict_of(&word("message.0"), &word("message.0"), false, false),
            TargetVerdict::Deliverable
        );
        // A caption: named by nothing, addressing nothing.
        assert_eq!(
            verdict_of(&nothing, &nothing, false, false),
            TargetVerdict::Inert
        );
        // Decoration over a control: the label is not addressable, the row is.
        assert_eq!(
            verdict_of(&nothing, &word("message.0"), false, false),
            TargetVerdict::Covering
        );
        // ★ A group gripped by its tab strip — the case that refuted the first
        // draft of this census on its first run against the node lab. Its
        // centre correctly addresses a card INSIDE it, and a probe on the strip
        // reaches the group.
        assert_eq!(
            verdict_of(&word("frame:host-a"), &word("node:R-01"), true, false),
            TargetVerdict::Handle
        );
        // ★★ The defect, and it is the SAME two answers as the row above with
        // the grip taken away — which is exactly why `gripped` has to be a
        // fact the census establishes rather than a reading of the two words.
        assert_eq!(
            verdict_of(&word("frame:host-a"), &word("node:R-01"), false, false),
            TargetVerdict::Unreachable
        );
        // And the shape a resize produced 166 times: named, addressing nothing.
        assert_eq!(
            verdict_of(&word("byte.7"), &nothing, false, false),
            TargetVerdict::Unreachable
        );
        // ★★★★★ R1736 — the sixth, and the one a centre sample cannot reach:
        // the two answers AGREE at the centre and some other point inside the
        // rectangle resolves where the paint put nothing. A person found this
        // by hand on a card whose middle picked it up and whose corners panned
        // the canvas behind it.
        assert_eq!(
            verdict_of(&word("node:P-01"), &word("node:P-01"), false, true),
            TargetVerdict::Astray
        );
        // ★ And it outranks a grip too: a rectangle reachable somewhere and
        // astray somewhere else is defective, not gripped.
        assert_eq!(
            verdict_of(&word("node:P-01"), &word("canvas"), true, true),
            TargetVerdict::Astray
        );
        // ★ But an UNNAMED rectangle cannot be astray — nothing claimed it
        // addresses anything, so a probe landing elsewhere is what decoration
        // does. Otherwise every caption over a control would be a defect.
        assert_eq!(
            verdict_of(&nothing, &word("message.0"), false, true),
            TargetVerdict::Covering
        );
        assert!(TargetVerdict::Unreachable.is_defect());
        assert!(TargetVerdict::Astray.is_defect());
        for benign in [
            TargetVerdict::Deliverable,
            TargetVerdict::Handle,
            TargetVerdict::Inert,
            TargetVerdict::Covering,
        ] {
            assert!(!benign.is_defect(), "{benign:?} has a benign reading");
        }
    }

    #[test]
    fn r1700_an_unanswered_surface_is_not_an_empty_one() {
        // R1691's rule in its pointer form: a total satisfied by declaring
        // everything silent measures nothing. `Unanswered` must not read as
        // `Nothing` anywhere, or a screen that implements neither method
        // reports as inspected.
        assert!(!PointerTarget::Unanswered.answered());
        assert!(PointerTarget::Nothing.answered());
        assert!(word("x").answered());
        assert_eq!(PointerTarget::Unanswered.word(), None);
        assert_eq!(PointerTarget::Nothing.word(), None);
    }

    #[test]
    fn r1700_every_probe_is_inside_the_rectangle_and_the_centre_is_first() {
        // The probes are what make `Handle` answerable, so a probe outside the
        // rectangle would credit a tag for a press landing on its neighbour.
        for rect in [
            Rect::new(0, 0, 1, 1),
            Rect::new(3, 5, 2, 40),
            Rect::new(10, 10, 16, 12),
            Rect::new(284, 100, 844, 800),
        ] {
            let set = probes(rect);
            assert_eq!(set[0], centre(rect), "the centre is probed first");
            for (x, y) in set {
                assert!(contains(rect, x, y), "probe ({x},{y}) is inside {rect:?}");
            }
        }
    }

    #[test]
    fn r1700_a_tall_group_is_probed_on_the_strip_that_grips_it() {
        // ★ The measurement behind the probe set: a host frame is ~200px tall
        // and its tab is the top ~22px, so a rule that sampled a grid at even
        // fractions would step over the only part of it that answers.
        let frame = Rect::new(100, 200, 300, 220);
        let tab = Rect::new(100, 200, 300, 22);
        assert!(
            probes(frame).into_iter().any(|(x, y)| contains(tab, x, y)),
            "some probe lands on the strip a group is gripped by"
        );
    }
}
