//! R683.B §5.16 §5.41 — backend-agnostic Splitter widget paint
//! composition + draggable handle [`External`].
//!
//! ## Role
//!
//! A `Splitter` divides a horizontal (`Row`) or vertical (`Column`)
//! container into two child panels with a draggable handle that the
//! user grabs to resize the split. The split position lives in a
//! shared `Signal<f32>` ratio ∈ `[0.0, 1.0]` so the application can
//! persist it (R665 storage substrate) + observe mutations (reactive
//! repaints, `dock-panel` tear-off layout, …).
//!
//! The Splitter is the canonical foundation of multi-pane DCC / IDE
//! / CAD professional GUI tooling (the Phase B north star). R683.B
//! atomic 3 (the `pinion_widget_paint::dock` surface) consumes this
//! widget as the resizable boundary between dock slots; binding-
//! level applications (R683.B atomic 4 `examples/hello-dock-panels`,
//! future settings / editor layouts) consume directly via
//! [`view_splitter`].
//!
//! ## Dep graph
//!
//! Sits beside [`scrollbar`](crate::scrollbar) /
//! [`tree_view`](crate::tree_view) — single-axis widget paint
//! helper, no `pinion-text` dependency, pure
//! [`Scene`] composition.
//!
//! ## API shape
//!
//! [`view_splitter`] is the single entry-point. It takes two child
//! `Scene`s (`left` / `right`), the shared
//! [`Signal<f32>`](pinion_core::Signal) ratio, the active
//! [`Theme`], and a [`SplitterStyle`] sidecar carrying orientation +
//! handle visual constants + the paint-side tag for input-router
//! routing. Returns a [`Scene::Container`] in flex layout (`Row` for
//! horizontal split, `Column` for vertical) with three flex children
//! (left panel `flex_grow = ratio`, handle fixed-size, right panel
//! `flex_grow = 1.0 - ratio`).
//!
//! The drag wire is the paired [`SplitterExternal`] the binding
//! registers via
//! [`WidgetCore::create_extra_externals`](pinion_core::WidgetCore::create_extra_externals)
//! tagged with the same [`SplitterStyle::tag`]. The
//! `InputRouter`'s deepest-tagged
//! hit-test routes `PointerDown` on the splitter Container to the
//! external; `wants_pointer_capture = true` pins the cursor lock for
//! the duration of the press; `pointer_move` translates the
//! normalised cursor delta into a ratio mutation. The application's
//! own child-panel tags out-rank the splitter tag in the hit-test
//! (deepest-tagged wins), so a tagged panel intercepts its own
//! clicks while the splitter's bare gutter strip + the handle
//! deliver to the external.
//!
//! ## Why one External per Splitter Container (not per handle child)
//!
//! Attaching the external to the splitter Container (instead of the
//! handle strip specifically) keeps the [`SplitterExternal::pointer_move`] coordinate
//! frame stable at the Container's bounding rect — `x_rel ∈ [0, 1]`
//! covers the full extent the user expects to drag across. Pinning
//! it to the handle strip alone would normalise `x_rel` over a 4 px
//! sliver, making every cursor pixel of movement multiplied by 1/4
//! when projected back onto the splitter's coordinate space. The
//! per-Container attach also matches the scrollbar pattern (R660
//! [`ScrollBarExternal`](pinion_core::widgets::scrollbar::ScrollBarExternal)
//! where the track + thumb both belong to one External).
//!
//! The application contract: child panel `Scene`s the binding hands
//! to [`view_splitter`] should carry their own `tag` to opt out of
//! splitter-side pointer interception. Untagged children fall under
//! the splitter's "deepest-tagged" hit and trigger the drag wire on
//! any click. Dock-surface (atomic 3) panels are always tagged so
//! the contract is satisfied automatically.

use std::borrow::Cow;
use std::collections::VecDeque;
use std::rc::Rc;

use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner, SchemaField, ThreadOwnership,
};
use pinion_core::input::{DragCalibration, PointerWireEvent};
use pinion_core::intent::Intent;
use pinion_core::reactive::Signal;
use pinion_core::scene::{ContainerNode, Scene};
use pinion_core::style::{
    AlignItems, BoxStyle, Color, CursorHint, FlexDirection, JustifyContent, LayoutStyle, Size,
    SizeValue,
};
use pinion_core::theme::{ColorRole, Theme};

/// (R683.B §5.16) Orientation of the splitter's main flex axis.
///
/// `Horizontal` lays children left-to-right (the `Row` form, with
/// handle as a vertical strip between two side-by-side panels);
/// `Vertical` lays children top-to-bottom (the `Column` form, with
/// handle as a horizontal strip between two stacked panels). The
/// pair maps directly onto [`FlexDirection::Row`] /
/// [`FlexDirection::Column`].
///
/// Mirrors [`pinion_core::widgets::scrollbar::ScrollBarOrientation`]
/// — the same {Horizontal, Vertical} taxonomy + the same per-axis
/// drag-coord projection (`Horizontal` → `x_rel` drives,
/// `Vertical` → `y_rel` drives). Authors who want the cross-axis
/// behaviour swap the orientation field instead of swapping `left` /
/// `right` callsite arguments.
///
/// (R685 §5.16 §5.49) Derives `serde::Serialize + Deserialize` so it
/// can participate in [`crate::dock::DockNode`] tree serialization
/// for AI-introspection. Serialized form mirrors `enum DockSlot`
/// (untagged unit variants, `"Horizontal"` / `"Vertical"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SplitterOrientation {
    /// Row layout: `Left | Handle | Right` with `Handle` as a
    /// vertical strip. `pointer_move`'s `x_rel` drives ratio
    /// mutation; `y_rel` is ignored.
    Horizontal,
    /// Column layout: `Top / Handle / Bottom` with `Handle` as a
    /// horizontal strip. `pointer_move`'s `y_rel` drives ratio
    /// mutation; `x_rel` is ignored.
    Vertical,
}

impl SplitterOrientation {
    /// Map to the matching [`FlexDirection`] for the outer
    /// `Scene::Container`'s `LayoutStyle::flex_direction`.
    #[must_use]
    pub const fn flex_direction(self) -> FlexDirection {
        match self {
            Self::Horizontal => FlexDirection::Row,
            Self::Vertical => FlexDirection::Column,
        }
    }
}

/// (R683.B §5.16) Sidecar carrying [`view_splitter`]'s
/// binding-local visual constants. `#[non_exhaustive]` so future
/// rounds can add axes (e.g. `hover_zone_w`, `handle_corner_radius`,
/// `disabled_alpha`) behind builders without breaking the
/// constructor surface.
///
/// Use [`Self::m3_default`] for the M3-canonical 4-px handle (the `the code editor` / `the IDE vendor` / the design tool
/// desktop default) tinted with [`ColorRole::Outline`] for visibility against any palette.
#[non_exhaustive]
/// (R685.B §5.16) Lost `Copy` (was R683.B) when `tag` became
/// `Cow<'static, str>` — `Cow::Owned(String)` is not `Copy`. `Clone`
/// is cheap (`Cow::Borrowed` clone is an `&'static str` copy; only
/// `Cow::Owned` allocates). All call sites that need `Self` by value
/// use `.clone()` explicitly.
#[derive(Debug, Clone)]
pub struct SplitterStyle {
    /// Orientation of the splitter's main flex axis. Determines
    /// both the outer `LayoutStyle::flex_direction` and which
    /// pointer-move axis drives ratio mutation. See
    /// [`SplitterOrientation`].
    pub orientation: SplitterOrientation,
    /// Extent (in logical pixels) of the handle strip along the
    /// splitter's main axis. The cross-axis extent stretches to fill
    /// the container (`AlignItems::Stretch`). M3 / desktop canonical
    /// is 4 px (thin hairline that the user can grab via the
    /// implicit hit-zone widening in [`SplitterExternal::pointer_move`]).
    pub handle_extent_px: u32,
    /// Paint-side tag attached to the outer splitter
    /// [`Scene::Container`] so the
    /// `InputRouter` deepest-tagged
    /// hit-test routes pointer events to the matching
    /// [`SplitterExternal`] registered through
    /// [`WidgetCore::create_extra_externals`](pinion_core::WidgetCore::create_extra_externals).
    /// Must match the tag the application registers the external
    /// under, or the drag wire stays inert (paint-only mode).
    ///
    /// (R685.B §5.16) `Cow<'static, str>` so dynamic split ids (e.g.
    /// R686 drag-to-reorganize-generated ids) can flow through
    /// [`crate::dock::view_dock_surface`] without forcing every Split's
    /// tag to be a compile-time `&'static str` constant. Static
    /// literals still coerce via `Cow::Borrowed` with zero overhead.
    pub tag: Cow<'static, str>,
    /// Minimum ratio bound, clamped on every pointer-move drag
    /// update. Defaults to `0.05` — keeps the smaller panel
    /// non-degenerate so the dock + tear-off arc has a non-zero
    /// drop target to fall into.
    pub min_ratio: f32,
    /// Maximum ratio bound, clamped on every pointer-move drag
    /// update. Defaults to `0.95` — symmetric with `min_ratio` so
    /// either panel can be the smaller of the two.
    pub max_ratio: f32,
}

impl SplitterStyle {
    /// (R683.B §5.16) M3-canonical default: 4-px handle strip,
    /// `[0.05, 0.95]` clamp bounds. The caller supplies the
    /// orientation + paint-side tag (the only binding-local axes
    /// every consumer must vary).
    ///
    /// (R685.B §5.16) `tag: impl Into<Cow<'static, str>>` — static
    /// `&'static str` literals coerce zero-cost via `Cow::Borrowed`;
    /// runtime-generated `String` ids (e.g. R686 dock-reorganize)
    /// flow via `Cow::Owned`. Same conversion contract every
    /// `with_tag` setter on Scene variants ships.
    #[must_use]
    pub fn m3_default(orientation: SplitterOrientation, tag: impl Into<Cow<'static, str>>) -> Self {
        Self {
            orientation,
            handle_extent_px: 4,
            tag: tag.into(),
            min_ratio: 0.05,
            max_ratio: 0.95,
        }
    }

    /// Override the handle extent (e.g. 8-px for touch interfaces).
    /// Material guidance recommends ≥ 12 px on touch surfaces; the
    /// 4-px default targets desktop pointers + the implicit
    /// hit-zone widening pointer-capture provides via cursor lock.
    #[must_use]
    pub const fn with_handle_extent_px(mut self, extent: u32) -> Self {
        self.handle_extent_px = extent;
        self
    }

    /// Override the `[min, max]` ratio clamp range. Caller is
    /// responsible for `0.0 <= min < max <= 1.0`; out-of-order
    /// inputs degrade the drag UX (the clamp passes through the
    /// pathological range) but do not break the paint composition.
    #[must_use]
    pub const fn with_ratio_bounds(mut self, min_ratio: f32, max_ratio: f32) -> Self {
        self.min_ratio = min_ratio;
        self.max_ratio = max_ratio;
        self
    }
}

/// (R683.B §5.16) Resolve the Material 3 handle fill for the given
/// interaction state. Composited via [`Color::lerp`] in linear
/// space ([[color-lerp-linear-space]]) so the state-layer overlay
/// renders perceptually-correct mid-tones across both light and
/// dark palettes.
///
/// | State    | Composition                                  | Why                              |
/// |----------|----------------------------------------------|----------------------------------|
/// | Idle     | `Outline`                                    | M3 hairline / divider weight     |
/// | Dragging | `Outline` lerp(`OnSurface`, 0.16)            | M3 dragged state-layer opacity   |
///
/// Mirrors the [`scrollbar`](crate::scrollbar) `thumb_fill_for_state`
/// role table — same M3 token + same lerp coefficient — so a palette
/// swap repaints both substrates in lock-step. v1 ships Idle +
/// Dragging only; Hover / Disabled axes land in a follow-up round
/// when a binding exercises them (R683.B + carry per
/// [[abstraction-needs-second-consumer]]).
#[must_use]
fn handle_fill_for_dragging(theme: &Theme, dragging: bool) -> Color {
    let base = theme.resolve(ColorRole::Outline);
    if dragging {
        base.lerp(theme.resolve(ColorRole::OnSurface), 0.16)
    } else {
        base
    }
}

/// (R1086 §5.21 §5.16) Set the three `LayoutStyle` fields that make a
/// child a CSS-canonical proportional flex item: `flex-basis: 0`,
/// `flex-grow: <ratio>`, and `min: 0` on the **main** axis (`min_size`).
///
/// The `min_size` is the R1086 addition. Without it, taffy's CSS
/// *automatic minimum size* pins a flex item to its content's
/// min-content extent on the main axis, so a panel whose content is
/// larger than its ratio share (a terminal grid, an image, a nested
/// scroll area) cannot shrink and overflows — the `flex-basis: 0`
/// alone was the incomplete half of the idiom. Zeroing the main-axis
/// `min` lets the flex pass distribute by ratio regardless of content
/// size. The cross axis is left untouched (`Auto`) so the outer
/// container's [`AlignItems::Stretch`] still fills it.
fn set_flex_main(layout: &mut LayoutStyle, flex_grow: f32, min_size: Size) {
    layout.flex_basis = Some(SizeValue::Px(0));
    layout.flex_grow = flex_grow;
    layout.min_size = min_size;
}

/// (R685 §5.21 §5.16; R1086 main-axis `min: 0`) Apply the CSS-canonical
/// `flex-basis: 0; flex-grow: <ratio>; min-<main>: 0` flex-item idiom
/// **directly** to a child `Scene`'s `LayoutStyle`. Returns the scene
/// with the flex props set, ready to drop into a parent flex container.
/// `orientation` selects which axis is the main axis the `min: 0`
/// applies to (`Horizontal` → width, `Vertical` → height).
///
/// Every `Scene` variant carrying a `layout: LayoutStyle` field
/// (`Container` / `Text` / `Box` / `Image` / `External` /
/// `ImmediateModeNode`) has its existing layout extended in place via
/// [`set_flex_main`]. The two variants without a `layout` field
/// (`Scroll` / `Effect`) auto-wrap in a thin Container carrying the same
/// flex props — both can still participate in flex distribution without
/// the caller having to write a wrapper manually.
///
/// ## Why direct application (not wrapping)
///
/// Pre-R685 [`view_splitter`] wrapped each child in an intermediate
/// `Scene::Container` carrying `flex_basis + flex_grow`. The
/// wrapper's `Display::Block` + `Size::Auto` made taffy treat its
/// flex-allocated space as indefinite for the inner layout pass —
/// the wrapper's child then sized to its intrinsic content rather
/// than to the wrapper's flex-distributed extent. Applying the
/// flex props directly on the child's own `LayoutStyle` eliminates
/// the indirection: taffy sees the child as a flex item of the
/// parent splitter directly, and the flex pass distributes parent
/// main-axis space proportionally without falling back to
/// intrinsic.
///
/// `Scroll` and `Effect` are auto-wrapped because the wrapper-free
/// path requires a `layout` field on the scene; the wrap is a
/// single Container with no `BoxStyle` (so the scene's visual
/// shape stays bit-identical) carrying just the flex props.
pub(crate) fn apply_flex_main(
    scene: Scene,
    flex_grow: f32,
    orientation: SplitterOrientation,
) -> Scene {
    // (R1086 §5.21) Zero the MAIN-axis minimum only; the cross axis stays
    // `Auto` so the outer container's `AlignItems::Stretch` fills it.
    let min_size = match orientation {
        SplitterOrientation::Horizontal => Size::auto().with_width(SizeValue::Px(0)),
        SplitterOrientation::Vertical => Size::auto().with_height(SizeValue::Px(0)),
    };
    match scene {
        Scene::Container(mut c) => {
            set_flex_main(&mut c.layout, flex_grow, min_size);
            Scene::Container(c)
        }
        Scene::Text(mut t) => {
            set_flex_main(&mut t.layout, flex_grow, min_size);
            Scene::Text(t)
        }
        Scene::Box(mut b) => {
            set_flex_main(&mut b.layout, flex_grow, min_size);
            Scene::Box(b)
        }
        Scene::Image(mut i) => {
            set_flex_main(&mut i.layout, flex_grow, min_size);
            Scene::Image(i)
        }
        Scene::External(mut e) => {
            set_flex_main(&mut e.layout, flex_grow, min_size);
            Scene::External(e)
        }
        Scene::ImmediateModeNode(mut im) => {
            set_flex_main(&mut im.layout, flex_grow, min_size);
            Scene::ImmediateModeNode(im)
        }
        // `Scroll` and `Effect` have no `layout` field. Plus the
        // `Scene` enum is `#[non_exhaustive]` — any future variant
        // without a `layout` field falls through to the same
        // auto-wrap branch. The wrapper carries no `BoxStyle` so
        // the visual shape stays bit-identical.
        other => Scene::Container(
            ContainerNode::new(vec![other]).with_layout(
                LayoutStyle::new()
                    .with_flex_basis(SizeValue::Px(0))
                    .with_flex_grow(flex_grow)
                    .with_min_size(min_size),
            ),
        ),
    }
}

/// (R683.B §5.16) Backend-agnostic Splitter composition.
///
/// Wraps the supplied `left` + `right` child `Scene`s in a flex
/// [`Scene::Container`] with three children:
///
/// 1. `left` panel — flex-grow `ratio` (the live `Signal<f32>` read).
/// 2. handle strip — fixed `handle_extent_px` along the main axis,
///    `Stretch` cross-axis, M3 `Outline` fill (linear-space
///    lerp toward `OnSurface` when `dragging`).
/// 3. `right` panel — flex-grow `1.0 - ratio`.
///
/// The outer Container carries [`SplitterStyle::tag`] so the
/// `InputRouter` routes pointer
/// events to the paired [`SplitterExternal`]. `dragging` is the
/// caller-resolved drag state read off the external's
/// [`SplitterExternal::is_dragging`] mirror; v1 expects the binding
/// to write it into a small reactive signal that the view fn
/// re-reads each paint (the [[abstraction-needs-second-consumer]]
/// gate fires when the second consumer makes the per-call boolean
/// awkward; pre-2nd consumer the boolean stays).
///
/// The ratio Signal is read inside an `Owner::run` wrap (the view
/// fn's surrounding scope), so the next `Signal::set` from a drag
/// re-runs the view automatically — no manual `redraw_requested`
/// arming required.
///
/// # Panics
///
/// Never panics on its own: `ratio.get()` clones the inner f32
/// without locks, and `flex_grow` accepts any f32 (NaN / Inf
/// degrade to taffy's degenerate layout but do not abort). The
/// helper does not clamp the read value because the drag wire's
/// [`SplitterExternal::pointer_move`] is the single mutator and it
/// clamps on write — a manual `signal.set(2.0)` from outside the
/// substrate would render an unbalanced layout, but that is a
/// caller contract issue not a substrate bug.
#[must_use]
pub fn view_splitter(
    left: Scene,
    right: Scene,
    ratio_signal: &Rc<Signal<f32>>,
    theme: &Theme,
    style: &SplitterStyle,
    dragging: bool,
) -> Scene {
    let ratio = ratio_signal.get();
    let one_minus_ratio = (1.0_f32 - ratio).max(0.0);
    // (R685 §5.21 §5.16) Handle cross-axis sizing — same R684 fix
    // [`view_dock_panel`]'s header strip already carries. Pre-R685 the
    // handle size was `Size::px(handle_extent_px, 0)` for Horizontal
    // and `Size::px(0, handle_extent_px)` for Vertical — the explicit
    // `0` on the cross axis pre-empted the splitter Container's
    // `AlignItems::Stretch` resolution (taffy treats `Px(0)` as a
    // *definite* size, not a *placeholder* for stretch). Layout
    // resolved the handle's cross axis to literal 0, which made
    // [`pinion_rpc::dispatch::find_rect_by_tag`] return `None` (its
    // `translate_rect_into_clip` helper rejects zero-area rects). Live
    // paint still showed the handle as a visible strip because the
    // Vello backend rasterises at the BoxStyle fill (rect → 0×4 +
    // stroke / fill pass interpolates), but AI-introspection paths
    // could not hit-test the handle.
    //
    // R685 uses [`Size::width_px`] / [`Size::height_px`] (R684 atomic
    // 0 substrate additions) so the cross axis stays `SizeValue::Auto`
    // and the outer Container's `AlignItems::Stretch` correctly
    // promotes the rect to the full cross-axis extent. Same fix
    // [`view_dock_panel`]'s header carries — substrate consistency
    // across both dock primitives.
    let handle_size = match style.orientation {
        SplitterOrientation::Horizontal => Size::width_px(style.handle_extent_px),
        SplitterOrientation::Vertical => Size::height_px(style.handle_extent_px),
    };
    // Handle stretches across the cross axis to fill the splitter's
    // perpendicular extent. The R685 cross-axis fix uses
    // [`Size::width_px`] / [`Size::height_px`] (cross-axis `Auto`) so the
    // outer Container's `AlignItems::Stretch` correctly promotes the
    // handle rect to the splitter's full cross-axis extent — the
    // pre-R685 `Size::px(handle_extent_px, 0)` form pinned cross-axis
    // to literal `Px(0)` and made the handle rect collapse to zero
    // area in the layout pass (live paint still drew the handle
    // because the Vello backend rasterises the BoxStyle fill against
    // the post-layout rect even at degenerate dimensions, but
    // AI-introspection hit-test paths could not find the handle).
    //
    // (R685 §5.16) Handle stays **untagged**. Composite-tag dispatch
    // (`{splitter_tag}#handle`) was considered + rejected: the
    // [`InputRouter::forward_pointer_move`] normalises the cursor
    // against the *composite tag's rect* (the handle's 4-pixel-wide
    // strip), but [`SplitterExternal::pointer_move`] expects the
    // cursor fraction to span the *splitter's full main-axis extent*
    // (so a drag of `Δratio = 0.5` corresponds to a cursor delta of
    // half the splitter width, not half the handle width). The
    // composite-tag dispatch would inflate `x_rel` by `splitter_w /
    // handle_w` (220×) and explode the ratio math past the [0.05,
    // 0.95] clamp on the very first frame. [`DockPanelExternal`] uses
    // composite-tag dispatch correctly because its tear-off threshold
    // is genuinely a fraction of the header rect — coord frames are
    // per-External.
    //
    // The deepest-tagged hit-test falls back to the splitter Container's
    // primary tag when the cursor lands on the (untagged) handle, which
    // is the textbook design: SplitterExternal receives drag events
    // with `x_rel` normalised against the splitter's full extent. AI
    // clients address the handle by computing its rect via
    // `scene/layout` + driving `scene/drag` with raw coordinates
    // (`{from: {x, y}, to: ...}`); the handle's main-axis position is
    // `splitter_extent * ratio`, where `ratio` is queryable through
    // the SplitterExternal's introspect schema (`ratio` slot).
    // (R1196 §5.16 §5.39) The handle declares its hover resize cursor per
    // orientation: a Horizontal splitter's handle is a VERTICAL divider between
    // side-by-side panes, dragged left-right (`↔`, col-resize); a Vertical
    // splitter's handle is a HORIZONTAL divider between stacked panes, dragged
    // up-down (`↕`, row-resize). The hint rides the handle's own `LayoutStyle`,
    // so the shell shows the cursor over the handle strip alone — the handle
    // stays untagged (its drag routes to the splitter's primary tag, R685), and
    // the cursor is resolved by pointer position, independent of that routing.
    let handle_cursor = match style.orientation {
        SplitterOrientation::Horizontal => CursorHint::ColResize,
        SplitterOrientation::Vertical => CursorHint::RowResize,
    };
    let handle = Scene::Container(
        ContainerNode::new(vec![])
            .with_style(BoxStyle::filled(handle_fill_for_dragging(theme, dragging)))
            .with_layout(
                LayoutStyle::new()
                    .with_size(handle_size)
                    .with_cursor(handle_cursor),
            ),
    );
    // (R685 §5.21 §5.16) Splitter ratio fix — substrate-canonical
    // form lands the flex-item props directly on each child Scene's
    // own `LayoutStyle`, eliminating the pre-R685 intermediate
    // wrapper Container.
    //
    // ## Why the wrapper-elimination matters
    //
    // The pre-R685 view_splitter wrapped each child in an
    // intermediate `Scene::Container` carrying `flex_basis(Px(0)) +
    // flex_grow(ratio)`. The intent: bind the flex-item props to
    // taffy's flex pass so the parent's main-axis space is
    // distributed proportionally. The defect: the wrapper itself
    // had `Display::Block` + `Size::Auto` (no explicit main-axis
    // size), and taffy treats Block flex items with Auto size as
    // having an **indefinite** main-axis size when laying out their
    // children. The wrapper's inner content (a recursive splitter
    // or dock panel) then sized to its own intrinsic content size,
    // not to the wrapper's flex-allocated space. Visible defect:
    // nested splitters' rendered extent collapsed toward the
    // content's intrinsic sum, not the declared `ratio` × parent
    // extent (the R685 5-pane editor topology made this glaring —
    // the topology occupied only the top ~50 % of the viewport).
    //
    // The R685 substrate-canonical fix applies the flex-item props
    // (`flex_basis` + `flex_grow`) **directly on the child Scene's
    // own `LayoutStyle`**. taffy now sees the child as a flex item
    // of the splitter Container directly — no wrapper indirection,
    // no Block-vs-Flex display-mode mismatch, no indefinite-size
    // fallback. The flex pass distributes parent main-axis
    // proportionally to `flex_grow` (with `flex_basis(Px(0))`
    // zeroing intrinsic basis per the CSS-canonical
    // "flex-basis: 0; flex-grow: r" idiom).
    //
    // Cross-axis (Y for Row, X for Column) stays governed by the
    // splitter Container's `AlignItems::Stretch` — the child's
    // cross-axis Auto size stretches to fill perpendicular extent.
    //
    // Pre-R685 the wrapper Container also served as a "uniform
    // shape" guarantee — children of arbitrary Scene variants
    // could be lowered through it. R685 uses [`apply_flex_main`]
    // which handles every variant with a `layout` field directly
    // and auto-wraps the two variants without one (`Scroll`,
    // `Effect`).
    let left_child = apply_flex_main(left, ratio, style.orientation);
    let right_child = apply_flex_main(right, one_minus_ratio, style.orientation);
    // R683.C / R686.A §5.16 — fill the splitter Container with the
    // active theme's `Surface` colour. This fill is **load-bearing**,
    // not a transient BLACK-clear workaround (the R685 Smell 12
    // "remove vs document" carry is resolved here as: keep + document).
    //
    // 1. Gutter / handle-gap background — the 4-px handle strip + the
    //    inter-panel gaps need a neutral backdrop. DCC / IDE / CAD
    //    shells all paint the splitter background under the panels;
    //    `ColorRole::Surface` (M3 Surface neutral) is the canonical
    //    pro-tool authoring backdrop.
    // 2. Root-clear sample source — when the splitter is the binding's
    //    root paint scene (every dock + tear-off application, by
    //    construction), `pinion_shell::paint_adapter::root_background`
    //    samples THIS Container's fill as the Vello surface clear
    //    colour (its fallback for a fill-less root is
    //    `PenikoColor::BLACK`, see `pinion_shell::VelloContext`). The
    //    fill is therefore exactly what prevents the BLACK clear —
    //    removing it would reintroduce the leak, not drop a redundant
    //    paint.
    //
    // Pinned by `r683_c_view_splitter_outer_container_filled_with_theme_surface`.
    Scene::Container(
        ContainerNode::new(vec![left_child, handle, right_child])
            .with_tag(style.tag.clone())
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(style.orientation.flex_direction())
                    .with_align_items(AlignItems::Stretch)
                    .with_justify(JustifyContent::Start),
            ),
    )
}

/// (R683.B §5.16) Cursor + ratio snapshot captured on the first
/// `pointer_move` under capture lock.
///
/// (R683.B §5.16) Draggable handle External for the
/// [`view_splitter`] paint helper. Captures the press → drag → release
/// span, projects normalised cursor delta into the shared
/// `Signal<f32>` ratio.
///
/// ## Wire
///
/// Registered by the binding via
/// [`WidgetCore::create_extra_externals`](pinion_core::WidgetCore::create_extra_externals)
/// tagged with the same [`SplitterStyle::tag`] the view fn carried.
/// The `InputRouter` routes
/// `PointerDown` on the splitter Container to this External,
/// [`Self::wants_pointer_capture`] returns `true` so the cursor
/// lock survives the press, and [`Self::pointer_move`] reads the
/// orientation-appropriate axis to drive ratio mutation.
///
/// ## Build
///
/// `SplitterExternal::new(orientation)` constructs an unattached
/// external (no ratio handle yet → drag wire is inert / paint-only
/// mode). The builder chain
/// [`Self::attach_ratio`] + [`Self::attach_bounds`] threads the
/// shared signal + clamp bounds. The bounds default to the
/// [`SplitterStyle::min_ratio`] / `max_ratio` the paint side carries
/// — keeping them attached as a builder lets the binding override
/// per-instance (a vertical pane with a `[0.2, 0.8]` clamp can
/// coexist with the default `[0.05, 0.95]` horizontal pane in the
/// same view fn).
///
/// ## Why no SCXML statechart
///
/// Unlike
/// [`ScrollBar`](pinion_core::widgets::scrollbar::ScrollBar) which
/// owns a full `{Idle, Hover, Dragging, Disabled}` SCXML graph,
/// `Splitter` v1 carries only the drag-in-progress boolean
/// ([`Self::is_dragging`]) — the M3 visual table currently uses
/// only `Idle` + `Dragging` (`handle_fill_for_dragging`). The
/// Hover / Disabled axes are deferred per
/// [[abstraction-needs-second-consumer]]: the splitter's hover
/// state visually overlaps with the cursor crossing the handle
/// strip (a 4 px region), and a v1 binding rendering hover state
/// without the matching cursor-shape hint (CSS `cursor: col-resize`)
/// would be misleading. R683.B atomic 3 / 4's first dock consumer
/// will surface the real hover requirement.
///
/// ## Two channels: live ratio vs. committed ratio (R1346)
///
/// Like [`SliderExternal`](pinion_core::widgets::slider::SliderExternal)
/// (`value_changing` / `value_committed`) and `ScrollBarExternal`
/// (`scroll_committed`), the splitter separates the live preview from
/// the settle edge:
///
/// * **live** — the attached `Signal<f32>` ratio handle
///   ([`Self::attach_ratio`]). Each captured `pointer_move` *after* the
///   press-time calibration frame writes it (the first only snapshots;
///   see [`Self::pointer_move`]), so subscribers repaint continuously
///   through the drag. Applications wanting a live preview read this
///   directly.
/// * **committed** — the `"ratio_committed"` §5.20 intent, carrying the
///   settled ratio as [`IntrospectValue::Float`], emitted once on the
///   `PointerUp` of a drag that actually moved the ratio (see
///   `commit_drag_state` for why a click must not qualify). This is
///   the `onChangeEnd` channel: persistence, analytics, mirroring
///   layout to a host process. Consumers must not infer the settle from
///   frame cadence — there is no guaranteed frame after a release (the
///   shell idles on `ControlFlow::Wait`), whereas this intent drains
///   inside the release dispatch itself.
///
/// ## Why the commit carries a payload when `ScrollBarExternal`'s does not
///
/// `scroll_committed` is a payload-less marker, and `scrollbar.rs`
/// gives the rule: no payload *because* the authoritative offset
/// already lives in `ScrollState` and reaches the application through
/// that signal stream. By that rule alone the splitter — whose
/// authoritative ratio likewise lives in a `Signal` the binding owns —
/// would be payload-less too. It carries the `f32` anyway, siding with
/// `value_committed` / `RangeSliderExternal` / `ColorAreaExternal` (the
/// family majority), for a reason the scrollbar's rule does not weigh:
/// §2 #2. An AI/RPC client harvesting this intent off the §5.20 channel
/// would otherwise have to chase it with a separate `scene/query` for
/// `ratio` to learn *what* settled — a second round-trip whose answer
/// can already have moved on. A self-contained commit is the
/// scene-as-data (§2 #7) shape: the edge and its value in one record.
///
/// The commit is emitted unprefixed; the §5.20 R22 walk namespaces it
/// with the `Scene::External` node's tag, so a binding whose splitter
/// node is tagged `"split"` matches `"split.ratio_committed"` in its
/// `WidgetCore::update` reducer ([[intent-tag-dotted-wire-form]]).
///
/// `PointerCancel` is deliberately silent and does **not** roll the
/// ratio back — see `clear_drag_state` for why (the family-wide
/// R51.93 §5.35 invariant).
pub struct SplitterExternal {
    orientation: SplitterOrientation,
    ratio: Option<Rc<Signal<f32>>>,
    min_ratio: f32,
    max_ratio: f32,
    /// R914 — the press-anchored capture-drag calibration ([`DragCalibration`],
    /// the SSOT shared with the R786 column resize + the R875 / R914 grid
    /// scrubs). The payload is the `ratio_at_press` (f32); the first move
    /// snapshots it + the cursor's anchor fraction, each later move yields the
    /// fraction delta the ratio is moved by. The cursor delta is already in
    /// ratio units (`x_rel` / `y_rel` are normalised to the splitter rect, a
    /// basis of `1.0`), so no pixel-width multiply is needed — the `[min_ratio,
    /// max_ratio]` clamp saturates a stray past the panel edge.
    drag: DragCalibration<f32>,
    /// R1346 §5.20 — pending intents for the `"ratio_committed"`
    /// drag-end channel (the [`SliderExternal`](pinion_core::widgets::slider::SliderExternal)
    /// `"value_committed"` peer). Plain `VecDeque`, not the
    /// `RefCell<VecDeque<_>>` the crate's dock Externals hold: those
    /// need interior mutability because `External::begin_drag(&self)`
    /// really does enqueue off a shared reference, whereas every
    /// splitter enqueue reaches here from `invoke(&mut self)`. No
    /// `&self` producer means no `RefCell`, hence no borrow-panic
    /// surface to reason about.
    pending_intents: VecDeque<Intent>,
}

impl core::fmt::Debug for SplitterExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SplitterExternal")
            .field("orientation", &self.orientation)
            .field("ratio_attached", &self.ratio.is_some())
            .field("min_ratio", &self.min_ratio)
            .field("max_ratio", &self.max_ratio)
            .field("is_dragging", &self.is_dragging())
            .finish_non_exhaustive()
    }
}

impl SplitterExternal {
    /// Construct an unattached splitter External for the given
    /// orientation. The drag wire is inert until
    /// [`Self::attach_ratio`] threads the shared
    /// [`Signal<f32>`](pinion_core::Signal) the
    /// [`view_splitter`] paint helper reads (paint-only mode for
    /// the unattached construction is intentional — useful for
    /// scene-snapshot tests + the demo bring-up sequence).
    #[must_use]
    pub fn new(orientation: SplitterOrientation) -> Self {
        Self {
            orientation,
            ratio: None,
            min_ratio: 0.05,
            max_ratio: 0.95,
            drag: DragCalibration::new(),
            pending_intents: VecDeque::new(),
        }
    }

    /// Builder: attach the shared `Signal<f32>` ratio handle. Drag
    /// wire becomes live; `pointer_move` under capture lock
    /// dispatches `Signal::set` with the projected ratio.
    #[must_use]
    pub fn attach_ratio(mut self, ratio: Rc<Signal<f32>>) -> Self {
        self.ratio = Some(ratio);
        self
    }

    /// Builder: override the ratio clamp bounds. Defaults are
    /// `[0.05, 0.95]` (matching [`SplitterStyle::m3_default`]).
    /// Caller is responsible for `0.0 <= min < max <= 1.0`;
    /// out-of-order inputs degrade the drag UX (`clamp(value, min,
    /// max)` saturates at `min` per the stdlib contract) but do
    /// not abort.
    #[must_use]
    pub fn attach_bounds(mut self, min_ratio: f32, max_ratio: f32) -> Self {
        self.min_ratio = min_ratio;
        self.max_ratio = max_ratio;
        self
    }

    /// Read the current orientation. Construction-time fixed —
    /// flipping orientation mid-binding would swap the cursor-axis
    /// projection without re-running the paint pipeline, leading to
    /// out-of-sync drag UX. The application constructs a fresh
    /// External when the dock surface flips a panel's split axis.
    #[must_use]
    pub fn orientation(&self) -> SplitterOrientation {
        self.orientation
    }

    /// Read the live drag-in-progress flag. `true` between the
    /// press-time `pointer_move` calibration and the
    /// `pointer_up` clear. The [`view_splitter`] paint helper reads
    /// this each paint to swap the handle fill toward `OnSurface`
    /// (the M3 dragged state-layer overlay).
    #[must_use]
    pub fn is_dragging(&self) -> bool {
        self.drag.is_active()
    }

    /// Currently-attached ratio Signal handle (diagnostic / test
    /// surface). `None` until [`Self::attach_ratio`] fires.
    #[must_use]
    pub fn ratio_signal(&self) -> Option<&Rc<Signal<f32>>> {
        self.ratio.as_ref()
    }

    /// Project the press-anchored ratio + the cursor-fraction `delta` into the
    /// clamped new ratio (`ratio_at_press + delta`, the basis-`1.0` application
    /// of the [`DragCalibration`] travel). Pure helper the [`Self::pointer_move`]
    /// dispatcher uses once the calibration has yielded a delta.
    fn project_ratio(&self, ratio_at_press: f32, delta: f32) -> f32 {
        (ratio_at_press + delta).clamp(self.min_ratio, self.max_ratio)
    }
}

impl External for SplitterExternal {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui, Backend::Rpc], BackendFallback::Skip)
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }

    /// R51.34 §5.15 + §5.35 — opt into capture lock so the cursor
    /// stays pinned to the splitter Container for the duration of
    /// the press, even when the cursor strays outside the rect.
    /// Required for the canonical splitter drag UX: the user can
    /// drag past the panel's clamped edge (or off the splitter
    /// entirely) without the press cancelling.
    fn wants_pointer_capture(&self) -> bool {
        true
    }

    /// R51.34 §5.15 + §5.35 — translate the cursor-fraction delta
    /// under capture lock into a ratio mutation via
    /// `Self::project_ratio` + `Signal::set` on the attached
    /// `ratio` handle.
    ///
    /// * **`Horizontal`**: `x_rel` drives, `y_rel` ignored.
    /// * **`Vertical`**: `y_rel` drives, `x_rel` ignored.
    ///
    /// First call (no `drag_start`) captures the press-time
    /// fraction + ratio snapshot and arms `is_dragging`; subsequent
    /// calls compute the delta and apply via `Signal::set` (R26
    /// equality-skip means no-mutation cursor jiggles do not
    /// trigger re-runs). The signal-side
    /// [`view_splitter`] subscriber re-runs the view on each
    /// `Signal::set`, painting the new ratio without manual
    /// redraw arming.
    ///
    /// No-op without an attached `ratio` handle (paint-only mode —
    /// the demo's bring-up sequence lands the visible peer first +
    /// wires the reactive state later).
    #[allow(
        clippy::cast_possible_truncation,
        reason = "the cursor fraction is in [~-1, ~2] under the capture lock; the \
                  f64 delta narrows to f32 with no meaningful precision loss for a \
                  ratio in [0, 1]"
    )]
    fn pointer_move(&mut self, x_rel: f32, y_rel: f32) {
        let cursor_fraction = match self.orientation {
            SplitterOrientation::Horizontal => x_rel,
            SplitterOrientation::Vertical => y_rel,
        };
        let Some(ratio_handle) = self.ratio.as_ref() else {
            return;
        };
        // R914 — the first move calibrates (snapshot `ratio_at_press` + the
        // cursor anchor, no mutation); each later move yields the fraction delta.
        if let Some((ratio_at_press, delta)) = self
            .drag
            .drive(f64::from(cursor_fraction), || Some(ratio_handle.get()))
        {
            ratio_handle.set(self.project_ratio(ratio_at_press, delta as f32));
        }
    }

    /// Surface [`ExternalIntrospect`] so the framework's pointer
    /// dispatch (R51.41 §5.15) can hand `PointerUp` / `PointerCancel`
    /// teardown events into [`Self::invoke`] — the canonical
    /// drag-release notification path.
    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        Some(self)
    }

    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        Some(self)
    }

    /// R1346 §5.20 — `true` while a `"ratio_committed"` intent is
    /// queued (the short-circuit that spares the `drain_intents`
    /// virtual call on the overwhelmingly common no-commit dispatch).
    fn is_dirty(&self) -> bool {
        !self.pending_intents.is_empty()
    }

    /// R1346 §5.20 — flush the drag-end commit channel into `sink`.
    /// Drained by `walk_scene_and_drain` from `CoreShell::tail`, i.e.
    /// within the same input dispatch that delivered the `PointerUp`.
    fn drain_intents(&mut self, sink: &mut dyn FnMut(Intent)) {
        while let Some(intent) = self.pending_intents.pop_front() {
            sink(intent);
        }
    }
}

/// (R683.B §5.16) Reset drag state — drops the `drag_start`
/// calibration snapshot + clears `is_dragging`. Called from
/// [`ExternalIntrospect::invoke`] on the framework-dispatched
/// `PointerCancel` symbolic event (R51.41 §5.35 `dispatch_send`
/// channel — the canonical drag-release notification path Pinion
/// externals receive). Also reachable from tests via the introspect
/// surface so the teardown path is exercised end-to-end without
/// spinning up the full `InputRouter` capture lock.
///
/// R1346 §5.20 — the **silent** half of the release pair. Cancel tears
/// the calibration down and emits nothing, and the ratio the in-flight
/// `pointer_move`s already applied **stays applied**. That is not an
/// oversight: it is the family-wide R51.93 §5.35 invariant this
/// splitter now joins, pinned for the peer widget by
/// `slider.rs::r51_93_pointer_cancel_during_drag_returns_to_idle_without_commit`
/// ("`set_value` during drag stays applied across `PointerCancel`"). The
/// rationale carries over verbatim — the ratio is a continuous-domain
/// sidecar, not a commit-bound enum, so the live `Signal` writes are
/// honest reports of where the drag was; the OS revoked the *commit*
/// signal, not the user's in-flight updates. A binding that persists
/// only on [`commit_drag_state`] therefore keeps the pre-drag ratio on
/// disk, which is precisely what "the gesture was aborted" should mean.
fn clear_drag_state(ext: &SplitterExternal) {
    ext.drag.end();
}

/// R1346 §5.20 — the **committing** half of the release pair, called
/// from [`ExternalIntrospect::invoke`] on the framework-dispatched
/// `PointerUp`.
///
/// Emits `"ratio_committed"` carrying the settled ratio. This is the
/// `onChangeEnd` channel: the live `Signal<f32>` stream is the preview,
/// this edge is "the drag ended, that was the final ratio, persist it
/// now". Bindings that mirror layout to a host process / disk write
/// here rather than heuristically inferring a settle from frame
/// cadence — the drain is input-driven (`CoreShell::tail` runs at the
/// end of the very dispatch that delivered this `PointerUp`), so the
/// commit needs no follow-up frame and survives `ControlFlow::Wait`
/// idling.
///
/// ## The gate: "the ratio moved", not "a drag calibrated"
///
/// The obvious gate — [`DragCalibration::end`]'s bool — is **wrong
/// here**, and subtly so. On a press over a widget that opts into
/// `wants_pointer_capture`, `InputRouter::pointer_down` forwards the
/// press-time cursor to that widget as an initial `pointer_move`
/// (R51.35 click-to-position, pinned by `pinion-runtime`
/// `input.rs::pointer_down_forwards_initial_cursor`), and this splitter
/// opts in — so a **bare click arms the anchor** and
/// `end()` returns `true` with zero travel. Gating on it makes every
/// click on the handle emit a commit for a ratio that never moved, and
/// a double-click emit two: spurious writes on the one channel whose
/// entire purpose is persistence.
///
/// This is also exactly where the
/// [`SliderExternal`](pinion_core::widgets::slider::SliderExternal)
/// analogy breaks, so it is worth naming. The slider positions
/// **absolutely** — its `pointer_move` writes `set_value` on every
/// move, press-time forward included, so a track click genuinely moves
/// the value and committing that release is right. The splitter is
/// **press-anchored / relative**: the calibration frame mutates
/// nothing by design. The two widgets are homomorphic in payload
/// shape, not on this edge.
///
/// So the honest predicate for a persistence channel is *did this
/// gesture settle on a different ratio than it started from?* —
/// answered by comparing the released value against the press snapshot
/// [`DragCalibration::end_payload`] hands back (the `ratio_at_press`
/// that plain `end()` discards). A click compares equal and stays
/// silent; so does a drag that wanders and returns home, or one that
/// spends its whole travel saturated against a clamp bound — in every
/// one of those there is genuinely nothing new to persist. Paint-only
/// mode (no attached ratio) is silent too.
fn commit_drag_state(ext: &mut SplitterExternal) {
    // Tear the calibration down FIRST and unconditionally — every
    // early return below must still leave a fresh cycle behind.
    let Some(ratio_at_press) = ext.drag.end_payload() else {
        return;
    };
    let Some(ratio) = ext.ratio.as_ref() else {
        return;
    };
    let settled = ratio.get();
    // Exact comparison deliberately mirrors `Signal::set`'s own R26
    // equality-skip: "the Signal holds something other than what the
    // press snapshotted" is precisely the question, and the Signal
    // decides changed-ness by the same `==`.
    #[allow(
        clippy::float_cmp,
        reason = "mirrors Signal::set's equality-skip — the question is literally \
                  whether the stored f32 differs from the press snapshot, not \
                  whether it differs by some tolerance"
    )]
    let unmoved = settled == ratio_at_press;
    if unmoved {
        return;
    }
    ext.pending_intents.push_back(Intent::new_static(
        pinion_core::widgets::commit::RATIO_COMMITTED_EVENT,
        IntrospectValue::Float(f64::from(settled)),
    ));
}

impl ExternalIntrospect for SplitterExternal {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(
            const {
                &[
                    SchemaField::new("orientation", "string"),
                    SchemaField::new("ratio", "float"),
                    SchemaField::new("dragging", "bool"),
                    SchemaField::new("send", "string"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "orientation" => Some(IntrospectValue::Text(match self.orientation {
                SplitterOrientation::Horizontal => "horizontal".to_string(),
                SplitterOrientation::Vertical => "vertical".to_string(),
            })),
            "ratio" => self
                .ratio
                .as_ref()
                .map(|r| IntrospectValue::Float(f64::from(r.get()))),
            "dragging" => Some(IntrospectValue::Bool(self.is_dragging())),
            _ => None,
        }
    }

    fn intervene(&mut self, path: &str, _value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            // `orientation` is construction-time fixed (the SCXML +
            // ARIA semantics differ between axes — a runtime flip
            // would change the meaning of in-flight `pointer_move`
            // dispatches). `ratio` flows through the shared
            // `Signal<f32>` handle the binding owns + the
            // `Self::pointer_move` mutates; AI clients drive it
            // through the binding's reactive substrate not through
            // intervene. `dragging` is framework-owned (the
            // `pointer_move` / `invoke` arc sets it).
            "orientation" | "ratio" | "dragging" => Err(InterveneError::ReadOnly),
            _ => Err(InterveneError::UnknownPath),
        }
    }

    /// R51.41 §5.15 §5.35 — framework synthetic event channel.
    ///
    /// The `InputRouter` calls `invoke("send", Text("PointerUp"))` /
    /// `invoke("send", Text("PointerCancel"))` whenever the user
    /// releases or the OS cancels the pointer capture span the
    /// External holds. The splitter clears its drag calibration
    /// snapshot + the `is_dragging` flag so the next press starts a
    /// fresh cycle, and on `PointerUp` — not on `PointerCancel` —
    /// queues the R1346 `"ratio_committed"` intent
    /// (`commit_drag_state` / `clear_drag_state`).
    /// `PointerDown` / `PointerEnter` / `PointerLeave`
    /// arrive too but the splitter ignores them — drag calibration
    /// happens in the first `pointer_move` under capture lock, not
    /// in the `PointerDown` arrival.
    ///
    /// AI clients can drive the same teardown via the RPC `scene/
    /// invoke {window: "...", target: "splitter_tag", path: "send",
    /// args: "PointerUp"}` shape — the demo's drag-release
    /// assertion goes through this channel to verify the substrate
    /// behaviour without RPC needing a separate "drag end" method.
    ///
    /// R1346 — but note that a bare `scene/invoke send PointerUp` is a
    /// **teardown, not a commit**: nothing over that path ever
    /// calibrated a drag (`pointer_move` is not an introspect slot and
    /// `intervene` refuses `ratio` as `ReadOnly`), so there is no
    /// press snapshot to have moved away from and
    /// `commit_drag_state` correctly stays silent. The RPC verb that
    /// *does* produce a `"ratio_committed"` — with full §2 #2 parity,
    /// because it feeds the very same `InputRouter` arm the physical
    /// release takes — is **`scene/drag`**, which synthesizes
    /// press → interpolated moves → release.
    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        if path != "send" {
            return Err(InvokeError::UnknownPath);
        }
        let event_name = args.as_str().ok_or(InvokeError::TypeMismatch)?;
        match PointerWireEvent::from_wire_name(event_name) {
            // R1346 §5.20 — the release pair diverges only in the
            // commit channel: `Up` settles the gesture and emits
            // `"ratio_committed"`, `Cancel` tears down silently
            // (R51.93 §5.35). Both drop the calibration, so either way
            // the next press starts a fresh cycle.
            Some(PointerWireEvent::Up) => {
                commit_drag_state(self);
                Ok(IntrospectValue::Null)
            }
            Some(PointerWireEvent::Cancel) => {
                clear_drag_state(self);
                Ok(IntrospectValue::Null)
            }
            // PointerDown / PointerEnter / PointerLeave: framework
            // sends them but the splitter does not need to react —
            // calibration happens in the first pointer_move under
            // capture lock. Returning Ok keeps the dispatch silent.
            Some(PointerWireEvent::Down | PointerWireEvent::Enter | PointerWireEvent::Leave) => {
                Ok(IntrospectValue::Null)
            }
            None => Err(InvokeError::UnknownPath),
        }
    }
}

#[cfg(test)]
mod tests {
    //! R683.B §5.16 — Splitter paint + drag wire unit tests.
    //!
    //! Pins the load-bearing invariants the dock + tear-off arc
    //! relies on:
    //!
    //! 1. **Paint shape**: outer Container carries `tag` + 3 children
    //!    (left flex-grow ratio, handle fixed-size, right flex-grow
    //!    1-ratio). Orientation maps to flex direction.
    //! 2. **Ratio read-through**: changing the `Signal<f32>` ratio
    //!    repaints the children with the matching `flex_grow`
    //!    values.
    //! 3. **Handle visual swap**: `dragging=true` lerps the handle
    //!    fill toward `OnSurface`; `dragging=false` keeps `Outline`.
    //! 4. **Drag projection**: press-time `pointer_move` calibrates
    //!    `drag_start`; subsequent moves compute
    //!    `clamp(ratio_at_press + cursor_delta, min, max)` and
    //!    dispatch `Signal::set`.
    //! 5. **Capture release**: `PointerUp` / `PointerCancel` events
    //!    clear `drag_start` + `is_dragging`.
    //! 6. **Orientation invariant**: `Horizontal` uses `x_rel`,
    //!    `Vertical` uses `y_rel`; the cross-axis is ignored.
    //! 7. **Bounds clamp**: drag past `min` / `max` saturates at the
    //!    bound without aborting.
    //! 8. **Paint-only mode**: External without an attached
    //!    `ratio_signal` is inert (no panic on pointer events).

    use super::{
        Intent, SplitterExternal, SplitterOrientation, SplitterStyle, apply_flex_main,
        handle_fill_for_dragging, view_splitter,
    };
    use pinion_core::external::{External, ExternalIntrospect, IntrospectValue};
    use pinion_core::reactive::{Owner, Signal};
    use pinion_core::scene::{ContainerNode, Scene};
    use pinion_core::style::FlexDirection;
    use pinion_core::theme::{ColorRole, Theme};
    use std::rc::Rc;

    const TEST_TAG: &str = "test_splitter";

    fn run_in_owner<R>(f: impl FnOnce() -> R) -> R {
        Owner::new().run(f)
    }

    fn empty_panel(tag: &'static str) -> Scene {
        Scene::Container(ContainerNode::new(vec![]).with_tag(tag))
    }

    fn theme_light() -> Theme {
        Theme::light()
    }

    #[test]
    fn r683_c_view_splitter_outer_container_filled_with_theme_surface() {
        // R683.C §5.16 — splitter Container fills with the active
        // theme's `Surface` so the `VelloContext`'s BLACK default
        // clear is overridden when the splitter is the binding's
        // root paint scene. Without this, dock + tear-off
        // applications painted on a black-cleared canvas with un-
        // filled splitter strips leaking through.
        run_in_owner(|| {
            let ratio: Rc<Signal<f32>> = Rc::new(Signal::new(0.5));
            let theme = theme_light();
            let style = SplitterStyle::m3_default(SplitterOrientation::Horizontal, TEST_TAG);
            let scene = view_splitter(
                empty_panel("left_panel"),
                empty_panel("right_panel"),
                &ratio,
                &theme,
                &style,
                false,
            );
            let Scene::Container(outer) = &scene else {
                panic!()
            };
            assert_eq!(
                outer.style.fill,
                theme.resolve(ColorRole::Surface),
                "splitter outer Container must fill with ColorRole::Surface so VelloContext BLACK clear is hidden",
            );
        });
    }

    #[test]
    fn r685_view_splitter_handle_stays_untagged_for_coord_frame_correctness() {
        // (R685 §5.16) The handle Container is intentionally
        // **untagged**. Composite-tag dispatch
        // (`{splitter_tag}#handle`) would route via
        // [`InputRouter::forward_pointer_move`] which normalises
        // `x_rel` / `y_rel` against the *composite tag's rect* — for
        // the handle, that's a 4-pixel-wide strip, blowing up
        // [`SplitterExternal::pointer_move`]'s ratio math (the
        // External expects normalisation against the splitter's full
        // extent). The textbook design lets the handle inherit its
        // deepest-tagged-ancestor via hit-test fallback: a cursor on
        // the handle Container resolves to the splitter's primary tag,
        // and `forward_pointer_move` normalises against the splitter's
        // full rect. AI clients address the handle by querying the
        // splitter's `ratio` slot + computing handle position from the
        // splitter's `scene/layout` rect, then driving `scene/drag`
        // with raw `{from: {x, y}, to: ...}` coordinates.
        run_in_owner(|| {
            let ratio: Rc<Signal<f32>> = Rc::new(Signal::new(0.5));
            let style = SplitterStyle::m3_default(SplitterOrientation::Horizontal, TEST_TAG);
            let scene = view_splitter(
                empty_panel("left_panel"),
                empty_panel("right_panel"),
                &ratio,
                &theme_light(),
                &style,
                false,
            );
            let Scene::Container(outer) = &scene else {
                panic!()
            };
            let Scene::Container(handle) = &outer.children[1] else {
                panic!("middle child should be the handle Container");
            };
            assert!(
                handle.tag.is_none(),
                "handle stays untagged (coord-frame correctness over composite-tag convenience)",
            );
        });
    }

    #[test]
    fn r683_view_splitter_outer_container_carries_tag_and_three_children() {
        run_in_owner(|| {
            let ratio: Rc<Signal<f32>> = Rc::new(Signal::new(0.5));
            let style = SplitterStyle::m3_default(SplitterOrientation::Horizontal, TEST_TAG);
            let scene = view_splitter(
                empty_panel("left_panel"),
                empty_panel("right_panel"),
                &ratio,
                &theme_light(),
                &style,
                false,
            );
            let Scene::Container(outer) = &scene else {
                panic!("outer scene must be a Container")
            };
            assert_eq!(outer.tag.as_deref(), Some(TEST_TAG));
            assert_eq!(outer.children.len(), 3);
            assert_eq!(
                outer.layout.flex_direction,
                FlexDirection::Row,
                "Horizontal orientation maps to Row flex direction",
            );
        });
    }

    #[test]
    fn r683_view_splitter_vertical_uses_column_flex_direction() {
        run_in_owner(|| {
            let ratio: Rc<Signal<f32>> = Rc::new(Signal::new(0.5));
            let style = SplitterStyle::m3_default(SplitterOrientation::Vertical, TEST_TAG);
            let scene = view_splitter(
                empty_panel("top_panel"),
                empty_panel("bottom_panel"),
                &ratio,
                &theme_light(),
                &style,
                false,
            );
            let Scene::Container(outer) = &scene else {
                panic!("outer scene must be a Container")
            };
            assert_eq!(outer.layout.flex_direction, FlexDirection::Column);
        });
    }

    #[test]
    fn r683_view_splitter_flex_grow_reflects_ratio_signal() {
        run_in_owner(|| {
            let ratio: Rc<Signal<f32>> = Rc::new(Signal::new(0.3));
            let style = SplitterStyle::m3_default(SplitterOrientation::Horizontal, TEST_TAG);
            let scene = view_splitter(
                empty_panel("left_panel"),
                empty_panel("right_panel"),
                &ratio,
                &theme_light(),
                &style,
                false,
            );
            let Scene::Container(outer) = &scene else {
                panic!("outer scene must be a Container")
            };
            // children[0] = left wrapper (flex_grow=0.3),
            // children[1] = handle (flex_grow=0.0 default),
            // children[2] = right wrapper (flex_grow=0.7)
            let left_grow = match &outer.children[0] {
                Scene::Container(c) => c.layout.flex_grow,
                _ => panic!("left wrapper must be Container"),
            };
            let right_grow = match &outer.children[2] {
                Scene::Container(c) => c.layout.flex_grow,
                _ => panic!("right wrapper must be Container"),
            };
            assert!((left_grow - 0.3).abs() < f32::EPSILON);
            assert!((right_grow - 0.7).abs() < f32::EPSILON);
        });
    }

    #[test]
    fn r683_view_splitter_ratio_re_emit_repaints_with_new_grow() {
        run_in_owner(|| {
            let ratio: Rc<Signal<f32>> = Rc::new(Signal::new(0.2));
            let style = SplitterStyle::m3_default(SplitterOrientation::Horizontal, TEST_TAG);
            let first = view_splitter(
                empty_panel("l"),
                empty_panel("r"),
                &ratio,
                &theme_light(),
                &style,
                false,
            );
            ratio.set(0.7);
            let second = view_splitter(
                empty_panel("l"),
                empty_panel("r"),
                &ratio,
                &theme_light(),
                &style,
                false,
            );
            let extract_grow = |s: &Scene, idx: usize| {
                let Scene::Container(c) = s else {
                    panic!("outer must be Container")
                };
                let Scene::Container(child) = &c.children[idx] else {
                    panic!("flex wrapper must be Container")
                };
                child.layout.flex_grow
            };
            assert!((extract_grow(&first, 0) - 0.2).abs() < f32::EPSILON);
            assert!((extract_grow(&second, 0) - 0.7).abs() < f32::EPSILON);
            assert!((extract_grow(&second, 2) - 0.3).abs() < f32::EPSILON);
        });
    }

    #[test]
    fn r683_handle_fill_dragging_lerps_toward_on_surface() {
        // The dragging state swaps the handle fill toward the
        // M3 OnSurface tint at 0.16 opacity; the idle state stays
        // at the raw Outline role.
        let theme = theme_light();
        let outline = theme.resolve(ColorRole::Outline);
        let idle = handle_fill_for_dragging(&theme, false);
        let dragging = handle_fill_for_dragging(&theme, true);
        assert_eq!(idle, outline);
        assert_ne!(
            dragging, outline,
            "dragging state must lerp the fill toward OnSurface",
        );
    }

    #[test]
    fn r683_view_splitter_handle_fill_swaps_with_dragging_arg() {
        run_in_owner(|| {
            let ratio: Rc<Signal<f32>> = Rc::new(Signal::new(0.5));
            let style = SplitterStyle::m3_default(SplitterOrientation::Horizontal, TEST_TAG);
            let theme = theme_light();
            let idle_scene = view_splitter(
                empty_panel("l"),
                empty_panel("r"),
                &ratio,
                &theme,
                &style,
                false,
            );
            let drag_scene = view_splitter(
                empty_panel("l"),
                empty_panel("r"),
                &ratio,
                &theme,
                &style,
                true,
            );
            let handle_fill = |s: &Scene| {
                let Scene::Container(c) = s else {
                    panic!("outer must be Container")
                };
                let Scene::Container(handle) = &c.children[1] else {
                    panic!("handle must be Container")
                };
                handle.style.fill
            };
            assert_ne!(handle_fill(&idle_scene), handle_fill(&drag_scene));
        });
    }

    #[test]
    fn r683_splitter_style_m3_default_carries_canonical_4px_handle() {
        let style = SplitterStyle::m3_default(SplitterOrientation::Horizontal, TEST_TAG);
        assert_eq!(style.handle_extent_px, 4);
        assert!((style.min_ratio - 0.05).abs() < f32::EPSILON);
        assert!((style.max_ratio - 0.95).abs() < f32::EPSILON);
        assert_eq!(style.tag, TEST_TAG);
        // (R685.B) Cow<'static, str> tag accepts Owned String too —
        // dynamic split ids (e.g. R686 dock-reorganize) flow through.
        let dynamic_style = SplitterStyle::m3_default(
            SplitterOrientation::Horizontal,
            format!("runtime_split_{}", 42),
        );
        assert_eq!(dynamic_style.tag.as_ref(), "runtime_split_42");
    }

    #[test]
    fn r686_a_apply_flex_main_auto_wraps_layoutless_scenes() {
        // (R686.A Smell 11) `apply_flex_main`'s `other =>` arm
        // auto-wraps the `Scene` variants without a `layout` field
        // (`Effect`, `Scroll`) in a thin flex Container carrying
        // `flex_basis(Px(0)) + flex_grow`. Pre-R686.A the branch was
        // exercised only indirectly; pinning it here guards the
        // wrapper contract every future no-`layout` Scene variant
        // inherits (the `Scene` enum is `#[non_exhaustive]`). The
        // wrapper must carry the flex props, hold exactly the original
        // scene, and add no tag (visual shape bit-identical).
        use pinion_core::scene::{EffectNode, Rect, ScrollNode};
        use pinion_core::style::SizeValue;

        let cases = [
            Scene::Effect(EffectNode::new()),
            Scene::Scroll(ScrollNode::new(
                Rect::default(),
                Scene::Container(ContainerNode::new(vec![])),
            )),
        ];
        for layoutless in cases {
            let wrapped = apply_flex_main(layoutless, 0.42, SplitterOrientation::Horizontal);
            let Scene::Container(c) = &wrapped else {
                panic!("layout-less scene must auto-wrap in a Container");
            };
            assert_eq!(
                c.layout.flex_basis,
                Some(SizeValue::Px(0)),
                "auto-wrap must zero the flex basis (CSS flex-basis:0 idiom)",
            );
            assert!(
                (c.layout.flex_grow - 0.42).abs() < f32::EPSILON,
                "auto-wrap must carry the requested flex_grow",
            );
            // (R1086 §5.21) The wrapper also zeroes the main-axis min so a
            // big-content Scroll/Effect distributes by ratio (Horizontal →
            // width); the cross axis stays Auto for Stretch.
            assert_eq!(
                c.layout.min_size.width,
                SizeValue::Px(0),
                "auto-wrap must zero the main-axis (width) min",
            );
            assert_eq!(
                c.layout.min_size.height,
                SizeValue::Auto,
                "cross-axis (height) min stays Auto for Stretch",
            );
            assert_eq!(
                c.children.len(),
                1,
                "wrapper holds exactly the original scene",
            );
            assert!(
                c.tag.is_none(),
                "wrapper carries no tag (bit-identical visual shape)",
            );
        }
    }

    /// (R1086 §5.21) A dock-style panel whose content is taller than its
    /// ratio share — a fixed `content_h`-px box, mirroring a terminal
    /// grid / image whose intrinsic main-axis size overflows a small slot.
    fn big_panel(tag: &'static str, content_h: u32) -> Scene {
        use pinion_core::scene::{BoxNode, Rect};
        use pinion_core::style::{Color, LayoutStyle, Size};
        Scene::Container(
            ContainerNode::new(vec![Scene::Box(
                BoxNode::filled(Rect::default(), Color::TRANSPARENT)
                    .with_layout(LayoutStyle::new().with_size(Size::px(100, content_h))),
            )])
            .with_tag(tag),
        )
    }

    #[test]
    fn r1196_view_splitter_handle_declares_orientation_cursor() {
        use pinion_core::style::CursorHint;
        run_in_owner(|| {
            let ratio: Rc<Signal<f32>> = Rc::new(Signal::new(0.5));
            let handle_cursor = |s: &Scene| -> Option<CursorHint> {
                let Scene::Container(outer) = s else {
                    panic!("outer container")
                };
                // Handle is the middle child (left/top, HANDLE, right/bottom).
                outer.children[1].cursor_hint()
            };
            // Horizontal (side-by-side panes) → vertical divider → col-resize.
            let h = view_splitter(
                empty_panel("left"),
                empty_panel("right"),
                &ratio,
                &theme_light(),
                &SplitterStyle::m3_default(SplitterOrientation::Horizontal, TEST_TAG),
                false,
            );
            assert_eq!(handle_cursor(&h), Some(CursorHint::ColResize));
            // Vertical (stacked panes) → horizontal divider → row-resize.
            let v = view_splitter(
                empty_panel("top"),
                empty_panel("bottom"),
                &ratio,
                &theme_light(),
                &SplitterStyle::m3_default(SplitterOrientation::Vertical, TEST_TAG),
                false,
            );
            assert_eq!(handle_cursor(&v), Some(CursorHint::RowResize));
        });
    }

    #[test]
    fn r1086_view_splitter_zeroes_main_axis_min_on_ratio_children() {
        use pinion_core::style::SizeValue;
        run_in_owner(|| {
            let ratio: Rc<Signal<f32>> = Rc::new(Signal::new(0.5));
            // Vertical splitter: main axis = height.
            let v = view_splitter(
                empty_panel("top"),
                empty_panel("bottom"),
                &ratio,
                &theme_light(),
                &SplitterStyle::m3_default(SplitterOrientation::Vertical, TEST_TAG),
                false,
            );
            let Scene::Container(outer) = &v else {
                panic!("outer")
            };
            for idx in [0usize, 2] {
                let Scene::Container(child) = &outer.children[idx] else {
                    panic!("vertical panel {idx}")
                };
                assert_eq!(
                    child.layout.min_size.height,
                    SizeValue::Px(0),
                    "vertical: main-axis (height) min zeroed",
                );
                assert_eq!(
                    child.layout.min_size.width,
                    SizeValue::Auto,
                    "vertical: cross-axis (width) min stays Auto for Stretch",
                );
            }
            // Horizontal splitter: main axis = width.
            let h = view_splitter(
                empty_panel("left"),
                empty_panel("right"),
                &ratio,
                &theme_light(),
                &SplitterStyle::m3_default(SplitterOrientation::Horizontal, TEST_TAG),
                false,
            );
            let Scene::Container(outer) = &h else {
                panic!("outer")
            };
            for idx in [0usize, 2] {
                let Scene::Container(child) = &outer.children[idx] else {
                    panic!("horizontal panel {idx}")
                };
                assert_eq!(
                    child.layout.min_size.width,
                    SizeValue::Px(0),
                    "horizontal: main-axis (width) min zeroed",
                );
                assert_eq!(
                    child.layout.min_size.height,
                    SizeValue::Auto,
                    "horizontal: cross-axis (height) min stays Auto for Stretch",
                );
            }
        });
    }

    #[test]
    fn r1086_vertical_splitter_distributes_large_content_both_panels_onscreen() {
        use pinion_runtime::layout::compute_layout;
        use pinion_text::LayoutCache;
        run_in_owner(|| {
            let ratio: Rc<Signal<f32>> = Rc::new(Signal::new(0.5));
            let style = SplitterStyle::m3_default(SplitterOrientation::Vertical, TEST_TAG);
            // Each panel's content is 800px tall — larger than the 0.5
            // share of a 600px viewport. Pre-R1086 (no main-axis min:0)
            // both clamp to 800 and the bottom panel falls off-screen
            // (the sprag P2 drag-to-dock symptom this PR fixes).
            let scene = view_splitter(
                big_panel("top", 800),
                big_panel("bottom", 800),
                &ratio,
                &theme_light(),
                &style,
                false,
            );
            let mut cache = LayoutCache::new();
            let mut scene = scene;
            compute_layout(&mut scene, &mut cache, 960, 600);
            let Scene::Container(outer) = &scene else {
                panic!("outer")
            };
            let Scene::Container(top) = &outer.children[0] else {
                panic!("top")
            };
            let Scene::Container(handle) = &outer.children[1] else {
                panic!("handle")
            };
            let Scene::Container(bottom) = &outer.children[2] else {
                panic!("bottom")
            };
            // Distributed by ratio over the (600 - handle) budget — each
            // ~half — NOT clamped to the 800px content.
            let expected = (600 - style.handle_extent_px) / 2;
            assert!(
                top.rect.h.abs_diff(expected) <= 2,
                "top distributes by ratio, h={} (~{expected})",
                top.rect.h,
            );
            // The bottom panel stays on-screen (the acceptance bar).
            assert!(
                bottom.rect.y + bottom.rect.h <= 600,
                "bottom panel must stay on-screen: y={} h={}",
                bottom.rect.y,
                bottom.rect.h,
            );
            // The handle keeps its thickness — it compressed to 0 under
            // the pre-fix overflow.
            assert_eq!(
                handle.rect.h, style.handle_extent_px,
                "handle keeps its thickness when distribution succeeds",
            );
            // Panels + handle fill the viewport exactly.
            assert_eq!(
                top.rect.h + handle.rect.h + bottom.rect.h,
                600,
                "panels + handle cover the viewport",
            );
        });
    }

    #[test]
    fn r683_splitter_external_first_pointer_move_calibrates_drag_start() {
        // Press-time frame captures the snapshot; ratio stays
        // unchanged because the user has not dragged yet (just
        // pressed). Subsequent moves apply the delta.
        let ratio: Rc<Signal<f32>> = Rc::new(Signal::new(0.5));
        let mut ext =
            SplitterExternal::new(SplitterOrientation::Horizontal).attach_ratio(Rc::clone(&ratio));
        assert!(!ext.is_dragging());
        ext.pointer_move(0.5, 0.0);
        assert!(ext.is_dragging(), "first pointer_move arms is_dragging");
        assert!(
            (ratio.get() - 0.5).abs() < f32::EPSILON,
            "press-time frame must not mutate the ratio",
        );
    }

    #[test]
    fn r683_splitter_external_subsequent_pointer_move_applies_delta() {
        let ratio: Rc<Signal<f32>> = Rc::new(Signal::new(0.5));
        let mut ext =
            SplitterExternal::new(SplitterOrientation::Horizontal).attach_ratio(Rc::clone(&ratio));
        ext.pointer_move(0.5, 0.0);
        // Cursor moves +0.2 along x — ratio becomes 0.5 + 0.2 = 0.7.
        ext.pointer_move(0.7, 0.0);
        assert!(
            (ratio.get() - 0.7).abs() < 1e-5,
            "expected ratio 0.7, got {}",
            ratio.get(),
        );
    }

    #[test]
    fn r683_splitter_external_clamps_to_max_bound() {
        let ratio: Rc<Signal<f32>> = Rc::new(Signal::new(0.5));
        let mut ext =
            SplitterExternal::new(SplitterOrientation::Horizontal).attach_ratio(Rc::clone(&ratio));
        ext.pointer_move(0.5, 0.0);
        // Cursor strays past the right edge under capture lock —
        // x_rel reaches 1.5 (0.5 + 1.0). Ratio should saturate at
        // max_ratio = 0.95.
        ext.pointer_move(1.5, 0.0);
        assert!(
            (ratio.get() - 0.95).abs() < f32::EPSILON,
            "ratio must clamp at max_ratio = 0.95, got {}",
            ratio.get(),
        );
    }

    #[test]
    fn r683_splitter_external_clamps_to_min_bound() {
        let ratio: Rc<Signal<f32>> = Rc::new(Signal::new(0.5));
        let mut ext =
            SplitterExternal::new(SplitterOrientation::Horizontal).attach_ratio(Rc::clone(&ratio));
        ext.pointer_move(0.5, 0.0);
        // Cursor strays past the left edge — x_rel reaches -0.5.
        // Ratio = 0.5 + (-0.5 - 0.5) = -0.5 → clamps to min 0.05.
        ext.pointer_move(-0.5, 0.0);
        assert!(
            (ratio.get() - 0.05).abs() < f32::EPSILON,
            "ratio must clamp at min_ratio = 0.05, got {}",
            ratio.get(),
        );
    }

    #[test]
    fn r683_splitter_external_pointer_up_clears_drag_state() {
        let ratio: Rc<Signal<f32>> = Rc::new(Signal::new(0.5));
        let mut ext =
            SplitterExternal::new(SplitterOrientation::Horizontal).attach_ratio(Rc::clone(&ratio));
        ext.pointer_move(0.5, 0.0);
        assert!(ext.is_dragging());
        // PointerUp clears via the framework's invoke("send", ...)
        // dispatch (the R51.41 §5.35 channel the InputRouter
        // dispatches through).
        ext.invoke("send", IntrospectValue::Text("PointerUp".to_string()))
            .expect("invoke send PointerUp returns Ok");
        assert!(!ext.is_dragging());
        // A new pointer_move after release starts a fresh
        // calibration cycle (no ratio mutation on the press-time
        // frame).
        ext.pointer_move(0.8, 0.0);
        assert!(
            (ratio.get() - 0.5).abs() < f32::EPSILON,
            "post-release press-time frame must not mutate ratio",
        );
    }

    #[test]
    fn r683_splitter_external_pointer_cancel_also_clears() {
        let ratio: Rc<Signal<f32>> = Rc::new(Signal::new(0.5));
        let mut ext =
            SplitterExternal::new(SplitterOrientation::Horizontal).attach_ratio(Rc::clone(&ratio));
        ext.pointer_move(0.5, 0.0);
        assert!(ext.is_dragging());
        ext.invoke("send", IntrospectValue::Text("PointerCancel".to_string()))
            .expect("invoke send PointerCancel returns Ok");
        assert!(
            !ext.is_dragging(),
            "PointerCancel mirrors PointerUp for drag teardown",
        );
    }

    /// R1346 §5.20 — the drag-end commit channel. Harvest helper: drain
    /// the External's pending intents into a plain Vec of
    /// `(tag, payload)` so the assertions read as the wire-form the
    /// binding's reducer sees.
    fn harvest(ext: &mut SplitterExternal) -> Vec<Intent> {
        let mut out = Vec::new();
        ext.drain_intents(&mut |i| out.push(i));
        out
    }

    #[test]
    fn r1346_splitter_drag_end_emits_ratio_committed_with_final_ratio() {
        let ratio: Rc<Signal<f32>> = Rc::new(Signal::new(0.5));
        let mut ext =
            SplitterExternal::new(SplitterOrientation::Horizontal).attach_ratio(Rc::clone(&ratio));
        // Calibrate, then drag to 0.7. The live channel writes the
        // Signal on every move; nothing is committed mid-drag.
        ext.pointer_move(0.5, 0.0);
        ext.pointer_move(0.7, 0.0);
        assert!(
            !ext.is_dirty(),
            "the live Signal stream is the preview channel — an in-flight drag commits nothing",
        );

        ext.invoke("send", IntrospectValue::Text("PointerUp".to_string()))
            .expect("invoke send PointerUp returns Ok");

        assert!(ext.is_dirty(), "drag end must arm the commit channel");
        let harvested = harvest(&mut ext);
        assert_eq!(harvested.len(), 1, "exactly one commit per drag");
        assert_eq!(harvested[0].tag_str(), "ratio_committed");
        // Payload = the final ratio, homomorphic to SliderExternal's
        // `value_committed`. 0.5 + (0.7 - 0.5) = 0.7.
        let IntrospectValue::Float(committed) = harvested[0].payload else {
            panic!(
                "ratio_committed payload must be Float, got {:?}",
                harvested[0].payload
            );
        };
        assert!(
            (committed - 0.7).abs() < 1e-6,
            "committed payload must be the final ratio, got {committed}",
        );
        assert!(
            (f64::from(ratio.get()) - committed).abs() < 1e-6,
            "committed payload must agree with the live Signal",
        );
        assert!(
            !ext.is_dirty(),
            "drain clears the queue — no duplicate commit"
        );
    }

    #[test]
    fn r1346_splitter_pointer_cancel_keeps_ratio_but_suppresses_commit() {
        let ratio: Rc<Signal<f32>> = Rc::new(Signal::new(0.5));
        let mut ext =
            SplitterExternal::new(SplitterOrientation::Horizontal).attach_ratio(Rc::clone(&ratio));
        ext.pointer_move(0.5, 0.0);
        ext.pointer_move(0.7, 0.0);

        ext.invoke("send", IntrospectValue::Text("PointerCancel".to_string()))
            .expect("invoke send PointerCancel returns Ok");

        // The family-wide R51.93 §5.35 invariant, pinned for the peer by
        // slider.rs::r51_93_pointer_cancel_during_drag_returns_to_idle_without_commit:
        // the in-flight ratio writes stay applied (the OS revoked the
        // commit, not the drag), and only the commit is suppressed.
        assert!(
            (ratio.get() - 0.7).abs() < f32::EPSILON,
            "in-flight ratio writes stay applied across PointerCancel, got {}",
            ratio.get(),
        );
        assert!(
            !ext.is_dirty(),
            "PointerCancel must not fire ratio_committed",
        );
        assert!(harvest(&mut ext).is_empty());
    }

    #[test]
    fn r1346_splitter_uncalibrated_release_commits_nothing() {
        let ratio: Rc<Signal<f32>> = Rc::new(Signal::new(0.5));
        let mut ext =
            SplitterExternal::new(SplitterOrientation::Horizontal).attach_ratio(Rc::clone(&ratio));
        // A `PointerUp` with no calibration at all — an orphan release
        // (no prior press) rather than a click. NOTE this is NOT the
        // click case: under the real router a click DOES calibrate (the
        // press-time forward), so the click arc lives in
        // `r1346_real_router_bare_click_commits_nothing` and only a
        // router-level test can cover it. This one pins the narrower
        // "no anchor → nothing to compare → silence" branch.
        ext.invoke("send", IntrospectValue::Text("PointerUp".to_string()))
            .expect("invoke send PointerUp returns Ok");
        assert!(!ext.is_dirty(), "an uncalibrated release must not commit");
        assert!(harvest(&mut ext).is_empty());
    }

    #[test]
    fn r1346_splitter_drag_returning_to_press_ratio_commits_nothing() {
        let ratio: Rc<Signal<f32>> = Rc::new(Signal::new(0.5));
        let mut ext =
            SplitterExternal::new(SplitterOrientation::Horizontal).attach_ratio(Rc::clone(&ratio));
        // Calibrate at 0.5, wander out to 0.7, then come back exactly
        // home before releasing. The gate is "did the ratio settle
        // somewhere new", not "did the cursor travel" — a round trip
        // leaves nothing to persist, so the channel stays silent even
        // though a very real drag ran.
        ext.pointer_move(0.5, 0.0);
        ext.pointer_move(0.7, 0.0);
        assert!((ratio.get() - 0.7).abs() < f32::EPSILON);
        ext.pointer_move(0.5, 0.0);
        assert!(
            (ratio.get() - 0.5).abs() < f32::EPSILON,
            "back to the press ratio"
        );

        ext.invoke("send", IntrospectValue::Text("PointerUp".to_string()))
            .expect("invoke send PointerUp returns Ok");
        assert!(
            !ext.is_dirty(),
            "a drag that returned to its press ratio settled nothing to persist",
        );
        assert!(harvest(&mut ext).is_empty());
    }

    #[test]
    fn r1346_splitter_drag_pinned_against_clamp_commits_nothing() {
        let ratio: Rc<Signal<f32>> = Rc::new(Signal::new(0.95));
        let mut ext =
            SplitterExternal::new(SplitterOrientation::Horizontal).attach_ratio(Rc::clone(&ratio));
        // Press while already saturated at max_ratio and shove further
        // right: every move clamps back to 0.95, so the ratio never
        // leaves the press snapshot and there is nothing new to write.
        ext.pointer_move(0.5, 0.0);
        ext.pointer_move(0.9, 0.0);
        ext.pointer_move(1.4, 0.0);
        assert!(
            (ratio.get() - 0.95).abs() < f32::EPSILON,
            "pinned at max_ratio"
        );

        ext.invoke("send", IntrospectValue::Text("PointerUp".to_string()))
            .expect("invoke send PointerUp returns Ok");
        assert!(
            !ext.is_dirty(),
            "a drag that stayed saturated against the clamp commits nothing",
        );
    }

    #[test]
    fn r1346_splitter_paint_only_mode_commits_nothing() {
        // No attached ratio (the bring-up / snapshot-test mode): the
        // drag wire is inert, so the release channel stays silent
        // rather than committing a ratio that does not exist.
        let mut ext = SplitterExternal::new(SplitterOrientation::Horizontal);
        ext.pointer_move(0.5, 0.0);
        ext.pointer_move(0.7, 0.0);
        ext.invoke("send", IntrospectValue::Text("PointerUp".to_string()))
            .expect("invoke send PointerUp returns Ok");
        assert!(!ext.is_dirty(), "paint-only mode has no ratio to commit");
        assert!(harvest(&mut ext).is_empty());
    }

    #[test]
    fn r1346_splitter_consecutive_drags_each_commit_once() {
        let ratio: Rc<Signal<f32>> = Rc::new(Signal::new(0.5));
        let mut ext =
            SplitterExternal::new(SplitterOrientation::Horizontal).attach_ratio(Rc::clone(&ratio));
        // First drag: 0.5 → 0.7.
        ext.pointer_move(0.5, 0.0);
        ext.pointer_move(0.7, 0.0);
        ext.invoke("send", IntrospectValue::Text("PointerUp".to_string()))
            .expect("invoke send PointerUp returns Ok");
        let first = harvest(&mut ext);
        assert_eq!(first.len(), 1);

        // Second drag re-calibrates from the settled ratio: 0.7 + (0.3
        // - 0.5) = 0.5. The release must commit again — the teardown
        // left no stale calibration that would swallow the next edge.
        ext.pointer_move(0.5, 0.0);
        ext.pointer_move(0.3, 0.0);
        ext.invoke("send", IntrospectValue::Text("PointerUp".to_string()))
            .expect("invoke send PointerUp returns Ok");
        let second = harvest(&mut ext);
        assert_eq!(second.len(), 1, "each drag commits exactly once");
        let IntrospectValue::Float(committed) = second[0].payload else {
            panic!("expected Float payload");
        };
        assert!(
            (committed - 0.5).abs() < 1e-6,
            "second commit carries the re-calibrated ratio, got {committed}",
        );
    }

    /// R1346 §5.20 §5.35 — the arc that actually matters, over the real
    /// `InputRouter` rather than hand-fed `invoke` calls.
    ///
    /// The consumer-visible failure this locks is an *integration* one:
    /// a binding that persists the split ratio must learn the settled
    /// value from the release itself, because there is no guaranteed
    /// frame afterwards (`AppShell::about_to_wait` idles on
    /// `ControlFlow::Wait`, so with nothing dirty frame N+1 never
    /// arrives and any settle inferred from frame cadence is silently
    /// dropped). Proving `SplitterExternal` queues an intent is not
    /// enough — this drives a real capture-lock press / drag / release
    /// and harvests through `walk_scene_and_drain`, which is verbatim
    /// what `CoreShell::tail` runs at the end of the very dispatch that
    /// delivered the `PointerUp`. Nothing here paints a frame.
    #[test]
    fn r1346_real_router_release_delivers_tag_prefixed_commit_without_a_frame() {
        use pinion_core::scene::ExternalNode;
        use pinion_runtime::input::{InputRouter, PointerId};
        use pinion_runtime::{IntentQueue, walk_scene_and_drain};

        run_in_owner(|| {
            let parent_w: u32 = 1000;
            let ratio: Rc<Signal<f32>> = Rc::new(Signal::new(0.5));

            // Paint scene: the laid-out splitter, so the router has a
            // real rect for TEST_TAG and normalises the cursor against
            // the true geometry (an x_rel the widget then reads as a
            // ratio delta) — not a synthetic fraction.
            let style = SplitterStyle::m3_default(SplitterOrientation::Horizontal, TEST_TAG);
            let mut paint = view_splitter(
                empty_panel("left_panel"),
                empty_panel("right_panel"),
                &ratio,
                &theme_light(),
                &style,
                false,
            );
            let mut cache = pinion_text::LayoutCache::new();
            pinion_runtime::layout::compute_layout(&mut paint, &mut cache, parent_w, 300);

            // State scene: the External the router dispatches into.
            let mut state = Scene::External(
                ExternalNode::new(Box::new(
                    SplitterExternal::new(SplitterOrientation::Horizontal)
                        .attach_ratio(Rc::clone(&ratio)),
                ))
                .with_tag(TEST_TAG),
            );

            let mut router = InputRouter::new();
            router.update_paint_scene(paint, &mut state);

            // Press at mid-splitter, drag right to 80% of the width,
            // release. The splitter opts into capture, so the release
            // reaches it through the framework's `send` wire.
            router.cursor_moved(PointerId::MOUSE, 500.0, 150.0, &mut state);
            router.pointer_down(PointerId::MOUSE, &mut state);
            assert_eq!(
                router.captured_target(PointerId::MOUSE),
                Some(TEST_TAG),
                "splitter captures the pointer for the drag span",
            );
            router.cursor_moved(PointerId::MOUSE, 800.0, 150.0, &mut state);
            let dragged_to = ratio.get();
            assert!(
                dragged_to > 0.5,
                "the live channel moved the ratio right during the drag, got {dragged_to}",
            );

            // Mid-drag the commit channel is still silent: an in-flight
            // drag is a preview, not a settle.
            let mut mid = IntentQueue::new();
            walk_scene_and_drain(&mut state, &mut mid);
            assert!(
                mid.drain().is_empty(),
                "no commit may escape before the release",
            );

            router.pointer_up(PointerId::MOUSE, &mut state);

            // The harvest `CoreShell::tail` performs, in the same
            // dispatch — no repaint in between.
            let mut queue = IntentQueue::new();
            walk_scene_and_drain(&mut state, &mut queue);
            let intents = queue.drain();
            assert_eq!(intents.len(), 1, "one commit per completed drag");
            // §5.20 R22: the reducer matches the tag-prefixed wire form
            // ([[intent-tag-dotted-wire-form]]), not the bare event name.
            assert_eq!(
                intents[0].tag_str(),
                "test_splitter.ratio_committed",
                "the commit reaches the binding namespaced by the node tag",
            );
            let IntrospectValue::Float(committed) = intents[0].payload else {
                panic!("expected Float payload, got {:?}", intents[0].payload);
            };
            assert!(
                (f64::from(dragged_to) - committed).abs() < 1e-6,
                "the committed ratio is the one the drag settled on: \
                 live={dragged_to}, committed={committed}",
            );
        });
    }

    /// R1346 §5.20 §5.35 — a bare **click** on the handle must not
    /// commit.
    ///
    /// This is the arc a hand-fed `invoke("send", "PointerUp")` test
    /// cannot reach, and the one that matters: `InputRouter::pointer_down`
    /// forwards the press-time cursor as an initial `pointer_move` to
    /// every capture-opting widget (R51.35 click-to-position, pinned by
    /// `input.rs::pointer_down_forwards_initial_cursor`), and the
    /// splitter opts into capture. So the press *always* arms the
    /// [`DragCalibration`] anchor — `end()`'s bool is "a calibration
    /// existed", NOT "the user dragged". Gating the commit on it alone
    /// makes every click emit a `ratio_committed` for a ratio that never
    /// moved, which for the persisting consumer this channel exists to
    /// serve is a spurious write (and a double-click sends two).
    #[test]
    fn r1346_real_router_bare_click_commits_nothing() {
        use pinion_core::scene::ExternalNode;
        use pinion_runtime::input::{InputRouter, PointerId};
        use pinion_runtime::{IntentQueue, walk_scene_and_drain};

        run_in_owner(|| {
            let ratio: Rc<Signal<f32>> = Rc::new(Signal::new(0.5));
            let style = SplitterStyle::m3_default(SplitterOrientation::Horizontal, TEST_TAG);
            let mut paint = view_splitter(
                empty_panel("left_panel"),
                empty_panel("right_panel"),
                &ratio,
                &theme_light(),
                &style,
                false,
            );
            let mut cache = pinion_text::LayoutCache::new();
            pinion_runtime::layout::compute_layout(&mut paint, &mut cache, 1000, 300);

            let mut state = Scene::External(
                ExternalNode::new(Box::new(
                    SplitterExternal::new(SplitterOrientation::Horizontal)
                        .attach_ratio(Rc::clone(&ratio)),
                ))
                .with_tag(TEST_TAG),
            );
            let mut router = InputRouter::new();
            router.update_paint_scene(paint, &mut state);

            // Press and release without moving the cursor one pixel.
            router.cursor_moved(PointerId::MOUSE, 500.0, 150.0, &mut state);
            router.pointer_down(PointerId::MOUSE, &mut state);
            router.pointer_up(PointerId::MOUSE, &mut state);

            assert!(
                (ratio.get() - 0.5).abs() < f32::EPSILON,
                "a click moves no ratio (the press-time forward only calibrates), got {}",
                ratio.get(),
            );
            let mut queue = IntentQueue::new();
            walk_scene_and_drain(&mut state, &mut queue);
            assert!(
                queue.drain().is_empty(),
                "a click that settled nothing must not reach the persistence channel",
            );
        });
    }

    /// R1346 §5.20 §5.35 — a click carrying ordinary mouse jitter.
    ///
    /// The honest boundary of the "did the ratio move" gate, stated
    /// rather than hidden: a press with a few pixels of hand-shake
    /// *does* move the ratio (the live channel already wrote it and the
    /// pane already repainted there), so the commit fires and reports
    /// it. That is correct — suppressing it would leave the persisted
    /// ratio disagreeing with what is on screen, which is strictly
    /// worse than one honest write. Pinned so the tradeoff is a
    /// decision on record and not an accident: if a future round wants
    /// jitter to be inert it must suppress the *live* write too (a
    /// `DRAG_CLICK_THRESHOLD_PX` dead zone), not just the commit.
    #[test]
    fn r1346_real_router_jitter_click_commits_what_it_actually_moved() {
        use pinion_core::scene::ExternalNode;
        use pinion_runtime::input::{InputRouter, PointerId};
        use pinion_runtime::{IntentQueue, walk_scene_and_drain};

        run_in_owner(|| {
            let ratio: Rc<Signal<f32>> = Rc::new(Signal::new(0.5));
            let style = SplitterStyle::m3_default(SplitterOrientation::Horizontal, TEST_TAG);
            let mut paint = view_splitter(
                empty_panel("left_panel"),
                empty_panel("right_panel"),
                &ratio,
                &theme_light(),
                &style,
                false,
            );
            let mut cache = pinion_text::LayoutCache::new();
            pinion_runtime::layout::compute_layout(&mut paint, &mut cache, 1000, 300);

            let mut state = Scene::External(
                ExternalNode::new(Box::new(
                    SplitterExternal::new(SplitterOrientation::Horizontal)
                        .attach_ratio(Rc::clone(&ratio)),
                ))
                .with_tag(TEST_TAG),
            );
            let mut router = InputRouter::new();
            router.update_paint_scene(paint, &mut state);

            router.cursor_moved(PointerId::MOUSE, 500.0, 150.0, &mut state);
            router.pointer_down(PointerId::MOUSE, &mut state);
            router.cursor_moved(PointerId::MOUSE, 502.0, 150.0, &mut state); // 2 px shake
            router.pointer_up(PointerId::MOUSE, &mut state);

            let settled = ratio.get();
            assert!(
                (settled - 0.502).abs() < 1e-4,
                "the live channel moved the pane 2px; got {settled}",
            );
            let mut queue = IntentQueue::new();
            walk_scene_and_drain(&mut state, &mut queue);
            let intents = queue.drain();
            assert_eq!(
                intents.len(),
                1,
                "the ratio genuinely moved, so the commit reports it rather than \
                 leaving screen and persisted state disagreeing",
            );
            let IntrospectValue::Float(committed) = intents[0].payload else {
                panic!("expected Float payload");
            };
            assert!(
                (f64::from(settled) - committed).abs() < 1e-6,
                "committed value tracks the screen",
            );
        });
    }

    #[test]
    fn r683_splitter_external_introspect_schema_lists_canonical_paths() {
        let ext = SplitterExternal::new(SplitterOrientation::Horizontal);
        let schema = ext.schema();
        let slot_names: Vec<&str> = schema.fields.iter().map(|f| f.path).collect();
        assert!(slot_names.contains(&"orientation"));
        assert!(slot_names.contains(&"ratio"));
        assert!(slot_names.contains(&"dragging"));
        assert!(slot_names.contains(&"send"));
    }

    #[test]
    fn r683_splitter_external_introspect_query_orientation() {
        let ext = SplitterExternal::new(SplitterOrientation::Vertical);
        let val = ext.query("orientation").expect("orientation is queryable");
        assert_eq!(val.as_str(), Some("vertical"));
    }

    #[test]
    fn r683_splitter_external_introspect_query_dragging_starts_false() {
        let ext = SplitterExternal::new(SplitterOrientation::Horizontal);
        let val = ext.query("dragging").expect("dragging is queryable");
        assert_eq!(val, IntrospectValue::Bool(false));
    }

    #[test]
    fn r683_splitter_external_invoke_unknown_event_returns_err() {
        let mut ext = SplitterExternal::new(SplitterOrientation::Horizontal);
        let res = ext.invoke(
            "send",
            IntrospectValue::Text("UnknownPointerEvent".to_string()),
        );
        assert!(res.is_err());
    }

    #[test]
    fn r683_splitter_external_vertical_uses_y_axis() {
        let ratio: Rc<Signal<f32>> = Rc::new(Signal::new(0.5));
        let mut ext =
            SplitterExternal::new(SplitterOrientation::Vertical).attach_ratio(Rc::clone(&ratio));
        // Press with y_rel = 0.5; x_rel ignored.
        ext.pointer_move(0.0, 0.5);
        // Move y_rel to 0.8 — ratio = 0.5 + 0.3 = 0.8.
        ext.pointer_move(0.0, 0.8);
        assert!(
            (ratio.get() - 0.8).abs() < 1e-5,
            "vertical splitter uses y_rel; got ratio = {}",
            ratio.get(),
        );
    }

    #[test]
    fn r683_splitter_external_horizontal_ignores_y_axis() {
        let ratio: Rc<Signal<f32>> = Rc::new(Signal::new(0.5));
        let mut ext =
            SplitterExternal::new(SplitterOrientation::Horizontal).attach_ratio(Rc::clone(&ratio));
        // Press with x_rel = 0.5.
        ext.pointer_move(0.5, 0.5);
        // Cursor jitters along y only — ratio stays at 0.5.
        ext.pointer_move(0.5, 0.9);
        assert!(
            (ratio.get() - 0.5).abs() < f32::EPSILON,
            "horizontal splitter must ignore y_rel; ratio drifted to {}",
            ratio.get(),
        );
    }

    #[test]
    fn r683_splitter_external_paint_only_mode_is_inert() {
        // No attached ratio handle → pointer events do not panic;
        // is_dragging stays false (the implementation early-returns
        // before arming the flag because the ratio handle is what
        // makes the wire meaningful).
        let mut ext = SplitterExternal::new(SplitterOrientation::Horizontal);
        ext.pointer_move(0.5, 0.5);
        ext.pointer_move(0.8, 0.5);
        assert!(!ext.is_dragging());
        assert!(ext.ratio_signal().is_none());
    }

    #[test]
    fn r683_splitter_external_attach_bounds_overrides_default_clamp() {
        let ratio: Rc<Signal<f32>> = Rc::new(Signal::new(0.5));
        let mut ext = SplitterExternal::new(SplitterOrientation::Horizontal)
            .attach_ratio(Rc::clone(&ratio))
            .attach_bounds(0.25, 0.75);
        ext.pointer_move(0.5, 0.0);
        // Drag past 0.75 (delta +0.5 from press), clamps at 0.75
        // not 0.95.
        ext.pointer_move(1.0, 0.0);
        assert!(
            (ratio.get() - 0.75).abs() < f32::EPSILON,
            "expected clamp at 0.75 from attach_bounds, got {}",
            ratio.get(),
        );
    }

    #[test]
    fn r683_splitter_external_orientation_readable_post_construction() {
        let ext_h = SplitterExternal::new(SplitterOrientation::Horizontal);
        let ext_v = SplitterExternal::new(SplitterOrientation::Vertical);
        assert_eq!(ext_h.orientation(), SplitterOrientation::Horizontal);
        assert_eq!(ext_v.orientation(), SplitterOrientation::Vertical);
    }

    // ─────────────────────────────────────────────────────────────────
    // R684 §5.21 §5.16 — splitter ratio main-axis proportional split.
    //
    // The splitter's left/right wrappers now declare
    // `flex_basis(Px(0)) + flex_grow(ratio)` so taffy distributes the
    // FULL parent main-axis extent proportionally to `ratio` rather
    // than to the leftover after intrinsic-content basis resolution
    // (the pre-R684 collapse-toward-~50/50 defect documented in
    // R683.C's honest carry).
    //
    // The pinned ratios are 0.80 + 0.20 — far enough from the 50/50
    // attractor that the pre-R684 leftover-distribution math would
    // observably fail.
    // ─────────────────────────────────────────────────────────────────

    fn lay_out_splitter(ratio_value: f32, parent_w: u32, parent_h: u32) -> Scene {
        use pinion_runtime::layout::compute_layout;
        use pinion_text::LayoutCache;
        let ratio: Rc<Signal<f32>> = Rc::new(Signal::new(ratio_value));
        let style = SplitterStyle::m3_default(SplitterOrientation::Horizontal, TEST_TAG);
        let scene = view_splitter(
            empty_panel("left_panel"),
            empty_panel("right_panel"),
            &ratio,
            &theme_light(),
            &style,
            false,
        );
        let mut cache = LayoutCache::new();
        let mut scene = scene;
        compute_layout(&mut scene, &mut cache, parent_w, parent_h);
        scene
    }

    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "test math — flexible_w fits in f32 mantissa (≤ 1000), expected_left clamped to [0, flexible_w]"
    )]
    fn expected_ratio_width(ratio: f32, flexible_w: u32) -> u32 {
        (ratio * flexible_w as f32) as u32
    }

    #[test]
    fn r684_splitter_left_width_matches_main_ratio_at_080() {
        run_in_owner(|| {
            let parent_w: u32 = 1000;
            let style = SplitterStyle::m3_default(SplitterOrientation::Horizontal, TEST_TAG);
            let scene = lay_out_splitter(0.80, parent_w, 300);
            let Scene::Container(outer) = &scene else {
                panic!()
            };
            let Scene::Container(left_wrap) = &outer.children[0] else {
                panic!()
            };
            let Scene::Container(right_wrap) = &outer.children[2] else {
                panic!()
            };
            // The 4-px handle occupies its own main-axis slice; the
            // flexible width budget the wrappers share is
            // `parent_w - handle_extent_px`.
            let flexible_w = parent_w - style.handle_extent_px;
            // Expected ~0.80 * flexible_w for left, ~0.20 * for right.
            // 5% tolerance per the R684 verification spec absorbs
            // taffy's f32→u32 rounding at the wrapper boundary.
            let expected_left = expected_ratio_width(0.80, flexible_w);
            let tolerance = expected_left / 20; // 5%
            assert!(
                left_wrap.rect.w.abs_diff(expected_left) <= tolerance,
                "R684 atomic 2: at ratio 0.80 left wrapper must span ~{expected_left} px \
                of flexible {flexible_w} px (±5%), got {}",
                left_wrap.rect.w,
            );
            // Total widths cover the parent exactly.
            assert_eq!(
                left_wrap.rect.w + style.handle_extent_px + right_wrap.rect.w,
                parent_w,
                "L + handle + R must equal parent extent",
            );
        });
    }

    #[test]
    fn r684_splitter_left_width_matches_main_ratio_at_020() {
        run_in_owner(|| {
            let parent_w: u32 = 1000;
            let style = SplitterStyle::m3_default(SplitterOrientation::Horizontal, TEST_TAG);
            let scene = lay_out_splitter(0.20, parent_w, 300);
            let Scene::Container(outer) = &scene else {
                panic!()
            };
            let Scene::Container(left_wrap) = &outer.children[0] else {
                panic!()
            };
            let Scene::Container(right_wrap) = &outer.children[2] else {
                panic!()
            };
            let flexible_w = parent_w - style.handle_extent_px;
            let expected_left = expected_ratio_width(0.20, flexible_w);
            // Minimum-tolerance floor of 5 px so the 5% rule does
            // not become smaller than taffy's u32 rounding noise on
            // the thinner panel.
            let tolerance = std::cmp::max(5, expected_left / 20);
            assert!(
                left_wrap.rect.w.abs_diff(expected_left) <= tolerance,
                "R684 atomic 2: at ratio 0.20 left wrapper must span ~{expected_left} px \
                of flexible {flexible_w} px (±5%), got {}",
                left_wrap.rect.w,
            );
            assert_eq!(
                left_wrap.rect.w + style.handle_extent_px + right_wrap.rect.w,
                parent_w,
                "L + handle + R must equal parent extent",
            );
        });
    }
}
