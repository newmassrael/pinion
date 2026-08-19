//! ★★★★★ R1731 §5.32 §5.40 §2 #7 — **read a specified surface back out of the
//! scene that was painted**, so a conformance check is about pixels rather than
//! about a table somebody could edit to make it pass.
//!
//! # Why the framework owns this
//!
//! [`conformance`](crate::conformance) says what a surface must be and compares
//! it with what a build declares. What it cannot do is get the second half from
//! the *paint*, and that is the half that matters: a screen whose tables say
//! seven columns and whose painter draws six passes every table-side check ever
//! written.
//!
//! R1730 wrote this walk inside the first screen that needed it. The second
//! screen needs the same walk over a different stem, and a verbatim copy is how
//! a mechanism becomes two — with the hazard [`screen_ink`](super::screen_ink)
//! already records for the ink metric: two screens measuring differently would
//! **disagree about the same defect**, and the one nobody ran would be the one
//! that was wrong.
//!
//! # The two rules, both derived
//!
//! **Which tags belong to a surface.** The parts under `stem` whose remainder
//! holds no further dot. A derivation rather than a list of exclusions, and
//! R1728 is why: a rail gate that named the chrome it had to skip was only as
//! good as whoever last updated the list, and it went wrong the first time a
//! seat grew a child. A part's own decoration is tagged *inside* it, so it is
//! excluded by the shape of its name rather than by being remembered.
//!
//! **What order they are in.** Reading order: parts whose painted rectangles
//! overlap vertically are on one line and sort across it; lines sort down the
//! screen. One rule for every kind of surface — a row of parts, a column of
//! them, and a two-by-two grid of single facts, which is two parts on one line
//! twice.
//!
//! ★ The naive `(y, x)` sort was wrong and R1730's gate said so on its first
//! run: a section header's three parts were vertically CENTRED against
//! different heights, so the filter box's top edge sat seven pixels above the
//! title's and it sorted first. Aligning the boxes to make the sort work would
//! have been fixing the screen to suit the check.

use crate::conformance::Part;
use crate::scene::{Rect, Scene};

/// Every tag in `scene` under `stem` whose remainder names a part, paired with
/// the rectangle the layout pass gave it.
///
/// Window-absolute, so two parts in different containers still compare.
#[must_use]
pub fn painted_parts(scene: &Scene, stem: &str) -> Vec<(String, Rect)> {
    let mut found: Vec<(String, Rect)> = Vec::new();
    scene.for_each_node(&mut |visit| {
        let (Some(rect), Some(tag)) = (visit.absolute_rect(), visit.node.tag()) else {
            return;
        };
        let Some(key) = tag.strip_prefix(stem) else {
            return;
        };
        if key.is_empty() || key.contains('.') {
            return;
        }
        if found.iter().any(|(seen, _)| seen == key) {
            return;
        }
        found.push((key.to_owned(), rect));
    });
    found
}

/// The same parts, in **reading order**.
#[must_use]
pub fn in_reading_order(mut parts: Vec<(String, Rect)>) -> Vec<(String, Rect)> {
    parts.sort_by_key(|(key, rect)| (rect.y, rect.x, key.clone()));
    let mut ordered: Vec<(String, Rect)> = Vec::with_capacity(parts.len());
    let mut line: Vec<(String, Rect)> = Vec::new();
    let mut bottom = 0;
    for (key, rect) in parts {
        if !line.is_empty() && rect.y >= bottom {
            line.sort_by_key(|(_, r)| r.x);
            ordered.append(&mut line);
            bottom = 0;
        }
        bottom = bottom.max(rect.y + rect.h);
        line.push((key, rect));
    }
    line.sort_by_key(|(_, r)| r.x);
    ordered.append(&mut line);
    ordered
}

/// ★★★★★ One surface, as the paint has it: the parts under `stem`, in reading
/// order, each titled by the running screen's own table.
///
/// `titles` answers what a key is called. It is the screen's table rather than
/// the paint because most parts of a record pane carry no label a reader sees —
/// the specification fixes that they are THERE and in what order, and a build
/// decides how it draws them. Where a title *is* drawn (a column header), a
/// screen holds the painted text against the specification separately; this
/// function has no way to know which of those a surface is.
///
/// A key the paint has and the table does not is given a title that says so
/// rather than an empty string, so the difference reports as a rename with a
/// readable right-hand side instead of as a blank.
#[must_use]
pub fn painted_surface(
    scene: &Scene,
    stem: &str,
    titles: &dyn Fn(&str) -> Option<String>,
) -> Vec<Part> {
    in_reading_order(painted_parts(scene, stem))
        .into_iter()
        .map(|(key, _)| {
            let title =
                titles(&key).unwrap_or_else(|| format!("<{key} is painted and no table names it>"));
            Part::new(key, title)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{in_reading_order, painted_parts, painted_surface};
    use crate::scene::{ContainerNode, Rect, Scene, TextNode};
    use crate::style::{LayoutStyle, Size, TextStyle};

    /// A tagged mark whose rectangle is already the one a layout pass would
    /// have given it.
    ///
    /// ★ Text nodes rather than containers, and the first draft of this fixture
    /// is why: a container's rectangle comes OUT of the layout pass, so a scene
    /// built here and never laid out answers `None` at every node and the walk
    /// found nothing. `pinion-core` cannot run that pass — the layout engine is
    /// downstream of it — so the fixture supplies rectangles the way a screen's
    /// own text runs do, which is also the shape most parts of a real surface
    /// have.
    fn at(tag: &str, rect: Rect) -> Scene {
        Scene::Text(
            TextNode::styled(tag.to_owned(), rect, TextStyle::default()).with_tag(tag.to_owned()),
        )
    }

    fn scene(children: Vec<Scene>) -> Scene {
        Scene::Container(
            ContainerNode::new(children)
                .with_layout(LayoutStyle::new().with_size(Size::px(1000, 800))),
        )
    }

    /// A part's own decoration is tagged inside it and is excluded by the shape
    /// of its name — no list of things to skip.
    #[test]
    fn a_parts_own_decoration_is_not_a_part() {
        let painted = painted_parts(
            &scene(vec![
                at("s.title", Rect::new(10, 0, 40, 10)),
                at("s.title.underline", Rect::new(10, 8, 40, 2)),
                at("s.filter", Rect::new(80, 0, 40, 10)),
                at("elsewhere.title", Rect::new(0, 0, 5, 5)),
            ]),
            "s.",
        );
        assert_eq!(
            painted.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(),
            ["title", "filter"],
        );
    }

    /// ★★★★★ The rule the naive sort got wrong: parts centred against different
    /// heights are on ONE line and sort across it.
    #[test]
    fn parts_that_overlap_vertically_are_one_line() {
        // A title 18 tall at y=14, a summary 16 tall at y=15, and a box 32 tall
        // at y=7 — the shape a vertically centred header row actually has. A
        // raw `(y, x)` sort answers filter, title, summary.
        let ordered = in_reading_order(vec![
            ("title".to_owned(), Rect::new(16, 14, 120, 18)),
            ("summary".to_owned(), Rect::new(148, 15, 260, 16)),
            ("filter".to_owned(), Rect::new(600, 7, 210, 32)),
        ]);
        assert_eq!(
            ordered.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(),
            ["title", "summary", "filter"],
        );
    }

    /// And a two-by-two grid of single facts comes out in the order it is drawn,
    /// without this having to know which kind of surface it is looking at.
    #[test]
    fn a_grid_of_pairs_reads_across_then_down() {
        let ordered = in_reading_order(vec![
            ("rate".to_owned(), Rect::new(160, 200, 139, 50)),
            ("by".to_owned(), Rect::new(16, 140, 139, 50)),
            ("matches".to_owned(), Rect::new(16, 200, 139, 50)),
            ("direction".to_owned(), Rect::new(160, 140, 139, 50)),
        ]);
        assert_eq!(
            ordered.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(),
            ["by", "direction", "matches", "rate"],
        );
    }

    /// A key the paint has and no table names reports as itself rather than as
    /// a blank, so the difference is readable.
    #[test]
    fn a_part_no_table_names_says_so() {
        let built = painted_surface(
            &scene(vec![at("s.stray", Rect::new(0, 0, 10, 10))]),
            "s.",
            &|_| None,
        );
        assert!(
            built[0].title.contains("no table names it"),
            "{:?}",
            built[0].title,
        );
    }
}
