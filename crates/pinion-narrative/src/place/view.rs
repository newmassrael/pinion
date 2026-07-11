//! The projection: a solved [`PlaceLayout`] → a queryable pinion [`Scene`].
//!
//! Boxes for places, an enclosing box per container, and a stroked line
//! per adjacency. Like the narrative projection this is plain structured
//! scene data (§2 #1 / #7) — an AI reads the map's topology and geometry,
//! not pixels — and it renders on GUI and TUI from the one structure.
//! Paint order is containers (back) → adjacency lines → place boxes
//! (front) so labels are never occluded.

use std::borrow::Cow;

use pinion_core::Color;
use pinion_core::scene::{
    BoxNode, ContainerNode, PathCommand, PathNode, PathPoint, Rect, Scene, TextNode,
};
use pinion_core::style::{Border, BoxStyle, PathStyle, Stroke};

use crate::place::layout::PlaceLayout;

/// Outer margin added to the map's content extent.
const MARGIN: u32 = 24;

const NODE_FILL: Color = Color::rgb(0x28, 0x2c, 0x38);
const NODE_BORDER: Color = Color::rgb(0xa0, 0xc0, 0xff);
const CONTAINER_FILL: Color = Color::rgb(0x18, 0x18, 0x20);
const CONTAINER_BORDER: Color = Color::rgb(0x55, 0x55, 0x66);
const EDGE_COLOR: Color = Color::rgb(0x70, 0x78, 0x90);

/// Project the solved layout into a retained scene.
#[must_use]
pub fn place_map_scene(layout: &PlaceLayout) -> Scene {
    if layout.is_empty() {
        let mut node = ContainerNode::default();
        node.rect = Rect::new(0, 0, 400, 80);
        node.children
            .push(text_row("(장소가 없습니다)", 16, 16, 360));
        return Scene::Container(node);
    }

    let mut children: Vec<Scene> = Vec::new();

    // 1. Container boxes — largest first so a nested box sits on top.
    let mut containers: Vec<(usize, Rect)> = (0..layout.nodes.len())
        .filter(|&i| layout.is_container(i))
        .map(|i| (i, layout.container_bounds(i)))
        .collect();
    containers.sort_by(|a, b| area(b.1).cmp(&area(a.1)).then(a.0.cmp(&b.0)));
    for (idx, bounds) in containers {
        children.push(box_node(bounds, CONTAINER_FILL, CONTAINER_BORDER, None));
        children.push(text_row(
            &format!("\u{25a3} {}", layout.nodes[idx].label),
            bounds.x + 8,
            bounds.y + 4,
            bounds.w.saturating_sub(12),
        ));
    }

    // 2. Adjacency lines, behind the place boxes.
    for [a, b] in &layout.edges {
        children.push(edge_line(layout.nodes[*a].rect, layout.nodes[*b].rect));
    }

    // 3. Place boxes + labels on top, tagged by place id (scene-as-data).
    for node in &layout.nodes {
        children.push(box_node(
            node.rect,
            NODE_FILL,
            NODE_BORDER,
            Some(node.id.clone()),
        ));
        children.push(text_row(
            &node.label,
            node.rect.x + 10,
            node.rect.y + node.rect.h.saturating_sub(16) / 2,
            node.rect.w.saturating_sub(16),
        ));
    }

    let (w, h) = map_extent(layout);
    let mut root = ContainerNode::default();
    root.rect = Rect::new(0, 0, w, h);
    root.children = children;
    Scene::Container(root)
}

fn box_node(rect: Rect, fill: Color, border: Color, tag: Option<String>) -> Scene {
    let mut node = BoxNode::new(
        rect,
        BoxStyle::filled(fill).with_border(Border::new(border, 1)),
    );
    node.tag = tag.map(Cow::Owned);
    Scene::Box(node)
}

fn edge_line(a: Rect, b: Rect) -> Scene {
    let (ax, ay) = center(a);
    let (bx, by) = center(b);
    Scene::Path(PathNode::new(
        segment_bounds(a, b),
        vec![
            PathCommand::MoveTo(PathPoint::new(ax, ay)),
            PathCommand::LineTo(PathPoint::new(bx, by)),
        ],
        PathStyle::stroked(Stroke::new(EDGE_COLOR, 2)),
    ))
}

fn text_row(content: &str, x: u32, y: u32, w: u32) -> Scene {
    Scene::Text(TextNode::new(
        content.to_string(),
        Rect::new(x, y, w.max(1), 16),
    ))
}

fn area(rect: Rect) -> u32 {
    rect.w.saturating_mul(rect.h)
}

/// Integer centre of a rect (used for the adjacency-line endpoints and its
/// bounding rect).
fn center_i(rect: Rect) -> (u32, u32) {
    (rect.x + rect.w / 2, rect.y + rect.h / 2)
}

/// Floating-point centre for the absolute [`PathPoint`] endpoints.
#[allow(clippy::cast_precision_loss)] // map coordinates are small (< 2^24), exact in f32.
fn center(rect: Rect) -> (f32, f32) {
    let (x, y) = center_i(rect);
    (x as f32, y as f32)
}

fn segment_bounds(a: Rect, b: Rect) -> Rect {
    let (ax, ay) = center_i(a);
    let (bx, by) = center_i(b);
    Rect::new(
        ax.min(bx),
        ay.min(by),
        ax.abs_diff(bx).max(1),
        ay.abs_diff(by).max(1),
    )
}

fn map_extent(layout: &PlaceLayout) -> (u32, u32) {
    let mut right = 0;
    let mut bottom = 0;
    for (i, node) in layout.nodes.iter().enumerate() {
        let r = if layout.is_container(i) {
            layout.container_bounds(i)
        } else {
            node.rect
        };
        right = right.max(r.x + r.w);
        bottom = bottom.max(r.y + r.h);
    }
    (right + MARGIN, bottom + MARGIN)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::place::layout::solve_layout;
    use crate::place::model::{Adjacency, Direction, Place, PlaceGraph};

    fn collect(scene: &Scene) -> (Vec<String>, usize, usize) {
        // (text contents, box count, path count)
        let mut text = Vec::new();
        let mut boxes = 0;
        let mut paths = 0;
        walk(scene, &mut text, &mut boxes, &mut paths);
        (text, boxes, paths)
    }

    fn walk(scene: &Scene, text: &mut Vec<String>, boxes: &mut usize, paths: &mut usize) {
        match scene {
            Scene::Text(t) => text.push(t.content.clone()),
            Scene::Box(_) => *boxes += 1,
            Scene::Path(_) => *paths += 1,
            Scene::Container(c) => {
                for child in &c.children {
                    walk(child, text, boxes, paths);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn projects_boxes_labels_and_one_line_per_edge() {
        let graph = PlaceGraph {
            places: vec![
                Place {
                    id: "village".to_string(),
                    label: "마을".to_string(),
                    contained_by: None,
                },
                Place {
                    id: "shrine".to_string(),
                    label: "굿당".to_string(),
                    contained_by: Some("village".to_string()),
                },
                Place {
                    id: "mudflat".to_string(),
                    label: "갯벌".to_string(),
                    contained_by: None,
                },
            ],
            adjacencies: vec![Adjacency {
                from: "village".to_string(),
                to: "mudflat".to_string(),
                direction: Some(Direction::East),
            }],
        };
        let layout = solve_layout(&graph);
        let (text, boxes, paths) = collect(&place_map_scene(&layout));

        // 3 place boxes + 1 container box (village).
        assert_eq!(boxes, 4);
        // 1 adjacency line.
        assert_eq!(paths, 1);
        let joined = text.join("\n");
        assert!(joined.contains("마을"));
        assert!(joined.contains("굿당"));
        assert!(joined.contains("갯벌"));
    }

    #[test]
    fn empty_graph_projects_a_valid_scene() {
        let layout = solve_layout(&PlaceGraph::default());
        let (text, _, _) = collect(&place_map_scene(&layout));
        assert!(text.iter().any(|t| t.contains("장소가 없습니다")));
    }
}
