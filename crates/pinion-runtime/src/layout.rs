//! Runtime layout pass (§5.21 R23/R24): translates the §5.3
//! [`LayoutStyle`] sidecars on a [`Scene`] tree into a taffy compute
//! over a flex / block layout tree, then writes the resulting pixel
//! rects back into each node's `rect` field.
//!
//! `pinion-core` stays free of any taffy dependency per the §5.21
//! spec; the wrapper [`LayoutStyle`] enum set is what `pinion-core`
//! exports, and this module owns the translation into `taffy::Style`.
//!
//! Single layout pass per frame; pure with respect to `(scene tree,
//! viewport)` — same inputs produce identical rects. The §6.3 view-fn
//! purity invariant is preserved because nothing in this module
//! observes time or external state.

use pinion_core::scene::{BoxNode, ContainerNode, ExternalNode, ImageNode, PathNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, Display, FlexDirection, JustifyContent, LayoutStyle, SizeValue,
};
use pinion_core::Scene;
use taffy::prelude::{auto, length, percent, AvailableSpace, NodeId, Size as TaffySize, TaffyTree};
use taffy::style::{
    AlignItems as TaffyAlign, Dimension, Display as TaffyDisplay,
    FlexDirection as TaffyFlexDir, JustifyContent as TaffyJustify, Style as TaffyStyle,
};

/// Compute the layout of `scene` against the given viewport extents.
///
/// Mutates each node's `rect` field in place; nothing else is
/// touched. Safe to call every frame.
///
/// # Panics
///
/// Panics if taffy reports a tree-construction error; this can only
/// happen on internal logic bugs (passing invalid `NodeId`s, etc.),
/// not on any user-supplied scene shape.
#[allow(clippy::cast_precision_loss)]
pub fn compute_layout(scene: &mut Scene, viewport_w: u32, viewport_h: u32) {
    let mut tree: TaffyTree<()> = TaffyTree::new();
    let layout_tree = build(scene, &mut tree);
    // Force the root to fill the viewport. The user's declared size
    // on the root is ignored at the top level; child sizing is the
    // user's domain. This mirrors how browsers treat `<html>`.
    let mut root_style = tree
        .style(layout_tree.node)
        .expect("root style query failed")
        .clone();
    root_style.size = TaffySize {
        width: length(viewport_w as f32),
        height: length(viewport_h as f32),
    };
    tree.set_style(layout_tree.node, root_style)
        .expect("set root style failed");
    let available = TaffySize {
        width: AvailableSpace::Definite(viewport_w as f32),
        height: AvailableSpace::Definite(viewport_h as f32),
    };
    tree.compute_layout(layout_tree.node, available)
        .expect("taffy compute_layout failed");
    apply(scene, &layout_tree, &tree, 0.0, 0.0);
}

/// Recursive shadow tree mirroring the Scene; each entry holds the
/// taffy `NodeId` and the children we registered for that node.
struct LayoutShadow {
    node: NodeId,
    children: Vec<LayoutShadow>,
}

fn build(scene: &Scene, tree: &mut TaffyTree<()>) -> LayoutShadow {
    let style = to_taffy_style(layout_style_of(scene));
    let children = match scene {
        Scene::Container(c) => c.children.iter().map(|s| build(s, tree)).collect(),
        _ => Vec::new(),
    };
    let child_ids: Vec<NodeId> = children.iter().map(|c| c.node).collect();
    let node = if child_ids.is_empty() {
        tree.new_leaf(style).expect("taffy new_leaf failed")
    } else {
        tree.new_with_children(style, &child_ids)
            .expect("taffy new_with_children failed")
    };
    LayoutShadow { node, children }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn apply(
    scene: &mut Scene,
    shadow: &LayoutShadow,
    tree: &TaffyTree<()>,
    parent_x: f32,
    parent_y: f32,
) {
    let layout = tree
        .layout(shadow.node)
        .expect("taffy layout query failed");
    let abs_x = parent_x + layout.location.x;
    let abs_y = parent_y + layout.location.y;
    let rect = Rect::new(
        abs_x.max(0.0) as u32,
        abs_y.max(0.0) as u32,
        layout.size.width.max(0.0) as u32,
        layout.size.height.max(0.0) as u32,
    );
    let _ = assign_rect(scene, rect);

    if let Scene::Container(c) = scene {
        for (child, shadow_child) in c.children.iter_mut().zip(&shadow.children) {
            apply(child, shadow_child, tree, abs_x, abs_y);
        }
    }
}

fn layout_style_of(scene: &Scene) -> &LayoutStyle {
    static FALLBACK: LayoutStyle = LayoutStyle::new();
    match scene {
        Scene::Box(n) => &n.layout,
        Scene::Text(n) => &n.layout,
        Scene::Path(n) => &n.layout,
        Scene::Image(n) => &n.layout,
        Scene::Container(n) => &n.layout,
        Scene::External(n) => &n.layout,
        // Effect + future non-exhaustive variants default to identity
        // layout (block, auto sizing). They participate in the flex
        // tree as zero-size leaves until a follow-up slice opts them
        // in explicitly.
        _ => &FALLBACK,
    }
}

/// Apply a rect to whichever variant carries one. Returns `false`
/// for variants without a `rect` field (Effect today; future
/// `non_exhaustive` additions) so the caller can skip them cleanly.
fn assign_rect(scene: &mut Scene, rect: Rect) -> bool {
    match scene {
        Scene::Box(BoxNode { rect: r, .. })
        | Scene::Text(TextNode { rect: r, .. })
        | Scene::Path(PathNode { rect: r, .. })
        | Scene::Image(ImageNode { rect: r, .. })
        | Scene::Container(ContainerNode { rect: r, .. })
        | Scene::External(ExternalNode { rect: r, .. }) => {
            *r = rect;
            true
        }
        _ => false,
    }
}

#[allow(
    clippy::cast_precision_loss,
    clippy::field_reassign_with_default,
    clippy::match_same_arms,
)]
fn to_taffy_style(layout: &LayoutStyle) -> TaffyStyle {
    let mut s = TaffyStyle::default();
    s.display = match layout.display {
        Display::Block => TaffyDisplay::Block,
        Display::Flex => TaffyDisplay::Flex,
        _ => TaffyDisplay::Block,
    };
    s.flex_direction = match layout.flex_direction {
        FlexDirection::Row => TaffyFlexDir::Row,
        FlexDirection::Column => TaffyFlexDir::Column,
        _ => TaffyFlexDir::Row,
    };
    s.justify_content = Some(match layout.justify_content {
        JustifyContent::Start => TaffyJustify::Start,
        JustifyContent::Center => TaffyJustify::Center,
        JustifyContent::End => TaffyJustify::End,
        JustifyContent::SpaceBetween => TaffyJustify::SpaceBetween,
        JustifyContent::SpaceAround => TaffyJustify::SpaceAround,
        _ => TaffyJustify::Start,
    });
    s.align_items = Some(match layout.align_items {
        AlignItems::Stretch => TaffyAlign::Stretch,
        AlignItems::Start => TaffyAlign::Start,
        AlignItems::Center => TaffyAlign::Center,
        AlignItems::End => TaffyAlign::End,
        _ => TaffyAlign::Stretch,
    });
    s.gap = TaffySize {
        width: length(layout.gap as f32),
        height: length(layout.gap as f32),
    };
    s.size = TaffySize {
        width: to_dimension(layout.size.width),
        height: to_dimension(layout.size.height),
    };
    s.flex_grow = layout.flex_grow;
    s
}

#[allow(clippy::cast_precision_loss, clippy::match_same_arms)]
fn to_dimension(value: SizeValue) -> Dimension {
    match value {
        SizeValue::Auto => auto(),
        SizeValue::Px(n) => length(n as f32),
        SizeValue::Percent(p) => percent(f32::from(p) / 100.0),
        _ => auto(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::external::StubExternal;
    use pinion_core::scene::ExternalNode;
    use pinion_core::style::{Color, FlexDirection, JustifyContent, Size};

    #[test]
    fn block_root_fills_viewport() {
        // A Container with Display::Block (default) and Auto size
        // expands to the viewport bounds taffy was given.
        let mut scene = Scene::Container(ContainerNode::new(vec![]));
        compute_layout(&mut scene, 320, 200);
        let Scene::Container(c) = &scene else {
            panic!("expected container")
        };
        assert_eq!(c.rect.w, 320);
        assert_eq!(c.rect.h, 200);
    }

    #[test]
    fn flex_row_centers_single_fixed_child() {
        // Container = Flex Row, justify_content=Center, align_items=Center.
        // Child = fixed 160x80. Expected center position in 320x200
        // viewport: (80, 60).
        let child = Scene::Box(
            BoxNode::filled(Rect::default(), Color::default())
                .with_layout(LayoutStyle::new().with_size(Size::px(160, 80))),
        );
        let layout = LayoutStyle::new()
            .flex(FlexDirection::Row)
            .with_justify(JustifyContent::Center)
            .with_align_items(AlignItems::Center);
        let mut scene = Scene::Container(ContainerNode::new(vec![child]).with_layout(layout));
        compute_layout(&mut scene, 320, 200);
        let Scene::Container(c) = &scene else {
            panic!("expected container")
        };
        assert_eq!(c.rect.w, 320);
        assert_eq!(c.rect.h, 200);
        let Scene::Box(b) = &c.children[0] else {
            panic!("expected box child")
        };
        assert_eq!(b.rect.w, 160);
        assert_eq!(b.rect.h, 80);
        assert_eq!(b.rect.x, 80);
        assert_eq!(b.rect.y, 60);
    }

    #[test]
    fn flex_column_distributes_two_children() {
        // Column flex with two leaves of fixed height; gap=10.
        // Expected: first child at y=0, second at y=80+10=90.
        let layout = LayoutStyle::new()
            .flex(FlexDirection::Column)
            .with_gap(10);
        let leaf = |h: u32| {
            Scene::Box(
                BoxNode::filled(Rect::default(), Color::default())
                    .with_layout(LayoutStyle::new().with_size(Size::px(100, h))),
            )
        };
        let mut scene = Scene::Container(
            ContainerNode::new(vec![leaf(80), leaf(60)]).with_layout(layout),
        );
        compute_layout(&mut scene, 200, 200);
        let Scene::Container(c) = &scene else {
            panic!("container")
        };
        let Scene::Box(a) = &c.children[0] else {
            panic!("first child")
        };
        let Scene::Box(b) = &c.children[1] else {
            panic!("second child")
        };
        assert_eq!(a.rect.y, 0);
        assert_eq!(a.rect.h, 80);
        assert_eq!(b.rect.y, 90);
        assert_eq!(b.rect.h, 60);
    }

    #[test]
    fn external_node_participates_with_explicit_size() {
        let layout = LayoutStyle::new()
            .flex(FlexDirection::Row)
            .with_justify(JustifyContent::Center)
            .with_align_items(AlignItems::Center);
        let ext = Scene::External(
            ExternalNode::new(Box::new(StubExternal::new()))
                .with_layout(LayoutStyle::new().with_size(Size::px(64, 32))),
        );
        let mut scene = Scene::Container(ContainerNode::new(vec![ext]).with_layout(layout));
        compute_layout(&mut scene, 200, 200);
        let Scene::Container(c) = &scene else {
            panic!("container")
        };
        let Scene::External(e) = &c.children[0] else {
            panic!("external")
        };
        assert_eq!(e.rect.w, 64);
        assert_eq!(e.rect.h, 32);
        assert_eq!(e.rect.x, 68); // (200 - 64) / 2
        assert_eq!(e.rect.y, 84); // (200 - 32) / 2
    }
}
