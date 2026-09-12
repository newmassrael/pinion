//! ★★★★★ R2152 §5.40 §5.50 — **which controls a reader cannot find**, which is
//! WCAG 1.4.11's question and which neither half of this tree could ask alone.
//!
//! # The two halves, and why the join had to be built
//!
//! SC 1.4.11 holds one thing to `3:1`:
//!
//! > Visual information **required to identify** user interface components and
//! > states.
//!
//! That is a conjunction of two facts about one mark, and this repository holds
//! them in two crates that had never met:
//!
//! * **required to identify** — a property of the painted frame. A box edge
//!   owes the ratio exactly when the box has nothing else to be found by;
//!   [`pinion_core::legibility::stroke_census`] derives that from the scene.
//! * **user interface component** — a property of the ROLE, declared in the
//!   accessibility tree a screen already publishes and readable through
//!   [`AriaRole::is_user_interface_component`].
//!
//! Apart, each half answers a question the standard did not ask. The census
//! alone answers *how many boxes have a faint edge*, and a card, a legend
//! swatch and a group header all count. The tree alone answers *what controls
//! are here*, and says nothing about whether they are visible. Only the
//! intersection is the standard's population — and the cost of not having it is
//! measured: a debt about a palette's boundary contrast stayed open for 313
//! rounds arguing over a total of box edges, because *the number of controls a
//! reader cannot find* was not a number anybody could produce.
//!
//! # What a consumer does with the answer
//!
//! ⚠ **Read it as a set to PIN, not as a total to drive to zero** — the same
//! rule [`pinion_core::legibility::shortfalls`] carries and for the same
//! reason. A palette is a design decision and several of the tones a screen
//! paints with arrive authored from outside this repository, so a gate
//! demanding an empty set would be demanding the right to change somebody
//! else's colours. What a gate can honestly assert is WHICH controls are in it,
//! so the day a new one joins, it is red — and the day a value is raised, the
//! set empties and the gate says which change did it.
//!
//! Named rather than counted for a second reason, stated on
//! [`AriaRole::is_user_interface_component`]: two of ARIA's widget roles are
//! not *controls* in WCAG's narrower sense, so a total can overstate by two and
//! nobody could tell. A named set can be argued with.

use std::collections::BTreeSet;

use pinion_core::legibility::StrokeCensus;

use crate::node::AccessNode;
use crate::role::AriaRole;

/// One control that is identified by nothing but a mark in the colour a census
/// was taken of.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Unfindable {
    /// The tag the mark and the announced node share — the address a reader of
    /// the report goes to.
    pub tag: String,
    /// The role that makes it a user interface component, so the report says
    /// *a text input* rather than *a rectangle*.
    pub role: AriaRole,
}

impl Unfindable {
    /// The finding as a sentence, for a report or a failure message.
    #[must_use]
    pub fn say(&self) -> String {
        format!("{} ({})", self.tag, self.role.aria_name())
    }
}

/// **Every announced user interface component whose only visual identifier is
/// an [`Identifying`] mark from `census`** — WCAG 1.4.11's population, in tag
/// order.
///
/// `announced` is what a screen publishes for the frame the census was taken
/// over; `census` is [`stroke_census`] for the colour in question. A component
/// appears here when the tree says it is a control AND the frame says the only
/// thing separating its box from what is behind it is a mark in that colour. If
/// that colour is short of [`Floor::Boundary`], every member is a control a
/// low-vision reader is not guaranteed to find.
///
/// # Why the match is on the exact tag
///
/// A component's announced tag is the tag its own surface is painted under —
/// that identity is what [`resolve_access_bounds`](crate::resolve_access_bounds)
/// already relies on to give a node its rectangle, so a looser match would be a
/// second, weaker convention beside a working one.
///
/// ⚠ Its consequence, stated rather than hidden: a component painted as several
/// fragments is seen only through the fragment carrying its own tag, so this
/// **under**-reports rather than over-reports. That is the safe direction for a
/// finding — a set that is too small is a repair somebody still owes, and a set
/// that is too large is a repair somebody wastes a round refusing.
///
/// [`Identifying`]: pinion_core::legibility::StrokeKind::Identifying
/// [`stroke_census`]: pinion_core::legibility::stroke_census
/// [`Floor::Boundary`]: pinion_core::legibility::Floor::Boundary
#[must_use]
pub fn components_identified_only_by(
    announced: &[AccessNode],
    census: &StrokeCensus,
) -> Vec<Unfindable> {
    let mut found: BTreeSet<Unfindable> = BTreeSet::new();
    for node in announced {
        if node.role.is_user_interface_component() && census.identifying.contains(&node.tag) {
            found.insert(Unfindable {
                tag: node.tag.clone(),
                role: node.role,
            });
        }
    }
    found.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::legibility::{Floor, StrokeCensus, stroke_census};
    use pinion_core::scene::{BoxNode, ContainerNode, Rect, Scene};
    use pinion_core::style::{Border, BoxStyle, Color};

    const GROUND: Color = Color::rgb(0xF6, 0xF7, 0xF9);
    const FAINT_EDGE: Color = Color::rgb(0xC9, 0xD0, 0xD8);

    /// A field-shaped box: white on the near-white page, so its edge is the
    /// only thing there is.
    fn field(tag: &'static str) -> Scene {
        bordered(tag, Color::rgb(0xFF, 0xFF, 0xFF))
    }

    /// A box with the faint edge and whatever fill the case is about.
    fn bordered(tag: &'static str, fill: Color) -> Scene {
        Scene::Box(
            BoxNode::new(
                Rect::new(0, 0, 40, 20),
                BoxStyle::filled(fill).with_border(Border::new(FAINT_EDGE, 1)),
            )
            .with_tag(tag),
        )
    }

    fn page(children: Vec<Scene>) -> Scene {
        let mut outer = ContainerNode::new(children);
        outer.rect = Rect::new(0, 0, 200, 200);
        outer.style = BoxStyle::filled(GROUND);
        Scene::Container(outer)
    }

    /// ★★★★★ The join, and the whole of what it adds: two boxes painted
    /// identically, and only the one the TREE calls a control is a finding.
    #[test]
    fn r2152_only_an_announced_component_is_a_finding() {
        let scene = page(vec![field("search"), field("card")]);
        let census = stroke_census(&scene, FAINT_EDGE, GROUND);
        assert_eq!(census.identifying(), 2, "the frame cannot tell them apart");

        let announced = vec![
            AccessNode::new("search", AriaRole::TextInput),
            AccessNode::new("card", AriaRole::Group),
        ];
        let found = components_identified_only_by(&announced, &census);
        assert_eq!(
            found.iter().map(Unfindable::say).collect::<Vec<_>>(),
            vec!["search (textbox)"],
            "a group is not a user interface component; a textbox is",
        );
    }

    /// ★★★★★ And a control the reader finds by its own fill is NOT a finding,
    /// which is the half the population was missing before R2152.
    ///
    /// Same role, same colour of border, different fill — and the standard
    /// asks nothing of the border on the second, because what identifies that
    /// control is a fill that clears the floor by itself.
    #[test]
    fn r2152_a_control_its_own_fill_identifies_is_not_a_finding() {
        // 4.38 against the page: found before the edge is drawn.
        let filled = bordered("apply", Color::rgb(0x1F, 0x8A, 0x4C));
        let scene = page(vec![field("search"), filled]);
        let census = stroke_census(&scene, FAINT_EDGE, GROUND);
        assert_eq!(census.identifying(), 1);
        assert_eq!(census.redundant(), 1);

        let announced = vec![
            AccessNode::new("search", AriaRole::TextInput),
            AccessNode::new("apply", AriaRole::Button),
        ];
        assert_eq!(
            components_identified_only_by(&announced, &census)
                .iter()
                .map(|f| f.tag.clone())
                .collect::<Vec<_>>(),
            vec!["search".to_owned()],
            "both are controls and only one owes the floor",
        );
    }

    /// An announced control that paints no mark in this colour at all is not a
    /// finding — so a screen cannot be charged for a component the census never
    /// saw.
    #[test]
    fn r2152_a_component_the_census_never_saw_is_not_a_finding() {
        let census = StrokeCensus::default();
        let announced = vec![AccessNode::new("search", AriaRole::TextInput)];
        assert!(components_identified_only_by(&announced, &census).is_empty());
    }

    /// The set is what makes the standard's arithmetic possible: it is empty
    /// exactly when the colour clears the floor OR nothing is identified by it,
    /// and the first of those is the repair a palette decision makes.
    #[test]
    fn r2152_the_floor_the_finding_is_measured_against_is_the_non_text_one() {
        assert_eq!(
            pinion_core::legibility::StrokeKind::Identifying.floor(),
            Some(Floor::Boundary),
        );
        assert!((Floor::Boundary.ratio() - 3.0).abs() < f32::EPSILON);
    }

    /// ★★ Every role is on one side of the line, and the two sides are both
    /// non-empty — so a predicate that answered a constant could not pass.
    #[test]
    fn r2152_the_component_line_partitions_the_whole_vocabulary() {
        let components: Vec<&str> = AriaRole::ALL
            .iter()
            .filter(|r| r.is_user_interface_component())
            .map(|r| r.aria_name())
            .collect();
        let rest: Vec<&str> = AriaRole::ALL
            .iter()
            .filter(|r| !r.is_user_interface_component())
            .map(|r| r.aria_name())
            .collect();
        assert_eq!(
            components.len() + rest.len(),
            AriaRole::ALL.len(),
            "the whole vocabulary is partitioned",
        );
        assert!(!components.is_empty() && !rest.is_empty(), "and both ways");
        // The four the specification is least ambiguous about, named so a
        // wholesale inversion cannot pass by staying balanced.
        for widget in [AriaRole::Button, AriaRole::TextInput, AriaRole::Slider] {
            assert!(
                widget.is_user_interface_component(),
                "{} is an ARIA widget role",
                widget.aria_name(),
            );
        }
        assert!(
            !AriaRole::Heading.is_user_interface_component(),
            "a heading is document structure, not a control",
        );
        println!(
            "[r2152] {} of {} role(s) are user interface components",
            components.len(),
            AriaRole::ALL.len(),
        );
    }
}
