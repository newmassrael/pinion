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
//!
//! R55.G.2 §5.45 — `Scene::Scroll` participates by getting its
//! content sub-tree laid out in a *separate* taffy pass: each Scroll
//! is treated as a layout leaf in the outer tree (its rect stays
//! app-set via `ScrollNode::viewport`), and the content beneath is
//! re-entered with [`compute_layout`] using `viewport.w` as the
//! cross-axis bound and `AvailableSpace::MaxContent` on the main
//! axis so flex children can overflow naturally instead of being
//! shrunk to fit the clip window. Content rects come out in
//! *content-local* coordinates (origin at the Scroll's content
//! origin, not absolute window space) — the hit-tester and paint
//! adapter translate via `Scroll.viewport.{x,y}` and `offset_{x,y}`
//! at read time, matching the §5.45 R55 substrate.

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
    compute_layout_inner(scene, cache, viewport_w, viewport_h, false);
}

/// R55.G.2 §5.45 — extension point for laying out a `Scene::Scroll`
/// content sub-tree. `main_axis_unbounded` swaps the height
/// constraint from `Definite(viewport_h)` to `MaxContent` so flex
/// children can grow past the clip window (the scroll case) instead
/// of being shrunk to fit (the outer-window case).
///
/// # Panics
///
/// Panics if taffy reports a tree-construction error; this can only
/// happen on internal logic bugs (passing invalid `NodeId`s, etc.),
/// not on any user-supplied scene shape.
#[allow(clippy::cast_precision_loss)]
fn compute_layout_inner(
    scene: &mut Scene,
    cache: &mut LayoutCache,
    viewport_w: u32,
    viewport_h: u32,
    main_axis_unbounded: bool,
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
    // R55.G.2 §5.45 — scroll content lays out with auto height so the
    // flex Column total can overflow the clip window instead of
    // being clamped. The outer-window pass keeps the explicit
    // `length(viewport_h)` cap so block defaults still fill.
    root_style.size = TaffySize {
        width: length(viewport_w as f32),
        height: if main_axis_unbounded {
            auto()
        } else {
            length(viewport_h as f32)
        },
    };
    tree.set_style(layout_tree.node, root_style)
        .expect("set root style failed");
    let available = TaffySize {
        width: AvailableSpace::Definite(viewport_w as f32),
        height: if main_axis_unbounded {
            AvailableSpace::MaxContent
        } else {
            AvailableSpace::Definite(viewport_h as f32)
        },
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
    // R55.G.2 §5.45 — outer apply does not descend into `Scene::Scroll`
    // content (build also stops at Scroll), so any Scroll in the
    // tree now needs its content re-entered with its own taffy
    // pass. Content rects come out in scroll-local coordinates.
    lay_out_scroll_contents(scene, cache);
}

/// R55.G.2 §5.45 — walks the scene and lays out each `Scene::Scroll`
/// content sub-tree as an independent taffy root sized by the
/// scroll's `viewport`. Recursion is handled by the inner
/// [`compute_layout_inner`] call's own tail invocation, so nested
/// Scrolls naturally cascade.
fn lay_out_scroll_contents(scene: &mut Scene, cache: &mut LayoutCache) {
    match scene {
        Scene::Container(c) => {
            for child in &mut c.children {
                lay_out_scroll_contents(child, cache);
            }
        }
        Scene::Scroll(s) => {
            let vw = s.viewport.w;
            let vh = s.viewport.h;
            compute_layout_inner(s.content.as_mut(), cache, vw, vh, true);
        }
        _ => {}
    }
}

/// Recursive shadow tree mirroring the Scene; each entry holds the
/// taffy `NodeId` and the children we registered for that node.
struct LayoutShadow {
    node: NodeId,
    children: Vec<LayoutShadow>,
}

fn build(scene: &Scene, tree: &mut TaffyTree<NodeContext>) -> LayoutShadow {
    // R55.G.4 §5.45 — Scroll's `layout` field now carries its
    // taffy style (seeded with `viewport.{w,h}` by
    // `ScrollNode::new`); the pre-R55.G.4 build-site size override
    // is retired in favour of the unified `layout_style_of` path.
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
        // R55.G.4 §5.45 — Scroll's `layout` is seeded with the clip
        // window size by `ScrollNode::new`, so taffy treats it as a
        // fixed-size leaf by default; callers that want `flex_grow`
        // / `margin` / parent-flex participation chain
        // `with_layout(...)`.
        Scene::Scroll(n) => &n.layout,
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
///
/// R55.G.4 §5.45 — `Scene::Scroll` writes the full layout-derived
/// rect into `viewport`. The pre-R55.G.4 partial write (x/y only)
/// was a side effect of the build-site size override; now that
/// `ScrollNode.layout` carries the size intent, taffy's output is
/// the authoritative dimensions and writing the full rect keeps
/// the substrate honest when the caller opts into `flex_grow` or
/// any other layout-driven resize.
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
        Scene::Scroll(s) => {
            s.viewport = rect;
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

    mod r55_g2 {
        //! R55.G.2 §5.45 — `compute_layout` descends into
        //! `Scene::Scroll.content`. Content rects come out in
        //! scroll-local coordinates with `MaxContent` on the main
        //! axis, so a flex Column overflows the clip window naturally
        //! instead of being shrunk to fit.

        use super::*;
        use pinion_core::scene::{ContainerNode, Rect, ScrollNode};
        use pinion_core::style::{
            Color, FlexDirection, JustifyContent, LayoutStyle, Size,
        };

        fn fixed_row(h: u32) -> Scene {
            fixed_row_w(220, h)
        }

        fn fixed_row_w(w: u32, h: u32) -> Scene {
            Scene::Container(
                ContainerNode::new(vec![])
                    .with_layout(LayoutStyle::new().with_size(Size::px(w, h))),
            )
        }

        #[test]
        fn scroll_content_flex_column_lays_out_row_y_positions() {
            // Content = flex Column with gap=6, 3 rows of fixed 220×28.
            // Expected: row[0]@(0,0), row[1]@(0,34), row[2]@(0,68).
            let content_layout = LayoutStyle::new()
                .flex(FlexDirection::Column)
                .with_gap(6);
            let content = Scene::Container(
                ContainerNode::new(vec![fixed_row(28), fixed_row(28), fixed_row(28)])
                    .with_layout(content_layout),
            );
            let scroll = ScrollNode::new(Rect::new(70, 78, 220, 164), content);
            let mut scene = Scene::Container(ContainerNode::new(vec![Scene::Scroll(scroll)]));

            compute_layout(&mut scene, &mut cache(), 360, 320);

            let Scene::Container(outer) = &scene else { panic!("outer") };
            let Scene::Scroll(s) = &outer.children[0] else { panic!("scroll") };
            // R55.G.3 §5.45 — viewport's w/h stay app-set (clip
            // window intent); viewport's x/y are layout-derived.
            // Block-display outer places Scroll at (0, 0).
            assert_eq!(s.viewport.w, 220);
            assert_eq!(s.viewport.h, 164);
            assert_eq!(s.viewport.x, 0);
            assert_eq!(s.viewport.y, 0);
            // Content rects are scroll-local (origin at (0, 0)).
            let Scene::Container(c) = s.content.as_ref() else { panic!("content") };
            let Scene::Container(r0) = &c.children[0] else { panic!("row0") };
            let Scene::Container(r1) = &c.children[1] else { panic!("row1") };
            let Scene::Container(r2) = &c.children[2] else { panic!("row2") };
            assert_eq!(r0.rect, Rect::new(0, 0, 220, 28));
            assert_eq!(r1.rect, Rect::new(0, 34, 220, 28));
            assert_eq!(r2.rect, Rect::new(0, 68, 220, 28));
        }

        #[test]
        fn r55_g4_scroll_with_flex_grow_stretches_in_parent_flex() {
            // R55.G.4 §5.45 — `with_layout` overrides the default
            // `Size::px(viewport.{w,h})` so the Scroll can opt into
            // `flex_grow` and fill the remaining cross-axis space.
            // Proves the layout sidecar plumbing reaches Scroll, not
            // just the size override that the R55.G.3 hack baked in.
            let content = Scene::Container(ContainerNode::new(vec![]));
            let scroll = ScrollNode::new(Rect::new(0, 0, 100, 50), content)
                .with_layout(LayoutStyle::new().with_flex_grow(1.0));
            let outer_layout = LayoutStyle::new().flex(FlexDirection::Row);
            let mut scene = Scene::Container(
                ContainerNode::new(vec![Scene::Scroll(scroll)]).with_layout(outer_layout),
            );

            compute_layout(&mut scene, &mut cache(), 360, 320);

            let Scene::Container(outer) = &scene else { panic!("outer") };
            let Scene::Scroll(s) = &outer.children[0] else { panic!("scroll") };
            // Scroll grew to the full 360-wide row instead of staying
            // at the 100-wide default `viewport` width — proves the
            // taffy size came from `layout.flex_grow`, not the
            // pre-R55.G.4 unconditional viewport override.
            assert_eq!(s.viewport.w, 360, "flex_grow stretched viewport.w");
        }

        #[test]
        fn r55_g3_scroll_centered_via_outer_flex_writes_viewport_position() {
            // Outer Container = flex Row + JustifyContent::Center +
            // AlignItems::Center. Scroll inside is 220×164 inside a
            // 360×320 viewport — expected centred at
            // ((360-220)/2, (320-164)/2) = (70, 78). Proves R55.G.3
            // routes Scroll through parent flex.
            let content_layout = LayoutStyle::new()
                .flex(FlexDirection::Column)
                .with_gap(6);
            let content = Scene::Container(
                ContainerNode::new(vec![fixed_row(28)]).with_layout(content_layout),
            );
            let scroll = ScrollNode::new(Rect::new(0, 0, 220, 164), content);
            let outer_layout = LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_justify(JustifyContent::Center)
                .with_align_items(AlignItems::Center);
            let mut scene = Scene::Container(
                ContainerNode::new(vec![Scene::Scroll(scroll)]).with_layout(outer_layout),
            );

            compute_layout(&mut scene, &mut cache(), 360, 320);

            let Scene::Container(outer) = &scene else { panic!("outer") };
            let Scene::Scroll(s) = &outer.children[0] else { panic!("scroll") };
            assert_eq!(s.viewport.x, 70, "viewport.x layout-derived");
            assert_eq!(s.viewport.y, 78, "viewport.y layout-derived");
            assert_eq!(s.viewport.w, 220, "viewport.w app-set");
            assert_eq!(s.viewport.h, 164, "viewport.h app-set");
        }

        #[test]
        fn scroll_content_total_height_can_exceed_viewport() {
            // 12 rows × 28 + 11 × 6 gap = 402 > 164 viewport. With
            // `MaxContent` on the main axis the flex column lays
            // children at their natural heights instead of shrinking.
            let rows: Vec<Scene> = (0..12).map(|_| fixed_row(28)).collect();
            let content = Scene::Container(
                ContainerNode::new(rows).with_layout(
                    LayoutStyle::new().flex(FlexDirection::Column).with_gap(6),
                ),
            );
            let scroll = ScrollNode::new(Rect::new(0, 0, 220, 164), content);
            let mut scene = Scene::Container(ContainerNode::new(vec![Scene::Scroll(scroll)]));

            compute_layout(&mut scene, &mut cache(), 360, 320);

            let Scene::Container(outer) = &scene else { panic!("outer") };
            let Scene::Scroll(s) = &outer.children[0] else { panic!("scroll") };
            let Scene::Container(c) = s.content.as_ref() else { panic!("content") };
            // Last row's y = 11 × (28 + 6) = 374, well past the
            // 164-tall viewport — proves flex did not compress.
            let Scene::Container(last) = c.children.last().unwrap() else { panic!() };
            assert_eq!(last.rect.y, 374);
            assert_eq!(last.rect.h, 28);
        }

        #[test]
        fn scroll_content_cross_axis_bounded_by_viewport_width() {
            // Content child without explicit width inherits viewport.w
            // (220) as the cross-axis bound under flex Column.
            let stretchy = Scene::Container(
                ContainerNode::new(vec![Scene::Box(
                    BoxNode::filled(Rect::default(), Color::default())
                        .with_layout(LayoutStyle::new().with_size(Size::px(40, 20))),
                )])
                .with_layout(
                    LayoutStyle::new()
                        .flex(FlexDirection::Row)
                        .with_justify(JustifyContent::Center),
                ),
            );
            let content = Scene::Container(
                ContainerNode::new(vec![stretchy]).with_layout(
                    LayoutStyle::new().flex(FlexDirection::Column),
                ),
            );
            let scroll = ScrollNode::new(Rect::new(0, 0, 220, 80), content);
            let mut scene = Scene::Container(ContainerNode::new(vec![Scene::Scroll(scroll)]));

            compute_layout(&mut scene, &mut cache(), 360, 320);

            let Scene::Container(outer) = &scene else { panic!("outer") };
            let Scene::Scroll(s) = &outer.children[0] else { panic!("scroll") };
            let Scene::Container(c) = s.content.as_ref() else { panic!("content") };
            let Scene::Container(stretchy) = &c.children[0] else { panic!("stretchy") };
            // Stretched to the viewport.w cross-axis bound.
            assert_eq!(stretchy.rect.w, 220);
            let Scene::Box(b) = &stretchy.children[0] else { panic!("box") };
            // Centered inside the 220-wide stretchy row.
            assert_eq!(b.rect.x, 90);
            assert_eq!(b.rect.w, 40);
        }

        #[test]
        fn nested_scroll_content_recurses_through_lay_out_scroll_contents() {
            // Outer Scroll content contains another Scroll, whose
            // own content must also be laid out by the recursive
            // pass — proves the lay_out_scroll_contents tail call
            // descends into nested scrolls.
            let inner_content = Scene::Container(
                ContainerNode::new(vec![fixed_row_w(200, 40)])
                    .with_layout(LayoutStyle::new().flex(FlexDirection::Column)),
            );
            let inner_scroll =
                Scene::Scroll(ScrollNode::new(Rect::new(0, 0, 200, 60), inner_content));
            let outer_content = Scene::Container(
                ContainerNode::new(vec![inner_scroll])
                    .with_layout(LayoutStyle::new().flex(FlexDirection::Column)),
            );
            let outer_scroll =
                Scene::Scroll(ScrollNode::new(Rect::new(0, 0, 220, 160), outer_content));
            let mut scene =
                Scene::Container(ContainerNode::new(vec![outer_scroll]));

            compute_layout(&mut scene, &mut cache(), 360, 320);

            let Scene::Container(root) = &scene else { panic!("root") };
            let Scene::Scroll(outer) = &root.children[0] else { panic!("outer scroll") };
            let Scene::Container(outer_c) = outer.content.as_ref() else { panic!("outer content") };
            let Scene::Scroll(inner) = &outer_c.children[0] else { panic!("inner scroll") };
            let Scene::Container(inner_c) = inner.content.as_ref() else { panic!("inner content") };
            // Inner content's row was laid out by the nested-scroll
            // recursive pass — rect is non-zero.
            let Scene::Container(row) = &inner_c.children[0] else { panic!("inner row") };
            assert_eq!(row.rect, Rect::new(0, 0, 200, 40));
        }
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
