//! R1775 §5.32 — **does the host paint on top of the screen it is showing?**
//!
//! A host with a destination roster hands one of its destinations a region and
//! a guest fills it. Everything the host draws *around* that region — its
//! navigation, its application bar — is chrome the guest is told about through
//! [`HostChrome`](pinion_core::chrome::HostChrome) so it can omit its own.
//! Nothing has ever asked the other question: **does the host draw anything
//! INSIDE the region it gave away?**
//!
//! # Why nothing here could see it
//!
//! The containment gates screens carry ask whether a mark lies inside the box
//! that owns it. A host's floating overlay is its own top-level box, so it is
//! inside itself and every such gate passes. Overlap gates ask whether two
//! marks of ONE screen cover each other. Between the two there is no question
//! that spans the host/guest seam, and the defect that prompted this module
//! lived there: an analysis tool's status toast was positioned against the
//! rectangle its dashboard uses for a canvas, which was the whole page while
//! the host painted every page itself. Once screens were mounted, the region a
//! destination receives became a *different, larger* rectangle, and the toast
//! landed on the guest's own palette. A person saw it; no gate could.
//!
//! # The host is derived from the scene, not from a name
//!
//! A mark belongs to the guest when the guest's node is one of its ancestors,
//! which is a fact about the tree rather than about a tag's spelling. That
//! matters here: a prefix convention would make this check a second place where
//! naming is load-bearing, and this repository has paid for that before.
//!
//! **An ancestor of the guest is not over it.** The window root contains the
//! page and so intersects it in the trivial way; a check that counted that
//! would report every host as covering every guest and would be useless. What
//! is reported is a mark that is neither inside the guest nor a container
//! holding it — something drawn *beside* the page in the tree and *on* it in
//! the window.

use std::collections::BTreeSet;

use pinion_core::Scene;
use pinion_core::scene::Rect;

/// One host mark found lying over one guest mark.
///
/// Both tags and both rectangles, because a report of an overlap owes the
/// reader the overlap: a count cannot be acted on, and this repository spent
/// three wrong repairs on a sibling gate that named a number and not a place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Overlap {
    /// The host's mark — what is on top.
    pub host: String,
    /// The guest's mark it covers.
    pub guest: String,
    /// Where the host's mark is, in window coordinates.
    pub host_rect: Rect,
    /// Where the guest's mark is, in the same coordinates.
    pub guest_rect: Rect,
}

/// Whether two rectangles share any pixel.
fn meets(a: Rect, b: Rect) -> bool {
    !(a.x >= b.x + b.w || b.x >= a.x + a.w || a.y >= b.y + b.h || b.y >= a.y + a.h)
}

/// Every mark of the host that lies over a mark of the guest at `guest_tag`.
///
/// Returns empty when the scene holds no node with that tag: a host showing no
/// guest cannot be covering one. Callers that need to know the guest was
/// actually there should assert that separately — see
/// [`assert_host_clears_guest`], which does.
#[must_use]
pub fn host_marks_over_guest(scene: &Scene, guest_tag: &str) -> Vec<Overlap> {
    let mut guest: Vec<(String, Rect)> = Vec::new();
    let mut host: Vec<(String, Rect)> = Vec::new();
    scene.for_each_node(&mut |visit| {
        let (Some(tag), Some(rect)) = (visit.node.tag(), visit.absolute_rect()) else {
            return;
        };
        let inside_guest = tag == guest_tag
            || visit
                .ancestors
                .iter()
                .any(|a| a.tag().is_some_and(|t| t == guest_tag));
        if inside_guest {
            // The guest's own root is the region itself. Its members are what a
            // host can cover; the region is what the host GAVE, so an overlay
            // over the region alone is not yet evidence of anything.
            if tag != guest_tag {
                guest.push((tag.to_owned(), rect));
            }
        } else {
            host.push((tag.to_owned(), rect));
        }
    });
    // ★★★★★ An ancestor of the guest holds it and does not cover it — and
    // ancestry is read from the TREE, not from the rectangles.
    //
    // The first draft asked whether the mark geometrically contained the
    // guest's region, and the very first run refused the window root: measured,
    // the region a mounted screen receives was `(52, 52, 1425, 848)` in a
    // 1440-wide window, so `52 + 1425 = 1477` puts it OUTSIDE its own root and
    // no ancestor contains it. Geometry cannot answer a question about
    // structure — least of all here, where a region overflowing its window is
    // one of the defects this module exists to help find.
    let held_by = ancestors_of(scene, guest_tag);
    host.retain(|(tag, _)| !held_by.contains(tag.as_str()));

    let mut found = Vec::new();
    for (host_tag, host_rect) in &host {
        for (guest_tag_found, guest_rect) in &guest {
            if meets(*host_rect, *guest_rect) {
                found.push(Overlap {
                    host: host_tag.clone(),
                    guest: guest_tag_found.clone(),
                    host_rect: *host_rect,
                    guest_rect: *guest_rect,
                });
            }
        }
    }
    found
}

/// The tags of every container the guest's node hangs beneath.
///
/// Read from the ancestor chain the walk already carries, because that is what
/// "holds" means. See the note at its one call site for what asking this
/// geometrically cost on the first run.
fn ancestors_of(scene: &Scene, guest_tag: &str) -> BTreeSet<String> {
    let mut held = BTreeSet::new();
    scene.for_each_node(&mut |visit| {
        if visit.node.tag() == Some(guest_tag) {
            for above in visit.ancestors {
                if let Some(tag) = above.tag() {
                    held.insert(tag.to_owned());
                }
            }
        }
    });
    held
}

/// The rectangle the guest's own node occupies, when it is in the scene.
#[must_use]
pub fn region_of(scene: &Scene, guest_tag: &str) -> Option<Rect> {
    let mut found = None;
    scene.for_each_node(&mut |visit| {
        if found.is_none() && visit.node.tag() == Some(guest_tag) {
            found = visit.absolute_rect();
        }
    });
    found
}

/// One host mark found lying over one of the guest's sentences.
///
/// Separate from [`Overlap`] because a text run carries no tag of its own — what
/// a reader lost is the WORDS, and a report that named the box around them would
/// be naming something the reader never saw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Covered {
    /// The host's mark — what is on top.
    pub host: String,
    /// What the guest was saying underneath it.
    pub said: String,
    /// Where the host's mark is, in window coordinates.
    pub host_rect: Rect,
    /// Where the sentence is, in the same coordinates.
    pub said_rect: Rect,
}

/// ★★★★★ R1861 — **every sentence of the guest that a mark of the host lies
/// over.**
///
/// # Why this is not [`host_marks_over_guest`] with a filter
///
/// That one is a RATCHET on which host marks reach the guest's region at all,
/// and it has to be: measured against the behaviour reference, its status toast
/// floats over the content too (`position: fixed; bottom: 22px`) and is
/// tolerable because it *leaves* after 2.6 seconds. Forbidding the overlap
/// outright would forbid what the reference does.
///
/// **Covering a sentence is a different claim, and it is one the reference never
/// makes**: what its toast floats over is empty canvas. So this asks the sharper
/// question, it can be zero, and a screen appearing here is a defect rather than
/// a budget line. Measured on the analysis tool at its shipping size before the
/// repair: the node lab lost the top 6 pixels of its gesture hint and the
/// capture viewer lost two lane readouts entirely.
///
/// A run belongs to the guest when the guest's node is one of its ancestors —
/// read from the tree, for the reason the module header gives.
#[must_use]
pub fn host_marks_over_guest_text(scene: &Scene, guest_tag: &str) -> Vec<Covered> {
    let mut said: Vec<(String, Rect)> = Vec::new();
    let mut host: Vec<(String, Rect)> = Vec::new();
    scene.for_each_node(&mut |visit| {
        let Some(rect) = visit.absolute_rect() else {
            return;
        };
        // ⚠ The guest's own root counts as inside it. Read as ancestry alone
        // this said `packet_view` was a HOST mark lying over every sentence of
        // `packet_view` — a node is not its own ancestor, and the first run of
        // this gate reported exactly that. Its sibling above has carried the
        // `tag == guest_tag` clause since R1775; this one was written without
        // it, which is the same predicate missing the same arm.
        let inside_guest = visit.node.tag() == Some(guest_tag)
            || visit
                .ancestors
                .iter()
                .any(|a| a.tag().is_some_and(|t| t == guest_tag));
        match (inside_guest, visit.node) {
            (true, Scene::Text(text)) => said.push((text.content.clone(), rect)),
            (false, _) => {
                if let Some(tag) = visit.node.tag() {
                    host.push((tag.to_owned(), rect));
                }
            }
            _ => {}
        }
    });
    // An ancestor of the guest holds it and does not cover it — the same
    // structural reading its sibling above uses, and for the same measured
    // reason.
    let held_by = ancestors_of(scene, guest_tag);
    host.retain(|(tag, _)| !held_by.contains(tag.as_str()));

    let mut found = Vec::new();
    for (host_tag, host_rect) in &host {
        for (content, said_rect) in &said {
            if meets(*host_rect, *said_rect) {
                found.push(Covered {
                    host: host_tag.clone(),
                    said: content.clone(),
                    host_rect: *host_rect,
                    said_rect: *said_rect,
                });
            }
        }
    }
    found
}

/// ★★★★★ The whole rule as one call: the guest is on this frame, it drew
/// something, and nothing of the host's lies on top of any of it.
///
/// # Panics
///
/// With the tags and both rectangles named, on any of the three counts. The
/// population clauses come first for the reason every census in this tree
/// carries one: a frame with no guest, or a guest with no marks, satisfies the
/// overlap clause vacuously and would read as *the host keeps out of the page*.
pub fn assert_host_clears_guest(scene: &Scene, guest_tag: &str) {
    let region = region_of(scene, guest_tag);
    assert!(
        region.is_some(),
        "no node tagged `{guest_tag}` is on this frame, so the question of \
         whether the host covers it cannot be asked — a check that passed here \
         would be reporting on a screen that is not showing",
    );
    let over = host_marks_over_guest(scene, guest_tag);
    let drew = {
        let mut n = 0usize;
        scene.for_each_node(&mut |visit| {
            if let Some(tag) = visit.node.tag()
                && tag != guest_tag
                && visit
                    .ancestors
                    .iter()
                    .any(|a| a.tag().is_some_and(|t| t == guest_tag))
            {
                n += 1;
            }
        });
        n
    };
    assert!(
        drew > 0,
        "`{guest_tag}` is on this frame and painted no marks of its own, so \
         nothing of the host's could be over one",
    );
    assert!(
        over.is_empty(),
        "the host paints on top of the screen it is showing — {over:#?}",
    );
}

#[cfg(test)]
mod tests {
    use super::{Overlap, host_marks_over_guest_text, meets};
    use pinion_core::Scene;
    use pinion_core::scene::Rect;

    /// A scene through the layout pass, which is what makes a container answer
    /// [`pinion_core::NodeVisit::absolute_rect`] at all.
    fn laid_out(mut scene: Scene) -> Scene {
        let mut cache = pinion_runtime::LayoutCache::new();
        pinion_runtime::compute_layout(&mut scene, &mut cache, 400, 300);
        scene
    }

    #[test]
    fn two_rectangles_that_share_a_pixel_meet() {
        assert!(meets(Rect::new(0, 0, 10, 10), Rect::new(9, 9, 10, 10)));
    }

    #[test]
    fn rectangles_that_only_touch_edges_do_not_meet() {
        assert!(!meets(Rect::new(0, 0, 10, 10), Rect::new(10, 0, 10, 10)));
        assert!(!meets(Rect::new(0, 0, 10, 10), Rect::new(0, 10, 10, 10)));
    }

    /// ★★★★★ The measurement that settled how ancestry is read here.
    ///
    /// A container holding the page was first identified by asking whether it
    /// geometrically CONTAINED the page, and the first run of this module
    /// against the assembled tool refused that immediately. These are its
    /// numbers: the region a mounted screen received reaches 52 + 1425 = 1477
    /// in a 1440-wide window, so the window root does not contain the page it
    /// holds. Ancestry is structural and is read from the tree — geometry
    /// cannot answer it, least of all here, where a region overflowing its
    /// window is one of the defects this module helps find.
    #[test]
    fn a_holder_can_be_smaller_than_what_it_holds() {
        let root = Rect::new(0, 0, 1440, 900);
        let region = Rect::new(52, 52, 1425, 848);
        assert!(
            region.x + region.w > root.x + root.w,
            "the measured region overflows its root, which is what makes a \
             geometric test for ancestry answer wrongly",
        );
        assert!(meets(root, region), "and it does still overlap it");
    }

    /// A mark that merely straddles the page is exactly what this module
    /// reports, and it must not be mistaken for a container of it.
    #[test]
    fn a_mark_that_straddles_the_page_is_an_overlap_not_a_holder() {
        assert!(meets(Rect::new(0, 0, 20, 20), Rect::new(10, 10, 20, 20)));
    }

    #[test]
    fn an_overlap_carries_both_places() {
        let over = Overlap {
            host: "shell.toast".into(),
            guest: "lab.palette".into(),
            host_rect: Rect::new(76, 842, 560, 34),
            guest_rect: Rect::new(68, 100, 230, 800),
        };
        assert_eq!(over.host, "shell.toast");
        assert!(meets(over.host_rect, over.guest_rect));
    }

    /// ★★★★★ R1861 — **a node is not its own ancestor, and the guest's ROOT is
    /// inside the guest.**
    ///
    /// Read as ancestry alone, [`host_marks_over_guest_text`] called the guest's
    /// own paint root a HOST mark lying over every sentence the guest painted —
    /// which is every sentence, so the answer was "everything is covered". The
    /// sibling predicate has carried the `tag == guest_tag` arm since R1775 and
    /// this one was written without it.
    ///
    /// ⚠ **This test exists because a counterfactual found the arm unguarded.**
    /// Removing it left every gate green: the assembled tool's own check walks
    /// the scene itself, over a wider population, so this function had no
    /// consumer holding it. A framework predicate whose only proof was a caller
    /// that stopped calling it is a predicate nothing is checking.
    #[test]
    fn r1861_the_guests_own_root_is_not_a_host_mark_over_it() {
        use pinion_core::scene::{ContainerNode, TextNode};
        use pinion_core::style::{LayoutStyle, Size};

        let seat = |rect: Rect| {
            LayoutStyle::new()
                .with_absolute_position(rect.x, rect.y)
                .with_size(Size::px(rect.w, rect.h))
        };
        let words = Rect::new(10, 60, 200, 12);
        let said = Scene::Text(
            TextNode::new("a sentence the guest painted", words).with_layout(seat(words)),
        );
        let guest = Scene::Container(
            ContainerNode::new(vec![said])
                .with_tag("guest")
                .with_layout(seat(Rect::new(0, 0, 400, 300))),
        );
        let scene = Scene::Container(ContainerNode::new(vec![guest]).with_tag("window"));

        let scene = laid_out(scene);
        // ⚠ The premise, asserted rather than assumed: a container answers
        // `absolute_rect` only after a layout pass, and without one the guest's
        // root is `None` and drops out of the population before the predicate
        // sees it. The first draft of this test had no layout pass and passed
        // with the arm it exists to hold DELETED — measured by hand, because a
        // counterfactual said so and a green test would not have.
        let mut root = None;
        scene.for_each_node(&mut |visit| {
            if visit.node.tag() == Some("guest") {
                root = visit.absolute_rect();
            }
        });
        assert!(
            root.is_some(),
            "the guest's root has no rectangle, so it is not in the population \
             this test is about"
        );

        let covered = host_marks_over_guest_text(&scene, "guest");
        assert!(
            covered.is_empty(),
            "the guest's own root, or a container holding it, was reported as \
             covering the guest's words: {covered:#?}"
        );
    }

    /// And the direction that must still fire, so the test above is not the
    /// predicate being switched off.
    #[test]
    fn r1861_a_host_mark_over_a_sentence_is_reported_with_the_words() {
        use pinion_core::scene::{BoxNode, ContainerNode, TextNode};
        use pinion_core::style::{BoxStyle, Color, LayoutStyle, Size};

        let seat = |rect: Rect| {
            LayoutStyle::new()
                .with_absolute_position(rect.x, rect.y)
                .with_size(Size::px(rect.w, rect.h))
        };
        let words = Rect::new(10, 60, 200, 12);
        let said = Scene::Text(
            TextNode::new("a sentence the guest painted", words).with_layout(seat(words)),
        );
        let guest = Scene::Container(
            ContainerNode::new(vec![said])
                .with_tag("guest")
                .with_layout(seat(Rect::new(0, 0, 400, 300))),
        );
        let box_seat = Rect::new(50, 55, 120, 30);
        let overlay = Scene::Box(
            BoxNode::new(box_seat, BoxStyle::filled(Color::rgb(1, 2, 3)))
                .with_tag("host.toast")
                .with_layout(seat(box_seat)),
        );
        let scene = laid_out(Scene::Container(
            ContainerNode::new(vec![guest, overlay]).with_tag("window"),
        ));

        let covered = host_marks_over_guest_text(&scene, "guest");
        assert_eq!(covered.len(), 1, "the overlay covers exactly one sentence");
        assert_eq!(covered[0].host, "host.toast");
        assert_eq!(covered[0].said, "a sentence the guest painted");
    }
}
