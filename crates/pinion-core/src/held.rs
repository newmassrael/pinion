//! ★★★★★ R1726 §5.21 §5.35 §5.39 §2 #7 — **what a surface is holding paints in
//! front of what it is not.**
//!
//! # What was missing, measured on two running screens
//!
//! Drag a node card over another one in this tree and it goes **underneath**.
//! Measured by paint order — which *is* z-order, since a scene is drawn
//! depth-first and [`Scene::hit_test`](crate::Scene::hit_test) walks the same
//! children in reverse — on both of the tree's node graphs:
//!
//! ```text
//! hello-node-lab     dragged card index 70, the card it is over index 80  -> BEHIND
//! hello-node-editor  dragged node  index 12, the node it is over index 15 -> BEHIND
//! ```
//!
//! The owner reported it as three separate complaints — *the card goes grey*,
//! *they do not overlap*, *is not overlapping better?* — and all three are this
//! one fact. The card does not go grey: the stationary card's opaque body is
//! drawn over it, so only the dim row labels of the held card survive. They do
//! overlap: after the drag the two sit at x=718 and x=720. What is missing is
//! that **the thing you picked up does not look picked up**.
//!
//! # Against the reference toolkit
//!
//! Measured by building a probe against 6.11.1 and running it — two overlapping
//! movable items on a canvas, pressed and dragged:
//!
//! | question | there | here |
//! |---|---|---|
//! | a per-item stacking declaration | **yes**, a z-value per item | the child order, which is the same fact |
//! | paint order and hit order are one fact | **yes** — the front item is what a hit answers | yes, and [`Scene::hit_test`](crate::Scene::hit_test) reads the children in reverse, so it is the *same* order rather than a parallel one |
//! | pressing a movable item raises it | **no** — the back item stayed behind through press, drag and release | [`Held`] |
//! | dragging it raises it | **no** | [`Held`] |
//! | is there a notion of *held* | **no.** `isSelected` and `isUnderMouse` exist; neither means "is being dragged" | [`ContainerNode::held`](crate::scene::ContainerNode::held) |
//! | elevation derives from being held | **no** — a drop shadow is an effect the application adds and removes itself | derived here |
//!
//! The floor therefore supplies a **mechanism** and no **rule**, which is why
//! applications there write the raise themselves — and why two screens of one
//! tree here got it wrong independently. What this module adds is the rule: one
//! declaration, from which the paint order, the hit order and the elevation all
//! follow, so an author cannot get half of it right.

use crate::Color;
use crate::style::BoxShadow;

/// The elevation a held child is given, when the surface does not name one.
///
/// A held card is *above* a resting one, so this is deliberately stronger than
/// the resting elevation a card carries: further offset and more blur read as
/// further from the surface. The numbers are the behaviour reference's resting
/// shadow (`0 4px 14px rgba(0, 0, 0, .28)`) lifted one step, so a screen that
/// reproduces that reference and then picks a card up stays in its visual
/// language rather than jumping into another one.
pub const HELD_SHADOW: BoxShadow = BoxShadow::new(Color::rgba(0, 0, 0, 0x66))
    .with_offset(0.0, 10.0)
    .with_blur(24.0);

/// ★★★★★ What a surface is holding: the children that are being dragged.
///
/// Constructed by [`ContainerNode::with_held`](crate::scene::ContainerNode::with_held)
/// or [`with_held_group`](crate::scene::ContainerNode::with_held_group), which
/// are the only ways to make one — because the declaration is not a note beside
/// the children, it is what **reorders** them. Recording it without moving the
/// children, or moving them without recording it, are the two halves of the
/// defect this exists to remove.
///
/// # Why a set and not one child
///
/// The **second** consumer said so, which is what a second consumer is for.
/// `hello-node-lab` drags one card; `hello-node-editor` drags the whole
/// selection rigidly, so a single-tag declaration would have been wrong on its
/// second use. Held children keep their order relative to each other and move
/// to the front together — a group picked up is one thing, and shuffling it
/// against itself would be an edit nobody asked for.
#[derive(Clone, Debug, PartialEq)]
pub struct Held {
    tags: Vec<String>,
    shadow: BoxShadow,
}

impl Held {
    /// The tags of the children being held, in the order they paint.
    #[must_use]
    pub fn tags(&self) -> &[String] {
        &self.tags
    }

    /// Whether `tag` is one of them.
    #[must_use]
    pub fn holds(&self, tag: &str) -> bool {
        self.tags.iter().any(|t| t == tag)
    }

    /// The elevation they are given.
    #[must_use]
    pub const fn shadow(&self) -> BoxShadow {
        self.shadow
    }

    pub(crate) fn new(tags: Vec<String>, shadow: BoxShadow) -> Self {
        Self { tags, shadow }
    }
}

/// ★★★★★ R1726 — move the held children of `children` to the front and raise
/// them, answering what was actually lifted.
///
/// The derivation itself, so that
/// [`ContainerNode::with_held`](crate::scene::ContainerNode::with_held) and a
/// caller who only ever holds a `Vec<Scene>` cannot drift. **The third consumer
/// is what asked for this**: the analysis tool's dashboard hands its cards
/// straight to a pane helper and never holds a container of its own, so the
/// builder form was unreachable there — and the alternative was that screen
/// hand-rolling the same partition, which is exactly the duplication this
/// capability exists to remove.
///
/// Held children keep their order relative to each other. Tags no child carries
/// are ignored, and lifting nothing answers `None`.
pub fn raise_to_front(
    children: &mut Vec<crate::Scene>,
    tags: &[String],
    shadow: BoxShadow,
) -> Option<Held> {
    use crate::Scene;

    let mut kept: Vec<Scene> = Vec::with_capacity(children.len());
    let mut lifted: Vec<Scene> = Vec::new();
    let mut held: Vec<String> = Vec::new();
    for mut child in std::mem::take(children) {
        let matched = child.tag().is_some_and(|tag| tags.iter().any(|w| w == tag));
        if !matched {
            kept.push(child);
            continue;
        }
        held.push(child.tag().unwrap_or_default().to_owned());
        // The elevation goes on the CHILD: what is raised is the card, and a
        // shadow on whatever is holding it would be a shadow around the whole
        // surface.
        match &mut child {
            Scene::Box(node) => node.style.shadows.push(shadow),
            Scene::Container(node) => node.style.shadows.push(shadow),
            _ => {}
        }
        lifted.push(child);
    }
    kept.extend(lifted);
    *children = kept;
    (!held.is_empty()).then(|| Held::new(held, shadow))
}

#[cfg(test)]
mod tests {
    use super::HELD_SHADOW;
    use crate::Scene;
    use crate::scene::{ContainerNode, Rect};

    /// Two overlapping cards, the first declared behind the second — the shape
    /// both of this tree's node graphs paint.
    ///
    /// The rects are set directly rather than laid out: the claim under test is
    /// about ORDER, and computing a layout would drag in a crate this one sits
    /// below.
    fn canvas() -> ContainerNode {
        let card = |tag: &'static str, x: u32| {
            let mut node = ContainerNode::new(Vec::new()).with_tag(tag);
            node.rect = Rect::new(x, 10, 120, 60);
            Scene::Container(node)
        };
        let mut root = ContainerNode::new(vec![card("card.a", 10), card("card.b", 60)]);
        root.rect = Rect::new(0, 0, 400, 200);
        root
    }

    fn order(node: &ContainerNode) -> Vec<&str> {
        node.children.iter().filter_map(Scene::tag).collect()
    }

    #[test]
    fn r1726_the_held_child_paints_in_front_of_its_siblings() {
        let at_rest = canvas();
        assert_eq!(order(&at_rest), vec!["card.a", "card.b"]);
        assert!(at_rest.held().is_none(), "nothing is held at rest");

        let holding = canvas().with_held("card.a");
        assert_eq!(
            order(&holding),
            vec!["card.b", "card.a"],
            "the held card moves to the END of the children, which is where the \
             front is: a scene paints depth-first"
        );
    }

    /// ★★★★★ The property that makes the reorder sufficient rather than half a
    /// fix: the hit test reads the SAME order, in reverse, so the child that
    /// paints in front is also the one a press finds first. Asserted through
    /// `hit_test` rather than by reading the order back, because "they are the
    /// same order" is the claim.
    #[test]
    fn r1726_what_paints_in_front_is_what_a_press_finds() {
        let scene = Scene::Container(canvas().with_held("card.a"));
        // A point inside BOTH cards.
        let hit = scene
            .hit_test(70, 30)
            .expect("the point is over both cards");
        let deepest = scene
            .lookup_path_ref(&hit.segments)
            .and_then(Scene::tag)
            .map(str::to_owned);
        assert_eq!(
            deepest.as_deref(),
            Some("card.a"),
            "the held card is what the press finds, because the hit test walks \
             the children in reverse and the held one is last"
        );
    }

    #[test]
    fn r1726_being_held_is_what_raises_it() {
        let holding = canvas().with_held("card.a");
        let held = holding.held().expect("the surface says what it holds");
        assert_eq!(held.tags(), ["card.a".to_owned()]);
        assert!(held.holds("card.a") && !held.holds("card.b"));
        assert_eq!(held.shadow(), HELD_SHADOW);

        let raised = holding
            .children
            .iter()
            .find(|c| c.tag() == Some("card.a"))
            .and_then(Scene::box_style)
            .map(|s| s.shadows.clone())
            .unwrap_or_default();
        assert_eq!(
            raised,
            vec![HELD_SHADOW],
            "the elevation is on the CARD, not on the surface holding it -- a \
             shadow on the surface would be a shadow around the whole canvas"
        );
        let resting = holding
            .children
            .iter()
            .find(|c| c.tag() == Some("card.b"))
            .and_then(Scene::box_style)
            .map(|s| s.shadows.clone())
            .unwrap_or_default();
        assert!(resting.is_empty(), "and only the held one is raised");
    }

    /// Holding something that is not on this surface is not a state to paint,
    /// so it changes nothing and says so — rather than reordering by accident
    /// or recording a tag no reader can resolve.
    #[test]
    fn r1726_holding_something_that_is_not_here_is_not_a_state() {
        let holding = canvas().with_held("card.z");
        assert_eq!(order(&holding), vec!["card.a", "card.b"]);
        assert!(holding.held().is_none());
    }

    /// The record and the order are one act. A test that only checked the order
    /// would pass for a builder that forgot to record, and a reader asking
    /// `held()` would then be told nothing while the picture said otherwise.
    #[test]
    fn r1726_the_record_and_the_order_cannot_disagree() {
        for tag in ["card.a", "card.b"] {
            let holding = canvas().with_held(tag);
            assert_eq!(
                holding.held().map(|h| h.tags().to_vec()),
                Some(vec![tag.to_owned()]),
            );
            assert_eq!(
                holding.children.last().and_then(Scene::tag),
                Some(tag),
                "what the surface SAYS it holds is what is at the front"
            );
        }
    }

    /// ★★★★★ The shape the SECOND consumer asked for: a selection dragged
    /// rigidly is held as a group.
    ///
    /// `hello-node-editor` moves every selected node together, so a single-tag
    /// declaration would have been wrong on this capability's second use. The
    /// group keeps its own order and travels to the front intact.
    #[test]
    fn r1726_a_group_picked_up_stays_one_thing() {
        let mut root = canvas();
        let mut third = ContainerNode::new(Vec::new()).with_tag("card.c");
        third.rect = Rect::new(110, 10, 120, 60);
        root.children.push(Scene::Container(third));

        // Name them out of order: the picture's order is the scene's, not the
        // caller's argument list.
        let holding = root.with_held_group(["card.c", "card.a"]);
        assert_eq!(
            order(&holding),
            vec!["card.b", "card.a", "card.c"],
            "the two held cards move to the front TOGETHER and keep the order \
             they were declared in"
        );
        let held = holding.held().expect("a group is held");
        assert_eq!(held.tags(), ["card.a".to_owned(), "card.c".to_owned()]);
        assert!(held.holds("card.a") && held.holds("card.c") && !held.holds("card.b"));
    }

    /// A group naming one tag that is not here holds the rest, rather than
    /// refusing or silently holding nothing.
    #[test]
    fn r1726_a_group_holds_the_children_that_are_here() {
        let holding = canvas().with_held_group(["card.a", "card.z"]);
        assert_eq!(order(&holding), vec!["card.b", "card.a"]);
        assert_eq!(
            holding.held().map(|h| h.tags().to_vec()),
            Some(vec!["card.a".to_owned()]),
            "the record names what was actually lifted, so a reader cannot be \
             told about a child that is not on this surface"
        );
    }
}
