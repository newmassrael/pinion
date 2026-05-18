//! Runtime layout pass (§5.21 R23/R24): translates the §5.3
//! [`LayoutStyle`] sidecars on a [`Scene`] tree into a taffy compute
//! over a flex / block layout tree, then writes the resulting pixel
//! rects back into each node's `rect` field.
//!
//! `pinion-core` stays free of any taffy dependency per the §5.21
//! spec; the wrapper [`LayoutStyle`] enum set is what `pinion-core`
//! exports, and this module owns the translation into `taffy::Style`.
//!
//! R47.4 §5.36 — `Scene::Text` leaves carry a parley measure context
//! so taffy resolves their intrinsic width / height through the
//! [`pinion_text::LayoutCache`] passed into [`compute_layout`]. The
//! same cache is consumed by `paint_adapter::to_vello`'s Text arm on
//! the same frame, so each label shapes once and the result is reused
//! by both measure + paint passes.
//!
//! Single layout pass per frame; pure with respect to `(scene tree,
//! cache contents, viewport)` — same inputs produce identical rects.
//! The §6.3 view-fn purity invariant is preserved because nothing in
//! this module observes time or external state; the cache is content-
//! addressable (text + style + `max_width`), not time-keyed.

use pinion_core::scene::{BoxNode, ContainerNode, ExternalNode, ImageNode, PathNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, Display, FlexDirection, JustifyContent, LayoutStyle, SizeValue, TextStyle,
};
use pinion_core::Scene;
use pinion_text::LayoutCache;
use std::collections::HashMap;
use taffy::prelude::{
    auto, length, percent, AvailableSpace, FromLength, LengthPercentage, NodeId,
    Rect as TaffyRect, Size as TaffySize, TaffyTree,
};
use taffy::style::{
    AlignItems as TaffyAlign, Dimension, Display as TaffyDisplay,
    FlexDirection as TaffyFlexDir, JustifyContent as TaffyJustify, Style as TaffyStyle,
};

/// R47.4 §5.36 — taffy `NodeContext` for leaves that need an intrinsic
/// measure callback. `Scene::Text` is the only consumer today; future
/// variants (image intrinsic / external opaque measure) extend this
/// enum without changing the closure shape.
pub enum NodeContext {
    /// `Scene::Text` leaf measure source — content + style flow into
    /// `LayoutCache::layout` to produce parley's intrinsic width /
    /// height. The clone is necessary because the closure outlives the
    /// `&Scene` ref used during build.
    Text { content: String, style: TextStyle },
}

/// Compute the layout of `scene` against the given viewport extents.
///
/// `cache` is the application-owned [`LayoutCache`] (the same instance
/// the Vello paint adapter consults later in the frame). Shape work
/// done here populates the LRU so the subsequent
/// `paint_adapter::to_vello` call is a cache hit on every static
/// label. Mutates each node's `rect` field in place; nothing else is
/// touched. Safe to call every frame.
///
/// # Panics
///
/// Panics if taffy reports a tree-construction error; this can only
/// happen on internal logic bugs (passing invalid `NodeId`s, etc.),
/// not on any user-supplied scene shape.
#[allow(clippy::cast_precision_loss)]
pub fn compute_layout(
    scene: &mut Scene,
    cache: &mut LayoutCache,
    viewport_w: u32,
    viewport_h: u32,
) {
    let mut tree: TaffyTree<NodeContext> = TaffyTree::new();
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
    // R51.1 §5.12 — side-channel `NodeId → line_count` table populated
    // by the measure callback, drained by `apply` into `TextNode.
    // line_count`. taffy's measure closure has no `&mut Scene` access
    // (Scene is borrowed read-only by `build`), and `NodeContext` is
    // owned by taffy without a mutable accessor on the path
    // `compute_layout_with_measure` returns, so a separate `HashMap`
    // bridges the measure pass to the apply pass. parley's `Layout::
    // lines().count()` is the shape backend agnostic source — the
    // `pinion_text::LayoutCache` swap to a self-hosted text engine
    // (§5.37.7 carry) keeps the same `.lines().count()` surface.
    let mut text_lines: HashMap<NodeId, u32> = HashMap::new();
    // R47.4 §5.36 — measure callback. Scene::Text leaves consult parley
    // (via `cache`) for intrinsic width / height; non-Text leaves
    // return `Size::ZERO`, matching the pre-R47.4 `compute_layout`
    // behaviour for variants without explicit `size` declarations.
    tree.compute_layout_with_measure(
        layout_tree.node,
        available,
        |known_dimensions, available_space, node_id, node_context, _style| {
            if let TaffySize { width: Some(width), height: Some(height) } = known_dimensions {
                return TaffySize { width, height };
            }
            match node_context {
                Some(NodeContext::Text { content, style }) => {
                    // available_space.width.Definite → parley wrap point
                    // (multi-line); MinContent / MaxContent → no wrap
                    // (single line / unbounded), matching how taffy
                    // probes the leaf during flex resolution.
                    let max_width = match available_space.width {
                        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                        AvailableSpace::Definite(w) if w.is_finite() && w >= 0.0 => {
                            Some(w as u32)
                        }
                        _ => None,
                    };
                    let layout = cache.layout(content, style, max_width);
                    // R51.1 §5.12 — capture line count on the last
                    // measure probe per node id; taffy may call this
                    // closure multiple times during flex resolution
                    // (MinContent / MaxContent / Definite). The final
                    // call uses the resolved Definite width, which is
                    // also what `apply` would re-measure against, so
                    // overwriting on every call is correct.
                    #[allow(clippy::cast_possible_truncation)]
                    let line_count = layout.lines().count() as u32;
                    text_lines.insert(node_id, line_count);
                    // R47.7.6 — integer pixel snapping. parley returns
                    // sub-pixel f32 widths; without `ceil` the value
                    // oscillates `77.0`/`77.8` between adjacent
                    // viewport widths, producing a visible 1-px text
                    // jitter on mouse-drag resize. `ceil` rounds toward
                    // "fits inside taffy's bound" so the result snaps
                    // monotonically and the cached `rect.w` stays stable
                    // across consecutive frames at the same content.
                    TaffySize {
                        width: layout.width().ceil(),
                        height: layout.height().ceil(),
                    }
                }
                None => TaffySize::ZERO,
            }
        },
    )
    .expect("taffy compute_layout failed");
    apply(scene, &layout_tree, &tree, &text_lines, 0.0, 0.0);
}

/// Recursive shadow tree mirroring the Scene; each entry holds the
/// taffy `NodeId` and the children we registered for that node.
struct LayoutShadow {
    node: NodeId,
    children: Vec<LayoutShadow>,
}

fn build(scene: &Scene, tree: &mut TaffyTree<NodeContext>) -> LayoutShadow {
    let style = to_taffy_style(layout_style_of(scene));
    let children = match scene {
        Scene::Container(c) => c.children.iter().map(|s| build(s, tree)).collect(),
        _ => Vec::new(),
    };
    let child_ids: Vec<NodeId> = children.iter().map(|c| c.node).collect();
    let node = if !child_ids.is_empty() {
        tree.new_with_children(style, &child_ids)
            .expect("taffy new_with_children failed")
    } else if let Scene::Text(t) = scene {
        // R47.4 §5.36 — Text leaves carry the parley measure context
        // so the closure passed to `compute_layout_with_measure` can
        // resolve their intrinsic size. Clone is unavoidable: the
        // closure FnMut bound + node-context ownership semantics keep
        // the context alive across the whole layout pass, beyond the
        // `&Scene` borrow this `build` recurses with.
        tree.new_leaf_with_context(
            style,
            NodeContext::Text {
                content: t.content.clone(),
                style: t.style.clone(),
            },
        )
        .expect("taffy new_leaf_with_context failed")
    } else {
        tree.new_leaf(style).expect("taffy new_leaf failed")
    };
    LayoutShadow { node, children }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn apply(
    scene: &mut Scene,
    shadow: &LayoutShadow,
    tree: &TaffyTree<NodeContext>,
    text_lines: &HashMap<NodeId, u32>,
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

    // R51.1 §5.12 — Text leaves carry their measured line count from
    // the side-channel populated during `compute_layout_with_measure`.
    // Other variants stay at the `TextNode::default()` `line_count = 0`
    // because the field is Text-only by semantics.
    if let Scene::Text(t) = scene {
        t.line_count = text_lines.get(&shadow.node).copied().unwrap_or(0);
    }

    if let Scene::Container(c) = scene {
        for (child, shadow_child) in c.children.iter_mut().zip(&shadow.children) {
            apply(child, shadow_child, tree, text_lines, abs_x, abs_y);
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
    // §5.21 R24 slice 4: Rect-as-4-inset (x=left, y=top, w=right,
    // h=bottom) → taffy Rect<LengthPercentage>. taffy's padding /
    // margin both take pixel lengths.
    s.padding = TaffyRect {
        left: LengthPercentage::from_length(layout.padding.x as f32),
        right: LengthPercentage::from_length(layout.padding.w as f32),
        top: LengthPercentage::from_length(layout.padding.y as f32),
        bottom: LengthPercentage::from_length(layout.padding.h as f32),
    };
    s.margin = TaffyRect {
        left: length(layout.margin.x as f32),
        right: length(layout.margin.w as f32),
        top: length(layout.margin.y as f32),
        bottom: length(layout.margin.h as f32),
    };
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
    use pinion_core::style::{Color, FlexDirection, JustifyContent, Size, TextStyle};

    fn cache() -> LayoutCache {
        LayoutCache::new()
    }

    #[test]
    fn block_root_fills_viewport() {
        // A Container with Display::Block (default) and Auto size
        // expands to the viewport bounds taffy was given.
        let mut scene = Scene::Container(ContainerNode::new(vec![]));
        compute_layout(&mut scene, &mut cache(), 320, 200);
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
        compute_layout(&mut scene, &mut cache(), 320, 200);
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
        compute_layout(&mut scene, &mut cache(), 200, 200);
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
    fn container_padding_offsets_child_origin() {
        // R24 slice 4: LayoutStyle.padding feeds taffy padding;
        // child rect.{x,y} shifts by the parent's left+top padding.
        let layout = LayoutStyle::new()
            .flex(FlexDirection::Row)
            .with_padding(pinion_core::scene::Rect::new(10, 20, 10, 20));
        let child = Scene::Box(
            BoxNode::filled(pinion_core::scene::Rect::default(), Color::default())
                .with_layout(LayoutStyle::new().with_size(Size::px(50, 30))),
        );
        let mut scene =
            Scene::Container(ContainerNode::new(vec![child]).with_layout(layout));
        compute_layout(&mut scene, &mut cache(), 200, 200);
        let Scene::Container(c) = &scene else {
            panic!("container")
        };
        let Scene::Box(b) = &c.children[0] else {
            panic!("box child")
        };
        // padding.x (left=10), padding.y (top=20) shift the child.
        assert_eq!(b.rect.x, 10);
        assert_eq!(b.rect.y, 20);
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
        compute_layout(&mut scene, &mut cache(), 200, 200);
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

    #[test]
    fn text_leaf_intrinsic_measure_drives_flex_center() {
        // R47.4 §5.36 — Scene::Text leaf with no explicit Size resolves
        // its width/height through parley measure (LayoutCache) and
        // participates in flex Center/Center as a non-zero box. Without
        // the MeasureFunc wire the leaf was 0×0 → a "centered" single
        // point at viewport mid; the user-visible bug R47.3 left open.
        let text = Scene::Text(TextNode::styled(
            "Click me!",
            Rect::default(),
            TextStyle::new().with_size_px(18),
        ));
        let layout = LayoutStyle::new()
            .flex(FlexDirection::Row)
            .with_justify(JustifyContent::Center)
            .with_align_items(AlignItems::Center);
        let mut scene = Scene::Container(ContainerNode::new(vec![text]).with_layout(layout));
        let mut c = cache();
        compute_layout(&mut scene, &mut c, 320, 200);
        let Scene::Container(container) = &scene else {
            panic!("expected container")
        };
        let Scene::Text(t) = &container.children[0] else {
            panic!("expected text child")
        };
        assert!(t.rect.w > 0, "text leaf width should be parley-measured");
        assert!(t.rect.h > 0, "text leaf height should be parley-measured");
        // R51.1 §5.12 — measured line count is populated alongside the
        // rect. A short single-word label in a 320-wide viewport must
        // resolve to a single line regardless of system font fallback.
        assert_eq!(
            t.line_count, 1,
            "single-word label in 320-wide viewport must be 1 line"
        );
        // flex Center → child rect.x ≈ (320 - w) / 2 and rect.y ≈
        // (200 - h) / 2. Exact pixel depends on the system font width;
        // we assert the offsets are non-trivial (not 0 = left/top edge).
        assert!(
            t.rect.x > 0,
            "Center flex must shift text right of x=0 (got x={})",
            t.rect.x
        );
        assert!(
            t.rect.y > 0,
            "Center flex must shift text below y=0 (got y={})",
            t.rect.y
        );
    }

    #[test]
    fn text_line_count_zero_before_layout() {
        // R51.1 §5.12 — `TextNode::styled` / `TextNode::new` default
        // `line_count = 0`. The measure pass populates it; readers
        // can rely on `0` meaning "no shape pass has run yet" as a
        // sentinel distinct from any valid measured count.
        let t = TextNode::styled(
            "Click me!",
            Rect::default(),
            TextStyle::new().with_size_px(18),
        );
        assert_eq!(t.line_count, 0);
    }

    #[test]
    fn text_line_count_stable_across_adjacent_viewport_widths() {
        // R47.7.6 / R51.1 §5.12 — sub-pixel parley widths get ceil'd
        // before they reach taffy, so adjacent integer viewport widths
        // (the per-frame sequence during mouse-drag resize) produce
        // the same `line_count`. Missing the `ceil` would let
        // `cache.layout(...).width()` return e.g. 77.8 while taffy's
        // child slot is 77 — parley would then break to a second
        // line on every other frame, jittering `line_count` between
        // 1 and 2 across the drag.
        let label = "Click me!";
        let style = TextStyle::new().with_size_px(18);
        let layout = LayoutStyle::new()
            .flex(FlexDirection::Row)
            .with_justify(JustifyContent::Center)
            .with_align_items(AlignItems::Center);
        let mut cache = LayoutCache::new();
        let mut counts = Vec::with_capacity(21);
        // 300..=320 — a 21-wide window straddles the natural label
        // width on every reasonable system font, exercising the
        // adjacent-width path that produced the original R47.7.6
        // jitter on mouse-drag resize.
        for w in 300_u32..=320 {
            let text = Scene::Text(TextNode::styled(label, Rect::default(), style.clone()));
            let mut scene =
                Scene::Container(ContainerNode::new(vec![text]).with_layout(layout));
            compute_layout(&mut scene, &mut cache, w, 200);
            let Scene::Container(container) = &scene else {
                panic!("container");
            };
            let Scene::Text(t) = &container.children[0] else {
                panic!("text child");
            };
            counts.push(t.line_count);
        }
        assert!(
            counts.iter().all(|&n| n == 1),
            "line_count must stay 1 across adjacent widths 300..=320 (got {counts:?})"
        );
    }

    #[test]
    fn text_line_count_increases_when_max_width_forces_wrap() {
        // R51.1 §5.12 — when the available width is genuinely narrower
        // than the natural text width, parley wraps and `line_count`
        // grows accordingly. This bounds the ceil-stability test
        // above: the surface really does report >1 lines when the
        // content truly does not fit, so the AI client can rely on
        // `line_count > 1` as a real wrap signal.
        let content = "The quick brown fox jumps over the lazy dog";
        let style = TextStyle::new().with_size_px(18);
        let text = Scene::Text(TextNode::styled(content, Rect::default(), style).with_layout(
            LayoutStyle::new().with_size(pinion_core::style::Size::px(60, 200)),
        ));
        let layout = LayoutStyle::new().flex(FlexDirection::Row);
        let mut scene =
            Scene::Container(ContainerNode::new(vec![text]).with_layout(layout));
        let mut cache = LayoutCache::new();
        compute_layout(&mut scene, &mut cache, 320, 200);
        let Scene::Container(container) = &scene else {
            panic!("container");
        };
        let Scene::Text(t) = &container.children[0] else {
            panic!("text child");
        };
        assert!(
            t.line_count >= 2,
            "60px-wide slot must force the sentence to wrap (got {})",
            t.line_count
        );
    }

    #[test]
    fn text_leaf_measure_populates_layout_cache() {
        // The measure pass should hit the same LayoutCache subsequent
        // paint passes use; shape work amortizes across measure + paint
        // within one frame.
        let text = Scene::Text(TextNode::styled(
            "Hello",
            Rect::default(),
            TextStyle::new().with_size_px(16),
        ));
        let mut scene = Scene::Container(ContainerNode::new(vec![text]).with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_justify(JustifyContent::Center),
        ));
        let mut c = cache();
        assert_eq!(c.len(), 0, "fresh cache is empty");
        compute_layout(&mut scene, &mut c, 320, 200);
        assert!(!c.is_empty(), "measure pass populates LayoutCache");
    }
}
