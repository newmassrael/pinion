//! R683.B §5.16 §5.41 (R1081 §5.51 R742) — backend-agnostic Dock-panel
//! primitive + drag-to-dock / drag-to-tear-off [`External`].
//!
//! ## Role
//!
//! A **`DockPanel`** is the atomic unit of a multi-pane DCC / IDE /
//! CAD layout (the Phase B → D north star surface). Each panel
//! carries a header strip the user can grab + drag: dropping it onto
//! another panel **docks** it there (split / swap, via the shared
//! [`DockReorganizer`]), and dragging it out of the dock **tears it
//! off** into a new floating window — the canonical pro-tool
//! authoring affordance every Photoshop / Figma / Unreal Editor /
//! `VSCode` panel system ships.
//!
//! The topology composition (recursive split tree, [`DockTopology`] +
//! the [`view_dock_surface`] walker) was lifted into this crate at the
//! R685 2nd-consumer gate ([[abstraction-needs-second-consumer]]); the
//! `hello-dock-panels-editor` binding is the consumer that surfaced its
//! contract.
//!
//! ## Drag wire (R742 §5.51)
//!
//! [`DockPanelExternal`] is the §5.51 R742 drag source. On a header
//! `PointerDown` it arms; the framework opens a drag session
//! ([`begin_drag`](pinion_core::external::External::begin_drag)) and,
//! on each cursor move, hands the panel the drop location under the
//! *absolute* cursor (the router resolves the nearest opted-in
//! `LayoutStyle::drop_target` panel, R1080). On release the panel
//! either docks onto the target panel through the shared coordinator,
//! or — when the cursor escaped every drop target — emits a `tear_off`
//! intent with the panel id as `IntrospectValue::Text` payload. The
//! click-vs-drag threshold is the framework's `DRAG_CLICK_THRESHOLD_PX`
//! SSOT (a press-release in place stays a click), NOT a per-panel knob.
//!
//! The binding's [`WidgetCore::update`](pinion_core::WidgetCore::update)
//! reducer matches against the dotted wire form
//! `{panel_tag}.tear_off` (per
//! [[intent-tag-dotted-wire-form]]) and on a successful match
//! pushes a new [`WindowSpec`](pinion_shell::WindowSpec) onto its
//! reactive `Signal<Vec<WindowSpec>>` (R683.A reconcile Effect
//! picks it up and a 2nd window appears with the torn-off panel's
//! content).
//!
//! ## Why intent-based tear-off (not direct `WindowSpec` push)
//!
//! The dock substrate cannot push `WindowSpec`s directly — Phase B
//! crate boundary discipline (`pinion-widget-paint` sits below
//! `pinion-shell`; reaching across would create a downward
//! reverse-dep). The intent channel is the canonical mechanism for
//! widget → binding signalling (mirror of every other widget's
//! event emission); the binding holds the
//! `Signal<Vec<WindowSpec>>` + the dock panel descriptors and
//! translates intents into topology mutations.
//!
//! ## Dep graph
//!
//! Sits beside [`splitter`](crate::splitter) (one tier above
//! [`text_field`](crate::text_field) / [`tree_view`](crate::tree_view)
//! since it composes via Splitter). Pure
//! [`Scene`](pinion_core::Scene) composition, no `pinion-text`
//! dependency, no Vello / winit coupling.

use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};

use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, DragPayload, DragUpdate, DropPoint, External,
    ExternalIntrospect, InterveneError, IntrospectSchema, IntrospectValue, InvokeError,
    RepaintOwner, ThreadOwnership,
};
use pinion_core::input::PointerWireEvent;
use pinion_core::intent::Intent;
use pinion_core::scene::{BoxNode, ContainerNode, Rect, Scene, TextNode};
use pinion_core::style::{
    AlignItems, Border, BoxStyle, Color, FlexDirection, JustifyContent, LayoutStyle, Size,
    SizeValue, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme};
use pinion_core::widget_core::WidgetStateName;
use pinion_core::widgets::Widget;
use pinion_core::widgets::dock_panel::{DockPanelEvent, DockPanelPolicy, DockPanelState};
use std::rc::Rc;

use pinion_core::reactive::Signal;
use pinion_core::undo::{SignalEdit, UndoStack};

use crate::splitter::{SplitterOrientation, SplitterStyle, view_splitter};
use crate::tabs::{TabsStyle, composite_tab_tag, view_tabs};
use pinion_a11y::{AccessNode, TabCell, tablist_tab_nodes};

// ─────────────────────────────────────────────────────────────────────
// R685 §5.16 §5.49 — DockSurface topology composition substrate.
//
// The topology types are pure data — no `External`, no `Scene`
// composition. They describe the recursive split tree (binary
// splits + leaf panels) an editor / DCC / IDE shell composes its
// multi-pane layout from. The matching `view_surface` walker
// (R685 atomic 1) lowers a [`DockTopology`] into a nested
// [`crate::splitter::view_splitter`] + [`view_dock_panel`] scene.
//
// ## Design references
//
//  - `VSCode`: Activity bar (left) + Side bar (left or right) +
//    Editor (center) + Panel (bottom) + Status bar — fixed slots
//    with draggable splits between them. The recursive Split/Leaf
//    abstraction here generalizes to that shape via nested splits.
//  - `IntelliJ`: Tool windows dock to Left / Right / Bottom / Top
//    of the editor — same Split tree, different topology shape.
//  - Photoshop: Side panels stack on left or right — N-leaf
//    horizontal split.
//  - Unreal Editor: Free-form docking via nested splits + tabs.
//
// The recursive Split tree is the textbook canonical abstraction
// every pro-tool authoring shell ships under the hood; the
// per-shell visual feel (tabs, drag-drop, etc.) is composed on
// top.
//
// ## Why binary splits (not N-ary)
//
// Every pane system the world ships ultimately reduces to a binary
// split tree because the user's drag UX is "grab the divider between
// two panes." A 3-way split is two nested binary splits with a
// shared edge; the binary form makes the drag wire trivial (one
// [`crate::splitter::SplitterExternal`] per Split node) and the
// serialization compact (recursive tree, no N-ary array bookkeeping).
//
// ## Stable Split ids (not positional addressing)
//
// Each [`DockNode::Split`] carries a stable [`DockNode::Split::id`]
// the binding uses to look up its per-Split state ratio Signal +
// the paint-side splitter tag. The id is stable across topology
// mutations — a leaf insert / Split rebalance / dock-reorganize
// gesture rewrites the tree shape but keeps every Split's id
// intact, so binding-side state keyed on id never silently
// re-binds to the wrong Split. Pre-R685 atomic 5b the walker
// indexed splits by depth-first traversal order (fragile under
// topology mutation); R685 atomic 5b lands the stable-id substrate
// as the textbook canonical form Phase D editor work requires.
//
// ## Crate dep boundary
//
// Pure data — no Vello, no winit, no `pinion-text`, no `pinion-shell`
// (the topology is the binding's responsibility; the substrate just
// renders whatever tree it receives). Lives in `pinion-widget-paint`
// because the [`view_surface`] walker (atomic 1) composes via
// [`view_dock_panel`] + [`crate::splitter::view_splitter`].
//
// ─────────────────────────────────────────────────────────────────────

/// (R685 §5.16 §5.49) Recursive node of a dock topology tree.
///
/// Either a binary [`Split`] (geometric divider between two
/// child sub-trees, oriented Horizontal or Vertical, with a
/// `ratio ∈ [0.0, 1.0]` controlling the divider position) or a
/// [`Leaf`] (a single docked panel addressable by `panel_id`).
///
/// The tree's structure encodes the editor's pane layout; the
/// per-Split `ratio` lives as plain data here, but the runtime
/// `view_surface` walker (R685 atomic 1) reads it through a paired
/// [`pinion_core::reactive::Signal<f32>`] so the user-driven splitter
/// drag mutates the shared ratio and the topology re-paints
/// reactively. The R685 atomic 1 contract: every Split node gets a
/// paired `Signal<f32>` registered alongside; the tree carries the
/// **initial** ratio + the application owns the live signal.
///
/// ## Why `Box<DockNode>` for children
///
/// Rust requires indirection (`Box`, `Rc`, `Arc`) for recursive
/// `enum` variants because the variant's size would otherwise be
/// infinite. `Box` is canonical for owned-by-parent trees with no
/// sharing requirement (the topology tree is a tree, not a DAG);
/// `Rc`/`Arc` would surface only if the same sub-tree appears
/// twice (R685+ carry — every editor we know of has a single
/// canonical topology).
///
/// ## Serialization
///
/// `#[serde(tag = "type")]` produces a `{"type": "Split", ...}`
/// or `{"type": "Leaf", ...}` JSON shape — internally tagged
/// because the variants have distinguishable field sets. AI
/// clients reading the topology via `scene/query` get this
/// shape verbatim.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type")]
pub enum DockNode {
    /// Binary geometric divider between two child sub-trees.
    ///
    /// `orientation = Horizontal` lays `first` on the left and
    /// `second` on the right; `orientation = Vertical` lays `first`
    /// on the top and `second` on the bottom. The split-position
    /// ratio `ratio ∈ [0.0, 1.0]` is the **initial** value the
    /// topology carries on disk; live runtime drag through
    /// [`crate::splitter::SplitterExternal`] writes a shared signal
    /// the binding owns. The shared signal + the splitter's paint-
    /// side tag are registered against this Split's stable
    /// [`Self::Split::id`] field — the [`view_dock_surface`]
    /// walker's `split_handle` callback receives the id as its
    /// first argument so the binding looks up its per-split state
    /// by **stable identifier** rather than by traversal order.
    ///
    /// ## Why a stable id (not positional index)
    ///
    /// Pre-R685 atomic 5b the walker indexed splits by depth-first
    /// traversal order. The index was fragile: any topology
    /// mutation (a leaf insert / Split rebalance / dock-reorganize
    /// gesture) shifted subsequent indices, so binding-side state
    /// keyed on index would silently re-bind to the wrong Split.
    /// Phase D's AAA editor needs topology mutation to be cheap +
    /// safe (every user drag-to-reorganize gesture rewrites the
    /// tree), and positional addressing makes that unsafe.
    ///
    /// The stable id is the textbook canonical: the binding
    /// declares the id once at topology construction; the
    /// `Rc<Signal<f32>>` ratio handle + the `SplitterStyle::tag`
    /// are both registered against the same id; mutation of the
    /// topology tree shape never affects which Split's state is
    /// which.
    Split {
        /// Stable Split identifier. Used as the lookup key for the
        /// `split_handle` callback in [`view_dock_surface`] +
        /// (typically) as the paint-side
        /// [`SplitterStyle::tag`](crate::splitter::SplitterStyle::tag)
        /// the binding registers its [`SplitterExternal`](crate::splitter::SplitterExternal)
        /// against. Stable across topology mutations.
        id: Cow<'static, str>,
        /// Layout axis of the divider — `Horizontal` for a vertical
        /// gutter splitting left / right panes; `Vertical` for a
        /// horizontal gutter splitting top / bottom panes.
        orientation: SplitterOrientation,
        /// Initial split position as a fraction of the parent's
        /// main-axis extent. `0.5` = even split. The runtime
        /// `view_dock_surface` walker reads this through a paired
        /// `Signal<f32>` so user drag mutations refresh the
        /// layout reactively.
        ratio: f32,
        /// Left child (Horizontal) or top child (Vertical).
        first: Box<DockNode>,
        /// Right child (Horizontal) or bottom child (Vertical).
        second: Box<DockNode>,
    },
    /// Single docked panel — leaf of the tree.
    ///
    /// `panel_id` is the stable identifier the binding uses to
    /// look up the panel's `Scene` content via the
    /// [`view_dock_surface`] walker's `panel_handle: Fn(&str) ->
    /// DockPanelHandle` callback. The same id also pairs with the
    /// [`DockPanelExternal::new`] tear-off wire so a drag from the
    /// panel's header forwards the right payload to the binding's
    /// reducer.
    ///
    /// (R685 atomic 5c) The pre-R685 atomic 5c `slot: DockSlot`
    /// field is removed — it was dead data (no consumer in either
    /// dock binding read it) and added speculative API surface
    /// without a clear contract. A future R686+ round can re-add a
    /// semantic slot label when a real consumer surfaces (focus-
    /// traversal-order ordering / ARIA region landmark / dock-back
    /// re-attach hint), with the consumer driving the design.
    Leaf {
        /// Stable panel identifier — must match the
        /// [`DockPanelStyle::tag`] the binding uses for this panel,
        /// the `panel_handle` callback key, and the
        /// `DockPanelExternal::new` first argument.
        panel_id: Cow<'static, str>,
    },
    /// (R1083 §5.51) Tab well — a stack of ≥2 panels sharing one pane
    /// slot, exactly one of them visible at a time (the [`Self::Tabs::active`]
    /// index). The tabbed-docking leaf: dropping panel A onto panel B's
    /// centre ([`DockDropZone::Center`]) merges them into a `Tabs` well
    /// the user clicks between, the VS Code / Unreal docking idiom.
    ///
    /// ## Invariants (enforced by [`DockTopology::try_new`])
    ///
    /// * `panels.len() >= 2` — a single-panel well is a [`Self::Leaf`],
    ///   not a degenerate `Tabs`. The mutation primitives keep this
    ///   canonical: a well that loses a panel down to one collapses back
    ///   to a `Leaf` ([`remove_leaf_rec`]). There is therefore exactly
    ///   one representation for each panel count — no `Leaf` vs
    ///   `Tabs[1]` ambiguity.
    /// * `active < panels.len()` — the visible tab index is always in
    ///   range.
    /// * Every `panels[i]` is a unique non-empty panel id sharing the
    ///   one topology id namespace with [`Self::Leaf::panel_id`] +
    ///   [`Self::Split::id`] + this well's own [`Self::Tabs::id`].
    ///
    /// ## Why a stable well `id` (mirrors [`Self::Split::id`])
    ///
    /// Like a Split, a tab well is a bindable runtime entity — the tab
    /// strip is hit-tested (click a tab to switch active) and a future
    /// selection `Signal<usize>` binds to it. Positional addressing
    /// would be fragile under topology mutation (every reorganize
    /// rewrites the tree), so the well carries a stable id the binding
    /// keys its per-well state on. A `Tabify` gesture that creates a
    /// fresh well mints `reorg-tabs-{seq}` ([`REORG_TABS_ID_PREFIX`]);
    /// tabifying into an existing well keeps that well's id.
    Tabs {
        /// Stable tab-well identifier — the [`view_tabs`] strip tag +
        /// the future per-well selection-signal lookup key. Shares the
        /// one topology id namespace (unique against every panel id +
        /// Split id + other well id).
        id: Cow<'static, str>,
        /// The ≥2 panels stacked in this well, in tab order. Each is a
        /// stable panel id, addressable exactly like a [`Self::Leaf`]'s
        /// `panel_id` (same `panel_handle` callback, same
        /// `DockPanelExternal` registration).
        panels: Vec<Cow<'static, str>>,
        /// Index into `panels` of the visible tab. Always `< panels.len()`.
        active: usize,
    },
}

impl DockNode {
    /// (R685 §5.16) Convenience constructor for a leaf node.
    #[must_use]
    pub fn leaf(panel_id: impl Into<Cow<'static, str>>) -> Self {
        Self::Leaf {
            panel_id: panel_id.into(),
        }
    }

    /// (R685 §5.16) Convenience constructor for a horizontal split
    /// (left | right panes). `id` is the stable Split identifier the
    /// binding uses as the lookup key for the `split_handle` callback +
    /// the [`SplitterStyle::tag`](crate::splitter::SplitterStyle::tag);
    /// `ratio` is the initial split fraction.
    #[must_use]
    pub fn split_horizontal(
        id: impl Into<Cow<'static, str>>,
        ratio: f32,
        first: DockNode,
        second: DockNode,
    ) -> Self {
        Self::Split {
            id: id.into(),
            orientation: SplitterOrientation::Horizontal,
            ratio,
            first: Box::new(first),
            second: Box::new(second),
        }
    }

    /// (R685 §5.16) Convenience constructor for a vertical split
    /// (top / bottom panes). `id` is the stable Split identifier;
    /// `ratio` is the initial split fraction.
    #[must_use]
    pub fn split_vertical(
        id: impl Into<Cow<'static, str>>,
        ratio: f32,
        first: DockNode,
        second: DockNode,
    ) -> Self {
        Self::Split {
            id: id.into(),
            orientation: SplitterOrientation::Vertical,
            ratio,
            first: Box::new(first),
            second: Box::new(second),
        }
    }

    /// (R1083 §5.51) Convenience constructor for a tab well. `id` is the
    /// stable well identifier ([`DockNode::Tabs::id`]); `panels` are the
    /// stacked panel ids in tab order (must be ≥2 + unique for a valid
    /// topology); `active` is the visible tab index (must be `< panels.len()`).
    /// The invariants are enforced at [`DockTopology::try_new`], not here.
    #[must_use]
    pub fn tabs(
        id: impl Into<Cow<'static, str>>,
        panels: impl IntoIterator<Item = Cow<'static, str>>,
        active: usize,
    ) -> Self {
        Self::Tabs {
            id: id.into(),
            panels: panels.into_iter().collect(),
            active,
        }
    }

    /// (R685 §5.16) Count of [`DockNode::Leaf`] nodes in the
    /// sub-tree rooted at `self`. Useful for `panel_views` callback
    /// validation + persistence size limits.
    #[must_use]
    pub fn leaf_count(&self) -> usize {
        match self {
            // (R1083) A tab well is one leaf of the split tree — one pane
            // slot — regardless of how many panels it stacks. Use
            // [`Self::panel_count`] for the total-panels figure.
            Self::Leaf { .. } | Self::Tabs { .. } => 1,
            Self::Split { first, second, .. } => first.leaf_count() + second.leaf_count(),
        }
    }

    /// (R1083 §5.51) Count of distinct panels in the sub-tree rooted at
    /// `self` — every [`DockNode::Leaf`] contributes 1, every
    /// [`DockNode::Tabs`] contributes its stacked-panel count. Equals
    /// `panel_ids().len()`; the number of [`DockPanelExternal`]s a
    /// binding registers (one per panel, tabbed or not).
    #[must_use]
    pub fn panel_count(&self) -> usize {
        match self {
            Self::Leaf { .. } => 1,
            Self::Tabs { panels, .. } => panels.len(),
            Self::Split { first, second, .. } => first.panel_count() + second.panel_count(),
        }
    }

    /// (R685 §5.16) Count of [`DockNode::Split`] nodes in the
    /// sub-tree rooted at `self`. The R685 atomic 1 walker pairs
    /// each Split with a `Signal<f32>` for the ratio drag wire;
    /// this count is the signal-pool size the binding must
    /// register up-front.
    #[must_use]
    pub fn split_count(&self) -> usize {
        match self {
            // A tab well carries no Split divider — only Split nodes pair
            // with a ratio signal.
            Self::Leaf { .. } | Self::Tabs { .. } => 0,
            Self::Split { first, second, .. } => 1 + first.split_count() + second.split_count(),
        }
    }

    /// (R685 §5.16) Walk all leaf panel ids in depth-first order
    /// (first child before second). Order is stable across
    /// serialization round-trips so `panel_views` callback indices
    /// align with `panel_ids()[i]` deterministically.
    #[must_use]
    pub fn panel_ids(&self) -> Vec<&str> {
        let mut out = Vec::with_capacity(self.panel_count());
        self.collect_panel_ids(&mut out);
        out
    }

    fn collect_panel_ids<'a>(&'a self, out: &mut Vec<&'a str>) {
        match self {
            Self::Leaf { panel_id, .. } => out.push(panel_id.as_ref()),
            // (R1083) Every stacked panel is independently addressable —
            // emit them in tab order so `panel_ids()` indices stay stable
            // across serialization round-trips.
            Self::Tabs { panels, .. } => {
                out.extend(panels.iter().map(Cow::as_ref));
            }
            Self::Split { first, second, .. } => {
                first.collect_panel_ids(out);
                second.collect_panel_ids(out);
            }
        }
    }

    /// (R685 §5.16) Walk all Split ids in depth-first pre-order
    /// (visit current Split before recursing into children, first
    /// child before second). Used by the binding to enumerate the
    /// `Rc<Signal<f32>>` ratio handles it must register up-front +
    /// by [`view_dock_surface`]'s validation paths.
    #[must_use]
    pub fn split_ids(&self) -> Vec<&str> {
        let mut out = Vec::with_capacity(self.split_count());
        self.collect_split_ids(&mut out);
        out
    }

    fn collect_split_ids<'a>(&'a self, out: &mut Vec<&'a str>) {
        if let Self::Split {
            id, first, second, ..
        } = self
        {
            out.push(id.as_ref());
            first.collect_split_ids(out);
            second.collect_split_ids(out);
        }
    }

    /// (R685.C atomic 2 §5.16) Depth-first pre-order walk over the
    /// sub-tree's [`DockNode::Split`] nodes; invokes
    /// `f(id, orientation, ratio)` once per Split. The substrate
    /// home for the Split-enumeration walk every dock consumer
    /// needs at boot to register one
    /// [`SplitterExternal`](crate::splitter::SplitterExternal) per
    /// Split.
    ///
    /// Pre-R685.C `hello-dock-panels-editor` carried a binding-local
    /// `for_each_split` copy of this exact walk (DRY violation — the
    /// substrate's [`view_dock_surface_node`] already traverses the
    /// same tree shape). R685.C lifts the walk to the substrate so
    /// every dock consumer shares one traversal implementation.
    pub fn for_each_split<F>(&self, f: &mut F)
    where
        F: FnMut(&str, SplitterOrientation, f32),
    {
        if let Self::Split {
            id,
            orientation,
            ratio,
            first,
            second,
        } = self
        {
            f(id.as_ref(), *orientation, *ratio);
            first.for_each_split(f);
            second.for_each_split(f);
        }
    }

    /// (R1096 §5.51) Depth-first pre-order walk over the sub-tree's
    /// [`DockNode::Tabs`] wells; invokes `f(well_id, active, panel_count)`
    /// once per well. The substrate home for the tab-well enumeration a
    /// binding needs to register one [`TabWellExternal`] per well (the
    /// click-to-switch pointer surface) — the tab-well peer of
    /// [`Self::for_each_split`]. A `Tabs` well has no [`DockNode`] children
    /// (its panels are leaves stacked in the well), so the walk descends
    /// only through [`DockNode::Split`].
    pub fn for_each_tabs_well<F>(&self, f: &mut F)
    where
        F: FnMut(&str, usize, usize),
    {
        match self {
            Self::Leaf { .. } => {}
            Self::Tabs { id, panels, active } => f(id.as_ref(), *active, panels.len()),
            Self::Split { first, second, .. } => {
                first.for_each_tabs_well(f);
                second.for_each_tabs_well(f);
            }
        }
    }

    /// (R1096 §5.51) Count of [`DockNode::Tabs`] wells in the sub-tree —
    /// the number of [`TabWellExternal`]s a binding registers. The tab-well
    /// peer of [`Self::split_count`].
    #[must_use]
    pub fn tabs_well_count(&self) -> usize {
        match self {
            Self::Leaf { .. } => 0,
            Self::Tabs { .. } => 1,
            Self::Split { first, second, .. } => first.tabs_well_count() + second.tabs_well_count(),
        }
    }

    /// (R1096 §5.51) The visible-tab index of the [`DockNode::Tabs`] well
    /// whose `id == well_id` in the sub-tree, or `None` when no such well
    /// exists (the id is absent, or names a [`DockNode::Leaf`] /
    /// [`DockNode::Split`]). The read peer of [`set_active_in_well_rec`]
    /// (the [`DockTopology::set_active_tab`] writer) — used to skip an
    /// already-active click without minting an undo edit.
    #[must_use]
    pub fn tab_well_active(&self, well_id: &str) -> Option<usize> {
        match self {
            Self::Leaf { .. } => None,
            Self::Tabs { id, active, .. } => (id.as_ref() == well_id).then_some(*active),
            Self::Split { first, second, .. } => first
                .tab_well_active(well_id)
                .or_else(|| second.tab_well_active(well_id)),
        }
    }
}

/// (R685 §5.16 §5.49) Root descriptor of a dock topology —
/// thin wrapper around the root [`DockNode`].
///
/// The wrapper exists so future R686+ axes (a `name` field for
/// AI-introspection, per-topology `version` for migration, etc.)
/// can land without breaking the on-disk serde shape (the wrapper
/// itself is `#[non_exhaustive]`). The R685 v1 form is minimal —
/// just the recursive tree.
///
/// ## Lifecycle
///
/// 1. Binding declares the topology at compile time (often via
///    [`DockTopology::single`] for a single-pane case + nested
///    `split_horizontal` / `split_vertical` builders for a
///    multi-pane editor).
/// 2. The binding registers a `Signal<f32>` per [`DockNode::Split`]
///    node (depth-first index) + a `panel_views` callback that
///    maps `panel_id → Scene`.
/// 3. R685 atomic 1's `view_surface(topology, panel_views, signals,
///    theme)` walker emits the nested splitter + panel composition.
/// 4. User drags a splitter → `SplitterExternal::pointer_move` →
///    `Signal<f32>::set` → reactive paint → updated ratio.
/// 5. User drags a panel's header past the tear-off threshold →
///    `DockPanelExternal` emits `tear_off` → binding reducer
///    pushes a `WindowSpec` + removes the leaf from the topology
///    + mutates the topology Signal → reactive repaint.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DockTopology {
    /// Recursive root of the dock tree. Private field (R685.B atomic
    /// 2) — every construction path runs through [`DockTopology::try_new`]
    /// (validation gate) or one of the convenience constructors
    /// ([`DockTopology::single`]), so the invariants every walker /
    /// mutation primitive relies on (unique panel ids, unique split
    /// ids, finite ratios) cannot be broken from outside the module.
    /// Read access via [`DockTopology::root`].
    root: DockNode,
}

/// (R685.B §5.16 §5.49) Errors [`DockTopology::try_new`] can produce
/// when the supplied [`DockNode`] tree violates a topology invariant.
///
/// `#[non_exhaustive]` so future R686+ rounds (drag-to-reorganize,
/// runtime mutation primitives) can introduce additional error
/// classes without breaking downstream `match` arms.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum TopologyError {
    /// Two [`DockNode::Leaf`] nodes carry the same `panel_id`. AI
    /// clients addressing the topology by panel id would resolve
    /// ambiguously; the walker's `panel_content` callback would
    /// receive duplicate calls; the binding's `Owner::cache` slot
    /// keyed on panel id would collide.
    DuplicatePanelId(String),
    /// Two [`DockNode::Split`] nodes carry the same `id`. The
    /// `split_state` callback would receive duplicate calls + the
    /// `Rc<Signal<f32>>` ratio handle would collide on `Owner::cache`.
    DuplicateSplitId(String),
    /// (R685.C atomic 1 §5.16) The same string appears as both a
    /// [`DockNode::Leaf::panel_id`] **and** a [`DockNode::Split::id`].
    /// Pre-R685.C the validator used two separate `HashSet`s and
    /// silently allowed cross-namespace collision; the runtime
    /// failure mode was nasty: the binding's
    /// `create_extra_externals` would register a `DockPanelExternal`
    /// and a `SplitterExternal` at the same paint-side tag, and the
    /// `InputRouter` deepest-tagged hit-test would resolve to
    /// whichever External the registration order put first
    /// (silent ambiguity, no diagnostic). R685.C lifts the
    /// validator to a single `HashSet` and surfaces the collision
    /// as an explicit error.
    IdCollision(String),
    /// A [`DockNode::Split`] carries a non-finite (NaN / Inf) or
    /// out-of-[0,1] `ratio`. Initial ratios outside `[0.0, 1.0]`
    /// produce undefined visual layouts; NaN would corrupt the
    /// `Rc<Signal<f32>>` seed value and propagate through every
    /// taffy flex computation.
    InvalidRatio { split_id: String, ratio: f32 },
    /// A [`DockNode::Split`] or [`DockNode::Leaf`] carries an empty
    /// id string. Empty ids are technically allowed by `Cow` but
    /// collide with the substrate's tag-as-`&str` consumers (the
    /// `InputRouter`'s deepest-tagged hit-test treats empty tags as
    /// no-tag, breaking dispatch).
    EmptyId,
    /// (R686 §5.16 §5.45) A mutation primitive
    /// ([`DockTopology::swap_leaves`] / [`DockTopology::split_leaf_into`]
    /// / [`DockTopology::remove_leaf`]) was given a `panel_id` that no
    /// [`DockNode::Leaf`] in the topology carries. The drag-to-reorganize
    /// gesture resolved a stale panel id, or the binding passed a typo;
    /// either way the mutation cannot proceed without inventing a target.
    PanelNotFound(String),
    /// (R686 §5.16 §5.45) [`DockTopology::remove_leaf`] was asked to
    /// remove the topology's sole panel (the root is a bare
    /// [`DockNode::Leaf`]). A dock topology must always describe at
    /// least one pane — an empty topology has no valid layout — so the
    /// last leaf cannot be removed. The binding closes the whole window
    /// instead.
    RootRemoval,
    /// (R1083 §5.51) Two [`DockNode::Tabs`] wells carry the same `id`.
    /// Like a duplicate Split id, the binding's per-well state
    /// (selection signal / tab-strip tag) would collide.
    DuplicateTabsId(String),
    /// (R1083 §5.51) A [`DockNode::Tabs`] well carries fewer than two
    /// panels. A single-panel well is not a canonical `Tabs` — it must
    /// be a [`DockNode::Leaf`]; the mutation primitives collapse a well
    /// back to a leaf when it shrinks to one panel, so a `< 2` well can
    /// only arise from a hand-built tree.
    TabsTooFew {
        /// The offending well's [`DockNode::Tabs::id`].
        tabs_id: String,
        /// How many panels the well carries (`< 2`).
        count: usize,
    },
    /// (R1083 §5.51) A [`DockNode::Tabs`] well's `active` index is not
    /// `< panels.len()` — the visible-tab index points past the end of
    /// the stack.
    ActiveOutOfRange {
        /// The offending well's [`DockNode::Tabs::id`].
        tabs_id: String,
        /// The out-of-range visible-tab index.
        active: usize,
        /// The well's panel count (`active` must be `< count`).
        count: usize,
    },
    /// (R1085 §5.51) [`DockTopology::set_active_tab`] was given a
    /// `well_id` that names no [`DockNode::Tabs`] well — either no node
    /// in the topology carries that id, or the id belongs to a
    /// [`DockNode::Leaf`] panel / [`DockNode::Split`] divider rather than
    /// a tab well. The `activate_tab` gesture resolved a stale / wrong-kind
    /// id; the activation cannot proceed without inventing a target well.
    TabsWellNotFound(String),
}

impl core::fmt::Display for TopologyError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::DuplicatePanelId(id) => write!(f, "duplicate panel_id: {id:?}"),
            Self::DuplicateSplitId(id) => write!(f, "duplicate split id: {id:?}"),
            Self::IdCollision(id) => write!(
                f,
                "id {id:?} appears as both a panel_id and a Split id; \
                 paint-side tags must be unique across the topology so the \
                 InputRouter deepest-tagged hit-test resolves unambiguously",
            ),
            Self::InvalidRatio { split_id, ratio } => {
                write!(
                    f,
                    "split {split_id:?} has invalid ratio {ratio}; must be finite in [0.0, 1.0]"
                )
            }
            Self::EmptyId => write!(
                f,
                "empty id (panel_id or split id) — empty tags collide with InputRouter dispatch"
            ),
            Self::PanelNotFound(id) => write!(f, "panel_id {id:?} not found in topology"),
            Self::RootRemoval => write!(
                f,
                "cannot remove the topology's sole panel (an empty topology has no valid layout)"
            ),
            Self::DuplicateTabsId(id) => write!(f, "duplicate tab-well id: {id:?}"),
            Self::TabsTooFew { tabs_id, count } => write!(
                f,
                "tab well {tabs_id:?} has {count} panel(s); a well must stack at least 2 \
                 (a single panel is a Leaf, not a Tabs)"
            ),
            Self::ActiveOutOfRange {
                tabs_id,
                active,
                count,
            } => write!(
                f,
                "tab well {tabs_id:?} active index {active} is out of range for {count} panel(s)"
            ),
            Self::TabsWellNotFound(id) => write!(f, "tab-well id {id:?} not found in topology"),
        }
    }
}

impl std::error::Error for TopologyError {}

impl DockTopology {
    /// (R685.B §5.16) Construct a topology from a [`DockNode`] tree
    /// after validating every invariant. Returns [`TopologyError`]
    /// on the first violation found (walk order = depth-first
    /// pre-order); reports the offending id / ratio for diagnosis.
    ///
    /// Invariants checked:
    ///
    /// * No two [`DockNode::Leaf`] nodes share a `panel_id`.
    /// * No two [`DockNode::Split`] nodes share an `id`.
    /// * Every Split's `ratio` is finite + in `[0.0, 1.0]`.
    /// * No panel id or split id is an empty string.
    ///
    /// The walker / mutation primitives downstream all assume these
    /// invariants hold; the validation gate is the only construction
    /// path so an invalid topology cannot reach them.
    ///
    /// # Errors
    ///
    /// Returns [`TopologyError`] on the first invariant violation
    /// encountered in depth-first pre-order walk over `root`. See
    /// the enum variants for the specific failure classes.
    pub fn try_new(root: DockNode) -> Result<Self, TopologyError> {
        // (R685.C atomic 1 §5.16) Unified id namespace — panel_ids
        // and split_ids share one HashSet so cross-namespace
        // collisions surface as `TopologyError::IdCollision`.
        // Tracking which-kind-saw-this-id-first lets the validator
        // emit the right error variant: `DuplicatePanelId` / `DuplicateSplitId`
        // for same-kind collisions, `IdCollision` for cross-kind.
        let mut seen = std::collections::HashMap::<String, NodeKind>::new();
        validate_node(&root, &mut seen)?;
        Ok(Self { root })
    }

    /// (R685.B §5.16) Construct a topology from a hand-built tree,
    /// panicking on invariant violation. Convenience wrapper around
    /// [`Self::try_new`] for tests / hard-coded topologies where a
    /// violation is a programmer bug rather than a runtime concern.
    ///
    /// # Panics
    ///
    /// Panics with the [`TopologyError`] `Display` representation
    /// if `root` violates any topology invariant.
    #[must_use]
    pub fn new(root: DockNode) -> Self {
        Self::try_new(root)
            .expect("DockTopology::new: invariant violation; use try_new for fallible construction")
    }

    /// (R685 §5.16) Convenience constructor for a single-panel
    /// topology (one leaf, no splits).
    ///
    /// (R685.C atomic 0 §5.16) Routes through [`Self::try_new`] for
    /// consistent panic shape — pre-R685.C used an inline `assert!`
    /// with a distinct message format. The unified path emits
    /// `expect("...{TopologyError}...")` matching [`Self::new`] for
    /// every other invariant violation, so callers see one
    /// `TopologyError`-driven failure mode regardless of constructor.
    ///
    /// # Panics
    ///
    /// Panics with the [`TopologyError`] `Display` representation
    /// if `panel_id` violates any topology invariant (empty id is
    /// the only failure mode reachable for a single-leaf tree).
    #[must_use]
    pub fn single(panel_id: impl Into<Cow<'static, str>>) -> Self {
        Self::try_new(DockNode::Leaf {
            panel_id: panel_id.into(),
        })
        .expect("DockTopology::single: panel_id must be non-empty")
    }

    /// (R685.B §5.16) Read-only access to the recursive root node.
    /// Pre-R685.B atomic 2 the `root` field was `pub` — every R686+
    /// mutation primitive (drag-to-reorganize) needs to enforce the
    /// validation invariants on its return type, so external mutation
    /// is no longer permitted (mutations go through future
    /// `swap_leaves` / `split_leaf_into` / `remove_leaf` primitives
    /// that all return `Result<DockTopology, TopologyError>`).
    #[must_use]
    pub fn root(&self) -> &DockNode {
        &self.root
    }

    /// (R685 §5.16) Depth-first ordered list of all panel ids in
    /// the topology. Order is stable across serialization.
    #[must_use]
    pub fn panel_ids(&self) -> Vec<&str> {
        self.root.panel_ids()
    }

    /// (R685 §5.16) Depth-first pre-order list of all Split ids in
    /// the topology. The binding registers one `Rc<Signal<f32>>`
    /// ratio handle per id at boot.
    #[must_use]
    pub fn split_ids(&self) -> Vec<&str> {
        self.root.split_ids()
    }

    /// (R685.C atomic 2 §5.16) Depth-first pre-order walk over the
    /// topology's [`DockNode::Split`] nodes; invokes
    /// `f(id, orientation, ratio)` once per Split. The canonical
    /// boot-time enumeration for dock consumers registering one
    /// [`SplitterExternal`](crate::splitter::SplitterExternal) per
    /// Split (mirror of [`view_dock_surface`]'s own traversal, so
    /// the binding never re-implements the walk).
    pub fn for_each_split<F>(&self, mut f: F)
    where
        F: FnMut(&str, SplitterOrientation, f32),
    {
        self.root.for_each_split(&mut f);
    }

    /// (R1096 §5.51) Depth-first pre-order walk over the topology's
    /// [`DockNode::Tabs`] wells; invokes `f(well_id, active, panel_count)`
    /// once per well. The canonical enumeration for a binding registering
    /// one [`TabWellExternal`] per well (so a tab click switches the active
    /// tab) — the tab-well peer of [`Self::for_each_split`], shared so the
    /// binding never re-walks the tree itself.
    pub fn for_each_tabs_well<F>(&self, mut f: F)
    where
        F: FnMut(&str, usize, usize),
    {
        self.root.for_each_tabs_well(&mut f);
    }

    /// (R1096 §5.51) Count of [`DockNode::Tabs`] wells — the number of
    /// [`TabWellExternal`]s the binding registers. The tab-well peer of
    /// [`Self::split_count`].
    #[must_use]
    pub fn tabs_well_count(&self) -> usize {
        self.root.tabs_well_count()
    }

    /// (R1096 §5.51) The visible-tab index of the [`DockNode::Tabs`] well
    /// `well_id`, or `None` when no such well exists. The read peer of
    /// [`Self::set_active_tab`].
    #[must_use]
    pub fn tab_well_active(&self, well_id: &str) -> Option<usize> {
        self.root.tab_well_active(well_id)
    }

    /// (R685 §5.16) Count of leaf panes (each [`DockNode::Leaf`] **and**
    /// each [`DockNode::Tabs`] well counts as one). For the total panel
    /// count (tab-well panels counted individually) use [`Self::panel_count`].
    #[must_use]
    pub fn leaf_count(&self) -> usize {
        self.root.leaf_count()
    }

    /// (R1083 §5.51) Count of distinct panels — equals
    /// `self.panel_ids().len()`. Each [`DockNode::Leaf`] contributes 1,
    /// each [`DockNode::Tabs`] well its stacked-panel count.
    #[must_use]
    pub fn panel_count(&self) -> usize {
        self.root.panel_count()
    }

    /// (R685 §5.16) Count of Split nodes. Equals the signal-pool
    /// size the binding must register up-front for the
    /// `view_dock_surface` walker's ratio drag wire.
    #[must_use]
    pub fn split_count(&self) -> usize {
        self.root.split_count()
    }

    // ─────────────────────────────────────────────────────────────────
    // R686 §5.16 §5.45 — drag-to-reorganize mutation primitives.
    //
    // Every primitive is `&self -> Result<DockTopology, TopologyError>`
    // (immutable / functional form): it produces a *new* validated
    // topology rather than mutating in place. This is the textbook
    // canonical shape for an editable document with undo/redo —
    // the binding holds a `Signal<Option<DockTopology>>` (R1084: `None`
    // = empty dock), computes the next value, and `set`s it (or discards
    // it on `Err`), so the reactive
    // re-render is a clean swap. Every result flows back through
    // [`Self::try_new`] so an invalid intermediate tree (a generated id
    // colliding with an existing one, say) surfaces as a typed error
    // instead of corrupting the live topology.
    // ─────────────────────────────────────────────────────────────────

    /// (R686 §5.16 §5.45) Swap the two named panels' positions in the
    /// tree — the panel previously at `panel_id_a`'s location now sits
    /// where `panel_id_b` was, and vice versa. The tree *shape* (every
    /// Split, every ratio, every split id) is unchanged; only which panel
    /// occupies which slot changes. (R1083) A swapped panel may live
    /// inside a [`DockNode::Tabs`] well — it is relabelled in place.
    ///
    /// No drop zone produces a swap: R1083 made the centre gesture
    /// [`DockTopology::tabify`] (a tab-stack). `swap_leaves` remains the
    /// primitive for an explicit programmatic swap.
    ///
    /// `panel_id_a == panel_id_b` (swapping a panel with itself) is a
    /// well-defined no-op that returns the topology unchanged.
    ///
    /// # Errors
    ///
    /// [`TopologyError::PanelNotFound`] if either id names no panel.
    pub fn swap_leaves(
        &self,
        panel_id_a: &str,
        panel_id_b: &str,
    ) -> Result<DockTopology, TopologyError> {
        let ids = self.root.panel_ids();
        if !ids.contains(&panel_id_a) {
            return Err(TopologyError::PanelNotFound(panel_id_a.to_string()));
        }
        if !ids.contains(&panel_id_b) {
            return Err(TopologyError::PanelNotFound(panel_id_b.to_string()));
        }
        let mut new_root = self.root.clone();
        swap_panel_ids_rec(&mut new_root, panel_id_a, panel_id_b);
        // Swapping two distinct existing unique ids preserves uniqueness,
        // so try_new cannot fail here; routing through it anyway keeps
        // the single-construction-path invariant uniform.
        Self::try_new(new_root)
    }

    /// (R686 §5.16 §5.45) Replace the leaf at `panel_id` with a new
    /// [`DockNode::Split`] holding the original panel beside a freshly
    /// inserted `new_leaf_panel_id`. `position` chooses which slot
    /// (`first` / `second`) the inserted panel takes; `new_orientation`
    /// + `new_ratio` describe the divider.
    ///
    /// This is the [`DockDropZone`] edge gesture's substrate side:
    /// dropping a dragged panel on the target's left edge calls
    /// `split_leaf_into(target, dragged, fresh_split_id, Horizontal,
    /// 0.5, First)`. To *move* an existing panel (rather than spawn a
    /// new one) the binding composes [`Self::remove_leaf`] first, then
    /// this — see the editor reducer (R686 atomic 3).
    ///
    /// # Errors
    ///
    /// * [`TopologyError::PanelNotFound`] if `panel_id` names no leaf.
    /// * [`TopologyError::DuplicatePanelId`] /
    ///   [`TopologyError::DuplicateSplitId`] /
    ///   [`TopologyError::IdCollision`] if `new_leaf_panel_id` or
    ///   `new_split_id` collides with an existing id (surfaced by the
    ///   [`Self::try_new`] validation gate).
    /// * [`TopologyError::InvalidRatio`] if `new_ratio` is non-finite or
    ///   outside `[0.0, 1.0]`.
    pub fn split_leaf_into(
        &self,
        panel_id: &str,
        new_leaf_panel_id: impl Into<Cow<'static, str>>,
        new_split_id: impl Into<Cow<'static, str>>,
        new_orientation: SplitterOrientation,
        new_ratio: f32,
        position: DockSplitPosition,
    ) -> Result<DockTopology, TopologyError> {
        if !self.root.panel_ids().contains(&panel_id) {
            return Err(TopologyError::PanelNotFound(panel_id.to_string()));
        }
        let mut insertion = Some(SplitInsertion {
            new_leaf_id: new_leaf_panel_id.into(),
            split_id: new_split_id.into(),
            orientation: new_orientation,
            ratio: new_ratio,
            position,
        });
        let new_root = split_leaf_rec(&self.root, panel_id, &mut insertion);
        Self::try_new(new_root)
    }

    /// (R686 §5.16 §5.45) Remove the leaf at `panel_id` and promote its
    /// sibling sub-tree into the parent Split's place. The parent Split
    /// (and its id + ratio) disappears; the surviving sibling slides up
    /// one level. Every other leaf and Split is untouched.
    ///
    /// This is the source side of a drag-to-reparent gesture: the
    /// binding calls `remove_leaf(dragged)` then
    /// [`Self::split_leaf_into`] at the drop target. It is also the
    /// topology mutation behind the R683 tear-off-to-floating-window
    /// gesture (the torn-off panel leaves the docked tree).
    ///
    /// # Errors
    ///
    /// * [`TopologyError::PanelNotFound`] if `panel_id` names no leaf.
    /// * [`TopologyError::RootRemoval`] if `panel_id` is the topology's
    ///   sole panel (the root is a bare leaf) — an empty topology has no
    ///   valid layout.
    pub fn remove_leaf(&self, panel_id: &str) -> Result<DockTopology, TopologyError> {
        if !self.root.panel_ids().contains(&panel_id) {
            return Err(TopologyError::PanelNotFound(panel_id.to_string()));
        }
        match remove_leaf_rec(&self.root, panel_id) {
            Some(new_root) => Self::try_new(new_root),
            // The recursion returns `None` only when the root node *is*
            // the target leaf — i.e. the topology has a single pane.
            None => Err(TopologyError::RootRemoval),
        }
    }

    /// (R1132 §5.51.1) Capture the **home anchor** of the bare leaf at
    /// `panel_id` — the parent-Split context needed to restore it after a
    /// [`Self::remove_leaf`]. The `(leaf_anchor, insert_leaf_at_anchor)` pair is
    /// the substrate a COLLAPSE-policy float needs: tear-off removes the leaf so
    /// the layout reflows, and dock-back restores it next to its original
    /// sibling — the typed home-anchor the §5.51.1 chart spec mandates *instead
    /// of* a hardcoded default redock slot (the §2 #5 drift this campaign
    /// retires). Read-only; the binding captures the anchor at float time and
    /// holds it (typed Rust sidecar) until dock-back.
    ///
    /// Returns `None` when the leaf has no parent Split to anchor against — the
    /// topology root (a sole pane, nothing to reflow) or a panel inside a
    /// [`DockNode::Tabs`] well (a well member restores by re-tabbing, a distinct
    /// anchor this round does not model; the binding falls back). The captured
    /// [`DockLeafAnchor::sibling`] is a representative panel of the OTHER child
    /// subtree; [`Self::insert_leaf_at_anchor`] re-splits it. For a **leaf**
    /// sibling the `remove_leaf` → `insert_leaf_at_anchor` round-trip is EXACT;
    /// for a **subtree** sibling the leaf returns adjacent to the sibling's
    /// representative (sensible, no panel lost — a fully subtree-faithful wrap is
    /// a deferred follow-up gated on a consumer that needs pixel-exact nesting).
    #[must_use]
    pub fn leaf_anchor(&self, panel_id: &str) -> Option<DockLeafAnchor> {
        leaf_anchor_rec(&self.root, panel_id)
    }

    /// (R1132 §5.51.1) Restore a leaf to the position [`Self::leaf_anchor`]
    /// captured — the dock-back inverse of [`Self::remove_leaf`] for a
    /// collapse-policy float. Re-splits the anchor's sibling, placing `panel_id`
    /// back on its original side with the original orientation / ratio / split id
    /// (reusing the id so the binding's per-split state re-binds). Pure
    /// composition over [`Self::split_leaf_into`].
    ///
    /// # Errors
    ///
    /// * [`TopologyError::PanelNotFound`] if the anchor's `sibling` no longer
    ///   exists (it was itself removed since capture) — the caller falls back to
    ///   a default dock.
    /// * the [`Self::split_leaf_into`] id-collision / non-finite-ratio errors.
    pub fn insert_leaf_at_anchor(
        &self,
        panel_id: impl Into<Cow<'static, str>>,
        anchor: &DockLeafAnchor,
    ) -> Result<DockTopology, TopologyError> {
        self.split_leaf_into(
            &anchor.sibling,
            panel_id,
            anchor.split_id.clone(),
            anchor.orientation,
            anchor.ratio,
            anchor.position,
        )
    }

    /// (R1083 §5.51) Merge the dragged `source` panel into `target`'s pane
    /// as a tab well — the [`DockDropZone::Center`] gesture's substrate
    /// side (replacing the tab-less v1 swap). `source` is removed from its
    /// current location (its old split collapses / its old well shrinks),
    /// then stacked onto `target`:
    ///
    /// * `target` a bare [`DockNode::Leaf`] → becomes a fresh
    ///   [`DockNode::Tabs`] well `[target, source]` keyed on `new_tabs_id`,
    ///   `source` the visible tab.
    /// * `target` already inside a [`DockNode::Tabs`] well → `source` is
    ///   appended and becomes the visible tab; `new_tabs_id` is unused.
    ///
    /// Two drops are well-defined without relocating a panel: `source ==
    /// target` (dropping a panel on its own centre) is an unchanged no-op;
    /// and a drop where `source` and `target` **already share a tab well**
    /// is an *in-well activation* — `source` becomes the visible tab, the
    /// well's `id` and panel order preserved. (R1084.1) The latter must NOT
    /// re-mint the well id: a same-well drop runs the remove-then-restack
    /// path only if unguarded, and for a 2-panel well that path collapses
    /// the well to a `Leaf` and re-promotes it under `new_tabs_id` — losing
    /// the stable well id that keys the binding's per-well state. The
    /// reachable trigger is the RPC `reorganize`/`drop` path naming an
    /// inactive well member as `source` (a pointer drag can only originate
    /// on the visible tab, so it cannot reach this). Otherwise the leaf
    /// count is invariant — one panel relocates, none created or destroyed.
    ///
    /// # Errors
    ///
    /// * [`TopologyError::PanelNotFound`] if `source` or `target` names no
    ///   panel.
    /// * [`TopologyError::DuplicateTabsId`] / [`TopologyError::IdCollision`]
    ///   if `new_tabs_id` collides with an existing id (only when a fresh
    ///   well is minted; surfaced by the [`Self::try_new`] gate).
    ///
    /// # Panics
    ///
    /// Never in practice: the internal source-removal is guarded to be
    /// unreachable as a tree-emptying operation, because the cross-pane path
    /// runs only when `source != target` and they are in different panes,
    /// both verified present, so `target` always survives the removal.
    pub fn tabify(
        &self,
        source: &str,
        target: &str,
        new_tabs_id: impl Into<Cow<'static, str>>,
    ) -> Result<DockTopology, TopologyError> {
        let ids = self.root.panel_ids();
        if !ids.contains(&source) {
            return Err(TopologyError::PanelNotFound(source.to_string()));
        }
        if !ids.contains(&target) {
            return Err(TopologyError::PanelNotFound(target.to_string()));
        }
        if source == target {
            // Unchanged no-op — but route through `try_new` (the input is
            // already valid) to keep the single-construction-path invariant
            // uniform, matching `swap_leaves`' `a == b` discipline.
            return Self::try_new(self.root.clone());
        }
        // (R1084.1) Same-well drop: `source` + `target` are already stacked
        // together, so stacking is a membership no-op — bring `source`
        // forward (active), preserving the well id + order. Skipping this
        // would re-mint the well id via the collapse-then-re-promote path.
        if let Some(new_root) = activate_in_shared_well(&self.root, source, target) {
            return Self::try_new(new_root);
        }
        // Cross-pane drop: remove source from its current location (collapsing
        // its old split / shrinking its old well), then stack it onto target.
        // They are in different panes and both present, so removal leaves
        // target in place and never empties the tree — `remove_leaf_rec`
        // never returns `None` here.
        let removed = remove_leaf_rec(&self.root, source)
            .expect("tabify: source removal cannot empty the tree (target still present)");
        let mut source_id = Some(Cow::Owned(source.to_string()));
        let mut new_tabs_id = Some(new_tabs_id.into());
        let new_root = tabify_into_rec(&removed, target, &mut source_id, &mut new_tabs_id);
        Self::try_new(new_root)
    }

    /// (R1126 §5.51 §2 #7 PR-33) Stack a FRESH `new_panel` leaf onto `target` as a
    /// tab well — the INSERT counterpart of [`Self::tabify`] (which MOVES an
    /// existing panel by removing it first). The explicit fresh verb so a
    /// COLLAPSE-policy redock (the returning floater's leaf was removed on
    /// tear-off, so it is absent) can tabify without a source-removal — mirroring
    /// [`Self::split_leaf_into`] for edge zones. The [`DockReorganizer::dock_panel_at_zone`]
    /// coordinator normalises to absence (remove-if-present) THEN calls this, so a
    /// present panel re-tabs via remove+insert and an absent one inserts directly:
    /// ONE zone-honoring redock total over both torn-slot policies.
    ///
    /// # Errors
    ///
    /// * [`TopologyError::PanelNotFound`] if `target` names no panel.
    /// * [`TopologyError::DuplicatePanelId`] if `new_panel` already exists (a fresh
    ///   insert must not duplicate a live panel — the caller removes it first).
    /// * [`TopologyError::DuplicateTabsId`] / [`TopologyError::IdCollision`] if
    ///   `new_tabs_id` collides (surfaced by the [`Self::try_new`] gate).
    pub fn tabify_fresh(
        &self,
        new_panel: impl Into<Cow<'static, str>>,
        target: &str,
        new_tabs_id: impl Into<Cow<'static, str>>,
    ) -> Result<DockTopology, TopologyError> {
        let new_panel = new_panel.into();
        let ids = self.root.panel_ids();
        if !ids.contains(&target) {
            return Err(TopologyError::PanelNotFound(target.to_string()));
        }
        if ids.contains(&new_panel.as_ref()) {
            return Err(TopologyError::DuplicatePanelId(new_panel.to_string()));
        }
        // Stack the fresh leaf onto `target` WITHOUT a prior removal (the panel is
        // genuinely new to this tree), minting a well if `target` is a bare leaf.
        let mut source_id = Some(new_panel);
        let mut new_tabs_id = Some(new_tabs_id.into());
        let new_root = tabify_into_rec(&self.root, target, &mut source_id, &mut new_tabs_id);
        Self::try_new(new_root)
    }

    /// (R1085 §5.51) Make tab `index` the visible tab of the
    /// [`DockNode::Tabs`] well identified by `well_id` — the tab-well
    /// navigation primitive (click a tab / `activate_tab` invoke).
    ///
    /// Returns a new validated topology with that well's
    /// [`DockNode::Tabs::active`] set to `index`. Unlike the
    /// reorganize mutations ([`Self::swap_leaves`] / [`Self::tabify`] /
    /// [`Self::split_leaf_into`]) this is **not a move**: no panel
    /// changes location, no id is minted, only the well's visible-tab
    /// index changes. Every other node is preserved byte-for-byte.
    ///
    /// `index == active` is an accepted no-op (the well rebuilds with the
    /// same active), mirroring [`Self::swap_leaves`]'s `a == b` /
    /// [`Self::tabify`]'s `source == target` idempotent discipline — the
    /// caller (the coordinator) decides whether to record / republish.
    ///
    /// # Errors
    ///
    /// * [`TopologyError::TabsWellNotFound`] if `well_id` names no
    ///   [`DockNode::Tabs`] well (no such id, or the id belongs to a
    ///   [`DockNode::Leaf`] panel / [`DockNode::Split`] divider).
    /// * [`TopologyError::ActiveOutOfRange`] if `index >= panels.len()`
    ///   for the well — surfaced by the [`Self::try_new`] validation gate
    ///   (the single invariant checker), not re-implemented here.
    pub fn set_active_tab(
        &self,
        well_id: &str,
        index: usize,
    ) -> Result<DockTopology, TopologyError> {
        let Some(new_root) = set_active_in_well_rec(&self.root, well_id, index) else {
            return Err(TopologyError::TabsWellNotFound(well_id.to_string()));
        };
        // `try_new` validates the new `active` against the well's panel
        // count (and re-checks every other invariant), so an out-of-range
        // index becomes `ActiveOutOfRange` from the one validation gate.
        Self::try_new(new_root)
    }
}

/// (R686 §5.16 §5.45) Which slot of a newly created
/// [`DockNode::Split`] an inserted leaf occupies in
/// [`DockTopology::split_leaf_into`].
///
/// `First` is the left child of a `Horizontal` split / the top child
/// of a `Vertical` split; `Second` is the right / bottom child. The
/// edge-zone → position mapping for drag-to-reorganize is:
/// [`DockDropZone::Left`] / [`DockDropZone::Top`] → `First`;
/// [`DockDropZone::Right`] / [`DockDropZone::Bottom`] → `Second`.
///
/// `#[non_exhaustive]` for symmetry with the other R686 enums (a
/// future N-ary or tab-stack insert position could land here).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockSplitPosition {
    /// Inserted leaf takes the Split's `first` slot (left / top); the
    /// pre-existing leaf moves to `second`.
    First,
    /// Inserted leaf takes the Split's `second` slot (right / bottom);
    /// the pre-existing leaf stays as `first`.
    Second,
}

/// (R1132 §5.51.1) The home anchor of a docked leaf — the parent-Split context
/// captured by [`DockTopology::leaf_anchor`] so a collapse-policy float can
/// [`remove_leaf`](DockTopology::remove_leaf) (reflow the layout) and a dock-back
/// [`insert_leaf_at_anchor`](DockTopology::insert_leaf_at_anchor) restore the
/// leaf next to its original sibling. This is the typed home-anchor the §5.51.1
/// dock-lifecycle chart spec mandates — the binding owns it as a typed Rust
/// sidecar — *instead of* a hardcoded default redock slot (the §2 #5 drift).
#[derive(Debug, Clone, PartialEq)]
pub struct DockLeafAnchor {
    /// A representative panel id of the sibling subtree the removed leaf was
    /// paired with under its parent Split. [`DockTopology::insert_leaf_at_anchor`]
    /// re-splits this panel — exact restore when the sibling was a bare leaf.
    pub sibling: String,
    /// The parent Split's stable id, reused on restore so the binding's per-split
    /// (ratio handle / [`SplitterExternal`](crate::splitter::SplitterExternal))
    /// state re-binds to the same id.
    pub split_id: String,
    /// The parent Split's layout axis.
    pub orientation: SplitterOrientation,
    /// The parent Split's position ratio.
    pub ratio: f32,
    /// Which side the removed leaf occupied (`First` = left / top).
    pub position: DockSplitPosition,
}

/// (R1132 §5.51.1) Recursive capture of a bare leaf's parent-Split anchor —
/// the read side of [`DockTopology::leaf_anchor`]. Returns the anchor for the
/// first Split whose `first` / `second` child is the bare [`DockNode::Leaf`]
/// `target`; `None` when `target` is the root leaf or lives inside a
/// [`DockNode::Tabs`] well (no parent Split to anchor against).
fn leaf_anchor_rec(node: &DockNode, target: &str) -> Option<DockLeafAnchor> {
    let DockNode::Split {
        id,
        orientation,
        ratio,
        first,
        second,
    } = node
    else {
        return None;
    };
    let is_target =
        |n: &DockNode| matches!(n, DockNode::Leaf { panel_id } if panel_id.as_ref() == target);
    if is_target(first) {
        return Some(DockLeafAnchor {
            sibling: second.panel_ids().first().copied()?.to_string(),
            split_id: id.to_string(),
            orientation: *orientation,
            ratio: *ratio,
            position: DockSplitPosition::First,
        });
    }
    if is_target(second) {
        return Some(DockLeafAnchor {
            sibling: first.panel_ids().first().copied()?.to_string(),
            split_id: id.to_string(),
            orientation: *orientation,
            ratio: *ratio,
            position: DockSplitPosition::Second,
        });
    }
    leaf_anchor_rec(first, target).or_else(|| leaf_anchor_rec(second, target))
}

/// (R686 §5.16 §5.45) In-place swap of two panel ids throughout a
/// cloned tree. Each leaf carrying `a` becomes `b` and vice versa.
/// Panel ids are unique (topology invariant) so at most one leaf
/// matches each id; the single mutable pass touches both.
fn swap_panel_ids_rec(node: &mut DockNode, a: &str, b: &str) {
    match node {
        DockNode::Leaf { panel_id } => {
            if panel_id.as_ref() == a {
                *panel_id = Cow::Owned(b.to_string());
            } else if panel_id.as_ref() == b {
                *panel_id = Cow::Owned(a.to_string());
            }
        }
        // (R1083) A swapped panel may live inside a tab well — relabel it
        // in place. Panel ids are unique so at most one of `a` / `b` is in
        // any one well, but a well could hold one of each (both relabel).
        DockNode::Tabs { panels, .. } => {
            for panel_id in panels {
                if panel_id.as_ref() == a {
                    *panel_id = Cow::Owned(b.to_string());
                } else if panel_id.as_ref() == b {
                    *panel_id = Cow::Owned(a.to_string());
                }
            }
        }
        DockNode::Split { first, second, .. } => {
            swap_panel_ids_rec(first, a, b);
            swap_panel_ids_rec(second, a, b);
        }
    }
}

/// (R686 §5.16 §5.45) The owned payload [`DockTopology::split_leaf_into`]
/// threads into the tree rebuild. Carried in an `Option` so
/// [`split_leaf_rec`] can `take()` it at the single target leaf — the
/// owned `Cow`s move into the new nodes exactly once (preserving
/// `Cow::Borrowed` for static ids; no re-allocation), and the `take`
/// makes "inserted at most once" a structural guarantee.
struct SplitInsertion {
    new_leaf_id: Cow<'static, str>,
    split_id: Cow<'static, str>,
    orientation: SplitterOrientation,
    ratio: f32,
    position: DockSplitPosition,
}

/// (R686 §5.16 §5.45) Rebuild `node`, replacing the `target` leaf with
/// a new Split holding the original panel beside the inserted leaf. The
/// caller has already verified `target` exists, so the `insertion` is
/// consumed at exactly one leaf; every other node is cloned verbatim.
fn split_leaf_rec(
    node: &DockNode,
    target: &str,
    insertion: &mut Option<SplitInsertion>,
) -> DockNode {
    match node {
        DockNode::Leaf { panel_id } if panel_id.as_ref() == target => {
            let ins = insertion.take().expect(
                "split_leaf_rec: target leaf visited more than once (unique-id invariant broken)",
            );
            let existing = DockNode::Leaf {
                panel_id: panel_id.clone(),
            };
            let inserted = DockNode::Leaf {
                panel_id: ins.new_leaf_id,
            };
            let (first, second) = match ins.position {
                DockSplitPosition::First => (inserted, existing),
                DockSplitPosition::Second => (existing, inserted),
            };
            DockNode::Split {
                id: ins.split_id,
                orientation: ins.orientation,
                ratio: ins.ratio,
                first: Box::new(first),
                second: Box::new(second),
            }
        }
        // (R1083) An edge drop landing on a panel that lives in a tab well
        // splits the **whole well** — the stacked panels stay together as
        // one child of the new Split, the dragged panel becomes the
        // sibling. (The router resolves the well's active panel as the
        // drop target, and that panel is in this well.)
        DockNode::Tabs { panels, .. } if panels.iter().any(|p| p.as_ref() == target) => {
            let ins = insertion.take().expect(
                "split_leaf_rec: target well visited more than once (unique-id invariant broken)",
            );
            let existing = node.clone();
            let inserted = DockNode::Leaf {
                panel_id: ins.new_leaf_id,
            };
            let (first, second) = match ins.position {
                DockSplitPosition::First => (inserted, existing),
                DockSplitPosition::Second => (existing, inserted),
            };
            DockNode::Split {
                id: ins.split_id,
                orientation: ins.orientation,
                ratio: ins.ratio,
                first: Box::new(first),
                second: Box::new(second),
            }
        }
        DockNode::Leaf { .. } | DockNode::Tabs { .. } => node.clone(),
        DockNode::Split {
            id,
            orientation,
            ratio,
            first,
            second,
        } => DockNode::Split {
            id: id.clone(),
            orientation: *orientation,
            ratio: *ratio,
            first: Box::new(split_leaf_rec(first, target, insertion)),
            second: Box::new(split_leaf_rec(second, target, insertion)),
        },
    }
}

/// (R686 §5.16 §5.45) Rebuild `node` with the `target` leaf removed.
///
/// Returns:
/// * `Some(rebuilt)` — the sub-tree with `target` gone (a Split whose
///   child was `target` collapses to its surviving sibling; an
///   unaffected sub-tree is cloned verbatim).
/// * `None` — `node` itself *is* the target leaf, signalling the parent
///   to drop it and promote the sibling. A `None` at the top level means
///   the root was the sole leaf ([`TopologyError::RootRemoval`]).
fn remove_leaf_rec(node: &DockNode, target: &str) -> Option<DockNode> {
    match node {
        DockNode::Leaf { panel_id } => {
            if panel_id.as_ref() == target {
                None
            } else {
                Some(node.clone())
            }
        }
        // (R1083) Removing a panel from a tab well shrinks the well; a
        // well that drops to a single panel collapses back to a [`Leaf`]
        // (the ≥2 canonical invariant). A well never returns `None` — it
        // always retains ≥1 panel — so it never signals the parent Split
        // to promote a sibling (only a bare target [`Leaf`] does that).
        DockNode::Tabs { id, panels, active } => {
            let Some(removed_idx) = panels.iter().position(|p| p.as_ref() == target) else {
                return Some(node.clone());
            };
            let mut remaining: Vec<Cow<'static, str>> = panels.clone();
            remaining.remove(removed_idx);
            if remaining.len() == 1 {
                return Some(DockNode::Leaf {
                    panel_id: remaining.into_iter().next().expect("len == 1"),
                });
            }
            // Keep the visible tab pointing at the same panel where
            // possible: a removal before `active` shifts it down one; a
            // removal of (or past the new end at) `active` clamps to the
            // last tab.
            let mut new_active = if *active > removed_idx {
                active - 1
            } else {
                *active
            };
            if new_active >= remaining.len() {
                new_active = remaining.len() - 1;
            }
            Some(DockNode::Tabs {
                id: id.clone(),
                panels: remaining,
                active: new_active,
            })
        }
        DockNode::Split {
            id,
            orientation,
            ratio,
            first,
            second,
        } => {
            let new_first = remove_leaf_rec(first, target);
            let new_second = remove_leaf_rec(second, target);
            match (new_first, new_second) {
                // Target was inside `first` and `first` collapsed to
                // nothing (it was the bare target leaf) → promote second.
                (None, Some(s)) => Some(s),
                // Symmetric: target was `second` → promote first.
                (Some(f), None) => Some(f),
                // Neither child was the bare target → keep this Split,
                // wiring in the (possibly rebuilt) children.
                (Some(f), Some(s)) => Some(DockNode::Split {
                    id: id.clone(),
                    orientation: *orientation,
                    ratio: *ratio,
                    first: Box::new(f),
                    second: Box::new(s),
                }),
                // Both children reported themselves as the target leaf —
                // impossible for a validated topology (panel ids are
                // unique, so the target appears at most once).
                (None, None) => {
                    unreachable!("panel id {target:?} cannot appear in both children of a Split")
                }
            }
        }
    }
}

/// (R1083 §5.51) Rebuild `node`, stacking `source` onto the pane that
/// holds `target`. The caller has verified both panels exist + differ, so
/// `source` is consumed at exactly one node (threaded through an `Option`
/// like [`SplitInsertion`], the "inserted at most once" structural
/// guarantee). `new_tabs_id` is taken only when `target` is a bare
/// [`DockNode::Leaf`] promoted to a fresh well — tabifying into an
/// existing well leaves it untouched.
fn tabify_into_rec(
    node: &DockNode,
    target: &str,
    source_id: &mut Option<Cow<'static, str>>,
    new_tabs_id: &mut Option<Cow<'static, str>>,
) -> DockNode {
    match node {
        // target is a bare leaf → promote to a 2-tab well, source visible.
        DockNode::Leaf { panel_id } if panel_id.as_ref() == target => {
            let id = new_tabs_id
                .take()
                .expect("tabify_into_rec: target visited more than once (unique-id invariant)");
            let source = source_id
                .take()
                .expect("tabify_into_rec: source consumed more than once");
            DockNode::Tabs {
                id,
                panels: vec![panel_id.clone(), source],
                active: 1,
            }
        }
        // target already in a well → append source, make it the visible tab.
        DockNode::Tabs { id, panels, .. } if panels.iter().any(|p| p.as_ref() == target) => {
            let source = source_id
                .take()
                .expect("tabify_into_rec: source consumed more than once");
            let mut panels = panels.clone();
            let new_active = panels.len();
            panels.push(source);
            DockNode::Tabs {
                id: id.clone(),
                panels,
                active: new_active,
            }
        }
        DockNode::Leaf { .. } | DockNode::Tabs { .. } => node.clone(),
        DockNode::Split {
            id,
            orientation,
            ratio,
            first,
            second,
        } => DockNode::Split {
            id: id.clone(),
            orientation: *orientation,
            ratio: *ratio,
            first: Box::new(tabify_into_rec(first, target, source_id, new_tabs_id)),
            second: Box::new(tabify_into_rec(second, target, source_id, new_tabs_id)),
        },
    }
}

/// (R1084.1 §5.51) If `source` and `target` are both panels of the **same**
/// [`DockNode::Tabs`] well, rebuild the tree with that well's `active` set to
/// `source`'s index — preserving the well `id` and panel order — and return
/// `Some`. Returns `None` when they are in different panes (the caller then
/// uses the remove-then-restack path). A same-well tabify is a membership
/// no-op, so bringing `source` forward is the only meaningful effect; the
/// stable well id MUST survive it (it keys the binding's per-well state), so
/// this never goes through the id-re-minting collapse/restack path.
fn activate_in_shared_well(node: &DockNode, source: &str, target: &str) -> Option<DockNode> {
    match node {
        DockNode::Leaf { .. } => None,
        DockNode::Tabs { id, panels, .. } => {
            let src_idx = panels.iter().position(|p| p.as_ref() == source)?;
            // Only this well's *own* members count — `source` is here, so the
            // well shares both iff `target` is here too.
            panels
                .iter()
                .any(|p| p.as_ref() == target)
                .then(|| DockNode::Tabs {
                    id: id.clone(),
                    panels: panels.clone(),
                    active: src_idx,
                })
        }
        DockNode::Split {
            id,
            orientation,
            ratio,
            first,
            second,
        } => {
            if let Some(f) = activate_in_shared_well(first, source, target) {
                return Some(DockNode::Split {
                    id: id.clone(),
                    orientation: *orientation,
                    ratio: *ratio,
                    first: Box::new(f),
                    second: second.clone(),
                });
            }
            activate_in_shared_well(second, source, target).map(|s| DockNode::Split {
                id: id.clone(),
                orientation: *orientation,
                ratio: *ratio,
                first: first.clone(),
                second: Box::new(s),
            })
        }
    }
}

/// (R1085 §5.51) Rebuild the tree with the [`DockNode::Tabs`] well whose
/// `id == well_id` carrying `active = index`, returning `Some(new_root)`.
/// Returns `None` when no `Tabs` well carries that id (the id is absent,
/// or belongs to a [`DockNode::Leaf`] / [`DockNode::Split`]) — the caller
/// ([`DockTopology::set_active_tab`]) maps that to
/// [`TopologyError::TabsWellNotFound`]. The rebuilt well keeps its `id` +
/// `panels` (only `active` changes); every other node is cloned
/// unchanged. The new `index` is *not* range-checked here — the well's
/// own panel count is the authority and [`DockTopology::try_new`]
/// validates it, so a stale index surfaces as one `ActiveOutOfRange` from
/// the single validation gate rather than a check duplicated here.
fn set_active_in_well_rec(node: &DockNode, well_id: &str, index: usize) -> Option<DockNode> {
    match node {
        DockNode::Leaf { .. } => None,
        DockNode::Tabs { id, panels, .. } => (id.as_ref() == well_id).then(|| DockNode::Tabs {
            id: id.clone(),
            panels: panels.clone(),
            active: index,
        }),
        DockNode::Split {
            id,
            orientation,
            ratio,
            first,
            second,
        } => {
            if let Some(f) = set_active_in_well_rec(first, well_id, index) {
                return Some(DockNode::Split {
                    id: id.clone(),
                    orientation: *orientation,
                    ratio: *ratio,
                    first: Box::new(f),
                    second: second.clone(),
                });
            }
            set_active_in_well_rec(second, well_id, index).map(|s| DockNode::Split {
                id: id.clone(),
                orientation: *orientation,
                ratio: *ratio,
                first: first.clone(),
                second: Box::new(s),
            })
        }
    }
}

/// (R685.C atomic 1 §5.16) Discriminator for the unified id-namespace
/// validator — tracks whether an id was first seen as a panel or
/// a Split, so duplicate detection produces the right error
/// variant (same-kind → `DuplicatePanelId` / `DuplicateSplitId`;
/// cross-kind → `IdCollision`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NodeKind {
    Panel,
    Split,
    /// (R1083 §5.51) A [`DockNode::Tabs`] well's own `id` (distinct from
    /// the panel ids it stacks, which are [`NodeKind::Panel`]).
    Tabs,
}

/// (R685.B / R685.C §5.16) Internal recursive validator. Walks the
/// node tree depth-first pre-order, accumulating panel + split ids
/// into a unified `HashMap<String, NodeKind>` for duplicate +
/// cross-namespace collision detection. Validates every Split's
/// ratio for finiteness + bounds; rejects empty ids on first
/// encounter.
/// (R1083 §5.51) Insert a panel id into the unified id namespace,
/// emitting the right error variant on collision. Shared by the
/// [`DockNode::Leaf`] and [`DockNode::Tabs`] validation arms so a panel
/// id collides identically whether it sits in a bare leaf or a tab well.
/// The empty-id check is the caller's (it precedes this for both arms).
fn insert_panel_id(
    seen: &mut std::collections::HashMap<String, NodeKind>,
    panel_id: &str,
) -> Result<(), TopologyError> {
    match seen.insert(panel_id.to_string(), NodeKind::Panel) {
        None => Ok(()),
        Some(NodeKind::Panel) => Err(TopologyError::DuplicatePanelId(panel_id.to_string())),
        Some(NodeKind::Split | NodeKind::Tabs) => {
            Err(TopologyError::IdCollision(panel_id.to_string()))
        }
    }
}

fn validate_node(
    node: &DockNode,
    seen: &mut std::collections::HashMap<String, NodeKind>,
) -> Result<(), TopologyError> {
    match node {
        DockNode::Leaf { panel_id } => {
            if panel_id.is_empty() {
                return Err(TopologyError::EmptyId);
            }
            insert_panel_id(seen, panel_id.as_ref())
        }
        DockNode::Tabs { id, panels, active } => {
            // The well's own id shares the topology id namespace.
            if id.is_empty() {
                return Err(TopologyError::EmptyId);
            }
            match seen.insert(id.to_string(), NodeKind::Tabs) {
                None => {}
                Some(NodeKind::Tabs) => return Err(TopologyError::DuplicateTabsId(id.to_string())),
                Some(NodeKind::Panel | NodeKind::Split) => {
                    return Err(TopologyError::IdCollision(id.to_string()));
                }
            }
            // Canonical-form invariants: a well stacks ≥2 panels and the
            // visible index is in range.
            if panels.len() < 2 {
                return Err(TopologyError::TabsTooFew {
                    tabs_id: id.to_string(),
                    count: panels.len(),
                });
            }
            if *active >= panels.len() {
                return Err(TopologyError::ActiveOutOfRange {
                    tabs_id: id.to_string(),
                    active: *active,
                    count: panels.len(),
                });
            }
            // Each stacked panel id is a panel in the shared namespace —
            // collides with Leaf panel ids exactly as a duplicate would.
            for panel_id in panels {
                if panel_id.is_empty() {
                    return Err(TopologyError::EmptyId);
                }
                insert_panel_id(seen, panel_id.as_ref())?;
            }
            Ok(())
        }
        DockNode::Split {
            id,
            ratio,
            first,
            second,
            ..
        } => {
            if id.is_empty() {
                return Err(TopologyError::EmptyId);
            }
            if !ratio.is_finite() || !(0.0..=1.0).contains(ratio) {
                return Err(TopologyError::InvalidRatio {
                    split_id: id.to_string(),
                    ratio: *ratio,
                });
            }
            match seen.insert(id.to_string(), NodeKind::Split) {
                None => {}
                Some(NodeKind::Split) => {
                    return Err(TopologyError::DuplicateSplitId(id.to_string()));
                }
                Some(NodeKind::Panel | NodeKind::Tabs) => {
                    return Err(TopologyError::IdCollision(id.to_string()));
                }
            }
            validate_node(first, seen)?;
            validate_node(second, seen)?;
            Ok(())
        }
    }
}

// ─────────────────────────────────────────────────────────────────────
// R686 §5.16 §5.45 — drag-to-reorganize drop-zone geometry.
// ─────────────────────────────────────────────────────────────────────

/// (R686 §5.16 §5.45) Fraction of a panel's main-axis extent occupied
/// by each edge drop band. A cursor within `DOCK_EDGE_ZONE_FRAC` of an
/// edge (normalised to that axis) classifies as that edge's directional
/// dock zone; the remaining centre rectangle classifies as
/// [`DockDropZone::Center`].
///
/// `0.25` gives a picture-frame of edge bands one-quarter of the way in
/// from each side, leaving a centre square half the panel's extent on
/// each axis — the canonical proportion pro-tool docking overlays ship.
pub const DOCK_EDGE_ZONE_FRAC: f64 = 0.25;

/// (R686 §5.16 §5.45) Geometric classification of a drag-to-reorganize
/// cursor position over a single dock panel's rect.
///
/// ## Geometry, not gesture
///
/// This enum names **where the cursor is** over the target panel — a
/// pure spatial classification. It deliberately does **not** name what
/// dropping there *does* (swap / reparent / tab-merge); that mapping is
/// the binding's reducer + the [`DockDragOverExternal`] intent layer's
/// responsibility (R686 atomic 2+). Keeping geometry and gesture
/// semantics separate mirrors every real docking framework: the drop
/// overlay highlights a *direction*, and the host decides the topology
/// edit. Conflating the two (e.g. naming a zone `SwapLeft`) bakes one
/// host's policy into the geometry primitive.
///
/// ## Zones
///
/// A panel rect is divided into a picture-frame of four edge bands
/// (each [`DOCK_EDGE_ZONE_FRAC`] of the corresponding axis) plus a
/// centre rectangle. Corner ambiguity — where two edge bands overlap —
/// resolves to the **nearest** edge, with a fixed `Left → Right → Top →
/// Bottom` precedence on exact ties (matches the enum declaration
/// order). The directional zones mean "dock the dragged panel to this
/// side of the target, splitting the target along the perpendicular
/// axis"; the centre means "swap the dragged panel with the target".
///
/// `#[non_exhaustive]` so a future tab-merge zone (`Center` splitting
/// into `CenterTab` / `CenterSwap`) or finer corner zones can land
/// without breaking downstream `match` arms.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DockDropZone {
    /// Cursor is outside the panel rect, or the rect is degenerate
    /// (zero width or height). No drop target.
    None,
    /// Cursor is near the panel's left edge — dock the dragged panel
    /// to the left, splitting the target horizontally.
    Left,
    /// Cursor is near the panel's right edge — dock the dragged panel
    /// to the right, splitting the target horizontally.
    Right,
    /// Cursor is near the panel's top edge — dock the dragged panel
    /// above, splitting the target vertically.
    Top,
    /// Cursor is near the panel's bottom edge — dock the dragged panel
    /// below, splitting the target vertically.
    Bottom,
    /// Cursor is in the panel's centre rectangle — stack the dragged
    /// panel onto the target as a tab well ([`DockReorganizeIntent::Tabify`],
    /// R1083). (Pre-R1083 v1 had no tabs, so a centre drop swapped.)
    Center,
}

/// (R686 §5.16 §5.45) Pure classification of a cursor position over a
/// panel rect into a [`DockDropZone`]. No allocation, no `Owner`, no
/// `Scene` — a deterministic geometry helper the drag-over External
/// (R686 atomic 2) and the demo / test harness share.
///
/// `panel_rect` is the panel's paint-side rect (integer logical pixels,
/// as `scene/layout` reports). `cursor_x` / `cursor_y` are the live
/// pointer position in the same coordinate space (f64, as the
/// `InputRouter` carries them). Containment is **half-open** — the
/// right / bottom edges are exclusive — to mirror
/// [`pinion_core::scene`]'s `rect_contains`, so adjacent panels tile
/// without a one-pixel double-claim seam.
///
/// Returns [`DockDropZone::None`] for a degenerate rect (`w == 0` or
/// `h == 0`) or a cursor outside the rect.
#[must_use]
pub fn dock_drop_zone_for(panel_rect: Rect, cursor_x: f64, cursor_y: f64) -> DockDropZone {
    dock_drop_zone_for_tabbing(panel_rect, cursor_x, cursor_y, true)
}

/// (R1112 §5.51 PR-37) [`dock_drop_zone_for`] with the surface's tab-docking
/// policy explicit — `tabbing == false` (a split-only dock) suppresses the
/// centre zone so an absolute cursor over a pane's centre classifies to the
/// nearest split edge, never [`DockDropZone::Center`]. The bare
/// [`dock_drop_zone_for`] keeps the tabbing default for every existing caller;
/// the `drop` RPC threads the [`DockReorganizer::tabbing`] flag here.
#[must_use]
pub fn dock_drop_zone_for_tabbing(
    panel_rect: Rect,
    cursor_x: f64,
    cursor_y: f64,
    tabbing: bool,
) -> DockDropZone {
    // Degenerate rect carries no pixels → never a drop target.
    if panel_rect.w == 0 || panel_rect.h == 0 {
        return DockDropZone::None;
    }
    let x0 = f64::from(panel_rect.x);
    let y0 = f64::from(panel_rect.y);
    let w = f64::from(panel_rect.w);
    let h = f64::from(panel_rect.h);
    // Normalise the absolute cursor into the panel rect, then classify with
    // the shared SSOT [`dock_drop_zone_normalized_tabbing`]. A cursor outside
    // the rect normalises to a coordinate outside `[0.0, 1.0)`, which that
    // classifier rejects with [`DockDropZone::None`] — exactly the half-open
    // `rect_contains` containment this function applied inline pre-R1080.
    dock_drop_zone_normalized_tabbing((cursor_x - x0) / w, (cursor_y - y0) / h, tabbing)
}

/// (R1080 §5.51) Classify a cursor already normalised over a panel rect
/// (`x_rel` / `y_rel` in `[0.0, 1.0)`, left / top = `0.0`) into a
/// [`DockDropZone`] — the SSOT zone geometry shared by
/// [`dock_drop_zone_for`] (which normalises an absolute cursor first) and the
/// §5.51 R742 pointer drag coordinator (which receives a pre-normalised
/// [`DropPoint`](pinion_core::external::DropPoint) over the drop-target
/// panel). One classifier, two callers — the edge-band fraction
/// ([`DOCK_EDGE_ZONE_FRAC`]) and the Left → Right → Top → Bottom tie order
/// cannot drift between the absolute and pointer-normalised paths.
///
/// Containment is **half-open**: a coordinate `< 0.0` or `>= 1.0` on either
/// axis is outside the panel and yields [`DockDropZone::None`], mirroring
/// [`dock_drop_zone_for`]'s `rect_contains` semantics so adjacent panels tile
/// without a one-pixel double-claim seam.
#[must_use]
pub fn dock_drop_zone_normalized(x_rel: f64, y_rel: f64) -> DockDropZone {
    dock_drop_zone_normalized_tabbing(x_rel, y_rel, true)
}

/// (R1111 §5.51 PR-37) [`dock_drop_zone_normalized`] with the tab-docking
/// policy explicit. `tabbing == true` (the default classification) keeps the
/// centre square as [`DockDropZone::Center`] (→ [`DockReorganizeIntent::Tabify`]);
/// `tabbing == false` (a split-only consumer, e.g. a terminal multiplexer)
/// suppresses it — the centre falls through to the nearest split edge, so the
/// whole panel resolves to an edge and a centre drop can never tabify (no dead
/// centre). [`DockPanelExternal::with_tabbing`] wires the consumer's choice
/// into the pointer path; the bare [`dock_drop_zone_normalized`] keeps the
/// tabbing default for every existing caller.
fn dock_drop_zone_normalized_tabbing(x_rel: f64, y_rel: f64, tabbing: bool) -> DockDropZone {
    // Half-open [0.0, 1.0): outside the panel on either axis → no zone.
    if !(0.0..1.0).contains(&x_rel) || !(0.0..1.0).contains(&y_rel) {
        return DockDropZone::None;
    }
    let from_left = x_rel;
    let from_right = 1.0 - from_left;
    let from_top = y_rel;
    let from_bottom = 1.0 - from_top;
    // Centre rectangle: at least one band-width clear of every edge. Only a
    // tab-docking panel claims it; a split-only panel lets the nearest edge win.
    let nearest = from_left.min(from_right).min(from_top).min(from_bottom);
    if tabbing && nearest >= DOCK_EDGE_ZONE_FRAC {
        return DockDropZone::Center;
    }
    // Edge band: the nearest edge wins; exact ties resolve in
    // Left → Right → Top → Bottom declaration order.
    if from_left <= from_right && from_left <= from_top && from_left <= from_bottom {
        DockDropZone::Left
    } else if from_right <= from_top && from_right <= from_bottom {
        DockDropZone::Right
    } else if from_top <= from_bottom {
        DockDropZone::Top
    } else {
        DockDropZone::Bottom
    }
}

// ─────────────────────────────────────────────────────────────────────
// R686 §5.16 §5.45 — drag-to-reorganize resolution + apply.
// ─────────────────────────────────────────────────────────────────────

/// (R686 §5.16 §5.45) Default split ratio a drag-to-reorganize
/// [`DockReorganizeIntent::SplitInsert`] seeds when it creates a new
/// divider — an even 50/50 split. The user drags the resulting
/// splitter afterward to rebalance.
pub const DEFAULT_REORGANIZE_RATIO: f32 = 0.5;

/// (R686 §5.16 §5.45) Prefix for the stable split ids the
/// [`DockReorganizeExternal`] mints when a `SplitInsert` gesture
/// creates a new divider (`reorg-split-{seq}`). Distinct from any
/// binding-declared split id so a generated split never collides with
/// the boot topology's ids.
pub const REORG_SPLIT_ID_PREFIX: &str = "reorg-split-";

/// (R1083 §5.51) Prefix for the stable tab-well ids the
/// [`DockReorganizeExternal`] mints when a [`DockReorganizeIntent::Tabify`]
/// gesture creates a **new** well (`reorg-tabs-{seq}`). Distinct from any
/// binding-declared well id + the [`REORG_SPLIT_ID_PREFIX`] split ids so a
/// generated well id never collides. A tabify that joins an *existing*
/// well leaves the minted id unused (a harmless gap in the sequence).
pub const REORG_TABS_ID_PREFIX: &str = "reorg-tabs-";

/// (R686 §5.16 §5.45) Map an edge [`DockDropZone`] to the
/// `(orientation, position)` a [`DockTopology::split_leaf_into`] needs:
/// a left/right drop splits the target **horizontally**, a top/bottom
/// drop splits it **vertically**; left/top place the dragged panel in
/// the `first` slot, right/bottom in `second`. Returns `None` for the
/// non-edge zones ([`DockDropZone::Center`] / [`DockDropZone::None`]),
/// which are not split gestures.
fn zone_split_geometry(zone: DockDropZone) -> Option<(SplitterOrientation, DockSplitPosition)> {
    match zone {
        DockDropZone::Left => Some((SplitterOrientation::Horizontal, DockSplitPosition::First)),
        DockDropZone::Right => Some((SplitterOrientation::Horizontal, DockSplitPosition::Second)),
        DockDropZone::Top => Some((SplitterOrientation::Vertical, DockSplitPosition::First)),
        DockDropZone::Bottom => Some((SplitterOrientation::Vertical, DockSplitPosition::Second)),
        DockDropZone::Center | DockDropZone::None => None,
    }
}

/// (R1081 §5.51) Map a classified drop — the dragged `source` panel, the
/// `target` panel under the cursor, and the [`DockDropZone`] the cursor
/// fell in — to the [`DockReorganizeIntent`] that performs it: a centre
/// drop swaps, an edge drop splits the target along the perpendicular
/// axis with the source in the near slot. Returns `None` for the
/// non-actionable [`DockDropZone::None`] (and, defensively, any future
/// zone [`zone_split_geometry`] declines to map).
///
/// The single source of truth for the zone → intent mapping, shared by
/// the cursor-driven [`resolve_dock_drop`], the symbolic `reorganize`
/// invoke, and the §5.51 R742 pointer drag-release coordinator
/// ([`DockPanelExternal::drag_release`]) so the three paths cannot drift.
fn intent_for_zone(source: &str, target: &str, zone: DockDropZone) -> Option<DockReorganizeIntent> {
    match zone {
        DockDropZone::None => None,
        // (R1083 §5.51) A centre drop now stacks the dragged panel onto the
        // target as a tab well (the tabbed-docking gesture), superseding the
        // tab-less v1 swap. `DockReorganizeIntent::Swap` remains a valid
        // public mutation (an explicit AI `reorganize`/test can request it)
        // but no zone produces it.
        DockDropZone::Center => Some(DockReorganizeIntent::Tabify {
            source: source.to_string(),
            target: target.to_string(),
        }),
        edge => zone_split_geometry(edge).map(|(orientation, position)| {
            DockReorganizeIntent::SplitInsert {
                source: source.to_string(),
                target: target.to_string(),
                orientation,
                position,
            }
        }),
    }
}

/// (R686 §5.16 §5.45) Parse the wire string a reorganize gesture
/// payload carries (`"Center"` / `"Left"` / `"Right"` / `"Top"` /
/// `"Bottom"`) into a [`DockDropZone`]. `"None"` and any unrecognised
/// string return `None` (the [`DockReorganizeExternal`] rejects the
/// invoke). The strings match the [`DockDropZone`] variant names so AI
/// clients can echo a zone they classified locally.
fn parse_drop_zone(s: &str) -> Option<DockDropZone> {
    match s {
        "Center" => Some(DockDropZone::Center),
        "Left" => Some(DockDropZone::Left),
        "Right" => Some(DockDropZone::Right),
        "Top" => Some(DockDropZone::Top),
        "Bottom" => Some(DockDropZone::Bottom),
        _ => None,
    }
}

/// (R1081 §5.51) Inverse of [`parse_drop_zone`]: the wire string a
/// [`DockDropZone`] serialises to in the `DockPanelExternal`
/// `drop_preview` introspection (and that the `reorganize` invoke
/// echoes). The variant names round-trip through [`parse_drop_zone`].
fn zone_wire_name(zone: DockDropZone) -> &'static str {
    match zone {
        DockDropZone::None => "None",
        DockDropZone::Left => "Left",
        DockDropZone::Right => "Right",
        DockDropZone::Top => "Top",
        DockDropZone::Bottom => "Bottom",
        DockDropZone::Center => "Center",
    }
}

/// (R687 §5.16 §5.45) Parse a `{"x","y","w","h"}` JSON object (as
/// `scene/layout` emits each node's integer rect) into a [`Rect`].
/// Returns `None` if any field is missing or not a non-negative
/// integer in `u32` range — the [`DockReorganizeExternal`] `drop`
/// action surfaces that as [`InvokeError::TypeMismatch`].
fn parse_json_rect(v: &serde_json::Value) -> Option<Rect> {
    let field = |k: &str| -> Option<u32> { u32::try_from(v.get(k)?.as_u64()?).ok() };
    Some(Rect::new(
        field("x")?,
        field("y")?,
        field("w")?,
        field("h")?,
    ))
}

/// (R686 §5.16 §5.45) A resolved drag-to-reorganize gesture — the
/// topology edit a panel-drag drop produces, fully decided (no
/// geometry / zone ambiguity left). Built by [`resolve_dock_drop`]
/// from a cursor position over a layout, or by the
/// [`DockReorganizeExternal`] from an AI client's invoke payload.
///
/// The three drop outcomes:
/// * `Tabify` — the cursor landed in a panel's **centre**; the dragged
///   panel stacks onto the target as a tab well ([`DockTopology::tabify`]).
///   (R1083) This is what a centre drop produces; [`intent_for_zone`] is
///   the single source of that mapping.
/// * `SplitInsert` — the cursor landed near a panel's **edge**; the
///   dragged panel docks to that side, splitting the target. The
///   `orientation` + `position` are pre-resolved from the edge zone
///   (so the intent carries no invalid-zone state), and applying it
///   moves the dragged panel via [`DockTopology::remove_leaf`] then
///   re-inserts it via [`DockTopology::split_leaf_into`].
/// * `Swap` — trade the two panels' places ([`DockTopology::swap_leaves`]).
///   A valid public mutation, but **no drop zone produces it** (pre-R1083
///   a centre drop swapped; R1083 made centre `Tabify`). Reachable only by
///   constructing it directly (tests / a future explicit swap gesture).
///
/// `#[non_exhaustive]` so a further outcome can land without breaking
/// downstream `match` arms.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DockReorganizeIntent {
    /// Swap the dragged `source` panel with the `target` panel. A valid
    /// mutation that no drop zone emits (centre now [`Self::Tabify`]).
    Swap {
        /// Panel being dragged.
        source: String,
        /// Panel dropped onto.
        target: String,
    },
    /// Dock the dragged `source` panel beside the `target`, splitting
    /// the target along `orientation` with the source in `position`.
    SplitInsert {
        /// Panel being dragged (moves: removed from its old slot, then
        /// re-inserted beside the target).
        source: String,
        /// Panel whose slot becomes a split.
        target: String,
        /// Split axis derived from the drop edge.
        orientation: SplitterOrientation,
        /// Which slot of the new split the dragged panel occupies.
        position: DockSplitPosition,
    },
    /// (R1083 §5.51) Stack the dragged `source` panel onto the `target`
    /// as a tab well (centre drop) — [`DockTopology::tabify`].
    Tabify {
        /// Panel being dragged (moves: removed from its old slot, then
        /// stacked onto the target's pane).
        source: String,
        /// Panel the source stacks onto (becomes / joins a tab well).
        target: String,
    },
}

impl DockReorganizeIntent {
    /// The panel being dragged.
    #[must_use]
    pub fn source(&self) -> &str {
        match self {
            Self::Swap { source, .. }
            | Self::SplitInsert { source, .. }
            | Self::Tabify { source, .. } => source,
        }
    }

    /// The panel dropped onto.
    #[must_use]
    pub fn target(&self) -> &str {
        match self {
            Self::Swap { target, .. }
            | Self::SplitInsert { target, .. }
            | Self::Tabify { target, .. } => target,
        }
    }

    /// (R686 §5.16 §5.45) Apply this gesture to `topology`, producing a
    /// new validated topology. `new_id` is the stable id any minting
    /// gesture needs — the divider id a `SplitInsert` creates, or the
    /// fresh tab-well id a `Tabify` mints (ignored by `Swap`, and by a
    /// `Tabify` that joins an existing well). `ratio` is the initial
    /// split fraction (typically [`DEFAULT_REORGANIZE_RATIO`]; used only
    /// by `SplitInsert`).
    ///
    /// `SplitInsert` + `Tabify` are **moves**: the source panel is
    /// removed from its current slot (collapsing its old parent split /
    /// shrinking its old well) and re-placed beside / onto the target.
    /// Composing the mutation primitives this way keeps the panel count
    /// invariant (one panel relocated, none created or destroyed).
    ///
    /// # Errors
    ///
    /// Propagates the underlying mutation primitives' errors:
    /// [`TopologyError::PanelNotFound`] if `source` or `target` names
    /// no panel, [`TopologyError::RootRemoval`] if `source` is the sole
    /// panel, or a duplicate/collision error if `new_id` clashes
    /// with an existing id.
    pub fn apply(
        &self,
        topology: &DockTopology,
        new_id: impl Into<Cow<'static, str>>,
        ratio: f32,
    ) -> Result<DockTopology, TopologyError> {
        match self {
            Self::Swap { source, target } => topology.swap_leaves(source, target),
            Self::SplitInsert {
                source,
                target,
                orientation,
                position,
            } => topology.remove_leaf(source)?.split_leaf_into(
                target,
                source.clone(),
                new_id,
                *orientation,
                ratio,
                *position,
            ),
            Self::Tabify { source, target } => topology.tabify(source, target, new_id),
        }
    }
}

/// (R686 §5.16 §5.45) Resolve a drag-to-reorganize drop into a
/// [`DockReorganizeIntent`], or `None` if the cursor is over no valid
/// target.
///
/// `panel_rects` is the live layout — each `(panel_id, rect)` pair the
/// caller read from `scene/layout` (the AI-native primary path) or the
/// shell's last paint layout. The cursor `(cursor_x, cursor_y)` is in
/// the same coordinate space. The dragged `source_panel_id` is skipped
/// (you cannot drop a panel onto itself), and the first remaining panel
/// whose rect contains the cursor decides the gesture: a centre hit
/// produces [`DockReorganizeIntent::Tabify`] (pre-R1083 it was `Swap`),
/// an edge hit a [`DockReorganizeIntent::SplitInsert`] with the edge
/// mapped to a split orientation + position.
///
/// Returns `None` when the cursor is outside every non-source panel, or
/// over the source itself.
#[must_use]
pub fn resolve_dock_drop(
    panel_rects: &[(&str, Rect)],
    source_panel_id: &str,
    cursor_x: f64,
    cursor_y: f64,
) -> Option<DockReorganizeIntent> {
    resolve_dock_drop_tabbing(panel_rects, source_panel_id, cursor_x, cursor_y, true)
}

/// (R1112 §5.51 PR-37) [`resolve_dock_drop`] with the dock surface's
/// tab-docking policy explicit. `tabbing == false` (a split-only surface)
/// suppresses the centre `Tabify` on the `drop` RPC path too — a centre cursor
/// resolves to the nearest split edge — so the RPC drive and the pointer drive
/// honour ONE surface policy ([`DockReorganizer::tabbing`]). The bare
/// [`resolve_dock_drop`] keeps the tabbing default for existing callers.
#[must_use]
pub fn resolve_dock_drop_tabbing(
    panel_rects: &[(&str, Rect)],
    source_panel_id: &str,
    cursor_x: f64,
    cursor_y: f64,
    tabbing: bool,
) -> Option<DockReorganizeIntent> {
    for (panel_id, rect) in panel_rects {
        if *panel_id == source_panel_id {
            continue;
        }
        // Classify the cursor over this panel and map the zone to its
        // intent through the [`intent_for_zone`] SSOT (shared with the
        // symbolic `reorganize` invoke + the R742 pointer coordinator).
        // `DockDropZone::None` yields `None` → keep scanning the next
        // panel, exactly as the pre-R1081 per-arm `match` did.
        let zone = dock_drop_zone_for_tabbing(*rect, cursor_x, cursor_y, tabbing);
        if let Some(intent) = intent_for_zone(source_panel_id, panel_id, zone) {
            return Some(intent);
        }
    }
    None
}

/// (R1081 §5.51) The dock reorganize **coordinator** — the reorganize-
/// commit machine, extracted from [`DockReorganizeExternal`] so the two
/// drives that reorganize a dock share ONE counter + topology + undo:
///
/// * the **symbolic / RPC** drive ([`DockReorganizeExternal`]'s
///   `invoke("drop" | "reorganize")`), and
/// * the **pointer** drive (the §5.51 R742
///   [`DockPanelExternal::drag_release`] mouse gesture).
///
/// Shared as an `Rc<DockReorganizer>`: the editor binding builds one and
/// hands clones to the invoke external + every panel external, so a
/// pointer drop and an AI invoke mint split ids from the same
/// `split_seq` (no `reorg-split-{n}` collision) and push onto the same
/// undo stack (one workspace history). Extracting the machine *with* its
/// second consumer — not before it existed — is the abstraction-needs-a-
/// second-consumer discipline: the `Rc` shape is precisely what the
/// pointer coordinator needs, so it lands when that coordinator does.
///
/// ## State
///
/// Holds a shared `Rc<Signal<Option<DockTopology>>>` — the live dock
/// surface the editor's view fn reads. `None` is the **empty dock** (no
/// docked panels — every pane torn off / floating), a first-class state
/// of tiling / terminal-multiplexer hosts. A successful reorganize calls
/// `Signal::set` with `Some(mutated)` (or records a reversible edit when
/// an undo stack is attached), and the view fn's reactive subscription
/// re-renders the new layout.
///
/// ## Total over the empty surface (R1084 §5.51)
///
/// The coordinator is total over `Option<DockTopology>`: a reorganize on
/// `None` is the **identity no-op** — an empty surface has no source to
/// drag and no target to drop onto, so "nothing changes" is the honest
/// result over the whole input type, not defensive code. The reorganize
/// gestures all *preserve* the panel count (`Swap` trivially; both
/// `SplitInsert` and `Tabify` are moves), so the coordinator never produces
/// `Some → None`: `None` arrives only as input (the empty surface a binding
/// hands in), and even then a pointer drag cannot originate on it — only the
/// `invoke` path can reach an empty-surface reorganize, and it gets the no-op.
/// [`DockTopology`]'s `leaf >= 1` invariant is therefore untouched —
/// absence is modelled by the `Option`, not by a degenerate topology.
/// (R1134 §5.51.1) Per-surface torn-slot policy — what happens to a panel's dock
/// LEAF when it floats out, the user-selectable "collapse vs placeholder" the live
/// dock review asked for.
///
/// * [`Placeholder`](Self::Placeholder) (default, bit-identical to pre-R1134) —
///   the leaf STAYS in the topology when the panel floats; the binding paints a
///   placeholder in the preserved slot, so the neighbours do NOT reflow and a
///   dock-back is a trivial slot-fill.
/// * [`Collapse`](Self::Collapse) — the leaf is REMOVED on float
///   ([`DockReorganizer::float_out_panel`]) so the siblings reclaim the space (the
///   VS Code / Blender model: the slot vanishes, neighbours grow). The home anchor
///   is captured first so a dock-back ([`DockReorganizer::restore_panel_home`])
///   restores the leaf next to its original sibling.
///
/// Owned by the coordinator — like [`tabbing`](DockReorganizer::tabbing) — so the
/// policy applies UNIFORMLY to every float path that drives through the shared
/// [`DockReorganizer`]: the pointer `drag_release` escape, the AI
/// `invoke("tear_off")`, and a `drag_cancel`. The timing is **release-as-floated**:
/// a collapse fires only when the gesture SETTLES as floated (release / invoke),
/// never mid-drag — removing the leaf mid-gesture would re-run the external factory
/// and disturb the live drag, so the slot stays put during the drag and collapses
/// once on release.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum FloatPolicy {
    /// Keep the leaf on float (slot preserved, no reflow) — the default.
    #[default]
    Placeholder,
    /// Remove the leaf on float (neighbours reclaim the space), restoring it to
    /// the captured home anchor on dock-back.
    Collapse,
}

impl FloatPolicy {
    /// The scene-as-data name (`"placeholder"` / `"collapse"`) — the value
    /// `query("float_policy")` returns and `invoke("set_float_policy", …)` accepts,
    /// so an AI client discovers + drives the policy over the §2 #7 wire.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            FloatPolicy::Placeholder => "placeholder",
            FloatPolicy::Collapse => "collapse",
        }
    }

    /// Parse a wire name back to a policy — the inverse of [`Self::as_str`]. An
    /// unknown name is `None` (the `set_float_policy` invoke arm rejects it). Named
    /// `from_wire` (not `from_str`) so it does not shadow the `FromStr` trait — the
    /// `Option` return is the discriminated-set semantics, not a `Result` parse.
    #[must_use]
    pub fn from_wire(name: &str) -> Option<Self> {
        match name {
            "placeholder" => Some(FloatPolicy::Placeholder),
            "collapse" => Some(FloatPolicy::Collapse),
            _ => None,
        }
    }
}

pub struct DockReorganizer {
    /// Live dock surface the editor view fn reads — `None` = empty dock.
    /// Mutated via `Signal::set(Some(..))` on a successful reorganize →
    /// reactive re-render.
    topology: Rc<Signal<Option<DockTopology>>>,
    /// Monotonic counter feeding the stable id of each generated
    /// split (`reorg-split-{seq}`). Bumped only when a `SplitInsert`
    /// actually lands, so ids stay gap-minimal + collision-free.
    split_seq: Cell<u64>,
    /// (R1083 §5.51) Monotonic counter feeding the stable id of each
    /// generated tab well (`reorg-tabs-{seq}`). Bumped on every applied
    /// [`DockReorganizeIntent::Tabify`]; a tabify that joins an existing
    /// well leaves the minted id unused (a harmless sequence gap).
    tabs_seq: Cell<u64>,
    /// Initial ratio each generated split seeds (even split by default).
    reorganize_ratio: f32,
    /// (R1112 §5.51 §2 #7 PR-37) Whether a centre drop tabifies on THIS dock
    /// surface (default `true` = the IDE / editor affordance; `false` = a
    /// split-only surface such as a terminal multiplexer, where stacking two
    /// panes as tabs is meaningless). Owned by the coordinator — NOT per-panel
    /// — because the policy is only meaningful when a reorganize can happen (a
    /// reorganizer exists) and must apply UNIFORMLY to every classifier
    /// consumer of this surface: the pointer path
    /// ([`DockPanelExternal::resolve_preview`]), the `drop` RPC
    /// ([`resolve_dock_drop`]), and cross-window redock
    /// ([`DockReorganizer::dock_panel_at_zone`]). A split-only surface never offers
    /// `Center` from any path — a centre cursor resolves to the nearest split
    /// edge. The explicit symbolic `reorganize` invoke (an AI naming a zone,
    /// no classification) is NOT gated — tabbing governs the zone CLASSIFIER,
    /// not direct topology edits.
    tabbing: bool,
    /// (R1134 §5.51.1) The torn-slot policy ([`FloatPolicy`]) for THIS dock
    /// surface — `Placeholder` (default) keeps a floated panel's leaf, `Collapse`
    /// removes it (neighbours reflow) and remembers the home anchor. Owned by the
    /// coordinator like [`tabbing`] so a collapse is uniform across the pointer +
    /// invoke float paths. A [`Signal`] (R1135) — not a bare `Cell` — so a binding's
    /// view fn that reads [`Self::float_policy`] (e.g. a "collapse vs placeholder"
    /// toolbar toggle's label) auto-subscribes and REPAINTS when the policy flips,
    /// whether the flip came from the GUI toggle or the `set_float_policy` invoke
    /// (one reactive SSOT, both paths consistent).
    ///
    /// [`tabbing`]: Self::tabbing
    float_policy: Signal<FloatPolicy>,
    /// (R1134 §5.51.1) Captured home anchors for collapsed panels, keyed by panel
    /// id — the collapse-policy "where home is" SSOT. [`Self::float_out_panel`]
    /// stashes a leaf's [`DockLeafAnchor`] before removing it;
    /// [`Self::restore_panel_home`] pops + re-inserts at it; a zone redock
    /// ([`Self::dock_panel_at_zone`]) clears it (the panel landed somewhere new).
    /// A root / `Tabs`-member panel has no parent-Split anchor, so its entry is
    /// absent and dock-back falls back to a zone redock.
    home_anchors: RefCell<HashMap<String, DockLeafAnchor>>,
    /// Last gesture outcome, surfaced via `query("last_outcome")` for
    /// AI clients to confirm an apply succeeded / why it was rejected.
    last_outcome: RefCell<Option<String>>,
    /// (R749 §5.52) When attached via [`with_undo`](Self::with_undo) each
    /// applied reorganize is recorded as a reversible
    /// [`SignalEdit<Option<DockTopology>>`] onto this stack (the **third**
    /// [`UndoCommand`](pinion_core::undo::UndoCommand) consumer — editor
    /// workspace history), instead of mutating the topology signal
    /// directly. `None` = the R686 direct-mutate behavior.
    undo: Option<Rc<UndoStack>>,
}

/// (R686 §5.16 §5.45) AI-native drag-to-reorganize handle — the
/// [`External`] a dock editor registers (via
/// [`WidgetCore::create_extra_externals`](pinion_core::WidgetCore::create_extra_externals))
/// to apply topology edits through the
/// [`scene/invoke`](pinion_core::external::ExternalIntrospect::invoke)
/// channel.
///
/// ## The invoke drive over a shared coordinator
///
/// This external is the **symbolic / RPC** drive of dock reorganize: an
/// AI client reading `scene/layout` classifies a drop with
/// [`resolve_dock_drop`] / [`dock_drop_zone_for`] and applies it through
/// `invoke("drop" | "reorganize")` — the §2 #2 RPC-as-primary-path
/// contract. R1081 §5.51 added the **pointer** drive
/// ([`DockPanelExternal`]'s R742 mouse gesture); both share one
/// [`Rc<DockReorganizer>`] so a pointer drop and an AI invoke mint split
/// ids from one counter and push one undo history. The external is a
/// thin [`ExternalIntrospect`] facade over that coordinator — it still
/// exposes the live topology as queryable JSON (`query("topology")`) for
/// §2 #7 scene-as-data introspection.
pub struct DockReorganizeExternal {
    /// The shared reorganize-commit machine. `Rc` so the editor binding
    /// hands the *same* coordinator to the R742 panel externals
    /// ([`reorganizer`](Self::reorganizer)).
    reorganizer: Rc<DockReorganizer>,
    /// (R1082.1 §5.51) A clone of the ONE shared live drop-preview the
    /// R742 panels write — so the canonical reorganize surface an AI
    /// client talks to can `query("drop_preview")` and OBSERVE an
    /// in-flight pointer drag (not only the committed `last_outcome`).
    /// `None` for an invoke-only editor with no pointer panels.
    drop_preview: Option<Rc<Signal<Option<DockDropPreview>>>>,
}

impl core::fmt::Debug for DockReorganizer {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DockReorganizer")
            .field("split_seq", &self.split_seq.get())
            .field("reorganize_ratio", &self.reorganize_ratio)
            .field("tabbing", &self.tabbing)
            .field("float_policy", &self.float_policy.get())
            .field("last_outcome", &self.last_outcome.borrow())
            .finish_non_exhaustive()
    }
}

impl DockReorganizer {
    /// Construct a reorganize coordinator over a shared dock-surface
    /// signal. The binding creates the `Rc<Signal<Option<DockTopology>>>`
    /// (via `Owner::cache`) and hands a clone here so the coordinator +
    /// the view fn share one source of truth. `None` = the empty dock; a
    /// reorganize on it is the identity no-op (see the type docs).
    #[must_use]
    pub fn new(topology: Rc<Signal<Option<DockTopology>>>) -> Self {
        Self {
            topology,
            split_seq: Cell::new(0),
            tabs_seq: Cell::new(0),
            reorganize_ratio: DEFAULT_REORGANIZE_RATIO,
            tabbing: true,
            float_policy: Signal::new(FloatPolicy::default()),
            home_anchors: RefCell::new(HashMap::new()),
            last_outcome: RefCell::new(None),
            undo: None,
        }
    }

    /// (R749 §5.52) Record every applied reorganize onto `stack` as a
    /// reversible [`SignalEdit<Option<DockTopology>>`], so `invoke "undo"` /
    /// `"redo"` on the stack step the whole layout back and forth — the
    /// editor's workspace history (Phase D seed). Without this the
    /// coordinator mutates the topology signal directly (the R686 behavior).
    #[must_use]
    pub fn with_undo(mut self, stack: Rc<UndoStack>) -> Self {
        self.undo = Some(stack);
        self
    }

    /// (R1112 §5.51 PR-37) Opt out of tab docking for this dock surface
    /// (default on). A split-only consumer — a terminal multiplexer — passes
    /// `false`, so a centre drop resolves to the nearest split edge instead of
    /// a tabify, on EVERY path (pointer + `drop` RPC + cross-window redock).
    /// See [`tabbing`](Self::tabbing) for the layering rationale.
    #[must_use]
    pub fn with_tabbing(mut self, tabbing: bool) -> Self {
        self.tabbing = tabbing;
        self
    }

    /// (R1112 §5.51 §2 #7 PR-37) Whether a centre drop tabifies — the dock
    /// surface policy every zone-classifier consumer reads (pointer preview,
    /// `drop` RPC, cross-window redock), so a split-only surface is uniform.
    #[must_use]
    pub fn tabbing(&self) -> bool {
        self.tabbing
    }

    /// (R1134 §5.51.1) Set this dock surface's torn-slot [`FloatPolicy`] at
    /// construction — `Collapse` makes a float remove the leaf (neighbours reflow),
    /// `Placeholder` (the default) keeps it. Like [`with_tabbing`](Self::with_tabbing)
    /// it is a per-surface policy the consumer picks; runtime toggling goes through
    /// [`set_float_policy`](Self::set_float_policy).
    #[must_use]
    pub fn with_float_policy(self, policy: FloatPolicy) -> Self {
        self.float_policy.set(policy);
        self
    }

    /// (R1134 §5.51.1) Switch the torn-slot policy at runtime — the `set_float_policy`
    /// invoke + a "collapse vs placeholder" UI toggle drive this, so a dock review
    /// flips the behaviour live without rebuilding the coordinator. Takes effect on
    /// the NEXT float (an already-floated panel is unaffected until it docks back).
    pub fn set_float_policy(&self, policy: FloatPolicy) {
        self.float_policy.set(policy);
    }

    /// (R1134 §5.51.1 §2 #7) This surface's live torn-slot policy — the value
    /// `query("float_policy")` projects + the float paths read to decide collapse
    /// vs placeholder.
    #[must_use]
    pub fn float_policy(&self) -> FloatPolicy {
        self.float_policy.get()
    }

    /// Diagnostic: how many splits this coordinator has minted so far.
    #[must_use]
    pub fn split_seq(&self) -> u64 {
        self.split_seq.get()
    }

    /// (R1083 §5.51) Diagnostic: how many tab-well ids this coordinator
    /// has minted (one per applied [`DockReorganizeIntent::Tabify`]).
    #[must_use]
    pub fn tabs_seq(&self) -> u64 {
        self.tabs_seq.get()
    }

    /// Read the last gesture outcome summary (`"<source> -> <target>"` on
    /// a successful edit, `"rejected: …"` / `"no drop target"` otherwise),
    /// for `query("last_outcome")` and the pointer coordinator's
    /// drop confirmation. `None` before any gesture.
    #[must_use]
    pub fn last_outcome(&self) -> Option<String> {
        self.last_outcome.borrow().clone()
    }

    /// Clone the shared dock-surface signal this coordinator mutates, so a
    /// panel external sharing this `Rc` can read the live topology JSON
    /// for introspection through one handle. `None` = empty dock.
    #[must_use]
    pub fn topology(&self) -> Rc<Signal<Option<DockTopology>>> {
        Rc::clone(&self.topology)
    }

    /// Record a non-applying outcome string (e.g. `"no drop target"` when
    /// a pointer drop landed over empty space) so `query("last_outcome")`
    /// reflects the gesture even when no topology edit happened.
    pub fn note_outcome(&self, outcome: impl Into<String>) {
        *self.last_outcome.borrow_mut() = Some(outcome.into());
    }

    /// Apply a resolved [`DockReorganizeIntent`] to the live topology,
    /// returning a human-readable outcome summary on success. Shared by
    /// the `invoke("reorganize" | "drop", …)` wire and the R742 pointer
    /// [`DockPanelExternal::drag_release`] coordinator.
    ///
    /// (R1084 §5.51) When the surface is empty (`None`) the call is the
    /// identity no-op — `Ok("empty surface — no-op")`, no panic, no
    /// counter bump, the signal stays `None`. See the type docs for why
    /// this is the total definition rather than defensive code.
    ///
    /// # Errors
    ///
    /// Returns the [`TopologyError`] from the underlying mutation when
    /// the gesture cannot apply (stale panel id, root removal, id
    /// collision). The live topology is left unchanged on error.
    pub fn apply_intent(&self, intent: &DockReorganizeIntent) -> Result<String, TopologyError> {
        let Some(current) = self.topology.get() else {
            // (R1084 §5.51) Empty dock surface: no panel to drag, none to
            // drop onto — the identity no-op over the whole input type.
            let outcome = "empty surface — no-op".to_string();
            *self.last_outcome.borrow_mut() = Some(outcome.clone());
            return Ok(outcome);
        };
        // (R1083 §5.51) Mint the stable id the gesture needs — a tab-well id
        // for `Tabify`, a split id otherwise. `Swap` ignores it.
        let new_id = match intent {
            DockReorganizeIntent::Tabify { .. } => {
                format!("{REORG_TABS_ID_PREFIX}{}", self.tabs_seq.get())
            }
            _ => format!("{REORG_SPLIT_ID_PREFIX}{}", self.split_seq.get()),
        };
        let next = match intent.apply(&current, new_id, self.reorganize_ratio) {
            Ok(next) => next,
            Err(e) => {
                // Record the rejection here so EVERY drive (the AI invoke
                // path + the R742 pointer drag_release) surfaces
                // `"rejected: …"` through `query("last_outcome")` — one
                // outcome SSOT, no per-caller bookkeeping (the pointer path
                // drops the `Result`, relying on this record).
                *self.last_outcome.borrow_mut() = Some(format!("rejected: {e}"));
                return Err(e);
            }
        };
        // Bump only the counter the applied gesture drew from, so each id
        // sequence stays collision-free.
        match intent {
            DockReorganizeIntent::SplitInsert { .. } => {
                self.split_seq.set(self.split_seq.get() + 1);
            }
            DockReorganizeIntent::Tabify { .. } => {
                self.tabs_seq.set(self.tabs_seq.get() + 1);
            }
            DockReorganizeIntent::Swap { .. } => {}
        }
        let summary = format!("{} -> {}", intent.source(), intent.target());
        Ok(self.commit(next, summary))
    }

    /// (R1126/R1128 §5.51 §2 #7 PR-33) Return a floating `panel` into the dock at the
    /// dropped zone — TOTAL over the panel's presence, so ONE redock path serves
    /// BOTH torn-slot policies:
    ///
    /// * **Placeholder policy** — the panel is still a leaf (its slot was kept as
    ///   a placeholder on tear-off): this MOVES it to the zone (remove-then-insert).
    /// * **Collapse policy** — the leaf was removed on tear-off ([`Self::float_out_panel`]),
    ///   so the panel is absent: this INSERTS a fresh leaf at the zone.
    ///
    /// The consumer picks the policy (by whether it floated the leaf out); this
    /// stays uniform. Normalises to absence (remove-if-present) then inserts fresh
    /// via [`DockTopology::split_leaf_into`] (edge) / [`DockTopology::tabify_fresh`]
    /// (centre), classified by the same [`dock_drop_zone_normalized`] SSOT the
    /// same-window drag uses. `panel == target` (a drop on the panel's own slot) is
    /// a home no-op — a panel cannot split against itself. A dead zone / empty
    /// surface is a no-op. The SINGLE redock SSOT (R1128 retired the
    /// placeholder-only `apply_zone_redock`, which rejected an absent source).
    ///
    /// # Errors
    ///
    /// Propagates the [`TopologyError`] from the underlying remove / insert when it
    /// cannot apply (id collision, stale target); the live topology is unchanged.
    ///
    /// # Panics
    ///
    /// Never in practice: the edge branch is reached only for a non-`None`,
    /// non-`Center` zone, and every such zone has [`zone_split_geometry`]; the
    /// `expect` documents that invariant (mirrors [`Self::tabify`]'s panic note).
    pub fn dock_panel_at_zone(
        &self,
        panel: &str,
        target: &str,
        x_rel: f64,
        y_rel: f64,
    ) -> Result<String, TopologyError> {
        let Some(current) = self.topology.get() else {
            let outcome = "empty surface — no-op".to_string();
            self.note_outcome(&outcome);
            return Ok(outcome);
        };
        if panel == target {
            let outcome = format!("{panel}: home redock (own slot) — no move");
            self.note_outcome(&outcome);
            return Ok(outcome);
        }
        let zone = dock_drop_zone_normalized_tabbing(x_rel, y_rel, self.tabbing);
        if zone == DockDropZone::None {
            let outcome = format!("{panel} -> {target}: no actionable zone — no move");
            self.note_outcome(&outcome);
            return Ok(outcome);
        }
        // Normalise to absence: a present panel (placeholder policy) is removed so
        // the insert below is uniform; an absent panel (collapse policy) inserts
        // directly. `remove_leaf` cannot fail here — `panel` is present + `target`
        // (!= panel) survives, so the tree never empties.
        let base = if current.panel_ids().contains(&panel) {
            current.remove_leaf(panel)?
        } else {
            current
        };
        let (next, summary) = if zone == DockDropZone::Center {
            let id = format!("{REORG_TABS_ID_PREFIX}{}", self.tabs_seq.get());
            let next = base.tabify_fresh(panel.to_string(), target, id)?;
            self.tabs_seq.set(self.tabs_seq.get() + 1);
            (next, format!("{panel} -> {target}"))
        } else {
            let (orientation, position) =
                zone_split_geometry(zone).expect("a non-None, non-Centre zone has split geometry");
            let id = format!("{REORG_SPLIT_ID_PREFIX}{}", self.split_seq.get());
            let next = base.split_leaf_into(
                target,
                panel.to_string(),
                id,
                orientation,
                self.reorganize_ratio,
                position,
            )?;
            self.split_seq.set(self.split_seq.get() + 1);
            (next, format!("{panel} -> {target}"))
        };
        // (R1134 §5.51.1) The panel landed at a zone, not its home — drop any stale
        // collapse home anchor so a later home-restore does not resurrect the old
        // slot. A no-op under placeholder (nothing was stashed).
        self.home_anchors.borrow_mut().remove(panel);
        Ok(self.commit(next, summary))
    }

    /// (R1126 §5.51 §2 #7 PR-33) Float `panel` OUT of the dock — the COLLAPSE
    /// torn-slot policy: remove its leaf so the sibling reclaims the space (vs the
    /// placeholder policy, which keeps the leaf and paints a placeholder). The
    /// return into the dock is [`Self::dock_panel_at_zone`], total over the
    /// resulting absence. Idempotent: a re-fired tear-off (panel already floated)
    /// or an empty surface is a no-op, so the live-drag's repeated emits are safe.
    ///
    /// # Errors
    ///
    /// Propagates [`TopologyError::RootRemoval`] if `panel` is the dock's SOLE
    /// panel (an empty dock has no valid layout — float the last panel keeps it
    /// docked); the topology is left unchanged.
    pub fn float_out_panel(&self, panel: &str) -> Result<String, TopologyError> {
        let Some(current) = self.topology.get() else {
            return Ok("empty surface — no-op".to_string());
        };
        if !current.panel_ids().contains(&panel) {
            return Ok(format!("{panel}: already floated — no-op"));
        }
        // (R1134 §5.51.1) Capture the home anchor BEFORE removing the leaf so a
        // later dock-back ([`Self::restore_panel_home`]) restores it next to its
        // original sibling. A root / `Tabs`-member panel has no parent-Split anchor
        // (`None`): it still collapses (the reflow), and dock-back falls back to a
        // zone redock since there is no home to stash.
        if let Some(anchor) = current.leaf_anchor(panel) {
            self.home_anchors
                .borrow_mut()
                .insert(panel.to_string(), anchor);
        }
        let next = current.remove_leaf(panel)?;
        Ok(self.commit(next, format!("{panel}: floated out (collapse)")))
    }

    /// (R1134 §5.51.1) Dock-back a collapsed `panel` to its captured home anchor —
    /// the HOME inverse of [`Self::float_out_panel`] (vs [`Self::dock_panel_at_zone`],
    /// the ZONE inverse). Pops the [`DockLeafAnchor`] this panel's float stashed and
    /// re-inserts the leaf next to its original sibling via
    /// [`DockTopology::insert_leaf_at_anchor`] (the original split id / orientation /
    /// ratio / side — so the binding's per-split state re-binds). The home-restore
    /// the `invoke("tear_off")` dock-back + a cancelled float drive under
    /// [`FloatPolicy::Collapse`].
    ///
    /// Idempotent / total: no stashed anchor (placeholder mode never collapsed it,
    /// it already restored, or it was a root / `Tabs`-member with no anchor), an
    /// empty surface, or a panel somehow already docked → a recorded no-op, never an
    /// error or panic.
    ///
    /// # Errors
    ///
    /// Propagates the [`DockTopology::insert_leaf_at_anchor`] error when the stashed
    /// anchor's `sibling` no longer exists (it was itself removed since capture);
    /// the caller falls back to a default dock.
    pub fn restore_panel_home(&self, panel: &str) -> Result<String, TopologyError> {
        let Some(anchor) = self.home_anchors.borrow_mut().remove(panel) else {
            let outcome = format!("{panel}: no home anchor — no-op");
            self.note_outcome(&outcome);
            return Ok(outcome);
        };
        let Some(current) = self.topology.get() else {
            let outcome = format!("{panel}: empty surface — no-op");
            self.note_outcome(&outcome);
            return Ok(outcome);
        };
        if current.panel_ids().contains(&panel) {
            let outcome = format!("{panel}: already docked — no-op");
            self.note_outcome(&outcome);
            return Ok(outcome);
        }
        let next = current.insert_leaf_at_anchor(panel.to_string(), &anchor)?;
        Ok(self.commit(next, format!("{panel}: docked home")))
    }

    /// (R1085 §5.51) Make tab `index` the visible tab of the
    /// [`DockNode::Tabs`] well `well_id` — the tab-well **navigation**
    /// gesture, shared by the `activate_tab` invoke (AI / RPC primary)
    /// and (R1096) the pointer tab-strip click. Distinct from
    /// [`Self::apply_intent`]: that funnels the drag-produced
    /// [`DockReorganizeIntent`]s (moves that mint ids); this only changes
    /// which tab is visible, so it touches no `split_seq` / `tabs_seq`. It
    /// shares the *same* [`Self::commit`] funnel, so the `last_outcome` +
    /// undo-or-set bookkeeping has one writer regardless of gesture.
    ///
    /// (R1084 §5.51) Total over the empty surface: `None` (empty dock) has
    /// no well to navigate, so the call is the identity no-op
    /// `Ok("empty surface — no-op")` — no panic, the signal stays `None`.
    ///
    /// `index == active` is an accepted no-op that still commits (records +
    /// republishes), matching [`Self::apply_intent`]'s idempotent-gesture
    /// behaviour; the pointer drive (R1096) guards the already-active click
    /// at the gesture layer so re-clicking a live tab does not churn undo.
    ///
    /// # Errors
    ///
    /// Returns the [`TopologyError`] from [`DockTopology::set_active_tab`]
    /// when the gesture cannot apply ([`TopologyError::TabsWellNotFound`]
    /// for a stale / wrong-kind id, [`TopologyError::ActiveOutOfRange`]
    /// for an index past the well's end). Records the rejection in
    /// `last_outcome` (the one SSOT) and leaves the live topology
    /// unchanged.
    pub fn activate_tab(&self, well_id: &str, index: usize) -> Result<String, TopologyError> {
        let Some(current) = self.topology.get() else {
            let outcome = "empty surface — no-op".to_string();
            *self.last_outcome.borrow_mut() = Some(outcome.clone());
            return Ok(outcome);
        };
        let next = match current.set_active_tab(well_id, index) {
            Ok(next) => next,
            Err(e) => {
                // Record the rejection here so every drive (the AI invoke
                // path + the R1096 pointer click) surfaces `"rejected: …"`
                // through `query("last_outcome")` — one outcome SSOT.
                *self.last_outcome.borrow_mut() = Some(format!("rejected: {e}"));
                return Err(e);
            }
        };
        let summary = format!("activate {well_id}#{index}");
        Ok(self.commit(next, summary))
    }

    /// (R1096 §5.51) The live visible-tab index of the [`DockNode::Tabs`]
    /// well `well_id`, read from the shared topology Signal — `None` when
    /// the surface is empty (`None`) or no well carries that id. A read-only
    /// projection of the topology (no commit, no signal write); the
    /// [`TabWellExternal`] consults it to skip an already-active click
    /// (avoiding an undo edit) and to back its `query("active")`.
    #[must_use]
    pub fn tab_well_active(&self, well_id: &str) -> Option<usize> {
        self.topology.get().and_then(|t| t.tab_well_active(well_id))
    }

    /// (R1085 §5.51) The single topology-commit funnel — the **sole
    /// writer** of the dock-surface signal. Records `summary` as the
    /// `last_outcome`, then either pushes a reversible
    /// [`SignalEdit<Option<DockTopology>>`] (when an undo stack is
    /// attached — which applies it) or sets the signal directly (the R686
    /// path). Shared by [`Self::apply_intent`] (drag-produced moves) and
    /// [`Self::activate_tab`] (tab navigation) so neither duplicates the
    /// `last_outcome` + undo-or-set bookkeeping — the R1082.1 sole-writer
    /// invariant, now the funnel both gestures pass through.
    fn commit(&self, next: DockTopology, summary: String) -> String {
        *self.last_outcome.borrow_mut() = Some(summary.clone());
        // (R749 §5.52) When an undo stack is attached, record the topology
        // change as a reversible edit (which applies it); else mutate the
        // signal directly (the R686 path).
        if let Some(stack) = &self.undo {
            stack.record(SignalEdit::to(&self.topology, Some(next), summary.clone()));
        } else {
            self.topology.set(Some(next));
        }
        summary
    }
}

impl core::fmt::Debug for DockReorganizeExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DockReorganizeExternal")
            .field("reorganizer", &self.reorganizer)
            .field("has_drop_preview", &self.drop_preview.is_some())
            .finish()
    }
}

impl DockReorganizeExternal {
    /// Construct a reorganize external over a freshly-built coordinator
    /// for `topology` — the convenience constructor for an invoke-only
    /// editor that does not (yet) wire pointer panels. Equivalent to
    /// [`from_reorganizer`](Self::from_reorganizer) of a new coordinator.
    #[must_use]
    pub fn new(topology: Rc<Signal<Option<DockTopology>>>) -> Self {
        Self::from_reorganizer(Rc::new(DockReorganizer::new(topology)))
    }

    /// Wrap an existing shared [`DockReorganizer`] — the path the editor
    /// binding uses to give the invoke external **and** the R742 panel
    /// externals the *same* coordinator (one `split_seq`, one undo stack).
    #[must_use]
    pub fn from_reorganizer(reorganizer: Rc<DockReorganizer>) -> Self {
        Self {
            reorganizer,
            drop_preview: None,
        }
    }

    /// (R1082.1 §5.51) Share the ONE live drop-preview signal the R742
    /// panels write, so this canonical reorganize surface exposes
    /// `query("drop_preview")` — an AI client driving / observing
    /// reorganize through one well-known tag sees the in-flight pointer
    /// drag, not just the committed outcome.
    #[must_use]
    pub fn with_drop_preview(mut self, preview: Rc<Signal<Option<DockDropPreview>>>) -> Self {
        self.drop_preview = Some(preview);
        self
    }

    /// Clone the shared coordinator handle so the binding can hand it to
    /// the R742 [`DockPanelExternal`]s via
    /// [`DockPanelExternal::with_reorganizer`].
    #[must_use]
    pub fn reorganizer(&self) -> Rc<DockReorganizer> {
        Rc::clone(&self.reorganizer)
    }

    /// Diagnostic: how many splits the shared coordinator has minted.
    #[must_use]
    pub fn split_seq(&self) -> u64 {
        self.reorganizer.split_seq()
    }

    /// Apply a resolved [`DockReorganizeIntent`] through the shared
    /// coordinator. Retained on the external surface for direct-use
    /// callers (the editor demo's boot seeding) that hold the external
    /// rather than the coordinator.
    ///
    /// # Errors
    ///
    /// Propagates the coordinator's [`TopologyError`] unchanged.
    pub fn apply_intent(&self, intent: &DockReorganizeIntent) -> Result<String, TopologyError> {
        self.reorganizer.apply_intent(intent)
    }

    /// (R1085 §5.51) Make tab `index` the visible tab of well `well_id`
    /// through the shared coordinator. Retained on the external surface
    /// for direct-use callers (the editor's boot seeding) that hold the
    /// external rather than the coordinator, mirroring
    /// [`Self::apply_intent`].
    ///
    /// # Errors
    ///
    /// Propagates the coordinator's [`TopologyError`] unchanged.
    pub fn activate_tab(&self, well_id: &str, index: usize) -> Result<String, TopologyError> {
        self.reorganizer.activate_tab(well_id, index)
    }
}

impl External for DockReorganizeExternal {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui, Backend::Rpc], BackendFallback::Skip)
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }

    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        Some(self)
    }

    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        Some(self)
    }
}

impl ExternalIntrospect for DockReorganizeExternal {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(&[
            ("topology", "json"),
            ("split_seq", "int"),
            // (R1084.1 §5.51) Symmetric with `split_seq` — how many tab-well
            // ids the coordinator has minted (one per applied `Tabify`), so an
            // AI auto-discovering capabilities sees tab-well-mint progress as a
            // first-class observable, not only split-mint progress.
            ("tabs_seq", "int"),
            // (R1112 §5.51 §2 #7 PR-37) This dock surface's tab-docking policy
            // (`false` = split-only), so an AI discovers whether a centre drop
            // tabifies before classifying one — the single policy the pointer
            // preview + `drop` RPC + cross-window redock all honour.
            ("tabbing", "bool"),
            // R1134 §5.51.1 §2 #7 — this surface's torn-slot policy
            // (`"placeholder"` / `"collapse"`), read here + driven by the
            // `set_float_policy` invoke so an AI toggles collapse vs placeholder.
            ("float_policy", "string"),
            ("set_float_policy", "string"),
            ("last_outcome", "string"),
            // R1082.1 §5.51 — the in-flight pointer drag observed on the
            // canonical reorganize surface (`{source, target, zone}` or
            // null), so an AI client watching one tag sees both the
            // committed `last_outcome` and the live drag.
            ("drop_preview", "json"),
            ("drop", "json"),
            ("reorganize", "json"),
            // (R1085 §5.51) Tab-well navigation: make tab `index` of well
            // `well_id` visible (`{"well_id": "...", "index": N}`). The
            // AI-first primary for tab activation — discoverable here so an
            // agent reasoning over a `Tabs` well can switch tabs symbolically
            // (no pixels), the §2 #2 RPC-as-primary-path contract.
            ("activate_tab", "json"),
        ])
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            // R1082.1 §5.51 — the shared live drag-preview (same SSOT
            // projection the panel externals expose). Null when no pointer
            // panels are wired / no drag is in flight.
            "drop_preview" => Some(drop_preview_introspect(self.drop_preview.as_ref())),
            "topology" => {
                // §2 #7 scene-as-data — the live dock surface as queryable
                // JSON. (R1084) `None` (empty dock) serialises to JSON
                // `null`, so an AI client reads the empty surface as `null`
                // rather than a fabricated tree. serde cannot fail for the
                // well-formed `Option<topology>`; fall back to Null
                // defensively rather than panic.
                serde_json::to_value(self.reorganizer.topology().get())
                    .ok()
                    .map(IntrospectValue::Json)
            }
            "split_seq" => Some(IntrospectValue::Int(
                i64::try_from(self.reorganizer.split_seq()).unwrap_or(i64::MAX),
            )),
            "tabs_seq" => Some(IntrospectValue::Int(
                i64::try_from(self.reorganizer.tabs_seq()).unwrap_or(i64::MAX),
            )),
            // R1112 §5.51 §2 #7 PR-37 — the surface tab-docking policy.
            "tabbing" => Some(IntrospectValue::Bool(self.reorganizer.tabbing())),
            // R1134 §5.51.1 §2 #7 — the surface torn-slot (collapse) policy.
            "float_policy" => Some(IntrospectValue::Text(
                self.reorganizer.float_policy().as_str().to_string(),
            )),
            "last_outcome" => Some(match self.reorganizer.last_outcome() {
                Some(s) => IntrospectValue::Text(s),
                None => IntrospectValue::Null,
            }),
            _ => None,
        }
    }

    fn intervene(&mut self, path: &str, _value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            // Topology mutation flows through `invoke("drop", …)` /
            // `invoke("reorganize", …)`, not direct slot writes — every
            // edit must pass the mutation primitives' validation gate.
            // `drop_preview` is a read-only observation slot (R1082.1).
            // R1134 — `float_policy` is set via the `set_float_policy` invoke, not
            // a direct slot write.
            "topology" | "split_seq" | "last_outcome" | "drop_preview" | "float_policy" => {
                Err(InterveneError::ReadOnly)
            }
            _ => Err(InterveneError::UnknownPath),
        }
    }

    /// (R687 §5.16 §5.45) Two reorganize action shapes, for the two
    /// first-class AI interaction modes:
    ///
    /// * **`drop`** — *geometry / cursor* driven. Payload
    ///   `{"source": "<panel>", "cursor": {"x": f64, "y": f64},
    ///   "panels": [{"tag": "<panel>", "rect": {"x","y","w","h"}}, …]}`.
    ///   The caller hands the observed `scene/layout` rects + the
    ///   release cursor; the **substrate** classifies the drop zone
    ///   ([`dock_drop_zone_for`]) and resolves the gesture
    ///   ([`resolve_dock_drop`]) — no client re-implements the zone
    ///   geometry (the Rust helper is the single source of truth). A
    ///   drop over empty space / the source itself is a well-defined
    ///   no-op (`Ok(Null)`), not an error. This is the path a mouse
    ///   drag-session (RPC today, an in-process shell session later)
    ///   uses.
    /// * **`reorganize`** — *symbolic* driven. Payload
    ///   `{"source": "<panel>", "target": "<panel>", "zone": "<Zone>"}`
    ///   where `zone` is a [`DockDropZone`] variant name. The path an
    ///   AI agent reasoning over panel ids (no pixels) uses to express
    ///   "dock console to the left of viewport" directly.
    /// * (R1085 §5.51) **`activate_tab`** — *tab navigation* driven.
    ///   Payload `{"well_id": "<tabs-id>", "index": N}`. Makes tab `index`
    ///   the visible tab of the [`DockNode::Tabs`] well `well_id`
    ///   ([`DockReorganizer::activate_tab`]) — the AI-first primary for
    ///   switching tabs symbolically. Returns
    ///   `"activate <well_id>#<index>"` on success; rejects a stale /
    ///   wrong-kind `well_id` or an out-of-range `index` with
    ///   [`InvokeError::Rejected`]. Over the empty surface it is the
    ///   identity no-op `Ok("empty surface — no-op")`.
    ///
    /// `drop` / `reorganize` return the outcome summary
    /// `"<source> -> <target>"` on a successful edit (and `activate_tab`
    /// its own summary) and leave the live topology unchanged on
    /// [`InvokeError::Rejected`] (stale id / root removal / id
    /// collision / out-of-range tab); [`InvokeError::TypeMismatch`] for a
    /// malformed payload.
    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match path {
            "drop" => {
                let IntrospectValue::Json(obj) = args else {
                    return Err(InvokeError::TypeMismatch);
                };
                let source = obj
                    .get("source")
                    .and_then(serde_json::Value::as_str)
                    .ok_or(InvokeError::TypeMismatch)?;
                let cursor = obj.get("cursor").ok_or(InvokeError::TypeMismatch)?;
                let cursor_x = cursor
                    .get("x")
                    .and_then(serde_json::Value::as_f64)
                    .ok_or(InvokeError::TypeMismatch)?;
                let cursor_y = cursor
                    .get("y")
                    .and_then(serde_json::Value::as_f64)
                    .ok_or(InvokeError::TypeMismatch)?;
                let panels_json = obj
                    .get("panels")
                    .and_then(serde_json::Value::as_array)
                    .ok_or(InvokeError::TypeMismatch)?;
                let mut panels: Vec<(String, Rect)> = Vec::with_capacity(panels_json.len());
                for panel in panels_json {
                    let tag = panel
                        .get("tag")
                        .and_then(serde_json::Value::as_str)
                        .ok_or(InvokeError::TypeMismatch)?;
                    let rect = panel
                        .get("rect")
                        .and_then(parse_json_rect)
                        .ok_or(InvokeError::TypeMismatch)?;
                    panels.push((tag.to_string(), rect));
                }
                let panel_refs: Vec<(&str, Rect)> =
                    panels.iter().map(|(t, r)| (t.as_str(), *r)).collect();
                // Substrate classifies + resolves — the single source of
                // truth for drop-zone geometry. No client re-implements it.
                // R1112 PR-37 — the `drop` RPC honours THIS surface's tabbing
                // policy, so a split-only dock never tabifies on the RPC path
                // either (uniform with the pointer path).
                let Some(intent) = resolve_dock_drop_tabbing(
                    &panel_refs,
                    source,
                    cursor_x,
                    cursor_y,
                    self.reorganizer.tabbing(),
                ) else {
                    // Dropped over no valid target (empty space or the
                    // source itself) — a cancel, not a failure.
                    self.reorganizer.note_outcome("no drop target");
                    return Ok(IntrospectValue::Null);
                };
                // `apply_intent` records the `"rejected: …"` outcome itself
                // (the one SSOT), so the caller only maps the error.
                match self.apply_intent(&intent) {
                    Ok(summary) => Ok(IntrospectValue::Text(summary)),
                    Err(_) => Err(InvokeError::Rejected),
                }
            }
            "reorganize" => {
                let IntrospectValue::Json(obj) = args else {
                    return Err(InvokeError::TypeMismatch);
                };
                let source = obj.get("source").and_then(serde_json::Value::as_str);
                let target = obj.get("target").and_then(serde_json::Value::as_str);
                let zone_str = obj.get("zone").and_then(serde_json::Value::as_str);
                let (Some(source), Some(target), Some(zone_str)) = (source, target, zone_str)
                else {
                    return Err(InvokeError::TypeMismatch);
                };
                let zone = parse_drop_zone(zone_str).ok_or(InvokeError::Rejected)?;
                // Zone → intent through the [`intent_for_zone`] SSOT; a
                // `None`/unmappable zone is a rejected gesture.
                let intent = intent_for_zone(source, target, zone).ok_or(InvokeError::Rejected)?;
                match self.apply_intent(&intent) {
                    Ok(summary) => Ok(IntrospectValue::Text(summary)),
                    Err(_) => Err(InvokeError::Rejected),
                }
            }
            "activate_tab" => {
                let IntrospectValue::Json(obj) = args else {
                    return Err(InvokeError::TypeMismatch);
                };
                let well_id = obj.get("well_id").and_then(serde_json::Value::as_str);
                let index = obj.get("index").and_then(serde_json::Value::as_u64);
                let (Some(well_id), Some(index)) = (well_id, index) else {
                    return Err(InvokeError::TypeMismatch);
                };
                // A well-formed but out-of-range / unknown-well index is a
                // rejected gesture (not a malformed payload). `usize::try_from`
                // only fails on a >usize::MAX index, which is always out of
                // range, so it folds into the same rejection.
                let Ok(index) = usize::try_from(index) else {
                    return Err(InvokeError::Rejected);
                };
                // `activate_tab` records the `"rejected: …"` outcome itself
                // (the one SSOT), so the caller only maps the error.
                match self.activate_tab(well_id, index) {
                    Ok(summary) => Ok(IntrospectValue::Text(summary)),
                    Err(_) => Err(InvokeError::Rejected),
                }
            }
            // R1134 §5.51.1 §2 #2 — toggle this surface's torn-slot policy live.
            // Arg is the wire name (`"collapse"` / `"placeholder"`); an unknown
            // name is a rejected gesture. The next float honours the new policy.
            "set_float_policy" => {
                let IntrospectValue::Text(name) = args else {
                    return Err(InvokeError::TypeMismatch);
                };
                let policy = FloatPolicy::from_wire(&name).ok_or(InvokeError::Rejected)?;
                self.reorganizer.set_float_policy(policy);
                Ok(IntrospectValue::Text(policy.as_str().to_string()))
            }
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

/// R1096 §5.51 — the pointer **click-to-switch** adapter for ONE
/// [`DockNode::Tabs`] well.
///
/// The dock walker paints each tab well's strip via [`view_tabs`]`(well_id,
/// …)` ([`view_dock_surface`]), tagging each tab `{well_id}#{i}`
/// ([`composite_tab_tag`]). The `InputRouter`'s R51.42 `#`-split protocol
/// resolves a pointer hit on `{well_id}#{i}` to the [`External`] registered
/// at the *primary* tag `{well_id}` and dispatches
/// `invoke("send", "{i}:<PointerEvent>")`. This external lives at that
/// well-id tag — the binding registers one per well via
/// [`DockTopology::for_each_tabs_well`], re-run on every topology change so a
/// freshly-tabified `reorg-tabs-{seq}` well wires automatically (the same
/// R688 reconcile path the per-split + per-panel externals ride) — and
/// translates the click-release edge into
/// [`DockReorganizer::activate_tab`]`(well_id, i)`: the SAME sole-writer
/// commit funnel the R1085 AI-first `activate_tab` invoke and the R742
/// pointer reorganize drags pass through.
///
/// ## One home for the active tab
///
/// Unlike a free-standing tab strip's `RadioGroupExternal` (which *owns* its
/// selection state), the dock's active tab lives **only** in the topology
/// ([`DockNode::Tabs::active`]). This external holds no selection — it reads
/// the live active index from the shared coordinator (to skip an
/// already-active click, the gesture-layer guard
/// [`DockReorganizer::activate_tab`]'s rustdoc defers here, avoiding undo
/// churn) and writes through `activate_tab`. The active tab has one SSOT.
#[derive(Debug)]
pub struct TabWellExternal {
    /// The stable [`DockNode::Tabs`] well id this external routes for — the
    /// [`view_tabs`] strip tag, the registration tag, and the R51.42 primary
    /// half of every `{well_id}#{i}` tab tag the walker paints.
    well_id: Cow<'static, str>,
    /// The shared reorganize coordinator — the sole writer of the topology
    /// (one `split_seq` / `tabs_seq` / undo stack across every dock
    /// gesture). `activate_tab` navigates through its commit funnel.
    reorganizer: Rc<DockReorganizer>,
}

impl TabWellExternal {
    /// Construct a click-to-switch adapter for the tab well `well_id`,
    /// driving the shared `reorganizer`. The binding registers it at the
    /// `well_id` tag (= the painted [`view_tabs`] strip tag).
    #[must_use]
    pub fn new(well_id: impl Into<Cow<'static, str>>, reorganizer: Rc<DockReorganizer>) -> Self {
        Self {
            well_id: well_id.into(),
            reorganizer,
        }
    }
}

impl External for TabWellExternal {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui, Backend::Rpc], BackendFallback::Skip)
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }

    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        Some(self)
    }

    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        Some(self)
    }
}

impl ExternalIntrospect for TabWellExternal {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(&[
            ("well_id", "string"),
            // The live visible-tab index of this well, read from the shared
            // topology (never stored here — the topology owns it). Null when
            // the well is gone (a reorganize collapsed it) / the surface is
            // empty.
            ("active", "int"),
            ("send", "string"),
        ])
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "well_id" => Some(IntrospectValue::Text(self.well_id.to_string())),
            "active" => Some(
                self.reorganizer
                    .tab_well_active(&self.well_id)
                    .and_then(|a| i64::try_from(a).ok())
                    .map_or(IntrospectValue::Null, IntrospectValue::Int),
            ),
            _ => None,
        }
    }

    fn intervene(&mut self, path: &str, _value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            // The active tab is topology-owned: AI clients switch tabs via
            // this `send` click wire or the canonical reorganize external's
            // `activate_tab` invoke — not by intervening on a derived read.
            "well_id" | "active" => Err(InterveneError::ReadOnly),
            _ => Err(InterveneError::UnknownPath),
        }
    }

    /// R51.42 §5.51 — the framework synthetic event channel. The router
    /// dispatches `"{i}:<PointerEvent>"` for a hit on the painted
    /// `{well_id}#{i}` tab tag (the `#` sub-index is the tab index). The
    /// click-release edge (`PointerUp`) activates that tab through the
    /// shared coordinator.
    ///
    /// Wire shape:
    /// * `"{i}:PointerUp"` — activate tab `i` (the click edge, matching the
    ///   WAI-ARIA / `RadioGroup` automatic-activation-on-release model and
    ///   the R794.1 click-vs-drag SSOT: a free press-release in place
    ///   replays as a trailing `PointerUp`, while a real drag commits via
    ///   the source coordinator and suppresses this). A click on the
    ///   *already-active* tab is an accepted no-op (no `activate_tab`, no
    ///   undo churn).
    /// * `"{i}:PointerDown" / "...Enter" / "...Leave" / "...Cancel"` —
    ///   no-op `Ok(Null)` (hover / press transitions don't switch tabs).
    /// * A bare event with no `#` sub-index (a press on the strip background
    ///   between tabs, tagged `{well_id}` not `{well_id}#i`) — no-op
    ///   `Ok(Null)`.
    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        if path != "send" {
            return Err(InvokeError::UnknownPath);
        }
        let raw = args.as_str().ok_or(InvokeError::TypeMismatch)?;
        // Split `"sub_index:Event[:mods]"` via the `:` grammar SSOT (a
        // held-modifier click still carries the trailing token).
        let (sub_index, event_name) = match pinion_core::composite_tag::split_send_payload(raw) {
            Some((sub, ev, _mods)) => (Some(sub), ev),
            None => (None, raw),
        };
        // Only the release edge activates a tab; every other pointer
        // transition (hover / press / cancel) is an accepted no-op.
        if !matches!(
            PointerWireEvent::from_wire_name(event_name),
            Some(PointerWireEvent::Up)
        ) {
            return Ok(IntrospectValue::Null);
        }
        // A bare `PointerUp` (no `#` sub-index — the strip background) or a
        // non-numeric sub-index (no painted tab carries it) switches nothing.
        let Some(index) = sub_index.and_then(|s| s.parse::<usize>().ok()) else {
            return Ok(IntrospectValue::Null);
        };
        // Skip the already-active tab — a click on the visible tab is a no-op
        // gesture, not a committing re-activation (avoids undo churn, the
        // guard `DockReorganizer::activate_tab`'s rustdoc defers to here).
        if self.reorganizer.tab_well_active(&self.well_id) == Some(index) {
            return Ok(IntrospectValue::Null);
        }
        // `activate_tab` records its own `"rejected: …"` outcome (the one
        // SSOT), so the caller only maps the error.
        match self.reorganizer.activate_tab(&self.well_id, index) {
            Ok(summary) => Ok(IntrospectValue::Text(summary)),
            Err(_) => Err(InvokeError::Rejected),
        }
    }
}

/// R683.B §5.16 (R1081 §5.51) — symbolic event name the
/// [`DockPanelExternal`] emits when a drag escapes every drop target
/// and tears the panel off into a floating window. Constant (not raw
/// literal) so binding-side reducer match arms can spell the dotted
/// intent tag via [`intent_tag!`](pinion_core::intent_tag) without
/// duplicating the literal: `intent_tag!(PANEL_TAG, dock::TEAR_OFF_EVENT)`.
pub const TEAR_OFF_EVENT: &str = "tear_off";

/// R1094 §5.16 §5.41 §5.51 — symbolic event the [`DockPanelExternal`]
/// emits on **every** drag move that has escaped every drop target: the
/// live follow signal a follow-the-cursor tear-off coordinator consumes.
/// Unlike the toggling [`TEAR_OFF_EVENT`] this is **ensure-only** — the
/// binding reducer creates the panel's floating window if absent and
/// writes its outer position from the forwarded cursor, never removes.
/// Reusing the toggle on every move would double-toggle against the
/// release that also fires (the R1071-R1078 lesson), so the live drag
/// path pairs this with [`TEAR_OFF_REDOCK_EVENT`] (remove-only) and
/// leaves `tear_off` as the discrete AI dock-back toggle. Payload is the
/// follow JSON `{panel, x, y}` ([`IntrospectValue::Json`], window-logical
/// cursor) — the panel id locates the window, the cursor positions it.
pub const TEAR_OFF_FOLLOW_EVENT: &str = "tear_off_follow";

/// R1118 §5.16 §5.41 §5.51 PR-38 — symbolic event the [`DockPanelExternal`]
/// emits when the panel's OWN floating window is dragged by its title bar (a
/// drag whose [`DragUpdate::source_window`] is the panel's declared
/// [`floating_window`](DockPanelExternal::with_floating_window)). This is a
/// WINDOW MOVE, distinct from [`TEAR_OFF_FOLLOW_EVENT`] (a docked panel escaping
/// its dock to float AT the cursor). The payload is a grab-relative DISPLACEMENT
/// `{panel, dx, dy}` — how far the cursor has strayed from the grab point, in
/// the floating window's own frame — and the binding reducer moves the window BY
/// that delta (`new_pos = current_pos + (dx, dy)`), keeping the grabbed
/// title-bar point under the cursor. Kept a SEPARATE event (not folded into
/// `tear_off_follow`) so the wire form is honest: a tear-off carries a cursor, a
/// window move carries a displacement — two distinct vocabularies pinned apart
/// (R1118 review-clearance; the project's wire-vocab-pin-not-fold discipline).
pub const WINDOW_MOVE_EVENT: &str = "window_move";

/// R1094 §5.16 §5.41 §5.51 — symbolic event the [`DockPanelExternal`]
/// emits when a drag that had torn the panel into a live floating
/// follower ends back in the dock: released over a dock zone (redock) or
/// snapped back / cancelled (restore). **Remove-only** — the binding
/// reducer drops the panel's floating window if present, an idempotent
/// no-op otherwise. Payload is the panel id ([`IntrospectValue::Text`]).
pub const TEAR_OFF_REDOCK_EVENT: &str = "tear_off_redock";

/// R1100 §5.51 §5.16 §2 #7 PR-33 — symbolic event the [`DockPanelExternal`]
/// emits when a drag tears a panel out of its floating window and releases it
/// over **another window's** dock zone (the cross-window drag-back redock the
/// per-window router cannot resolve, composed by the shell —
/// [`pinion_runtime::resolve_cross_window_drop`]). Distinct from
/// [`TEAR_OFF_REDOCK_EVENT`]: that is **remove-only** "restore to where it
/// came from"; this is **dock-at** "re-insert into window `window` at the drop
/// zone under the cursor". The two semantics are two intents — never a
/// polymorphic payload — so a restore (Text payload) and a dock-at (JSON
/// payload) can never be confused by a reducer. Payload
/// ([`IntrospectValue::Json`]): `{panel, window, target, x_rel, y_rel}` — the
/// floated panel, the target window's spec id, the target dock zone's paint
/// tag, and the cursor normalised over that zone's rect (the binding /
/// coordinator classifies the edge-vs-centre zone from `x_rel`/`y_rel`, the
/// single zone-geometry SSOT, exactly as the same-window drop path does).
pub const TEAR_OFF_REDOCK_AT_EVENT: &str = "tear_off_redock_at";

/// R683.B §5.16 — sidecar carrying [`view_dock_panel`]'s
/// binding-local visual + behavioural constants. `#[non_exhaustive]`
/// so future axes (resize handles, close button, collapse arrow)
/// land via builders without breaking the constructor surface.
///
/// Use [`Self::m3_default`] for the M3-canonical 28-px header strip.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct DockPanelStyle {
    /// Header strip extent (logical pixels) along the cross axis of
    /// the panel (height for the default `FlexDirection::Column`
    /// layout). Material 3 list / app-bar dense-row convention is
    /// 28 px; pro-tool authoring surfaces (DCC / IDE panels) use
    /// 24-32 px for compactness.
    pub header_height_px: u32,
    /// Paint-side tag the panel's outer [`Scene::Container`] carries.
    /// The header strip is tagged `{tag}#header` (composite-tag
    /// convention R51.42); the content area is tagged `{tag}#content`.
    /// The [`DockPanelExternal`] is registered against this **panel
    /// root tag** (R683.C, NOT the `{tag}#header` composite) — the
    /// router splits a composite paint tag at `#` and routes header /
    /// content presses to the root external by the primary half.
    pub tag: Cow<'static, str>,
    /// Font size for the header title text. M3 label-medium token
    /// = 12 sp by default; reads tightly against the 28-px header
    /// strip.
    pub header_font_size_px: u32,
    /// (R1083 §5.51) Whether [`view_dock_panel`] paints the title header
    /// strip. Default `true`. A panel rendered as the visible tab of a
    /// [`DockNode::Tabs`] well sets this `false` — the well's tab strip
    /// *is* the header, so the per-panel header would be redundant. The
    /// `{tag}#header` composite hit-region simply isn't emitted when
    /// suppressed; the panel root `drop_target` + content are unchanged.
    pub show_header: bool,
    /// (R1116 §5.51 PR-38) Whether the panel root opts in as a
    /// [`LayoutStyle::drop_target`](pinion_core::style::LayoutStyle::drop_target).
    /// Default `true` — a docked panel is a drop target so the R742 router
    /// climbs a cursor over the panel's deeper content tag to the root and
    /// hands the coordinator the panel id + a normalised cursor (reorder /
    /// split / tab / tear-off). A panel that is the **sole content of its own
    /// floating window** sets this `false`.
    ///
    /// **Load-bearing effect (R1118-corrected): cross-window-drop REJECTION.**
    /// The floating window's only drop-target candidate is this panel root; with
    /// `false` the floater exposes NO drop target, so
    /// [`pinion_runtime::resolve_cross_window_drop`] resolves nothing there and
    /// another panel cannot be docked INTO the single-panel floater (a panel
    /// cannot dock into its own / a sole floater). The window MOVE itself is
    /// **NOT** driven by this flag — that is the dedicated
    /// [`DockPanelExternal::drag_to_at`] window-move branch gated on
    /// `source_window == floating_window` (R1116/R1117), which returns before any
    /// escape/`over` logic. (Pre-R1118 this doc claimed the move came from a
    /// "header drag escapes" mechanism; that was superseded and is dead for the
    /// floater — `resolve_drop_point` hover-fallback means a floater never
    /// reports `over==None`, so no escape ever fires.)
    pub drop_target: bool,
}

impl DockPanelStyle {
    /// (R683.B §5.16) M3-canonical default: 28-px header, 12-px header
    /// font, header shown.
    #[must_use]
    pub fn m3_default(tag: impl Into<Cow<'static, str>>) -> Self {
        Self {
            header_height_px: 28,
            tag: tag.into(),
            header_font_size_px: 12,
            show_header: true,
            drop_target: true,
        }
    }

    /// Override the header strip height in logical pixels. Touch
    /// surfaces want ≥ 44 px (Material touch-target floor).
    #[must_use]
    pub const fn with_header_height_px(mut self, height: u32) -> Self {
        self.header_height_px = height;
        self
    }

    /// (R1116 §5.51 PR-38) Override whether the panel root opts in as a drop
    /// target (default `true`). A panel floated as the sole content of its own
    /// window sets this `false` so its header drag is a window move, not a
    /// self-redock the single panel could never satisfy (see the field doc).
    #[must_use]
    pub const fn with_drop_target(mut self, drop_target: bool) -> Self {
        self.drop_target = drop_target;
        self
    }

    /// (R1083 §5.51) Override whether the title header strip is painted
    /// (default `true`). A tab-well's active panel sets this `false`.
    #[must_use]
    pub const fn with_show_header(mut self, show_header: bool) -> Self {
        self.show_header = show_header;
        self
    }
}

/// (R683.B §5.16) Composite-tag suffix for the dock panel's header
/// strip. The header is the drag-able surface; the
/// [`DockPanelExternal`] attaches to the composite tag
/// `{panel_tag}#header` so `PointerDown` on the header routes to it
/// (deepest-tagged hit-test).
pub const HEADER_TAG_SUFFIX: &str = "header";

/// (R683.B §5.16) Composite-tag suffix for the dock panel's content
/// area. Always present so AI clients can introspect the panel's
/// inner content tree via `scene/snapshot {path: "{panel_tag}#content"}`.
pub const CONTENT_TAG_SUFFIX: &str = "content";

/// (R683.B §5.16) Backend-agnostic dock-panel composition.
///
/// Builds a vertical [`Scene::Container`] (`FlexDirection::Column`)
/// with two children:
///
/// 1. Header strip — fixed `header_height_px` tall, tagged
///    `{panel_tag}#header`, M3 `SurfaceContainerHigh` fill,
///    contains a single [`TextNode`] with the panel's `title`.
/// 2. Content area — flex-grow 1, tagged
///    `{panel_tag}#content`, transparent fill, wraps the
///    application-supplied `content` Scene.
///
/// The outer Container carries [`DockPanelStyle::tag`] (the panel's
/// canonical id) so AI introspection + future dock topology code
/// can locate the panel root.
///
/// The header strip is the drag handle: the [`DockPanelExternal`] the
/// binding registers against the **panel root tag** (R683.C — NOT the
/// `{tag}#header` composite) opens an R742 drag session on a header press
/// ([`begin_drag`](External::begin_drag)) and, on release, docks the
/// panel onto another panel (via the shared [`DockReorganizer`]) or tears
/// it off into a floating window — see the [`DockPanelExternal`] rustdoc.
///
/// # Panics
///
/// Never panics on its own — `title` is borrowed verbatim into a
/// `TextNode`; `content` is moved into the content container
/// without inspection.
/// (R1081 §5.51) `active_drop_zone` is `Some(zone)` while this panel is
/// the live drop target of an in-flight R742 drag (the binding reads the
/// shared [`DockDropPreview`] and passes `Some(preview.zone)` for the
/// matching panel) — the panel then paints a [`dock_drop_zone_highlight`]
/// overlay over the band the drop would dock into. `None` (the default
/// for every static / floating panel) paints no overlay.
#[must_use]
pub fn view_dock_panel(
    title: &str,
    content: Scene,
    theme: &Theme,
    style: &DockPanelStyle,
    active_drop_zone: Option<DockDropZone>,
) -> Scene {
    let header_tag = composite_tag(&style.tag, HEADER_TAG_SUFFIX);
    let content_tag = composite_tag(&style.tag, CONTENT_TAG_SUFFIX);
    let header_title = Scene::Text(TextNode::styled(
        title.to_string(),
        Rect::default(),
        TextStyle::new()
            .with_size_px(style.header_font_size_px)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ));
    // (R684 §5.21 §5.16) Header strip layout — the R683.C honest
    // carry now closed via the R684 substrate. The original R683.B
    // emit used `Size::px(0, header_height_px)` — taffy interprets
    // an explicit `Px(0)` width as "fixed 0 wide" BEFORE the cross-
    // axis [`AlignItems::Stretch`] resolution runs, so the header
    // rect collapsed to `padding-left + padding-right` (16 px). The
    // textbook fix is the cross-axis = `SizeValue::Auto` so the
    // outer Column container's `AlignItems::Stretch` can promote
    // the rect to the dock panel's full width.
    //
    // R684 atomic 0 lands [`Size::height_px`] as the substrate
    // primitive — pinion-widget-paint cannot construct
    // `Size { width: Auto, height: Px(h) }` directly because `Size`
    // is `#[non_exhaustive]` (cross-crate struct expression
    // restriction); the substrate constructor + the call site here
    // pair land in the same round per [[substrate-incompleteness-
    // signal]] + Rule-of-One adoption discipline.
    //
    // Note: the dock-panel header is NOT a flex-`grow` participant.
    // The outer Column flex parent's main axis is Y (height); the
    // header's height is pinned at `Px(header_height_px)` so the
    // content wrapper's `with_flex_grow(1.0)` claims all leftover
    // Y space deterministically. Adding `with_flex_basis(Px(0)) +
    // with_flex_grow(1.0)` to the header would make it compete with
    // the content wrapper for the parent's Y axis, breaking the
    // fixed-height header invariant. The R684 splitter atomic 2
    // is the canonical [[r684-flex-basis-substrate]] consumer; the
    // dock header strip is a cross-axis-stretch fix only.
    let header = Scene::Container(
        ContainerNode::new(vec![header_title])
            .with_tag(header_tag)
            .with_style(BoxStyle::filled(
                theme.resolve(ColorRole::SurfaceContainerHigh),
            ))
            .with_layout(
                LayoutStyle::new()
                    .with_size(Size::height_px(style.header_height_px))
                    .with_align_items(AlignItems::Center)
                    .with_justify(JustifyContent::Start)
                    .with_padding(Rect::new(8, 0, 8, 0)),
            ),
    );
    // (R1109 PR-35 §5.21) The content wrapper is the Column flex parent's
    // sole grow child, claiming all leftover Y space after the fixed-height
    // header. Pre-R1109 it carried `with_flex_grow(1.0)` alone — the
    // incomplete half of the idiom: taffy's CSS automatic flex minimum then
    // pins the wrapper to its content's intrinsic min-content height, so
    // content taller than the panel (a reflow terminal grid, an image, a
    // nested grid) clamps to content and overflows instead of shrinking to
    // the panel. Applying the full R1086 flex-main idiom —
    // `flex_basis: 0 + flex_grow: 1 + min-height: 0` — lets the wrapper
    // shrink below its content on the main (Y) axis. The cross axis stays
    // `Size::auto()` so the outer `AlignItems::Stretch` still fills the
    // width. Same idiom `view_splitter`'s `apply_flex_main` applies to its
    // ratio children; see [[layoutstyle-min-size-flex-shrink]].
    let content_wrapper = Scene::Container(
        ContainerNode::new(vec![content])
            .with_tag(content_tag)
            .with_layout(
                LayoutStyle::new()
                    .with_flex_basis(SizeValue::Px(0))
                    .with_flex_grow(1.0)
                    .with_min_size(Size::auto().with_height(SizeValue::Px(0))),
            ),
    );
    // The header (when shown) + content lay out in the Column flex flow;
    // the optional drop-zone overlay is an absolutely-positioned
    // (out-of-flow) last child painted on top, so it never shifts content.
    // (R1083) A tab-well's active panel suppresses the header — the well's
    // tab strip supplies the title row instead.
    let mut children = if style.show_header {
        vec![header, content_wrapper]
    } else {
        vec![content_wrapper]
    };
    if let Some(zone) = active_drop_zone {
        if zone != DockDropZone::None {
            children.push(dock_drop_zone_highlight(zone, theme));
        }
    }
    Scene::Container(
        ContainerNode::new(children)
            .with_tag(style.tag.clone())
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::SurfaceContainer)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Stretch)
                    // (R1080/R1081 §5.51) A docked panel opts in as a drop
                    // target so the R742 router climbs a cursor over the
                    // panel's deeper content tag to THIS root, handing the
                    // pointer coordinator the panel id + a cursor normalised
                    // over the whole panel rect (the zone classifier's input).
                    // (R1116 §5.51 PR-38) A sole-floater panel sets it `false` so
                    // the floater exposes no drop target (cross-window-drop
                    // rejection — see the field doc); the window move is a
                    // separate `drag_to_at` branch, not this flag.
                    .with_drop_target(style.drop_target),
            ),
    )
}

fn composite_tag(panel_tag: &str, suffix: &'static str) -> String {
    format!("{panel_tag}#{suffix}")
}

/// (R1111 §5.51 PR-37) The percent of the target a drop's RESULT occupies —
/// what the drop preview highlights. An edge split-insert takes the
/// [`DEFAULT_REORGANIZE_RATIO`] half (`50`), NOT the `0.25` classification
/// band: pre-R1111 the overlay mirrored the (narrower) trigger band, so the
/// painted affordance under-showed where the panel would land. Centre tabify
/// takes the whole target (100% — the tab well covers the pane), handled
/// directly in [`dock_drop_zone_highlight`]. Kept in sync with the split
/// ratio by `dock_split_result_pct_matches_ratio`.
///
/// **Invariant (R1112):** this mirrors the *default* [`DEFAULT_REORGANIZE_RATIO`],
/// which is what [`DockReorganizer`] always splits at today (`reorganize_ratio`
/// is seeded from the default and has no setter). The highlight fn takes only
/// `(zone, theme)`, so it cannot see a per-coordinator ratio — IF
/// `reorganize_ratio` ever becomes settable to a non-default value, the preview
/// would drift from the actual split and the ratio must be threaded into
/// [`dock_drop_zone_highlight`]. The `dock_split_result_pct_matches_ratio` test
/// guards the default-ratio assumption.
const DOCK_SPLIT_RESULT_PCT: u8 = 50;

/// (R1081 §5.51) Alpha the drop-zone highlight tint is drawn at (~40% of
/// the [`ColorRole::Accent`] colour) so the docked-into band reads as a
/// translucent overlay, not an opaque fill that hides the panel content.
const DOCK_DROP_HIGHLIGHT_ALPHA: u8 = 0x66;

/// (R1125 §5.51) The canonical drop-zone highlight tint — the theme Accent at the
/// shared overlay alpha. Exposed so the shell can supply it as the cross-window
/// preview `tint` without re-deriving the alpha (the binding's
/// preview tint), and reused by [`dock_drop_zone_highlight`].
#[must_use]
pub fn dock_drop_highlight_tint(theme: &Theme) -> Color {
    theme
        .resolve(ColorRole::Accent)
        .with_alpha(DOCK_DROP_HIGHLIGHT_ALPHA)
}

/// (R1139 §5.51) Alpha for the CROSS-WINDOW redock preview FILL — deliberately
/// bolder than the in-window [`DOCK_DROP_HIGHLIGHT_ALPHA`] (0x66 ≈ 40%). The
/// redock preview is drawn OVER opaque panel content (the dragged floater
/// occludes whatever sits behind it, and the target panel is fully painted), so
/// a 40% wash reads as near-invisible — the live-test "안 보임" failure. A redock
/// is also a rarer, higher-stakes action (a whole panel relocates), which
/// warrants an unmistakable cue.
const DOCK_REDOCK_PREVIEW_ALPHA: u8 = 0xCC;

/// (R1139 §5.51) Border width (logical px) of the cross-window redock preview
/// outline — an OPAQUE accent edge that reads regardless of how close the
/// content behind it is to the fill tint.
const DOCK_REDOCK_PREVIEW_BORDER_PX: u32 = 3;

/// (R1139 §5.51) The cross-window redock preview FILL tint — theme Accent at the
/// bolder [`DOCK_REDOCK_PREVIEW_ALPHA`]. A dock binding's
/// [`dock_drop_preview`](crate) hook supplies this to
/// [`dock_drop_preview_overlay`] so the on-floater redock hint AND the
/// target-window preview read clearly over opaque content, distinct from the
/// subtler in-window [`dock_drop_highlight_tint`] (which stays 0x66 — an
/// in-window split affordance sits over the SAME panel and needs no shout).
#[must_use]
pub fn dock_redock_preview_tint(theme: &Theme) -> Color {
    theme
        .resolve(ColorRole::Accent)
        .with_alpha(DOCK_REDOCK_PREVIEW_ALPHA)
}

/// (R1125 §5.51 §2 #7 PR-33) Tag for the cross-window drop-zone PREVIEW overlay
/// the shell injects into the TARGET window while a floater is dragged over it —
/// so the strip is strippable / idempotent like every other shell overlay.
pub const DOCK_DROP_PREVIEW_TAG: &str = "__dock_drop_preview";

/// (R1125 §5.51 §2 #7 PR-33) Build the cross-window drop-zone PREVIEW as a single
/// pointer-transparent [`Scene::Box`] whose rect is the RESULT region (the split
/// half the redock will occupy, or the whole pane for a centre tabify) computed
/// in ABSOLUTE pixels from `panel_rect` — the target panel's window-absolute rect.
///
/// Unlike [`dock_drop_zone_highlight`] (a `Percent`-sized child of the panel's
/// view, resolved by the layout pass), this is injected by the shell AFTER layout
/// as a top-level overlay, so it carries explicit pixel rects (no `Percent` to
/// resolve). `tint` is the already-resolved highlight colour (the binding supplies
/// it via the binding's `dock_drop_preview` hook). `None` for [`DockDropZone::None`]
/// (a dead zone paints nothing).
#[must_use]
pub fn dock_drop_preview_overlay(
    panel_rect: Rect,
    zone: DockDropZone,
    tint: Color,
) -> Option<Scene> {
    let half_w = panel_rect.w * u32::from(DOCK_SPLIT_RESULT_PCT) / 100;
    let half_h = panel_rect.h * u32::from(DOCK_SPLIT_RESULT_PCT) / 100;
    let (x, y, w, h) = match zone {
        DockDropZone::None => return None,
        DockDropZone::Left => (panel_rect.x, panel_rect.y, half_w, panel_rect.h),
        DockDropZone::Right => (
            panel_rect.x + panel_rect.w - half_w,
            panel_rect.y,
            half_w,
            panel_rect.h,
        ),
        DockDropZone::Top => (panel_rect.x, panel_rect.y, panel_rect.w, half_h),
        DockDropZone::Bottom => (
            panel_rect.x,
            panel_rect.y + panel_rect.h - half_h,
            panel_rect.w,
            half_h,
        ),
        DockDropZone::Center => (panel_rect.x, panel_rect.y, panel_rect.w, panel_rect.h),
    };
    // (R1139 §5.51) An OPAQUE accent border around the result region — a hard
    // edge that reads even when the floater's content is close to the fill hue
    // (a translucent wash alone can vanish over similar-hued content, the
    // live-test failure). Derived from `tint` so the binding's colour choice
    // stays the single source; the fill alpha is the binding's
    // (`dock_redock_preview_tint` for the bolder cross-window cue).
    let border = Border::new(tint.with_alpha(0xFF), DOCK_REDOCK_PREVIEW_BORDER_PX);
    let mut node = BoxNode::new(
        Rect::new(x, y, w, h),
        BoxStyle::filled(tint).with_border(border),
    );
    node.tag = Some(DOCK_DROP_PREVIEW_TAG.into());
    // Decorative overlay: never shadow the live drag it represents.
    node.layout = node.layout.with_pointer_transparent(true);
    Some(Scene::Box(node))
}

/// (R1081 §5.51; R1111 PR-37 result-region) Build the drop-zone highlight
/// overlay a dock panel paints while it is the live drop target of an
/// in-flight R742 drag — an absolutely-positioned, pointer-transparent layer
/// covering the whole panel, tinting the region the drop would OCCUPY:
/// `Left`/`Right`/`Top`/`Bottom` = the [`DOCK_SPLIT_RESULT_PCT`] split half on
/// that side (the region the source lands in, ratio [`DEFAULT_REORGANIZE_RATIO`]),
/// `Center` = the whole target (a tabify stacks the source into a tab well that
/// covers the pane). R1111 widened the strip from the narrow 25% classification
/// band to the result region so the affordance shows WHERE the panel will land,
/// not merely which edge the cursor is near (the desktop-dock convention —
/// VS Code / `IntelliJ` shade the result area). The painted region is always the
/// one for the cursor's currently-classified zone.
///
/// `pointer_transparent` so the overlay never intercepts the drag, and
/// `absolute_position(0, 0)` + 100%×100% so it sits on top without
/// disturbing the panel's header / content flex flow. [`DockDropZone::None`]
/// paints an empty (zero-band) overlay.
#[must_use]
pub fn dock_drop_zone_highlight(zone: DockDropZone, theme: &Theme) -> Scene {
    let overlay_layout = || {
        LayoutStyle::new()
            .with_absolute_position(0, 0)
            .with_size(
                Size::auto()
                    .with_width(SizeValue::Percent(100))
                    .with_height(SizeValue::Percent(100)),
            )
            .with_pointer_transparent(true)
    };
    // (direction, justify, align, strip width%, strip height%) per zone —
    // justify pins the strip to the near edge, the percents size the band.
    // R1111 PR-37 — the strip is the RESULT region (the split half the drop
    // will occupy), NOT the narrower 25% classification band, so the affordance
    // shows WHERE the panel lands. An edge takes `half`; a centre tabify takes
    // the whole target (the tab well covers the pane).
    let half = DOCK_SPLIT_RESULT_PCT;
    let (dir, justify, align, w, h) = match zone {
        DockDropZone::None => {
            return Scene::Container(ContainerNode::new(vec![]).with_layout(overlay_layout()));
        }
        DockDropZone::Left => (
            FlexDirection::Row,
            JustifyContent::Start,
            AlignItems::Stretch,
            half,
            100,
        ),
        DockDropZone::Right => (
            FlexDirection::Row,
            JustifyContent::End,
            AlignItems::Stretch,
            half,
            100,
        ),
        DockDropZone::Top => (
            FlexDirection::Column,
            JustifyContent::Start,
            AlignItems::Stretch,
            100,
            half,
        ),
        DockDropZone::Bottom => (
            FlexDirection::Column,
            JustifyContent::End,
            AlignItems::Stretch,
            100,
            half,
        ),
        DockDropZone::Center => (
            FlexDirection::Row,
            JustifyContent::Center,
            AlignItems::Center,
            100,
            100,
        ),
    };
    let tint = dock_drop_highlight_tint(theme);
    let strip = Scene::Container(
        ContainerNode::new(vec![])
            .with_style(BoxStyle::filled(tint))
            .with_layout(
                LayoutStyle::new().with_size(
                    Size::auto()
                        .with_width(SizeValue::Percent(w))
                        .with_height(SizeValue::Percent(h)),
                ),
            ),
    );
    Scene::Container(
        ContainerNode::new(vec![strip]).with_layout(
            overlay_layout()
                .flex(dir)
                .with_justify(justify)
                .with_align_items(align),
        ),
    )
}

// ─────────────────────────────────────────────────────────────────────
// R685 §5.16 §5.49 — Floating-panel placeholder paint helper.
//
// Lifted from `hello-dock-panels` (R683.C, 1st dock consumer's
// `view_floating_placeholder`) on its 2nd-consumer signal
// (`hello-dock-panels-editor` R685 atomic 2) per
// [[abstraction-needs-second-consumer]]. The placeholder Container
// goes in the dock slot when the panel is currently torn off into
// a floating window — the structural slot stays present so the
// dock layout doesn't reshuffle when the user tears off a panel.
//
// ## Why a substrate helper (not a binding-local fn)
//
// Both dock consumers paint the same shape: subdued
// `SurfaceContainerLow` fill + centered "({panel_id} torn off)"
// text in muted on-surface colour. The composition has zero
// per-binding variance worth duplicating — the text label format,
// the M3 token choices, and the center-aligned Column layout are
// canonical. Future dock consumers inherit the same look
// automatically; the Material 3 token choices remain coherent
// even if a theme overhaul lands.
// ─────────────────────────────────────────────────────────────────────

/// (R685 §5.16) Style sidecar for [`view_floating_placeholder`].
/// `#[non_exhaustive]` so future axes (custom label, icon glyph,
/// click-to-re-attach hint) land via builders without breaking the
/// constructor surface. Use [`Self::m3_default`] for the canonical
/// 14-px Body Medium font tinted with `OnSurfaceMuted` against a
/// `SurfaceContainerLow` fill.
#[non_exhaustive]
#[derive(Debug, Clone, Copy)]
pub struct FloatingPlaceholderStyle {
    /// Font size for the placeholder label (logical pixels). M3
    /// Body Medium 14 sp is the canonical fit for a dense dock
    /// slot label.
    pub label_font_size_px: u32,
}

impl FloatingPlaceholderStyle {
    /// M3-canonical default: 14-px Body Medium label.
    #[must_use]
    pub const fn m3_default() -> Self {
        Self {
            label_font_size_px: 14,
        }
    }

    /// Override the placeholder label font size in logical pixels.
    #[must_use]
    pub const fn with_label_font_size_px(mut self, size: u32) -> Self {
        self.label_font_size_px = size;
        self
    }
}

/// (R685 §5.16) Suffix appended to the placeholder Container's
/// `tag`. The placeholder paint emits tag `"{panel_id}_placeholder"`
/// so AI introspection can detect "this slot is currently floating"
/// without descending into the panel content tree.
pub const PLACEHOLDER_TAG_SUFFIX: &str = "_placeholder";

/// (R685 §5.16 §5.49) Paint the canonical "(panel torn off)"
/// placeholder Container for a dock slot whose panel is currently
/// floating.
///
/// Subdued `SurfaceContainerLow` fill + centered
/// `"({panel_id} torn off)"` Text in `OnSurfaceMuted` colour. The
/// outer Container carries tag `"{panel_id}_placeholder"` so AI
/// clients can detect placeholders via `scene/query` without
/// descending into the panel's full content tree.
///
/// Used by both R685 dock consumers (`hello-dock-panels` after the
/// R685 atomic 2 retrofit + `hello-dock-panels-editor` 2nd consumer).
/// Production binding-local equivalents collapse into one call
/// through this substrate.
#[must_use]
pub fn view_floating_placeholder(
    panel_id: &str,
    theme: &Theme,
    style: &FloatingPlaceholderStyle,
) -> Scene {
    Scene::Container(
        ContainerNode::new(vec![Scene::Text(TextNode::styled(
            format!("({panel_id} torn off)"),
            Rect::default(),
            TextStyle::new()
                .with_size_px(style.label_font_size_px)
                .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
        ))])
        .with_tag(format!("{panel_id}{PLACEHOLDER_TAG_SUFFIX}"))
        .with_style(BoxStyle::filled(
            theme.resolve(ColorRole::SurfaceContainerLow),
        ))
        .with_layout(
            // (R1140 §5.51 PR-39) The torn slot is a DROP TARGET that FILLS its
            // leaf: a floater dragged back over its OWN emptied home slot (or any
            // panel dropped onto a torn slot) must resolve here so the shell
            // paints the redock hint + the release docks. Without `drop_target`
            // the home slot was invisible to the cross-window resolver (no preview
            // at all — the self-home gap); without the 100%×100% size the
            // placeholder shrank to its label (~19px), leaving a sliver-thin hit
            // target instead of the whole slot.
            LayoutStyle::new()
                .flex(FlexDirection::Column)
                .with_justify(JustifyContent::Center)
                .with_align_items(AlignItems::Center)
                .with_drop_target(true)
                .with_size(
                    Size::auto()
                        .with_width(SizeValue::Percent(100))
                        .with_height(SizeValue::Percent(100)),
                ),
        ),
    )
}

/// (R685 §5.16 §5.49) Floating-window id convention — the canonical
/// `"{prefix}{panel_id}"` form both dock consumers use when minting
/// a [`WindowSpec`](pinion_shell::WindowSpec)-like id for a
/// torn-off panel. Pulled into substrate as a string-only helper
/// (no `WindowSpec` dependency) so the prefix convention is
/// consistent across consumers + AI clients reading the topology
/// JSON can detect floating panels without per-binding prefix
/// knowledge.
///
/// `prefix` is conventionally `"torn-"` (the
/// [`DEFAULT_FLOATING_WINDOW_PREFIX`] constant); custom prefixes
/// land if a binding has a competing convention.
#[must_use]
pub fn floating_window_id(prefix: &str, panel_id: &str) -> String {
    format!("{prefix}{panel_id}")
}

/// (R685 §5.16) Default floating-window id prefix — `"torn-"`.
/// Both R685 dock consumers use this; AI clients reading topology
/// JSON can rely on the prefix to identify torn-off panels by
/// stripping it.
pub const DEFAULT_FLOATING_WINDOW_PREFIX: &str = "torn-";

// ─────────────────────────────────────────────────────────────────────
// R685 §5.16 §5.49 — DockSurface recursive composition walker.
//
// `view_dock_surface` lowers a [`DockTopology`] into a nested
// `view_splitter` + `view_dock_panel` scene. Each `DockNode::Leaf`
// emits one panel (via a binding-supplied `panel_handle`
// callback); each `DockNode::Split` emits one splitter (via a
// binding-supplied `split_handle` callback keyed on the Split's
// stable [`DockNode::Split::id`]).
//
// The walker is **pure** — no `Owner::cache`, no `Effect`, no
// `Signal::set`. The application owns the per-Split `Rc<Signal<f32>>`
// ratio handle + the per-Panel `Scene` content + the per-Split
// `dragging: bool` mirror; the walker just stitches them through
// `view_splitter` / `view_dock_panel`.
//
// ## Stable split id addressing
//
// Each [`DockNode::Split`] carries a stable [`DockNode::Split::id`]
// the walker passes to `split_handle(id, orientation)`. The binding
// looks up its `Rc<Signal<f32>>` ratio handle + the paint-side
// [`SplitterStyle::tag`](crate::splitter::SplitterStyle::tag) by
// stable id — topology mutations (leaf insert / Split rebalance /
// dock-reorganize) rewrite the tree shape but keep every Split's
// id intact, so binding-side state stays bound to the right Split.
// ─────────────────────────────────────────────────────────────────────

/// (R685.B §5.16 §5.49) Per-split reactive state the binding hands
/// to [`view_dock_surface`] for each [`DockNode::Split`] in the
/// topology, keyed by the Split's stable [`DockNode::Split::id`].
///
/// `ratio_signal` is the live `Rc<Signal<f32>>` the
/// [`SplitterExternal`] mutates on drag. The Signal is the run-time
/// source-of-truth for the current split position; the topology's
/// [`DockNode::Split::ratio`] field is the **initial** value (boot /
/// persistence default). `dragging` is the boolean the view fn reads
/// off
/// [`SplitterExternal::is_dragging`](crate::splitter::SplitterExternal::is_dragging)
/// so the M3 dragged-overlay tint paints correctly mid-drag.
///
/// (R685.B atomic 1 simplification) Pre-R685.B [`DockSplitHandle`]
/// also carried the `style: SplitterStyle` field — the walker now
/// builds the splitter style from the topology's
/// [`DockNode::Split::id`] + `orientation` automatically (single
/// source of truth — the topology IS the splitter shape), so the
/// binding hands only the live reactive state through this struct.
/// Pre-R685.B [`DockPanelHandle`] is fully removed for the same
/// reason — the walker builds [`DockPanelStyle`] from
/// [`DockNode::Leaf::panel_id`] automatically.
pub struct DockSplitState {
    /// Live ratio signal — the application owns this `Rc<Signal<f32>>`
    /// (typically via [`pinion_core::reactive::Owner::cache`]).
    pub ratio_signal: Rc<Signal<f32>>,
    /// Drag-state mirror, read off the
    /// [`SplitterExternal`](crate::splitter::SplitterExternal) on
    /// each paint cycle.
    pub dragging: bool,
}

/// (R685.B §5.16 §5.49) Recursive walker — lower a [`DockTopology`]
/// into a nested splitter + dock-panel [`Scene`]. The topology IS
/// the source of truth for tree shape, panel identity, split
/// identity, orientations, and initial ratios; the binding only
/// supplies (a) the inner content `Scene` for each panel and (b)
/// the live reactive state ([`DockSplitState`]) for each split.
///
/// ## Callback contract
///
/// * `panel_content: Fn(&str) -> Scene` — invoked once per
///   [`DockNode::Leaf`]; receives the leaf's `panel_id`; returns
///   the inner content `Scene` the panel hosts (toolbar text /
///   outliner tree / viewport viewport / property table / etc.).
///   The walker wraps the returned content in
///   [`view_dock_panel`](crate::dock::view_dock_panel) with a
///   [`DockPanelStyle::m3_default(panel_id)`] automatically — the
///   panel tag is the leaf's stable `panel_id`, single source of
///   truth.
/// * `split_state: Fn(&str, f32) -> DockSplitState` — invoked once
///   per [`DockNode::Split`]; receives the split's stable `id` +
///   its topology-declared `initial_ratio` (so the binding's
///   `Rc<Signal<f32>>` constructor seeds the same value the
///   topology declares — no defaults duplication). Returns the
///   reactive state pair. The walker builds [`SplitterStyle::m3_default(orientation, id)`]
///   automatically.
///
/// ## SSOT (single source of truth)
///
/// The R685.B walker rewrite enforces three SSOT contracts the
/// pre-R685.B form violated:
///
/// 1. **Panel tag**: leaf `panel_id` IS the
///    [`DockPanelStyle::tag`]. The binding cannot hand the wrong
///    tag because the walker doesn't accept one.
/// 2. **Splitter tag**: Split `id` IS the
///    [`SplitterStyle::tag`]. Same enforcement; binding cannot
///    drift.
/// 3. **Initial ratio**: the topology's `ratio` field IS the
///    initial value the binding's Signal constructor receives.
///    Binding has no place to declare it independently — pre-R685.B
///    the binding had a `default_ratio_for_split` helper duplicating
///    the topology's ratios.
///
/// The walker is pure — no `Owner::cache`, no `Effect`, no
/// `Signal::set`. The application owns the reactive substrate;
/// this function just stitches the supplied state through the
/// [`view_splitter`](crate::splitter::view_splitter) /
/// [`view_dock_panel`](crate::dock::view_dock_panel) composition.
/// (R1081 §5.51) `drop_zone` maps a leaf `panel_id` to the live
/// [`DockDropZone`] the in-flight R742 drag is over that panel, or `None`
/// when it is not the drop target — the binding's closure reads the
/// shared [`DockDropPreview`] (`|id| preview.filter(|p| p.target == id).map(|p| p.zone)`),
/// so the panel under the cursor paints the zone overlay reactively. A
/// static (no-drag) surface passes `|_| None`.
#[must_use]
pub fn view_dock_surface<P, S, Z>(
    topology: &DockTopology,
    panel_content: P,
    split_state: S,
    drop_zone: Z,
    theme: &Theme,
) -> Scene
where
    P: Fn(&str) -> Scene,
    S: Fn(&str, f32) -> DockSplitState,
    Z: Fn(&str) -> Option<DockDropZone>,
{
    view_dock_surface_node(
        topology.root(),
        &panel_content,
        &split_state,
        &drop_zone,
        theme,
    )
}

/// (R685.B §5.16) Internal recursive helper — walks one
/// [`DockNode`] subtree. Each [`DockNode::Leaf`] paints via
/// [`view_dock_panel`] with a [`DockPanelStyle::m3_default`] keyed
/// on the leaf's `panel_id`. Each [`DockNode::Split`] paints via
/// [`view_splitter`](crate::splitter::view_splitter) with a
/// [`SplitterStyle::m3_default`] keyed on the Split's `id` +
/// `orientation`, and forwards the topology's declared `ratio` as
/// the initial-value seed for the binding's reactive Signal
/// constructor.
fn view_dock_surface_node<P, S, Z>(
    node: &DockNode,
    panel_content: &P,
    split_state: &S,
    drop_zone: &Z,
    theme: &Theme,
) -> Scene
where
    P: Fn(&str) -> Scene,
    S: Fn(&str, f32) -> DockSplitState,
    Z: Fn(&str) -> Option<DockDropZone>,
{
    match node {
        DockNode::Leaf { panel_id } => {
            let content = panel_content(panel_id.as_ref());
            // Walker builds the panel style from the topology's
            // panel_id — no caller drift possible (SSOT).
            let style = DockPanelStyle::m3_default(panel_id.clone());
            view_dock_panel(
                panel_id.as_ref(),
                content,
                theme,
                &style,
                drop_zone(panel_id.as_ref()),
            )
        }
        // (R1083 §5.51) A tab well renders a [`view_tabs`] strip (keyed on
        // the well's stable id) above the active panel's content. The
        // active panel uses [`view_dock_panel`] with the header suppressed
        // — the strip is the title row — so it keeps the panel root
        // `drop_target` + content + drop affordance, and a drop onto the
        // well resolves the *active* panel id as the target (which lives in
        // this well, so an edge drop splits the whole well + a centre drop
        // tabifies into it). `active < panels.len()` by topology invariant.
        DockNode::Tabs { id, panels, active } => {
            let labels: Vec<&str> = panels.iter().map(Cow::as_ref).collect();
            let strip = view_tabs(
                id.clone(),
                &labels,
                Some(*active),
                theme,
                &TabsStyle::m3_default(),
            );
            let active_panel_id = panels[*active].as_ref();
            let style =
                DockPanelStyle::m3_default(active_panel_id.to_string()).with_show_header(false);
            let active_view = view_dock_panel(
                active_panel_id,
                panel_content(active_panel_id),
                theme,
                &style,
                drop_zone(active_panel_id),
            );
            // The strip is fixed-height; the active panel grows to fill the
            // remaining pane height (it has no intrinsic flex-grow, so wrap
            // it in a grow container).
            let active_grow = Scene::Container(
                ContainerNode::new(vec![active_view])
                    .with_layout(LayoutStyle::new().with_flex_grow(1.0)),
            );
            Scene::Container(
                ContainerNode::new(vec![strip, active_grow]).with_layout(
                    LayoutStyle::new()
                        .flex(FlexDirection::Column)
                        .with_align_items(AlignItems::Stretch),
                ),
            )
        }
        DockNode::Split {
            id,
            orientation,
            ratio,
            first,
            second,
        } => {
            let state = split_state(id.as_ref(), *ratio);
            // Walker builds the splitter style from the topology's
            // id + orientation — SSOT.
            let style = SplitterStyle::m3_default(*orientation, id.clone());
            let first_scene =
                view_dock_surface_node(first, panel_content, split_state, drop_zone, theme);
            let second_scene =
                view_dock_surface_node(second, panel_content, split_state, drop_zone, theme);
            view_splitter(
                first_scene,
                second_scene,
                &state.ratio_signal,
                theme,
                &style,
                state.dragging,
            )
        }
    }
}

/// R1095 §5.51 §5.27 §5.40 — the accessible name of every dock tab well's
/// `tablist` (WAI-ARIA requires a tab list to be nameable; the per-tab and
/// per-panel names come from the panel ids via scene name-from-contents
/// enrichment).
const DOCK_TABLIST_NAME: &str = "Panel tabs";

/// R1095 §5.51 §5.27 §5.40 — WAI-ARIA `tablist` / `tab` / `tabpanel`
/// [`AccessNode`]s for every [`DockNode::Tabs`] well in `topology`: the AT
/// surface of R1083 tabbed docking + R1085 `activate_tab`, so a screen
/// reader announces the well's tabs and which one is selected.
///
/// Tags MIRROR the painted scene so the shell's post-layout bounds fill +
/// name-from-contents enrichment land on the right nodes: the `tablist` is
/// the well's stable id (the [`view_tabs`] strip tag), each `tab` is
/// [`composite_tab_tag`]`(id, i)` (the painted per-tab tag the router splits
/// at `#`), and the `tabpanel` is the active panel's id (the
/// header-suppressed active content [`view_dock_surface`] renders). Each
/// tab's `selected` lowers to `aria-selected`; `aria-posinset` /
/// `aria-setsize` come from the slice. `label: None` defers each tab's name
/// to enrichment of its painted panel-id label (the [[two-text-layouts-paint-vs-geometry]]
/// name-from-contents path, like `hello-tabs`). `focused` (the focus
/// manager's tag at emit time) drives the roving active descendant: when a
/// well's strip owns focus, its active tab is the active descendant.
///
/// Built on the lifted [`pinion_a11y::tablist_tab_nodes`] (the dock is its
/// 3rd consumer after `hello-tabs` / `hello-tab-reorder`). Wells nested in
/// `Split`s are walked recursively; `Leaf`s contribute nothing (a docked
/// panel is not a tab).
#[must_use]
pub fn dock_tablist_access_nodes(
    topology: &DockTopology,
    focused: Option<&str>,
) -> Vec<AccessNode> {
    let mut out = Vec::new();
    collect_dock_tablist_nodes(topology.root(), focused, &mut out);
    out
}

/// Recursive walk for [`dock_tablist_access_nodes`].
fn collect_dock_tablist_nodes(node: &DockNode, focused: Option<&str>, out: &mut Vec<AccessNode>) {
    match node {
        DockNode::Tabs { id, panels, active } => {
            // The strip owning focus makes its active tab the active
            // descendant (the WAI-ARIA roving-tabindex pattern).
            let group_focused = focused == Some(id.as_ref());
            let tab_tags: Vec<String> = (0..panels.len())
                .map(|i| composite_tab_tag(id, i))
                .collect();
            let cells: Vec<TabCell<'_>> = tab_tags
                .iter()
                .enumerate()
                .map(|(i, tag)| TabCell {
                    tag: tag.as_str(),
                    label: None,
                    selected: i == *active,
                    focused: group_focused && i == *active,
                })
                .collect();
            let active_panel = panels[*active].as_ref();
            out.extend(tablist_tab_nodes(
                id.as_ref(),
                DOCK_TABLIST_NAME,
                &cells,
                active_panel,
                active_panel,
            ));
        }
        DockNode::Split { first, second, .. } => {
            collect_dock_tablist_nodes(first, focused, out);
            collect_dock_tablist_nodes(second, focused, out);
        }
        DockNode::Leaf { .. } => {}
    }
}

/// (R1081 §5.51) The dragged-panel + drop-zone the R742 pointer
/// coordinator resolves under the cursor, shared across every panel
/// external through one injected `Rc<Signal<Option<DockDropPreview>>>`
/// so the *target* panel's view fn paints the drop affordance while the
/// *source* panel drives the gesture. `None` between drags / when the
/// cursor is over no actionable panel zone.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DockDropPreview {
    /// The panel being dragged (the gesture's source).
    pub source: String,
    /// The panel under the cursor the drop would dock into — the
    /// [`LayoutStyle::drop_target`](pinion_core::style::LayoutStyle::drop_target)
    /// root tag the router (R1080) resolved.
    pub target: String,
    /// Where over `target` the cursor sits → which split / swap the drop
    /// performs.
    pub zone: DockDropZone,
}

impl DockDropPreview {
    /// (R1082.1 §5.51) Project to the introspection JSON
    /// (`{source, target, zone}`, the `zone` as its [`zone_wire_name`]) —
    /// the SSOT both `query("drop_preview")` surfaces share
    /// ([`DockPanelExternal`] and the canonical [`DockReorganizeExternal`])
    /// so the wire shape cannot drift between the two AI surfaces.
    #[must_use]
    pub fn to_introspect(&self) -> IntrospectValue {
        IntrospectValue::Json(serde_json::json!({
            "source": self.source,
            "target": self.target,
            "zone": zone_wire_name(self.zone),
        }))
    }
}

/// (R1082.1 §5.51) Project a shared drop-preview signal to its
/// introspection value — `Null` when no signal is wired or no drag is in
/// flight, else [`DockDropPreview::to_introspect`]. Shared by both AI
/// surfaces' `query("drop_preview")`.
fn drop_preview_introspect(
    preview: Option<&Rc<Signal<Option<DockDropPreview>>>>,
) -> IntrospectValue {
    preview
        .and_then(|s| s.get())
        .map_or(IntrospectValue::Null, |p| p.to_introspect())
}

/// (R1093 §5.15 §5.51 §2 #7) Project the forwarded drag cursor onto the wire:
/// `[x, y]` (window-logical pixels) when a drag has run, else null.
fn drag_cursor_introspect(cursor: Option<(f64, f64)>) -> IntrospectValue {
    cursor.map_or(IntrospectValue::Null, |(x, y)| {
        IntrospectValue::Json(serde_json::json!([x, y]))
    })
}

/// R1094 §5.16 §5.41 §5.51 — build the [`TEAR_OFF_FOLLOW_EVENT`] payload:
/// the panel id plus the forwarded window-logical cursor, so the binding
/// reducer both locates the panel's floating window and writes its
/// position. The widget reports a cursor in the SOURCE window's frame; the
/// binding adds that window's outer origin to reach a desktop position
/// (the widget crate must not know about windows — the gap(b) the
/// coordinator closes binding-side).
///
/// (R1107 §5.51) `source_window` names WHICH window's frame the cursor is in
/// (`DragUpdate::source_window`, threaded from the driving router) so the
/// binding adds the CORRECT window's origin — re-dragging an already-floating
/// header reports a cursor in that floater's frame, not main's. `None` (the
/// cursor-less degenerate fallback, or a single-window shell) → the binding's
/// primary window. Emitted as `null` when absent so an AI reads the SSOT.
fn tear_off_follow_payload(
    panel_id: &str,
    cursor: (f64, f64),
    source_window: Option<&str>,
) -> IntrospectValue {
    IntrospectValue::Json(serde_json::json!({
        "panel": panel_id,
        "x": cursor.0,
        "y": cursor.1,
        "source_window": source_window,
    }))
}

/// R1118 §5.51 PR-38 — build the [`WINDOW_MOVE_EVENT`] payload: the panel id plus
/// the grab-relative DISPLACEMENT `(dx, dy)` (cursor − `press_cursor`, in the
/// floating window's own frame). Distinct from [`tear_off_follow_payload`]: the
/// fields are `dx`/`dy` (a delta), never a cursor, so the wire form does not
/// lie about which quantity it carries — the binding reducer adds it to the
/// window's current position.
fn window_move_payload(panel_id: &str, delta: (f64, f64)) -> IntrospectValue {
    IntrospectValue::Json(serde_json::json!({
        "panel": panel_id,
        "dx": delta.0,
        "dy": delta.1,
    }))
}

/// R1120 §5.51 §5.15 PR-39 — the WINDOW MOVE delta in the window-INDEPENDENT
/// DESKTOP frame, which breaks the feedback loop that made the live drag jitter.
///
/// The R1118 delta `cursor_local − press_cursor` is measured relative to the
/// MOVING window (winit reports the cursor against the window's current origin),
/// so with the binding's `pos += delta` and the WM's `set_outer_position`
/// apply-lag the closed loop is marginally stable (error recurrence
/// `e_{n+1} = e_n − e_{n-1}`, roots of magnitude 1) → sustained oscillation. It is
/// invisible to fixed-frame / stepwise-settle tests (effective apply-lag 0).
///
/// Fix: `actual_origin` (the source window's `Window::outer_position()`, NOT the
/// declared position which lags) makes `actual_origin + cursor_local` the true
/// desktop pointer position regardless of how far the window has moved; emitting
/// the FRAME INCREMENT `desktop_now − desktop_prev`, accumulated by the binding's
/// `pos += delta`, gives `pos = pos_0 + (cursor_desktop − cursor_desktop_0)` with
/// NO feedback term — stable at any apply-lag (proved by the `simulate_window_move`
/// closed-loop harness). The first sample (`prev_desktop` None) anchors `prev` at
/// the grab's desktop (`actual_origin + press_cursor`), so the first delta is the
/// grab-relative move (≈0 while the cursor is still on the grab point). `None`
/// `actual_origin` (headless RPC drive / single-window) falls back to the
/// window-local cursor as the desktop estimate — correct at apply-lag 0. Returns
/// `(delta, new_prev_desktop)`.
#[must_use]
fn window_move_delta(
    actual_origin: Option<(f64, f64)>,
    cursor_local: (f64, f64),
    press_cursor: (f64, f64),
    prev_desktop: Option<(f64, f64)>,
) -> ((f64, f64), (f64, f64)) {
    let (ax, ay) = actual_origin.unwrap_or((0.0, 0.0));
    let desktop = (ax + cursor_local.0, ay + cursor_local.1);
    let prev = prev_desktop.unwrap_or((ax + press_cursor.0, ay + press_cursor.1));
    ((desktop.0 - prev.0, desktop.1 - prev.1), desktop)
}

/// R1103 §5.51 §2 #7 — read a normalised drop-zone cursor fraction from a
/// JSON redock-at payload, defaulting to the zone centre (`0.5`) when the
/// caller omits it. The `f64 → f32` narrowing is the documented `DropPoint`
/// representation choice: a `[0, 1]` cursor fraction loses no meaningful
/// precision in `f32`, so the truncation is intentional, not a bug.
#[allow(
    clippy::cast_possible_truncation,
    reason = "normalised [0,1] drop-zone cursor fraction; f64→f32 precision loss is irrelevant to zone classification"
)]
fn json_rel(obj: &serde_json::Value, key: &str) -> f32 {
    obj.get(key)
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(0.5) as f32
}

/// (R1081 §5.51) The `DragPayload::kind` discriminator a dock-panel drag
/// carries, so a future cross-widget drop target can match dock panels
/// before reading the payload value (the panel id).
pub const DOCK_PANEL_DRAG_KIND: &str = "dock-panel";

/// (R683.B §5.16 → R1081 §5.51) Drag-to-dock / drag-to-tear-off
/// [`External`] for the [`view_dock_panel`] header strip. Registered by
/// the binding via
/// [`WidgetCore::create_extra_externals`](pinion_core::WidgetCore::create_extra_externals)
/// against the **panel root tag** (e.g. `"inspector"`, the
/// [`DockPanelStyle::tag`]), NOT the composite `"inspector#header"` tag
/// the paint emits on the header strip — the R51.42 `dispatch_send` path
/// splits a composite paint tag at `#` and looks up the state-scene
/// External by the primary half, so the External must live at the panel
/// root for the `InputRouter` to route events to it.
///
/// ## The pointer drive (R742, replacing the R683 capture threshold)
///
/// R1081 §5.51 moved this external off the pre-R742 capture mechanism
/// ([`wants_pointer_capture`](External::wants_pointer_capture) +
/// `pointer_move` L∞ threshold) onto the §5.51 R742 drag-session hooks —
/// the two are mutually exclusive (both = a double-driven gesture). The
/// `InputRouter` drives the gesture:
///
/// 1. **`PointerDown`** dispatches `"header:PointerDown"` /
///    `"content:PointerDown"` through `invoke("send", …)`, which arms
///    ([`is_drag_armed`](Self::is_drag_armed)) only for a header press.
/// 2. **[`begin_drag`](External::begin_drag)** (called right after) opens
///    a session iff armed, returning a [`DragPayload`](pinion_core::external::DragPayload)
///    of kind [`DOCK_PANEL_DRAG_KIND`] carrying the panel id.
/// 3. **[`drag_to`](External::drag_to)** (every cursor move) resolves the
///    [`DropPoint`](pinion_core::external::DropPoint) over the nearest
///    opted-in drop-target panel (R1080) and writes the shared
///    [`DockDropPreview`] so the target panel's view highlights the zone.
/// 4. **[`drag_release`](External::drag_release)** classifies the drop:
///    over a *different* panel with a valid zone and an attached
///    [`DockReorganizer`] → the panel docks (split / swap) through the
///    shared coordinator; over **no** panel (the cursor escaped every
///    drop target — dragged out of the dock / window) → the panel
///    **tears off** into a floating window (the R683 outcome, now
///    release-driven); anything else (dropped back on itself, a dead
///    zone, or no coordinator) → a no-op snap-back.
/// 5. **[`drag_cancel`](External::drag_cancel)** (OS abort) discards the
///    gesture without committing.
///
/// ## Two modes by coordinator presence
///
/// * **dock-or-float** — a [`DockReorganizer`] is attached
///   ([`with_reorganizer`](Self::with_reorganizer)): drops onto other
///   panels reorganize the shared topology, escape-drops float. The
///   editor consumer.
/// * **tear-off-only** — no coordinator: every escape-drop floats and a
///   drop onto another panel is a no-op (the panel has no topology to
///   reorganize). The flat `hello-dock-panels` consumer.
///
/// ## Direct AI tear-off
///
/// `invoke("tear_off")` enqueues the same `tear_off` intent without a
/// pointer drag (R683.C) — the path an AI client drives the dock-back of
/// a freshly-floated panel before the router has a paint scene for the
/// new window.
#[allow(clippy::doc_markdown)]
pub struct DockPanelExternal {
    /// Stable panel identifier carried into the `tear_off` intent
    /// payload + the R742 [`DragPayload`](pinion_core::external::DragPayload)
    /// value. The binding's reducer + the `Signal<Vec<WindowSpec>>` push
    /// use this to decide which panel floated; the coordinator uses it as
    /// the reorganize `source`.
    panel_id: Cow<'static, str>,
    /// Pending intents waiting for the framework's
    /// [`External::drain_intents`] poll. v1 enqueues exactly one
    /// `tear_off` per gesture, so the queue depth is `≤ 1` in steady
    /// state; the `VecDeque` leaves room for future multi-event drags.
    pending_intents: RefCell<VecDeque<Intent>>,
    /// R683.C §5.16 — drag-arm flag, the
    /// [`begin_drag`](External::begin_drag) gate. Set `true` on
    /// `invoke("send", "header:PointerDown")`, `false` on
    /// `"content:PointerDown"` (a content press must not start a panel
    /// drag), re-armed on `PointerUp` / `PointerCancel`. Defaults `true`
    /// so a direct `begin_drag` (unit tests, AI invoke) arms without
    /// simulating the press arc.
    is_drag_armed: Cell<bool>,
    /// Diagnostic: a drag session this panel began is in flight (between
    /// [`begin_drag`](External::begin_drag) and the matching
    /// release / cancel). Surfaced via `query("dragging")`.
    dragging: Cell<bool>,
    /// Diagnostic: the last [`drag_release`](External::drag_release) tore
    /// the panel off (escaped every drop target) rather than docking /
    /// snapping back. Surfaced via `query("tear_off_fired")`.
    tear_off_fired: Cell<bool>,
    /// (R1093 §5.15 §5.51 §2 #7) The last absolute **window-logical** cursor
    /// the router forwarded during the in-flight drag (via
    /// [`drag_to_at`](External::drag_to_at) / [`drag_release_at`](External::drag_release_at)),
    /// or `None` before any drag. Reset on [`begin_drag`](External::begin_drag)
    /// and persists after release so an AI can read where the gesture went.
    /// Surfaced as scene-as-data via `query("drag_cursor")` (`[x, y]` / null)
    /// — the observability seam a follow-the-cursor tear-off coordinator
    /// reads (the cursor is in the SOURCE window's frame; the desktop
    /// position additionally needs the source window's outer position).
    drag_cursor: Cell<Option<(f64, f64)>>,
    /// (R1094 §5.16 §5.41 §5.51) `true` once the in-flight (or last) drag
    /// escaped every drop target at least once, emitting a
    /// [`TEAR_OFF_FOLLOW_EVENT`] that tore the panel into a live floating
    /// follower. Drives the release / cancel arms: a snap-back or cancel
    /// of a drag that DID detach emits [`TEAR_OFF_REDOCK_EVENT`] to restore
    /// (remove the floating window this gesture created), whereas a drag
    /// that never escaped snaps back with no window churn. Reset on
    /// [`begin_drag`](External::begin_drag); surfaced via `query("detached")`.
    ///
    /// R1101 §5.51 — set from the router's click-vs-drag verdict
    /// ([`DragUpdate::became_drag`]) on each move, NOT a private per-panel
    /// `DragLatch`. The R1097 `detach_latch` re-derived that verdict from the
    /// first post-move sample (it could not see the real press point), drifting
    /// from the router's SSOT; the panel now consumes the verdict the router
    /// already owns.
    ///
    /// R1110 §5.51 PR-36 — the float is gated on ESCAPE, not the bare drag
    /// verdict: the panel tears off only once the drag verdict lands AND the
    /// cursor is over no same-window dock zone. While still over a zone the
    /// gesture stays docked and shows the reorganize/split preview (the textbook
    /// desktop-dock model: reorder within the surface, float only when dragged
    /// out). Once escaped it latches floating for the rest of the gesture.
    detached: Cell<bool>,
    /// (R1102 §5.51 §2 #7 PR-33) The last cross-window dock-at this panel
    /// resolved — the `{panel, window, target, x_rel, y_rel}` payload of the
    /// most recent [`TEAR_OFF_REDOCK_AT_EVENT`], or `None` before any. The
    /// emitted intent is transient (drained into the reducer); this persists so
    /// an AI agent (and the live-drag demo) can observe via `query("redock_at")`
    /// that a cross-window redock fired — the §2 #7 introspection of the R1100
    /// contract now that R1102 wires the shell's `over_window` resolution live.
    /// Reset on [`begin_drag`](External::begin_drag) for the fresh gesture.
    last_redock_at: RefCell<Option<serde_json::Value>>,
    /// (R1107 §5.51 §2 #7) The window the most recent live tear-off move was
    /// measured in (`DragUpdate::source_window`) — the source window the
    /// binding adds the outer origin of to place the follower. Recorded each
    /// `drag_to_at`, reset on `begin_drag`. Surfaced via
    /// `query("source_window")` so an AI observes WHICH window a follow drag is
    /// in (a docked tear-off reports `"main"`; re-dragging a floating header
    /// reports that `torn-<panel>` window). `None` before any move / a
    /// cursor-less gesture.
    last_source_window: RefCell<Option<String>>,
    /// (R1120 §5.51 PR-39) The previous DESKTOP cursor of an in-flight title-bar
    /// WINDOW MOVE — the running anchor for the frame-increment delta
    /// (`desktop_now − prev_desktop`, [`window_move_delta`]). Reset on
    /// [`begin_drag`](External::begin_drag); seeded from the grab's desktop on the
    /// first move. Carrying the desktop frame (window-INDEPENDENT) is what makes
    /// the move stable under the WM's apply-lag (no feedback / jitter).
    prev_desktop: Cell<Option<(f64, f64)>>,
    /// (R1081 §5.51) The shared reorganize coordinator. `Some` puts the
    /// panel in dock-or-float mode (drops onto other panels reorganize
    /// the shared topology); `None` is tear-off-only. Cloned from the
    /// editor's one [`Rc<DockReorganizer>`] so a pointer dock and an AI
    /// invoke share one `split_seq` + undo stack.
    reorganizer: Option<Rc<DockReorganizer>>,
    /// (R1081 §5.51) The shared live drop-preview, written by whichever
    /// panel is the drag source and read by every panel's view fn to
    /// paint the zone affordance. `None` = no overlay binding (the
    /// gesture still docks / tears off, just without the preview paint).
    drop_preview: Option<Rc<Signal<Option<DockDropPreview>>>>,
    /// (R1129 §5.51.1 §5.38 §2 #5) The panel's persistent dock LIFECYCLE — the
    /// `docked ↔ floating` statechart ([`DockPanelPolicy`], `dock_panel.scxml`),
    /// the §5.38 per-widget-SCXML home for the float/redock/restore decision the
    /// imperative `detached` bool + hardcoded redock defaults used to make ad hoc
    /// (the §2 #5 drift this campaign retires). The chart is the AUTHORITY for the
    /// lifecycle IO gate: a `dropped`/`dock_back` is inert while `docked`, so the
    /// chart — not a bare bool — enforces "a panel that never floated cannot
    /// redock". Driven on the same gesture outcomes the router resolves
    /// ([`drag_to_at`](External::drag_to_at) escape → `Escaped`; the
    /// [`drag_release`](External::drag_release) reorganize / cross-window arms →
    /// `Dropped`; snap-back / cancel → `DockBack`) and surfaced as scene-as-data
    /// via `query("lifecycle")` (§2 #7). Per the Slider "SCXML owns the
    /// interaction state; the binding owns the typed value" split, the home
    /// anchor / target / zone / topology stay typed Rust in this binding, NOT the
    /// chart datamodel. `RefCell` because the lifecycle-driving methods span
    /// `&self` ([`begin_drag`](External::begin_drag) /
    /// [`settle_cross_window_redock`](Self::settle_cross_window_redock)) and
    /// `&mut self` (the drag arms), like the other interior-mutable diagnostics.
    ///
    /// Distinct from [`detached`](Self::detached), which is a per-GESTURE follow
    /// latch (reset on `begin_drag`): the chart is the PERSISTENT lifecycle. In
    /// every current consumer flow the two agree at the IO gates; the chart adds
    /// the explicit state + transition enforcement + introspection. The
    /// collapse|placeholder policy + the topology-reconstruct cross-gesture
    /// persistence are the campaign's STAGE 3 (carried).
    lifecycle: RefCell<Widget<DockPanelPolicy>>,
    /// (R1116 §5.51 PR-38) The id of THIS panel's own floating window, if the
    /// binding floats it (e.g. `"torn-viewport"`). When a drag's
    /// [`DragUpdate::source_window`] equals this, the drag is happening in the
    /// panel's own floating window, so its header (title bar) drag is a WINDOW
    /// MOVE (drag the borderless floater by its title bar — the OS-title-bar
    /// replacement `decorations:false` removed), NOT a dock tear-off. `None`
    /// (the default) = the panel is never floated by this binding, so every
    /// drag stays on the docked tear-off / reorganize path (bit-identical to
    /// pre-R1116). The binding passes its own window-naming SSOT, so the widget
    /// hardcodes no convention.
    floating_window: Option<String>,
}

impl core::fmt::Debug for DockPanelExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DockPanelExternal")
            .field("panel_id", &self.panel_id)
            .field("is_drag_armed", &self.is_drag_armed.get())
            .field("dragging", &self.dragging.get())
            .field("tear_off_fired", &self.tear_off_fired.get())
            .field("drag_cursor", &self.drag_cursor.get())
            .field("detached", &self.detached.get())
            .field("lifecycle", &self.lifecycle_name())
            .field("last_redock_at", &self.last_redock_at.borrow())
            .finish_non_exhaustive()
    }
}

impl DockPanelExternal {
    /// Construct a dock-panel drag External for the given panel id.
    /// Tear-off-only by default (no topology to reorganize); call
    /// [`with_reorganizer`](Self::with_reorganizer) to enter dock-or-float
    /// mode and [`with_drop_preview`](Self::with_drop_preview) to wire the
    /// live zone affordance.
    ///
    /// R1081 §5.51 dropped the per-panel `tear_off_threshold_frac`
    /// argument: the click-vs-drag threshold is now the `InputRouter`'s
    /// `DRAG_CLICK_THRESHOLD_PX` SSOT (a press-release in place stays a
    /// click; any real drag opens the session), so the panel no longer
    /// carries its own distance threshold.
    #[must_use]
    pub fn new(panel_id: impl Into<Cow<'static, str>>) -> Self {
        Self {
            panel_id: panel_id.into(),
            pending_intents: RefCell::new(VecDeque::new()),
            is_drag_armed: Cell::new(true),
            dragging: Cell::new(false),
            tear_off_fired: Cell::new(false),
            drag_cursor: Cell::new(None),
            detached: Cell::new(false),
            last_redock_at: RefCell::new(None),
            last_source_window: RefCell::new(None),
            prev_desktop: Cell::new(None),
            // R1129 §5.51.1 — the lifecycle chart starts `docked` (its SCXML
            // `initial`), matching a freshly-registered panel that has not floated.
            lifecycle: RefCell::new(Widget::new()),
            reorganizer: None,
            drop_preview: None,
            floating_window: None,
        }
    }

    /// (R1116 §5.51 PR-38) Declare the id of this panel's own floating window so
    /// a header drag IN that window is a WINDOW MOVE (drag the borderless floater
    /// by its title bar), not a dock tear-off. The binding passes its own
    /// window-naming SSOT (e.g. `dock::floating_window_id(prefix, panel)`); the
    /// widget compares [`DragUpdate::source_window`] to it and hardcodes no
    /// naming convention. Omit (the default `None`) for a panel the binding never
    /// floats — every drag then stays on the docked tear-off path.
    #[must_use]
    pub fn with_floating_window(mut self, window_id: impl Into<String>) -> Self {
        self.floating_window = Some(window_id.into());
        self
    }

    /// (R1133 §5.51.1 §2 #5) Re-hydrate the lifecycle chart from the binding's
    /// float truth at construction. A `DockPanelExternal` is rebuilt every time
    /// the binding's external factory re-runs (the R688 reconcile on any topology
    /// change), and a fresh chart starts `Docked` (the SCXML initial). For a
    /// panel that IS currently floating that would RESET the chart to `Docked`
    /// while its window exists — desyncing the chart from the binding's
    /// `windows_signal` (the persistent float truth). The binding passes
    /// `floating = is_panel_floating(panel)` here so the reconstructed chart is
    /// re-driven to `Floating` via the chart's own `escaped` transition (a replay
    /// of the float, not a back-door state poke), keeping the R1131 2-layer
    /// invariant under reconstruction. `false` (the default `new`) leaves the
    /// chart `Docked`. This makes `windows_signal` the single persistent SSOT and
    /// the chart its re-hydratable projection.
    #[must_use]
    pub fn with_initial_floating(self, floating: bool) -> Self {
        if floating {
            self.send_lifecycle(DockPanelEvent::Escaped);
        }
        self
    }

    /// (R1081 §5.51) Share the editor's reorganize coordinator so a drop
    /// onto another panel docks (split / swap) through the same
    /// `split_seq` + undo history the AI `invoke` path uses. Without it
    /// the panel is tear-off-only.
    #[must_use]
    pub fn with_reorganizer(mut self, reorganizer: Rc<DockReorganizer>) -> Self {
        self.reorganizer = Some(reorganizer);
        self
    }

    /// (R1081 §5.51) Share the live drop-preview signal the binding also
    /// reads in its view fn to paint the target panel's zone overlay. One
    /// signal is injected into every panel external so the source panel's
    /// `drag_to` updates the affordance the target panel paints.
    #[must_use]
    pub fn with_drop_preview(mut self, preview: Rc<Signal<Option<DockDropPreview>>>) -> Self {
        self.drop_preview = Some(preview);
        self
    }

    /// Read the panel id this external carries — the payload the
    /// `tear_off` intent + the R742 drag carry.
    #[must_use]
    pub fn panel_id(&self) -> &str {
        &self.panel_id
    }

    /// Diagnostic: a drag session this panel began is in flight.
    #[must_use]
    pub fn is_dragging(&self) -> bool {
        self.dragging.get()
    }

    /// Diagnostic: whether the last release tore the panel off (vs
    /// docked / snapped back).
    #[must_use]
    pub fn tear_off_fired(&self) -> bool {
        self.tear_off_fired.get()
    }

    /// (R1094 §5.16 §5.41 §5.51) Diagnostic: whether the in-flight (or
    /// last) drag tore the panel into a live floating follower (escaped a
    /// drop target mid-drag, emitting [`TEAR_OFF_FOLLOW_EVENT`]).
    #[must_use]
    pub fn detached(&self) -> bool {
        self.detached.get()
    }

    /// (R1129 §5.51.1 §5.38 §2 #7) The panel's persistent dock-lifecycle state
    /// name — `"Docked"` / `"Floating"`, the [`DockPanelState`] mapped through the
    /// §5.16 [`WidgetStateName`] SSOT (so the introspect name and a binding's
    /// `from_name_or_default` read share one variant list). Surfaced as
    /// scene-as-data via `query("lifecycle")`.
    #[must_use]
    pub fn lifecycle_name(&self) -> &'static str {
        self.lifecycle.borrow().state().as_name()
    }

    /// (R1129 §5.51.1) Whether the lifecycle chart is currently floating — the
    /// gate the redock / restore IO replaces the per-gesture [`detached`] bool
    /// with. `Dropped` / `DockBack` only redock a panel the chart records as
    /// having floated; the chart enforces that a never-floated panel stays put.
    ///
    /// R1137 — `RedockArmed` (the floater is over a dock zone) is a floating
    /// SUB-mode, so a panel is "floating" in BOTH `Floating` and `RedockArmed`
    /// (it is a floating window in both; only the over-a-zone hint differs).
    ///
    /// [`detached`]: Self::detached
    fn is_floating(&self) -> bool {
        matches!(
            self.lifecycle.borrow().state(),
            DockPanelState::Floating | DockPanelState::RedockArmed
        )
    }

    /// (R1137 §5.51.1) Whether the floater is ARMED to re-dock — i.e. its live
    /// move is currently over another window's dock zone (the binding paints the
    /// drop preview while armed). A release while armed re-docks; a release while
    /// merely `Floating` (free) keeps the floater where it is.
    fn is_redock_armed(&self) -> bool {
        matches!(self.lifecycle.borrow().state(), DockPanelState::RedockArmed)
    }

    /// (R1129 §5.51.1 §5.38) Drive the lifecycle statechart. `&self` (interior
    /// mutable) so the `&self` [`begin_drag`](External::begin_drag) /
    /// [`settle_cross_window_redock`](Self::settle_cross_window_redock) and the
    /// `&mut self` drag arms all transition it. An event invalid for the current
    /// state (e.g. `Dropped` while `docked`) is the chart's own no-op — the
    /// caller does not pre-check.
    fn send_lifecycle(&self, event: DockPanelEvent) {
        self.lifecycle.borrow_mut().send(event);
    }

    /// (R1134 §5.51.1) Under [`FloatPolicy::Collapse`], remove this panel's leaf so
    /// the neighbours reflow — the topology mirror of the window-side float (the
    /// `tear_off` / `tear_off_follow` intent), capturing the home anchor for a
    /// later dock-back. A no-op under `Placeholder` (the leaf stays = the slot is
    /// preserved) or a tear-off-only external (no coordinator). Called at the float
    /// SETTLE points — `drag_release` escape + the `invoke("tear_off")` float — and
    /// **never** mid-drag ([`drag_to_at`](External::drag_to_at)): removing the leaf
    /// mid-gesture would re-run the external factory and disturb the live drag, so
    /// the slot stays put during the drag and collapses once on release
    /// (release-as-floated).
    fn collapse_leaf_if_policy(&self) {
        if let Some(reorg) = self.reorganizer.as_ref()
            && reorg.float_policy() == FloatPolicy::Collapse
        {
            let _ = reorg.float_out_panel(&self.panel_id);
        }
    }

    /// (R1134 §5.51.1) Under [`FloatPolicy::Collapse`], restore this panel's leaf to
    /// its captured home anchor — the topology mirror of the window-side restore
    /// (the `tear_off_redock` intent), driven at every dock-back point that removes
    /// the float window (snap-back, cancel, `invoke("tear_off")` dock-back). A
    /// no-op under `Placeholder` or when nothing was stashed (the panel never
    /// collapsed), so it is safe to mirror unconditionally wherever the window is
    /// restored.
    fn restore_leaf_home_if_policy(&self) {
        if let Some(reorg) = self.reorganizer.as_ref()
            && reorg.float_policy() == FloatPolicy::Collapse
        {
            let _ = reorg.restore_panel_home(&self.panel_id);
        }
    }

    /// (R1136 §5.51 PR-39) Whether `source_window` is THIS panel's own floating
    /// window — i.e. the in-flight drag is a borderless-floater title-bar WINDOW
    /// MOVE (R1116), not a docked tear-off. The same discriminator the
    /// [`drag_to_at`](External::drag_to_at) window-move branch uses, lifted so the
    /// release / cancel paths agree (a floater move never snaps back home).
    fn is_floater_window_move(&self, source_window: Option<&str>) -> bool {
        self.floating_window.as_deref().is_some()
            && self.floating_window.as_deref() == source_window
    }

    /// (R1136 §5.51 PR-39) End a floater WINDOW MOVE that released without landing
    /// on another window's dock: the floater stays FLOATING at its new position (no
    /// snap-back home). Clears the per-gesture drag diagnostics + re-arms + resets
    /// the desktop-frame anchor, leaving the lifecycle chart untouched (`Floating`).
    fn end_window_move(&self) {
        self.dragging.set(false);
        self.is_drag_armed.set(true);
        self.set_drop_preview(None);
        self.prev_desktop.set(None);
        // R1137 — a stay-floating release left the chart in `redock_armed` if the
        // last move was over a zone; disarm it back to plain `floating` so the
        // lifecycle is consistent (the floater is free again, not armed).
        if self.is_redock_armed() {
            self.send_lifecycle(DockPanelEvent::LeaveZone);
        }
    }

    /// (R1081 §5.51) Classify a resolved [`DropPoint`] into the dock
    /// preview it implies: `None` when the cursor is over no panel, over
    /// this same panel (a self-drop is a no-op), or in a dead zone
    /// ([`DockDropZone::None`]); otherwise `Some` with the target panel +
    /// zone. The single classifier `drag_to` (preview) and `drag_release`
    /// (commit) share so the painted affordance and the applied edit
    /// cannot disagree.
    /// (R1112 §5.51 PR-37) This panel's effective tab-docking policy — the
    /// shared [`DockReorganizer`]'s [`tabbing`](DockReorganizer::tabbing) when
    /// one is wired, else `true` (a tear-off-only panel never reorganizes, so
    /// the policy is moot and the default is harmless).
    fn effective_tabbing(&self) -> bool {
        self.reorganizer.as_ref().is_none_or(|r| r.tabbing())
    }

    /// (R1134 §5.51.1) The torn-slot [`FloatPolicy`] this panel follows — its
    /// shared coordinator's surface policy, or [`FloatPolicy::Placeholder`] for a
    /// tear-off-only external (no coordinator). The value `query("float_policy")`
    /// projects.
    fn effective_float_policy(&self) -> FloatPolicy {
        self.reorganizer
            .as_ref()
            .map_or(FloatPolicy::Placeholder, |r| r.float_policy())
    }

    fn resolve_preview(&self, over: Option<&DropPoint>) -> Option<DockDropPreview> {
        let over = over?;
        // The drop target is the panel ROOT (R1080 marks only the root
        // `.drop_target`), but split defensively at `#` in case a future
        // nested target resolves to a composite tag.
        let target = over.tag.split('#').next().unwrap_or(over.tag.as_str());
        if target == self.panel_id.as_ref() {
            return None;
        }
        // R1112 PR-37 — the tab-docking policy is the dock SURFACE's
        // ([`DockReorganizer::tabbing`]), read through this panel's shared
        // coordinator so the pointer preview, the `drop` RPC, and cross-window
        // redock all honour ONE policy. A split-only surface never classifies
        // the centre as a tabify — the nearest edge wins. A tear-off-only
        // panel (no coordinator) cannot reorganize at all, so the default
        // (tabbing) is moot.
        let zone = dock_drop_zone_normalized_tabbing(
            f64::from(over.x_rel),
            f64::from(over.y_rel),
            self.effective_tabbing(),
        );
        if zone == DockDropZone::None {
            return None;
        }
        Some(DockDropPreview {
            source: self.panel_id.to_string(),
            target: target.to_string(),
            zone,
        })
    }

    /// (R1081 §5.51) Write the shared drop-preview, deduping against the
    /// current value so a stationary cursor mid-drag does not churn
    /// repaints. No-op when no preview signal is wired.
    fn set_drop_preview(&self, preview: Option<DockDropPreview>) {
        if let Some(sig) = &self.drop_preview {
            if sig.get() != preview {
                sig.set(preview);
            }
        }
    }

    /// Enqueue the `tear_off` intent — the binding's reducer turns it
    /// into a `WindowSpec` push. Called by `drag_release` on an
    /// escape-drop and by the direct `invoke("tear_off")` channel.
    fn enqueue_tear_off(&self) {
        self.pending_intents.borrow_mut().push_back(Intent {
            tag: Cow::Borrowed(TEAR_OFF_EVENT),
            payload: IntrospectValue::Text(self.panel_id.to_string()),
        });
    }

    /// (R1094 §5.16 §5.41 §5.51) Enqueue the ensure+position follow intent
    /// for a live tear-off drag move at `cursor` (window-logical). The
    /// binding reducer creates the panel's floating window if absent and
    /// writes its position; non-toggling so repeated moves only reposition.
    /// (R1107 §5.51) `source_window` names the window the cursor is measured
    /// in (`DragUpdate::source_window`) so the binding converts to a desktop
    /// position via the RIGHT origin.
    fn enqueue_tear_off_follow(&self, cursor: (f64, f64), source_window: Option<&str>) {
        self.pending_intents.borrow_mut().push_back(Intent {
            tag: Cow::Borrowed(TEAR_OFF_FOLLOW_EVENT),
            payload: tear_off_follow_payload(&self.panel_id, cursor, source_window),
        });
    }

    /// (R1118 §5.51 PR-38) Enqueue a [`WINDOW_MOVE_EVENT`] — the grab-relative
    /// displacement for dragging this panel's OWN floating window by its title
    /// bar. Distinct from the tear-off follow: the binding moves the window BY
    /// this delta, it does not place it at a cursor.
    fn enqueue_window_move(&self, delta: (f64, f64)) {
        self.pending_intents.borrow_mut().push_back(Intent {
            tag: Cow::Borrowed(WINDOW_MOVE_EVENT),
            payload: window_move_payload(&self.panel_id, delta),
        });
    }

    /// (R1094 §5.16 §5.41 §5.51) Enqueue the remove-only redock / restore
    /// intent. The binding reducer removes the panel's floating window if
    /// present (idempotent no-op otherwise).
    fn enqueue_tear_off_redock(&self) {
        self.pending_intents.borrow_mut().push_back(Intent {
            tag: Cow::Borrowed(TEAR_OFF_REDOCK_EVENT),
            payload: IntrospectValue::Text(self.panel_id.to_string()),
        });
    }

    /// (R1100 §5.51 §5.16 §2 #7 PR-33) Enqueue the cross-window dock-at redock:
    /// the floated panel was released over `target_window`'s dock zone `point`.
    /// The payload carries the panel id, the target window, and the drop zone
    /// (`point.tag` + the normalised `x_rel`/`y_rel` the same-window drop's
    /// zone-geometry SSOT classifies). What the binding reducer DOES with it is
    /// binding-defined: it redocks the panel into `target_window`, and a
    /// topology-bearing binding MAY place it at the classified zone — but the
    /// flat `hello-dock-panels` reducer (R1103) just drops the floating window so
    /// the panel re-installs in its fixed home slot; honouring the zone is its
    /// deferred slice-4 dock-at-zone (it needs a `DockTopology`, which that flat
    /// demo lacks). Distinct from [`Self::enqueue_tear_off_redock`]
    /// (remove-only restore) — see [`TEAR_OFF_REDOCK_AT_EVENT`].
    fn enqueue_tear_off_redock_at(&self, target_window: &str, point: &DropPoint) {
        let payload = serde_json::json!({
            "panel": self.panel_id.as_ref(),
            "window": target_window,
            "target": point.tag,
            "x_rel": point.x_rel,
            "y_rel": point.y_rel,
        });
        // R1102 §2 #7 — persist the cross-window redock for `query("redock_at")`;
        // the emitted intent itself is transient (drained into the reducer).
        *self.last_redock_at.borrow_mut() = Some(payload.clone());
        self.pending_intents.borrow_mut().push_back(Intent {
            tag: Cow::Borrowed(TEAR_OFF_REDOCK_AT_EVENT),
            payload: IntrospectValue::Json(payload),
        });
    }

    /// R1103 §5.51 §2 #7 PR-33 — commit a cross-window dock-at + settle the
    /// gesture latches. The single source of truth shared by the live
    /// [`drag_release_at`](External::drag_release_at) cross-window arm and the
    /// AI-primary [`TEAR_OFF_REDOCK_AT_EVENT`] invoke channel: both enqueue the
    /// dock-at into `window`'s zone `point`, then clear the in-flight drag —
    /// the floating follower this drag tracked is consumed by the dock-at, so
    /// `detached` / `tear_off_fired` reset and the panel re-arms for the next
    /// press. Returns nothing; the two callers differ only in their own
    /// return type.
    fn settle_cross_window_redock(&self, window: &str, point: &DropPoint) {
        self.enqueue_tear_off_redock_at(window, point);
        // R1129 §5.51.1 — the floating panel re-docked into another window's
        // zone: drive the lifecycle `dropped` (floating → docked). The dock-at IO
        // itself is unconditional (the panel WAS floating to reach this arm), so
        // the chart just records the transition + exposes it via introspection.
        self.send_lifecycle(DockPanelEvent::Dropped);
        self.dragging.set(false);
        self.set_drop_preview(None);
        self.is_drag_armed.set(true);
        self.detached.set(false);
        self.tear_off_fired.set(false);
    }
}

impl External for DockPanelExternal {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui, Backend::Rpc], BackendFallback::Skip)
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }

    /// (R1081 §5.51) R742 drag-source arm. The `InputRouter` calls this
    /// right after the `PointerDown` dispatch (which set
    /// [`is_drag_armed`](Self::is_drag_armed) via `invoke("send", …)`),
    /// so a press on the header opens a session and a press on the
    /// content body does not. Returns a [`DragPayload`] of kind
    /// [`DOCK_PANEL_DRAG_KIND`] carrying the panel id. Called on `&self`
    /// (arming is observation of the press the send-arm already recorded)
    /// — the [`dragging`](Self::dragging) / [`tear_off_fired`](Self::tear_off_fired)
    /// diagnostics are interior-mutable.
    fn begin_drag(&self) -> Option<DragPayload> {
        if !self.is_drag_armed.get() {
            return None;
        }
        self.dragging.set(true);
        self.tear_off_fired.set(false);
        // R1093 — a fresh gesture clears the previous drag's last cursor.
        self.drag_cursor.set(None);
        // R1094 — and the live-follower latch (this gesture has not yet
        // escaped a drop target). R1101 — detach is now driven by the router's
        // per-move `became_drag` verdict, so there is no private latch to reset.
        self.detached.set(false);
        // R1102 — and the last cross-window redock diagnostic.
        *self.last_redock_at.borrow_mut() = None;
        // R1107 — and the last follow-drag source window.
        *self.last_source_window.borrow_mut() = None;
        // R1120 — and the window-move desktop anchor (re-seeded on the first move).
        self.prev_desktop.set(None);
        Some(DragPayload {
            kind: Cow::Borrowed(DOCK_PANEL_DRAG_KIND),
            value: IntrospectValue::Text(self.panel_id.to_string()),
        })
    }

    /// (R1081 §5.51) R742 live update — resolve the drop the cursor is
    /// over into the shared [`DockDropPreview`] so the target panel paints
    /// the zone affordance. `None` over no actionable panel.
    fn drag_to(&mut self, _payload: &DragPayload, over: Option<DropPoint>) {
        self.set_drop_preview(self.resolve_preview(over.as_ref()));
    }

    /// (R1081 §5.51) R742 drop commit. Over a *different* panel with a
    /// valid zone and an attached coordinator → dock (split / swap)
    /// through the shared [`DockReorganizer`]; over **no** panel (cursor
    /// escaped every drop target) → tear off into a floating window;
    /// anything else → a no-op snap-back. A self-drop / dead-zone / panel
    /// drop without a coordinator does NOT tear off (only an escape-drop
    /// floats), so a click on the header (released over its own panel)
    /// snaps back and the router's trailing click still fires.
    fn drag_release(&mut self, _payload: &DragPayload, over: Option<DropPoint>) {
        self.dragging.set(false);
        self.set_drop_preview(None);
        self.is_drag_armed.set(true);
        // 1. Dock: a valid drop over another panel, with a coordinator.
        if let (Some(reorganizer), Some(preview)) = (
            self.reorganizer.as_ref(),
            self.resolve_preview(over.as_ref()),
        ) {
            self.tear_off_fired.set(false);
            // R1094/R1129 §5.51.1 — the floater landed back on a dock zone:
            // drive the lifecycle `dropped` (floating → docked). If the drag had
            // torn the panel into a live floating follower, remove that window
            // first so the reorganizer re-places a single panel (redock). The
            // chart — not the per-gesture `detached` bool — is the IO gate: a drag
            // that never escaped is still `docked`, so `dropped` is inert and no
            // redock window-removal fires.
            let was_floating = self.is_floating();
            self.send_lifecycle(DockPanelEvent::Dropped);
            if was_floating {
                self.enqueue_tear_off_redock();
            }
            // `resolve_preview` already rejected the dead zone, so
            // `intent_for_zone` is `Some`; the `if let` keeps the SSOT
            // mapping panic-free. Dropping the `Result` is intentional: a
            // rejected apply (stale id / collision) leaves the topology
            // unchanged and `apply_intent` itself records `"rejected: …"`
            // on `last_outcome`, so an AI client observes the failure with
            // no per-caller bookkeeping here.
            if let Some(intent) = intent_for_zone(&preview.source, &preview.target, preview.zone) {
                let _ = reorganizer.apply_intent(&intent);
            }
            self.detached.set(false);
            return;
        }
        // 2. Escape-drop → the panel floats. The live follow already
        // created + positioned the window during the drag; the release
        // emits a final ensure+position at the release cursor (idempotent)
        // so a degenerate gesture whose only escaped sample is the release
        // still floats. Non-toggling — it cannot remove the window the
        // follow created (the R1071-R1078 double-toggle lesson). The
        // cursor-less fallback keeps the legacy `tear_off` toggle so an
        // escape still floats without a forwarded cursor. It runs when no
        // `_at` ever ran — pre-R1093 unit paths / direct `drag_release`, OR
        // the genuinely-degenerate router edge of press -> CursorLeft (button
        // held, zero intervening moves) -> outside release, where the router
        // has already dropped `cursors[id]`. The toggle is idempotent +
        // graceful there (it cannot orphan); live pointer use essentially
        // always has a forwarded cursor and takes the follow arm above.
        if over.is_none() {
            if let Some(cursor) = self.drag_cursor.get() {
                // R1107.1 — the final release-follow carries the SAME source
                // window the gesture's moves recorded (`drag_to_at` /
                // `drag_release_at` stamp `last_source_window`). `drag_release`
                // is cursor-less so it cannot read the live `DragUpdate`, but
                // the recorded source IS the right one (a drag is captured by
                // one window's router for its whole life). A truly-degenerate
                // gesture with zero recorded moves leaves it `None` → the
                // binding's primary window. (R1107 originally hard-coded
                // `None` here, dropping the source on the release arm — the
                // R1107.1 review-clearance fix.)
                let source = self.last_source_window.borrow().clone();
                self.enqueue_tear_off_follow(cursor, source.as_deref());
            } else {
                self.enqueue_tear_off();
            }
            // R1129 §5.51.1 — the drag escaped every drop target: drive the
            // lifecycle `escaped` (docked → floating). Idempotent with the
            // `drag_to_at` escape — `Escaped` has no transition out of `floating`,
            // so a gesture that already floated mid-drag is a no-op here.
            self.send_lifecycle(DockPanelEvent::Escaped);
            // R1134 §5.51.1 — the gesture SETTLED as floated: under Collapse remove
            // the leaf now (release-as-floated), so the slot reflows on release, not
            // mid-drag. No-op under Placeholder (the slot stays).
            self.collapse_leaf_if_policy();
            self.detached.set(true);
            self.tear_off_fired.set(true);
            return;
        }
        // 3. Snapped back over a panel / dead zone / no coordinator. A drag
        // that had floated returns home → `dock_back` (restore): remove the
        // floating window this gesture created; a drag that never escaped is the
        // plain snap-back (no commit, today's behaviour). R1129 §5.51.1 — the
        // chart drives the restore: `dock_back` is inert while `docked`, so the
        // never-floated snap-back transitions nothing and removes no window.
        let was_floating = self.is_floating();
        self.send_lifecycle(DockPanelEvent::DockBack);
        if was_floating {
            self.enqueue_tear_off_redock();
            // R1134 §5.51.1 — mirror the window restore on the topology: under
            // Collapse re-insert the leaf at its home anchor. A no-op when nothing
            // was stashed (a snap-back's float was the mid-drag follow, which never
            // removed the leaf under release-as-floated).
            self.restore_leaf_home_if_policy();
        }
        self.detached.set(false);
        self.tear_off_fired.set(false);
    }

    /// (R1093 §5.15 §5.51 §2 #7) Record the absolute window-logical cursor
    /// the router forwards, drive threshold-based tear-off from the router's
    /// click-vs-drag verdict, then delegate to the cursor-less
    /// [`drag_to`](Self::drag_to) so the existing preview/dock logic is
    /// unchanged. The recorded cursor is exposed as scene-as-data via
    /// `query("drag_cursor")`.
    fn drag_to_at(&mut self, payload: &DragPayload, update: &DragUpdate) {
        self.drag_cursor.set(Some(update.cursor));
        // R1107 §5.51 §2 #7 — record the window this move was measured in so an
        // AI observes it via `query("source_window")` and the binding converts
        // the cursor to a desktop position via the right origin.
        *self.last_source_window.borrow_mut() = update.source_window.map(str::to_owned);
        // R1116 §5.51 PR-38 — WINDOW MOVE. A drag IN this panel's OWN floating
        // window (`source_window` == the binding-declared `floating_window`) is
        // the borderless floater's title-bar drag: it MOVES the window, distinct
        // from a dock tear-off. A floating window has nothing to "escape" — the
        // cursor stays on its own title bar, so the router's hover-fallback
        // `over` is never `None` and the R1110 escape can never fire (that is why
        // a tear-off-style path could not move it). Every move repositions; no
        // detach / escape / drop-preview. The docked tear-off path below is
        // untouched (its `source_window` is the dock window).
        if let Some(fw) = self.floating_window.as_deref() {
            if update.source_window == Some(fw) {
                // R1120 PR-39 — compute the move in the window-INDEPENDENT DESKTOP
                // frame via the source window's ACTUAL origin, then emit the FRAME
                // INCREMENT. Measuring in the moving-window frame (R1118) closed a
                // feedback loop with the WM apply-lag that JITTERED the live drag;
                // the desktop frame has no feedback (see `window_move_delta`).
                let actual = update
                    .source_window_origin
                    .map(|(x, y)| (f64::from(x), f64::from(y)));
                let (delta, new_prev) = window_move_delta(
                    actual,
                    update.cursor,
                    update.press_cursor,
                    self.prev_desktop.get(),
                );
                self.prev_desktop.set(Some(new_prev));
                self.enqueue_window_move(delta);
                // R1137 §5.51.1 — drive the redock-armed lifecycle from the
                // shell-resolved cross-window over: a floater whose move is over
                // ANOTHER window's dock zone is ARMED to redock (the binding paints
                // the drop preview while armed); leaving every zone disarms it (a
                // plain window move). Edge-triggered (like the R1129 escape) so the
                // chart transitions once per enter / leave, not every move. A
                // release while armed redocks; while free, it stays floating.
                let over_zone = update.over_window.is_some();
                if over_zone && !self.is_redock_armed() {
                    self.send_lifecycle(DockPanelEvent::OverZone);
                } else if !over_zone && self.is_redock_armed() {
                    self.send_lifecycle(DockPanelEvent::LeaveZone);
                }
                return;
            }
        }
        // R1100 PR-33 — a cross-window `over` (`over_window: Some`) names ANOTHER
        // window's dock zone; do NOT paint it into THIS window's drop preview
        // (the cross-window redock affordance is the target window's overlay, a
        // later slice). A same-window `over` keeps the existing preview path.
        // R1110 reads this BEFORE the detach decision below, so it is computed
        // first now.
        let same_window_over = if update.over_window.is_some() {
            None
        } else {
            update.over.clone()
        };
        // R1110 §5.51 PR-36 — detach-on-ESCAPE, not detach-on-threshold. R1097/
        // R1101 detached the instant the router called the press a drag, EVEN
        // while the cursor was still over a same-window dock zone, so an in-dock
        // reorder / split drag floated a follower and flickered and the drop
        // preview oscillated. The textbook desktop-dock model (VS Code / Blender
        // / a real browser tab: reorder WITHIN the tab bar, detach only when
        // dragged OUT) floats only once the cursor leaves every same-window dock
        // zone. So detach when the drag verdict lands AND the cursor is over no
        // same-window drop target (`same_window_over` None = escaped this
        // window's dock surface; a cross-window `over` counts as escaped too).
        // While still over a zone the move stays docked and the `drag_to` below
        // shows the reorganize / split preview, which the release then applies.
        // `detached` is a one-way latch per gesture: once escaped it stays
        // floating, so coming back over a zone shows the PR-33 redock preview
        // (follow + preview coexist) rather than re-docking mid-drag. Consuming
        // the router's verdict (rather than re-deriving distance) stays
        // single-SSOT: the router measures from the real press point.
        if update.became_drag && same_window_over.is_none() {
            // R1129 §5.51.1 — drive the lifecycle `escaped` (docked → floating)
            // on the RISING edge of the detach latch, so the chart transitions
            // exactly once per tear-off rather than on every escaped move
            // (`Escaped` is inert once `floating`, but the edge keeps it crisp +
            // avoids per-move chart churn).
            if !self.detached.get() {
                self.send_lifecycle(DockPanelEvent::Escaped);
            }
            self.detached.set(true);
            // R1134 §5.51.1 — deliberately NO collapse here. Under Collapse the
            // topology leaf is removed at the SETTLE (`drag_release` escape), never
            // mid-drag: removing it now would re-run the external factory and
            // disturb this live drag session, so the slot stays put (placeholder-
            // style) during the drag and reflows once on release (release-as-floated).
        }
        // While detached, the follower tracks the cursor on EVERY move (the
        // ensure+position follow), whether or not it is back over a zone — and
        // the preview below still highlights a dock zone underneath it, so
        // follow + redock preview coexist (PR-33 §2). A press still docked over
        // a zone (or in the `became_drag` false click dead-zone) does neither.
        if self.detached.get() {
            // R1107 §5.51 — carry the SOURCE window (the one the router that
            // drove this update belongs to) so the binding converts the
            // window-logical cursor to a desktop position via the right origin.
            // A tear-off jumps the panel to the cursor (the window-move
            // grab-offset path returned early above).
            self.enqueue_tear_off_follow(update.cursor, update.source_window);
        }
        self.drag_to(payload, same_window_over);
    }

    /// (R1093 §5.15 §5.51 §2 #7) Record the release cursor, then commit the
    /// drop. R1100 PR-33: a cross-window release (`over_window: Some` — the
    /// cursor escaped this floating window into ANOTHER window's dock zone)
    /// redocks the panel INTO that window at the zone via
    /// [`TEAR_OFF_REDOCK_AT_EVENT`], distinct from a same-window drop (which
    /// delegates to the cursor-less [`drag_release`](Self::drag_release): own
    /// reorganize / escape-float / snap-back). The cursor persists after the
    /// gesture so an AI can read where the drop landed.
    fn drag_release_at(&mut self, payload: &DragPayload, update: &DragUpdate) {
        self.drag_cursor.set(Some(update.cursor));
        // R1107.1 — stamp the release's source window so the cursor-less
        // `drag_release` escape-follow below reads it (a degenerate release
        // with no preceding move still names the window it released in).
        *self.last_source_window.borrow_mut() = update.source_window.map(str::to_owned);
        // R1100 PR-33 — cross-window dock-at: the drop resolved in a DIFFERENT
        // window's dock zone. Re-insert the panel there + drop its floating
        // window. This is NOT a same-window reorganize (the panel is not in
        // this window's topology) — it is the dock-at redock the per-window
        // router could never resolve.
        if let (Some(target_window), Some(point)) = (update.over_window, update.over.as_ref()) {
            self.settle_cross_window_redock(target_window, point);
            return;
        }
        // R1136 §5.51 PR-39 — a borderless FLOATER's title-bar drag is a WINDOW
        // MOVE (R1116). A release that did NOT land on another window's dock (the
        // redock above) leaves the floater FLOATING at its moved position — just
        // end the gesture. It must NOT snap back home the way a DOCKED panel's
        // tear-off does (the `drag_release` arm below): repositioning a floating
        // window is not a redock, and the cursor sits over the floater's OWN
        // header (a self-drop) the whole move, which would otherwise fall to the
        // snap-back-home arm. The lifecycle chart stays `Floating`.
        if self.is_floater_window_move(update.source_window) {
            self.end_window_move();
            return;
        }
        self.drag_release(payload, update.over.clone());
    }

    /// (R937.1 §5.51) R742 drag abort — the OS revoked the gesture.
    /// Discard it: clear the preview + diagnostics WITHOUT committing a
    /// dock or a tear-off.
    fn drag_cancel(&mut self, _payload: &DragPayload) {
        self.dragging.set(false);
        self.tear_off_fired.set(false);
        self.set_drop_preview(None);
        self.is_drag_armed.set(true);
        // R1136 §5.51 PR-39 — a cancelled FLOATER WINDOW MOVE stays floating (no
        // snap-back home), like a released one: repositioning a floating window is
        // not a redock. `drag_cancel` carries no `DragUpdate`, so the floater is
        // recognised from the last recorded move source (`last_source_window`,
        // stamped each `drag_to_at`). The chart stays `Floating`.
        let last_source = self.last_source_window.borrow().clone();
        if self.is_floater_window_move(last_source.as_deref()) {
            self.prev_desktop.set(None);
            self.detached.set(false);
            return;
        }
        // R1094/R1129 §5.51.1 — a cancelled drag that floated restores home
        // (`dock_back`): drive the chart + remove the follow window it created. A
        // drag that never escaped is `docked`, so `dock_back` is inert and no
        // window is removed.
        let was_floating = self.is_floating();
        self.send_lifecycle(DockPanelEvent::DockBack);
        if was_floating {
            self.enqueue_tear_off_redock();
            // R1134 §5.51.1 — mirror the window restore: under Collapse re-insert the
            // leaf at its home anchor (a no-op when nothing was stashed).
            self.restore_leaf_home_if_policy();
        }
        self.detached.set(false);
    }

    fn is_dirty(&self) -> bool {
        !self.pending_intents.borrow().is_empty()
    }

    fn drain_intents(&mut self, sink: &mut dyn FnMut(Intent)) {
        let mut queue = self.pending_intents.borrow_mut();
        while let Some(intent) = queue.pop_front() {
            sink(intent);
        }
    }

    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        Some(self)
    }

    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        Some(self)
    }
}

impl ExternalIntrospect for DockPanelExternal {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(&[
            ("panel_id", "string"),
            // R1111 §5.51 §2 #7 PR-37 — whether a centre drop tabifies. A
            // split-only consumer sets it false; an AI discovers the policy.
            ("tabbing", "bool"),
            // R1134 §5.51.1 §2 #7 — the torn-slot policy (`"placeholder"` /
            // `"collapse"`) this panel's float follows.
            ("float_policy", "string"),
            ("dragging", "bool"),
            ("tear_off_fired", "bool"),
            // R1081 §5.51 — the live drop the in-flight drag is over
            // (`{source, target, zone}` or null), so an AI agent observes
            // the same drop-zone affordance the user sees.
            ("drop_preview", "json"),
            // R1093 §5.15 §5.51 §2 #7 — the absolute window-logical cursor
            // of the in-flight/last drag (`[x, y]` or null), so an AI reads
            // the live pointer the router forwards even when the cursor has
            // escaped every tagged region (the tear-off case `drop_preview`
            // goes null on).
            ("drag_cursor", "json"),
            // R1094 §5.16 §5.41 §5.51 — whether the in-flight/last drag
            // tore the panel into a live floating follower (escaped a drop
            // target). Paired with `scene/windows` (the floating window's
            // live declared position), an AI observes a tear-off + follow.
            ("detached", "bool"),
            // R1102 §5.51 §2 #7 PR-33 — the last cross-window dock-at
            // (`{panel, window, target, x_rel, y_rel}` or null), so an AI
            // observes that a drag redocked into ANOTHER window's zone — the
            // live firing of the R1100 contract the R1102 shell wiring enables.
            ("redock_at", "json"),
            // R1107 §5.51 §2 #7 — the window the last follow-drag move was
            // measured in (`"main"` / a `torn-<panel>` id / null).
            ("source_window", "string"),
            ("send", "string"),
            // R683.C §5.16 §5.49 — direct tear-off invoke channel.
            // See `invoke` rustdoc.
            (TEAR_OFF_EVENT, "string"),
            // R1103 §5.51 §2 #7 PR-33 — direct cross-window redock invoke
            // (the AI-primary driver for the executable floater→slot-window
            // case). JSON payload `{window, target, x_rel, y_rel}`.
            (TEAR_OFF_REDOCK_AT_EVENT, "json"),
        ])
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "panel_id" => Some(IntrospectValue::Text(self.panel_id.to_string())),
            // R1112 §5.51 §2 #7 PR-37 — the effective tab-docking policy this
            // panel follows (its shared coordinator's surface policy).
            "tabbing" => Some(IntrospectValue::Bool(self.effective_tabbing())),
            // R1134 §5.51.1 §2 #7 — the effective torn-slot policy this panel
            // follows (its coordinator's surface policy).
            "float_policy" => Some(IntrospectValue::Text(
                self.effective_float_policy().as_str().to_string(),
            )),
            "dragging" => Some(IntrospectValue::Bool(self.is_dragging())),
            "tear_off_fired" => Some(IntrospectValue::Bool(self.tear_off_fired())),
            // R1081 §5.51 — the shared live preview (any panel's external
            // reads the one shared signal). Null when no drag is in
            // flight / no preview signal is wired.
            "drop_preview" => Some(drop_preview_introspect(self.drop_preview.as_ref())),
            // R1093 §5.15 §5.51 §2 #7 — the forwarded absolute cursor.
            "drag_cursor" => Some(drag_cursor_introspect(self.drag_cursor.get())),
            // R1094 §5.16 §5.41 §5.51 — the live-follower latch.
            "detached" => Some(IntrospectValue::Bool(self.detached())),
            // R1129 §5.51.1 §5.38 §2 #7 — the persistent dock-lifecycle state
            // (`"Docked"` / `"Floating"`, the `WidgetStateName` SSOT), the SCXML
            // chart the float/redock/restore decision now lives in (was implicit
            // in the binding's window signal + the per-gesture `detached` bool).
            "lifecycle" => Some(IntrospectValue::Text(self.lifecycle_name().to_string())),
            // R1102 §5.51 §2 #7 PR-33 — the last cross-window dock-at payload
            // (null before any cross-window redock this gesture).
            "redock_at" => Some(
                self.last_redock_at
                    .borrow()
                    .clone()
                    .map_or(IntrospectValue::Null, IntrospectValue::Json),
            ),
            // R1107 §5.51 §2 #7 — the window the last follow-drag was measured in.
            "source_window" => Some(
                self.last_source_window
                    .borrow()
                    .clone()
                    .map_or(IntrospectValue::Null, IntrospectValue::Text),
            ),
            _ => None,
        }
    }

    fn intervene(&mut self, path: &str, _value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            // Every slot is framework-owned. AI clients drive the gesture
            // through the `invoke("send", ...)` / `invoke("tear_off")`
            // channels + the shared coordinator — not by intervening on
            // dragging / tear_off_fired / drop_preview / drag_cursor /
            // detached directly.
            "panel_id" | "tabbing" | "float_policy" | "dragging" | "tear_off_fired"
            | "drop_preview" | "drag_cursor" | "detached" | "redock_at" | "source_window"
            | "lifecycle" => Err(InterveneError::ReadOnly),
            _ => Err(InterveneError::UnknownPath),
        }
    }

    /// R51.41 §5.15 §5.35 — framework synthetic event channel.
    ///
    /// R683.C R51.42 §5.35 — the `InputRouter` dispatches the wire
    /// payload as `"<sub_index>:<EventName>"` when the External lives
    /// at the panel primary tag and the paint hit-test resolved a
    /// composite tag (`"inspector#header"` → primary `"inspector"` +
    /// sub-index `"header"`). The sub-index discriminator distinguishes
    /// header presses (arm the drag) from content presses (disarm so
    /// drags through the content body do not fire `tear_off`).
    ///
    /// Wire shape table:
    /// * `"header:PointerDown"` / `"header:PointerEnter"` — arm the
    ///   drag. Pre-R683.C unit tests call the bare variants
    ///   (`"PointerDown"` etc.) and the construction default keeps
    ///   `is_drag_armed = true`, so the legacy direct-invoke path
    ///   still arms — no test churn.
    /// * `"content:PointerDown"` / `"content:PointerEnter"` — disarm
    ///   the drag so a press on the content body does not propagate
    ///   into tear-off when the user drags through the content area.
    /// * `"header:PointerUp"` / `"PointerUp"` / `"header:PointerCancel"`
    ///   / `"PointerCancel"` — re-arm (`is_drag_armed = true`) so the next
    ///   press starts fresh. The real drag teardown is the R742
    ///   `drag_release` / `drag_cancel`; this send path only fires for the
    ///   click-up case (a press-release in place the router replays as a
    ///   trailing `PointerUp`).
    /// * `"header:PointerLeave"` / `"content:PointerLeave"` / bare
    ///   `"PointerLeave"` — no-op (the hover-leave does not affect arming).
    /// * Other / unknown event names — `InvokeError::UnknownPath`.
    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        // R683.C §5.16 §5.49 — direct tear-off invoke. The pointer path
        // requires a populated `InputRouter::last_paint_scene` for the
        // addressed window; on a freshly-spawned floating window under
        // headless RPC the router has no paint scene until winit's next
        // paint cycle. This channel bypasses the pointer drag so AI
        // clients drive the dock toggle without depending on winit's paint
        // timing.
        //
        // R1130 §5.51.1 — the toggle is now CHART-DIRECTED: the lifecycle chart
        // owns the direction (`docked` → float / `floating` → dock-back) and the
        // External emits the matching DIRECTED intent, so the chart and the
        // binding's window list cannot diverge. R1129 mapped this channel only to
        // `Escaped`, so a 2nd `tear_off` invoke drove the chart `Escaped`→inert
        // (stayed floating) while the binding reducer's `tear_off` toggle docked
        // the window — the chart said floating, the window said docked. Now a
        // floating panel's `tear_off` dock-backs through the chart + the
        // `tear_off_redock` intent (the same restore the R742 snap-back drives),
        // and a docked panel floats through `Escaped` + the `tear_off` intent.
        if path == TEAR_OFF_EVENT {
            if self.is_floating() {
                // floating → dock-back (restore home): the directed restore intent.
                self.send_lifecycle(DockPanelEvent::DockBack);
                self.enqueue_tear_off_redock();
                // R1134 §5.51.1 — under Collapse re-insert the leaf at its home
                // anchor (the topology mirror of the window restore).
                self.restore_leaf_home_if_policy();
                self.tear_off_fired.set(false);
            } else {
                // docked → float (tear-off): the same transition the R742
                // escape-drop drives.
                self.send_lifecycle(DockPanelEvent::Escaped);
                self.enqueue_tear_off();
                // R1134 §5.51.1 — a discrete float settle (not mid-drag): under
                // Collapse remove the leaf now so the slot reflows.
                self.collapse_leaf_if_policy();
                self.tear_off_fired.set(true);
            }
            self.dragging.set(false);
            self.is_drag_armed.set(true);
            return Ok(IntrospectValue::Null);
        }
        // R1103 §5.51 §2 #7 PR-33 — direct cross-window redock invoke
        // (mutation slice 3), the AI-primary driver for the executable
        // case (a FLOATING panel returning into a slot-bearing window).
        // The live-pointer floater→main drag is blocked: the source
        // floater follows the cursor (R1094 `tear_off_follow`), which the
        // R1095 moving-source desktop-conversion carry has not yet cleared,
        // so the abs cursor double-counts the moving origin. This channel
        // bypasses the live pointer the same way the `tear_off` arm above
        // does — same intent + payload as the R742 `drag_release_at`
        // cross-window arm, no winit paint-timing dependency. Payload:
        // `{window, target, x_rel, y_rel}` — the target (slot-bearing)
        // window + the dock zone the redock lands on. `target`/`x_rel`/
        // `y_rel` default to the zone centre when omitted.
        if path == TEAR_OFF_REDOCK_AT_EVENT {
            let IntrospectValue::Json(obj) = args else {
                return Err(InvokeError::TypeMismatch);
            };
            let window = obj
                .get("window")
                .and_then(serde_json::Value::as_str)
                .ok_or(InvokeError::TypeMismatch)?;
            let point = DropPoint {
                tag: obj
                    .get("target")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                x_rel: json_rel(&obj, "x_rel"),
                y_rel: json_rel(&obj, "y_rel"),
            };
            // Same dock-at + latch settle the live `drag_release_at`
            // cross-window arm runs — one SSOT, no pointer dependency.
            self.settle_cross_window_redock(window, &point);
            return Ok(IntrospectValue::Null);
        }
        if path != "send" {
            return Err(InvokeError::UnknownPath);
        }
        let raw = args.as_str().ok_or(InvokeError::TypeMismatch)?;
        // Split `"sub_index:Event[:mods]"` into `(Some(sub_index), Event)`
        // or `(None, raw_event)` if no `:` separator is present — via the
        // R880.1 `split_send_payload` `:` grammar SSOT, so a held-modifier
        // release ("t0:PointerUp:c") still re-arms (the hand-rolled
        // split_once read "PointerUp:c" as the event name and returned
        // UnknownPath, skipping the Up arm).
        let (sub_index, event_name) = match pinion_core::composite_tag::split_send_payload(raw) {
            Some((sub, ev, _mods)) => (Some(sub), ev),
            None => (None, raw),
        };
        match PointerWireEvent::from_wire_name(event_name) {
            Some(PointerWireEvent::Up | PointerWireEvent::Cancel) => {
                self.is_drag_armed.set(true);
                Ok(IntrospectValue::Null)
            }
            Some(PointerWireEvent::Down | PointerWireEvent::Enter) => {
                match sub_index {
                    Some("header") | None => self.is_drag_armed.set(true),
                    _ => self.is_drag_armed.set(false),
                }
                Ok(IntrospectValue::Null)
            }
            Some(PointerWireEvent::Leave) => Ok(IntrospectValue::Null),
            None => Err(InvokeError::UnknownPath),
        }
    }
}

#[cfg(test)]
mod tests {
    //! R683.B §5.16 (R1081 §5.51 R742) — Dock-panel paint + R742 drag
    //! wire tests.
    //!
    //! Pins the load-bearing invariants the dock consumers rely on:
    //!
    //! 1. **Paint shape**: outer Container carries `tag` + 2 children
    //!    (header strip + content wrapper). Header tagged `{tag}#header`,
    //!    content tagged `{tag}#content`. With an active drop zone a
    //!    third out-of-flow overlay child is appended.
    //! 2. **Header height / text**: header child's layout matches
    //!    `header_height_px`; header holds the title `TextNode`.
    //! 3. **`.drop_target(true)`**: every panel root opts in for the
    //!    R1080 router climb.
    //! 4. **R742 `begin_drag` arm gate**: a header press opens a session,
    //!    a content press does not.
    //! 5. **`drag_to` preview**: writes the shared `DockDropPreview` for
    //!    the target panel; a self-hover clears it.
    //! 6. **`drag_release` outcome**: a valid drop over another panel
    //!    docks via the shared coordinator; an escape-drop (`over` =
    //!    None) tears off; a self / dead-zone / no-coordinator drop
    //!    snaps back (no tear-off).
    //! 7. **`drag_cancel`**: discards without committing; clears preview.
    //! 8. **Shared coordinator**: two panel externals + the invoke
    //!    external mint from one `split_seq` (no collision).
    //! 9. **Introspect schema + query**: `panel_id` / `dragging` /
    //!    `tear_off_fired` / `drop_preview` queryable.
    //! 10. **Composite tag format**: `{tag}#header` / `{tag}#content`.

    use super::{
        CONTENT_TAG_SUFFIX, DOCK_DROP_PREVIEW_TAG, DockDropZone, DockNode, DockPanelExternal,
        DockPanelStyle, DockReorganizer, DockTopology, FloatPolicy, HEADER_TAG_SUFFIX,
        TEAR_OFF_EVENT, TEAR_OFF_FOLLOW_EVENT, TEAR_OFF_REDOCK_AT_EVENT, TEAR_OFF_REDOCK_EVENT,
        WINDOW_MOVE_EVENT, composite_tag, dock_drop_preview_overlay, dock_drop_zone_highlight,
        dock_tablist_access_nodes, view_dock_panel, window_move_delta,
    };
    use crate::tabs::composite_tab_tag;
    use pinion_a11y::AccessNode;
    use pinion_core::external::{
        DragPayload, DragUpdate, DropPoint, External, ExternalIntrospect, InterveneError,
        IntrospectValue, InvokeError,
    };
    use pinion_core::intent::Intent;
    use pinion_core::reactive::{Owner, Signal};
    use pinion_core::scene::{ContainerNode, Rect, Scene};
    use pinion_core::theme::Theme;
    use std::rc::Rc;

    const PANEL_TAG: &str = "test_panel";

    fn run_in_owner<R>(f: impl FnOnce() -> R) -> R {
        Owner::new().run(f)
    }

    fn empty_content() -> Scene {
        Scene::Container(ContainerNode::new(vec![]).with_tag("test_panel_content_payload"))
    }

    fn theme_light() -> Theme {
        Theme::light()
    }

    #[test]
    fn r683_view_dock_panel_outer_container_carries_tag_and_two_children() {
        run_in_owner(|| {
            let style = DockPanelStyle::m3_default(PANEL_TAG);
            let scene = view_dock_panel("My Panel", empty_content(), &theme_light(), &style, None);
            let Scene::Container(outer) = &scene else {
                panic!()
            };
            assert_eq!(outer.tag.as_deref(), Some(PANEL_TAG));
            assert_eq!(outer.children.len(), 2);
        });
    }

    #[test]
    fn r683_view_dock_panel_header_tagged_with_composite_suffix() {
        run_in_owner(|| {
            let style = DockPanelStyle::m3_default(PANEL_TAG);
            let scene = view_dock_panel("Title", empty_content(), &theme_light(), &style, None);
            let Scene::Container(outer) = &scene else {
                panic!()
            };
            let Scene::Container(header) = &outer.children[0] else {
                panic!()
            };
            assert_eq!(
                header.tag.as_deref(),
                Some(composite_tag(PANEL_TAG, HEADER_TAG_SUFFIX).as_str()),
            );
        });
    }

    #[test]
    fn r683_view_dock_panel_content_tagged_with_composite_suffix() {
        run_in_owner(|| {
            let style = DockPanelStyle::m3_default(PANEL_TAG);
            let scene = view_dock_panel("Title", empty_content(), &theme_light(), &style, None);
            let Scene::Container(outer) = &scene else {
                panic!()
            };
            let Scene::Container(content) = &outer.children[1] else {
                panic!()
            };
            assert_eq!(
                content.tag.as_deref(),
                Some(composite_tag(PANEL_TAG, CONTENT_TAG_SUFFIX).as_str()),
            );
        });
    }

    #[test]
    fn r683_view_dock_panel_header_height_matches_style() {
        run_in_owner(|| {
            let style = DockPanelStyle::m3_default(PANEL_TAG).with_header_height_px(32);
            let scene = view_dock_panel("Title", empty_content(), &theme_light(), &style, None);
            let Scene::Container(outer) = &scene else {
                panic!()
            };
            let Scene::Container(header) = &outer.children[0] else {
                panic!()
            };
            // size.height is a SizeValue::Px(32) — match the
            // numeric extent via the layout.size field.
            let height_px = match header.layout.size.height {
                pinion_core::style::SizeValue::Px(px) => Some(px),
                _ => None,
            };
            assert_eq!(height_px, Some(32));
        });
    }

    #[test]
    fn r683_view_dock_panel_header_contains_title_text() {
        run_in_owner(|| {
            let style = DockPanelStyle::m3_default(PANEL_TAG);
            let scene = view_dock_panel("Inspector", empty_content(), &theme_light(), &style, None);
            let Scene::Container(outer) = &scene else {
                panic!()
            };
            let Scene::Container(header) = &outer.children[0] else {
                panic!()
            };
            // Header has exactly one child: the title TextNode.
            assert_eq!(header.children.len(), 1);
            let Scene::Text(text) = &header.children[0] else {
                panic!()
            };
            assert_eq!(text.content, "Inspector");
        });
    }

    #[test]
    fn r683_dock_panel_style_m3_default_carries_canonical_defaults() {
        let style = DockPanelStyle::m3_default(PANEL_TAG);
        assert_eq!(style.header_height_px, 28);
        assert_eq!(style.header_font_size_px, 12);
        assert_eq!(style.tag.as_ref(), PANEL_TAG);
    }

    // ─────────────────────────────────────────────────────────────────
    // R1081 §5.51 — DockPanelExternal R742 pointer drag (replacing the
    // R683 capture + L∞-threshold tear-off). The router arms via
    // `invoke("send", "header:PointerDown")`, opens a session with
    // `begin_drag`, feeds `drag_to` / `drag_release` a `DropPoint`, and
    // the panel docks (with a coordinator), tears off (escape-drop), or
    // snaps back. Headless: we drive the hooks directly.
    // ─────────────────────────────────────────────────────────────────

    /// Two-leaf topology `a | b` for the reorganize tests.
    fn ab_topology() -> DockTopology {
        DockTopology::new(DockNode::split_horizontal(
            "root_h",
            0.5,
            DockNode::leaf("a"),
            DockNode::leaf("b"),
        ))
    }

    /// A `DropPoint` over panel `tag` at normalised `(x_rel, y_rel)`.
    fn drop_point(tag: &str, x_rel: f32, y_rel: f32) -> DropPoint {
        DropPoint {
            tag: tag.to_string(),
            x_rel,
            y_rel,
        }
    }

    fn dummy_payload() -> DragPayload {
        DragPayload {
            kind: std::borrow::Cow::Borrowed("dock-panel"),
            value: IntrospectValue::Text("a".to_string()),
        }
    }

    /// (R1101 §5.51) Build the [`DragUpdate`] the router forwards: the
    /// rect-relative `over`, the absolute `cursor`, the resolving `over_window`
    /// (`None` = own window), and the router's click-vs-drag `became_drag`
    /// verdict — which the panel now CONSUMES (the F1 clearance) instead of
    /// re-deriving with a private latch. A test plays the router's role by
    /// passing the verdict explicitly.
    fn upd(
        over: Option<DropPoint>,
        cursor: (f64, f64),
        over_window: Option<&str>,
        became_drag: bool,
    ) -> DragUpdate<'_> {
        DragUpdate {
            over,
            cursor,
            over_window,
            // R1107 — existing tests do not exercise the source window; the
            // R1107 follow tests build `DragUpdate` directly with a value.
            source_window: None,
            became_drag,
            // R1117 — these tests do not exercise the window-move grab-offset;
            // the press point defaults to the cursor (a degenerate in-place grab,
            // so `cursor - press_cursor` is 0 — irrelevant to the tear-off path).
            press_cursor: cursor,
            // R1120 — no real window in these unit drives; the window-move path
            // is exercised by the `simulate_window_move` harness, not `upd`.
            source_window_origin: None,
        }
    }

    #[test]
    fn r1081_begin_drag_arms_on_header_not_content() {
        let ext = DockPanelExternal::new("a");
        // Header press arms (default `is_drag_armed = true`).
        assert!(ext.begin_drag().is_some(), "header press opens a session");
        assert!(ext.is_dragging());
        // A content press disarms → no session.
        let mut ext2 = DockPanelExternal::new("a");
        ext2.invoke(
            "send",
            IntrospectValue::Text("content:PointerDown".to_string()),
        )
        .expect("send parses");
        assert!(
            ext2.begin_drag().is_none(),
            "content press must not start a drag",
        );
    }

    #[test]
    fn r1081_drag_to_writes_shared_preview_for_target_panel() {
        let preview = Rc::new(Signal::new(None));
        let mut ext = DockPanelExternal::new("a").with_drop_preview(Rc::clone(&preview));
        let _ = ext.begin_drag();
        // Cursor over panel "b" near its left edge → Left zone.
        ext.drag_to(&dummy_payload(), Some(drop_point("b", 0.1, 0.5)));
        let p = preview.get().expect("preview written");
        assert_eq!(p.source, "a");
        assert_eq!(p.target, "b");
        assert_eq!(p.zone, DockDropZone::Left);
        // Cursor back over self → preview clears (a self-drop is a no-op).
        ext.drag_to(&dummy_payload(), Some(drop_point("a", 0.5, 0.5)));
        assert!(preview.get().is_none(), "self-hover clears the preview");
    }

    #[test]
    fn r1112_with_tabbing_false_surface_resolves_center_to_edge_not_tabify() {
        // R1112 PR-37 — the tab-docking policy is the dock SURFACE's
        // (DockReorganizer::with_tabbing), read through the panel's shared
        // coordinator. A panel on a split-only surface never previews a centre
        // tabify: a centre drop over a sibling resolves to the nearest split
        // edge. A panel on a tab-docking surface (or none) keeps Center.
        let on_prev = Rc::new(Signal::new(None));
        let reorg_on = Rc::new(DockReorganizer::new(Rc::new(Signal::new(Some(
            ab_topology(),
        )))));
        let mut on = DockPanelExternal::new("a")
            .with_reorganizer(reorg_on)
            .with_drop_preview(Rc::clone(&on_prev));
        let _ = on.begin_drag();
        on.drag_to(&dummy_payload(), Some(drop_point("b", 0.5, 0.5)));
        assert_eq!(
            on_prev.get().expect("preview written").zone,
            DockDropZone::Center,
            "tab-docking surface: centre over a sibling previews Tabify",
        );

        let off_prev = Rc::new(Signal::new(None));
        let reorg_off = Rc::new(
            DockReorganizer::new(Rc::new(Signal::new(Some(ab_topology())))).with_tabbing(false),
        );
        let mut off = DockPanelExternal::new("a")
            .with_reorganizer(reorg_off)
            .with_drop_preview(Rc::clone(&off_prev));
        let _ = off.begin_drag();
        off.drag_to(&dummy_payload(), Some(drop_point("b", 0.5, 0.5)));
        assert_eq!(
            off_prev.get().expect("preview written").zone,
            DockDropZone::Left,
            "split-only surface: centre resolves to nearest edge (ties Left), never Center",
        );
        // The effective policy is observable (§2 #7) + read-only on the panel.
        assert_eq!(off.query("tabbing"), Some(IntrospectValue::Bool(false)));
        assert_eq!(on.query("tabbing"), Some(IntrospectValue::Bool(true)));
        // A tear-off-only panel (no coordinator) reports the default — moot,
        // since it can never reorganize.
        assert_eq!(
            DockPanelExternal::new("a").query("tabbing"),
            Some(IntrospectValue::Bool(true)),
        );
        assert!(matches!(
            off.intervene("tabbing", IntrospectValue::Bool(true)),
            Err(InterveneError::ReadOnly),
        ));
    }

    #[test]
    fn r1081_drag_release_over_another_panel_reorganizes_via_shared_coordinator() {
        let topology = Rc::new(Signal::new(Some(ab_topology())));
        let reorganizer = Rc::new(DockReorganizer::new(Rc::clone(&topology)));
        let mut ext = DockPanelExternal::new("a").with_reorganizer(Rc::clone(&reorganizer));
        let _ = ext.begin_drag();
        // Drop "a" on the centre of "b" → swap.
        ext.drag_release(&dummy_payload(), Some(drop_point("b", 0.5, 0.5)));
        assert!(!ext.tear_off_fired(), "a dock is not a tear-off");
        assert_eq!(
            reorganizer.last_outcome().as_deref(),
            Some("a -> b"),
            "the shared coordinator applied the swap",
        );
    }

    #[test]
    fn r1081_drag_release_over_no_panel_tears_off() {
        let topology = Rc::new(Signal::new(Some(ab_topology())));
        let reorganizer = Rc::new(DockReorganizer::new(topology));
        let mut ext = DockPanelExternal::new("a").with_reorganizer(reorganizer);
        let _ = ext.begin_drag();
        // Released over no drop target (escaped the dock).
        ext.drag_release(&dummy_payload(), None);
        assert!(ext.tear_off_fired(), "escape-drop tears off");
        let mut received: Vec<Intent> = Vec::new();
        ext.drain_intents(&mut |i| received.push(i));
        assert_eq!(received.len(), 1, "exactly one tear_off");
        assert_eq!(received[0].tag.as_ref(), TEAR_OFF_EVENT);
        assert_eq!(received[0].payload.as_str(), Some("a"));
    }

    #[test]
    fn r1081_drag_release_back_on_self_snaps_back_no_tear_off() {
        // A click on the header (press-release in place, over its own
        // panel) must NOT tear off — only an escape-drop floats.
        let mut ext = DockPanelExternal::new("a");
        let _ = ext.begin_drag();
        ext.drag_release(&dummy_payload(), Some(drop_point("a", 0.5, 0.5)));
        assert!(!ext.tear_off_fired(), "self-drop snaps back");
        let mut received: Vec<Intent> = Vec::new();
        ext.drain_intents(&mut |i| received.push(i));
        assert!(received.is_empty(), "no tear_off on a self-drop");
    }

    #[test]
    fn r1081_tear_off_only_mode_no_coordinator_drop_on_panel_is_noop() {
        // Without a reorganizer (the flat hello-dock-panels consumer) a
        // drop onto another panel snaps back; only an escape-drop floats.
        let mut ext = DockPanelExternal::new("a");
        let _ = ext.begin_drag();
        ext.drag_release(&dummy_payload(), Some(drop_point("b", 0.5, 0.5)));
        assert!(!ext.tear_off_fired());
        let mut received: Vec<Intent> = Vec::new();
        ext.drain_intents(&mut |i| received.push(i));
        assert!(received.is_empty(), "no coordinator + panel drop = no-op");
    }

    #[test]
    fn r1081_drag_cancel_discards_without_committing() {
        let topology = Rc::new(Signal::new(Some(ab_topology())));
        let reorganizer = Rc::new(DockReorganizer::new(topology));
        let preview = Rc::new(Signal::new(None));
        let mut ext = DockPanelExternal::new("a")
            .with_reorganizer(Rc::clone(&reorganizer))
            .with_drop_preview(Rc::clone(&preview));
        let _ = ext.begin_drag();
        ext.drag_to(&dummy_payload(), Some(drop_point("b", 0.5, 0.5)));
        assert!(preview.get().is_some(), "preview shows mid-drag");
        ext.drag_cancel(&dummy_payload());
        assert!(!ext.is_dragging());
        assert!(preview.get().is_none(), "cancel clears the preview");
        assert_eq!(reorganizer.last_outcome(), None, "cancel commits nothing");
    }

    #[test]
    fn r1081_drag_release_clears_the_shared_preview() {
        let preview = Rc::new(Signal::new(None));
        let mut ext = DockPanelExternal::new("a").with_drop_preview(Rc::clone(&preview));
        let _ = ext.begin_drag();
        ext.drag_to(&dummy_payload(), Some(drop_point("b", 0.5, 0.5)));
        assert!(preview.get().is_some());
        ext.drag_release(&dummy_payload(), None);
        assert!(preview.get().is_none(), "release clears the overlay");
    }

    #[test]
    fn r1081_invoke_tear_off_enqueues_without_a_pointer_drag() {
        // R683.C direct AI tear-off — still valid (dock-back of a floated
        // panel before the router has its paint scene).
        let mut ext = DockPanelExternal::new("a");
        ext.invoke(TEAR_OFF_EVENT, IntrospectValue::Null)
            .expect("direct tear_off invoke");
        assert!(ext.tear_off_fired());
        let mut received: Vec<Intent> = Vec::new();
        ext.drain_intents(&mut |i| received.push(i));
        assert_eq!(received.len(), 1);
        assert_eq!(received[0].tag.as_ref(), TEAR_OFF_EVENT);
    }

    #[test]
    fn r1081_shared_coordinator_means_one_split_seq_across_both_panels() {
        // Two panel externals sharing ONE coordinator: a split minted by
        // one is visible to the other (no `reorg-split-{n}` collision).
        let topology = Rc::new(Signal::new(Some(ab_topology())));
        let reorganizer = Rc::new(DockReorganizer::new(topology));
        let mut a = DockPanelExternal::new("a").with_reorganizer(Rc::clone(&reorganizer));
        let mut b = DockPanelExternal::new("b").with_reorganizer(Rc::clone(&reorganizer));
        let _ = a.begin_drag();
        // Dock "a" to the left edge of "b" → a SplitInsert mints split 0.
        a.drag_release(&dummy_payload(), Some(drop_point("b", 0.05, 0.5)));
        assert_eq!(reorganizer.split_seq(), 1, "one split minted");
        // The other panel's external sees the same bumped counter.
        let _ = b.begin_drag();
        b.drag_release(&dummy_payload(), Some(drop_point("a", 0.05, 0.5)));
        assert_eq!(
            reorganizer.split_seq(),
            2,
            "second split mints id 1, not a colliding 0",
        );
    }

    #[test]
    fn r880_1_pointer_up_with_modifier_segment_still_re_arms() {
        // "t0:PointerUp:c" (the R781 modifier segment) must still parse
        // through the split_send_payload SSOT and hit the Up arm — the
        // pre-R880.1 hand-rolled split read "PointerUp:c" as the event
        // name and returned UnknownPath.
        let mut ext = DockPanelExternal::new("a");
        // Disarm via a content press, then the modifier-tagged release
        // re-arms so the next header drag opens a session.
        ext.invoke(
            "send",
            IntrospectValue::Text("content:PointerDown".to_string()),
        )
        .expect("content press parses");
        assert!(ext.begin_drag().is_none(), "content press disarmed");
        ext.invoke("send", IntrospectValue::Text("t0:PointerUp:c".to_string()))
            .expect("modifier-held release still parses");
        assert!(ext.begin_drag().is_some(), "release re-armed the drag");
    }

    #[test]
    fn r683_dock_panel_external_introspect_schema_includes_canonical_paths() {
        let ext = DockPanelExternal::new("p1");
        let schema = ext.schema();
        let fields: Vec<&str> = schema.fields.iter().map(|(n, _)| *n).collect();
        for needed in [
            "panel_id",
            "dragging",
            "tear_off_fired",
            "drop_preview",
            "drag_cursor",
            "detached",
            "send",
        ] {
            assert!(fields.contains(&needed), "schema must include {needed}");
        }
    }

    #[test]
    fn r1093_drag_cursor_records_forwarded_cursor_and_resets_per_gesture() {
        // R1093 §5.15 — the drag_cursor slot is null until a drag forwards a
        // cursor, then carries the absolute [x, y]; a fresh begin_drag resets
        // it; and it is read-only (driven by the router, not intervene).
        let mut ext = DockPanelExternal::new("p1");
        assert_eq!(
            ext.query("drag_cursor"),
            Some(IntrospectValue::Null),
            "drag_cursor is null before any drag"
        );
        let _ = ext.begin_drag();
        // A move forwards the cursor even when over is None (escaped tags).
        ext.drag_to_at(&dummy_payload(), &upd(None, (123.0, 45.0), None, false));
        assert_eq!(
            ext.query("drag_cursor"),
            Some(IntrospectValue::Json(serde_json::json!([123.0, 45.0]))),
            "drag_cursor mirrors the forwarded move cursor"
        );
        // The release cursor overwrites it and persists post-gesture.
        ext.drag_release_at(&dummy_payload(), &upd(None, (200.0, 88.0), None, false));
        assert_eq!(
            ext.query("drag_cursor"),
            Some(IntrospectValue::Json(serde_json::json!([200.0, 88.0]))),
            "drag_cursor mirrors the release cursor and persists after the drop"
        );
        // A new gesture clears the stale cursor.
        let _ = ext.begin_drag();
        assert_eq!(
            ext.query("drag_cursor"),
            Some(IntrospectValue::Null),
            "begin_drag resets drag_cursor for the fresh gesture"
        );
        // The slot is framework-owned, not AI-writable.
        assert_eq!(
            ext.intervene(
                "drag_cursor",
                IntrospectValue::Json(serde_json::json!([1.0, 2.0]))
            ),
            Err(InterveneError::ReadOnly),
            "drag_cursor is read-only (router-driven)"
        );
    }

    /// (R1094) Extract `(panel, x, y)` from a [`TEAR_OFF_FOLLOW_EVENT`]
    /// payload for the live-follow assertions below.
    fn follow_fields(payload: &IntrospectValue) -> (String, f64, f64) {
        let IntrospectValue::Json(v) = payload else {
            panic!("tear_off_follow payload must be Json; got {payload:?}");
        };
        (
            v.get("panel")
                .and_then(serde_json::Value::as_str)
                .expect("panel field")
                .to_string(),
            v.get("x")
                .and_then(serde_json::Value::as_f64)
                .expect("x field"),
            v.get("y")
                .and_then(serde_json::Value::as_f64)
                .expect("y field"),
        )
    }

    /// (R1097 PR-32 / R1101 / R1110 PR-36) Drive a gesture into a detach: the
    /// first sample is the grab (`became_drag` false, no follow), the second
    /// carries the router's drag verdict (`became_drag` true). Under R1110 the
    /// verdict alone no longer detaches — the cursor must ALSO be over no
    /// same-window dock zone, so `over_panel` must be `None` (escaped) for the
    /// second sample to tear into a follower. Passing `Some(panel)` keeps the
    /// gesture docked (in-dock reorganize preview, no float) — every detach
    /// caller therefore passes `None`. The caller has already `begin_drag`'d.
    fn cross_detach(ext: &mut DockPanelExternal, over_panel: Option<&str>, far: (f64, f64)) {
        let over = || over_panel.map(|p| drop_point(p, 0.5, 0.5));
        ext.drag_to_at(&dummy_payload(), &upd(over(), (10.0, 10.0), None, false));
        ext.drag_to_at(&dummy_payload(), &upd(over(), far, None, true));
    }

    // ── R1107 §5.51 §2 #7 — the follow-drag SOURCE window ────────────

    #[test]
    fn r1107_follow_payload_and_diagnostic_carry_source_window() {
        let mut ext = DockPanelExternal::new("viewport");
        let _ = ext.begin_drag();
        // A move whose router belongs to the FLOATING window the header is being
        // re-dragged in: `source_window` names that window, not main, so the
        // binding adds the right origin.
        let update = DragUpdate {
            over: None,
            cursor: (40.0, 25.0),
            over_window: None,
            source_window: Some("torn-viewport"),
            became_drag: true,
            press_cursor: (40.0, 25.0),
            source_window_origin: None,
        };
        ext.drag_to_at(&dummy_payload(), &update);
        // The emitted tear_off_follow carries the source window.
        let mut received: Vec<Intent> = Vec::new();
        ext.drain_intents(&mut |i| received.push(i));
        let follow = received
            .iter()
            .find(|i| i.tag.as_ref() == TEAR_OFF_FOLLOW_EVENT)
            .expect("a tear_off_follow fired");
        let IntrospectValue::Json(v) = &follow.payload else {
            panic!("follow payload is Json");
        };
        assert_eq!(
            v.get("source_window").and_then(serde_json::Value::as_str),
            Some("torn-viewport"),
            "the follow payload names the window the cursor was measured in",
        );
        // And it is observable as scene-as-data (§2 #7).
        assert_eq!(
            ext.query("source_window"),
            Some(IntrospectValue::Text("torn-viewport".to_string())),
        );
    }

    #[test]
    fn r1107_source_window_is_read_only_and_resets_per_gesture() {
        let mut ext = DockPanelExternal::new("viewport");
        let _ = ext.begin_drag();
        ext.drag_to_at(
            &dummy_payload(),
            &DragUpdate {
                over: None,
                cursor: (1.0, 1.0),
                over_window: None,
                source_window: Some("torn-viewport"),
                became_drag: true,
                press_cursor: (1.0, 1.0),
                source_window_origin: None,
            },
        );
        assert_eq!(
            ext.query("source_window"),
            Some(IntrospectValue::Text("torn-viewport".to_string())),
        );
        // Router-owned diagnostic — an AI cannot poke it directly.
        assert!(matches!(
            ext.intervene("source_window", IntrospectValue::Null),
            Err(InterveneError::ReadOnly),
        ));
        // A fresh gesture clears it.
        let _ = ext.begin_drag();
        assert_eq!(ext.query("source_window"), Some(IntrospectValue::Null));
    }

    #[test]
    fn r1107_1_release_follow_carries_source_window_not_none() {
        // R1107.1 review-clearance: the FINAL release-follow (the escape-drop
        // arm of `drag_release`) must carry the gesture's SOURCE window, not a
        // hard-coded None → main. Without the fix a floater re-drag's release
        // re-positioned at main's origin (the R1095.1 defect on the release arm).
        let mut ext = DockPanelExternal::new("viewport");
        let _ = ext.begin_drag();
        // A move in the floating window records its source.
        ext.drag_to_at(
            &dummy_payload(),
            &DragUpdate {
                over: Some(drop_point("viewport", 0.5, 0.5)),
                cursor: (10.0, 10.0),
                over_window: None,
                source_window: Some("torn-viewport"),
                became_drag: true,
                press_cursor: (10.0, 10.0),
                source_window_origin: None,
            },
        );
        let mut drained: Vec<Intent> = Vec::new();
        ext.drain_intents(&mut |i| drained.push(i)); // clear the move's intents
        // An escape RELEASE (over None, same window) emits the final follow.
        ext.drag_release_at(
            &dummy_payload(),
            &DragUpdate {
                over: None,
                cursor: (40.0, 25.0),
                over_window: None,
                source_window: Some("torn-viewport"),
                became_drag: true,
                press_cursor: (40.0, 25.0),
                source_window_origin: None,
            },
        );
        let mut received: Vec<Intent> = Vec::new();
        ext.drain_intents(&mut |i| received.push(i));
        let follow = received
            .iter()
            .find(|i| i.tag.as_ref() == TEAR_OFF_FOLLOW_EVENT)
            .expect("the escape release emits a final follow");
        let IntrospectValue::Json(v) = &follow.payload else {
            panic!("follow payload is Json");
        };
        assert_eq!(
            v.get("source_window").and_then(serde_json::Value::as_str),
            Some("torn-viewport"),
            "the release-follow carries the gesture's source window, not None→main",
        );
    }

    // ── R1100 §5.51 PR-33 — cross-window dock-at redock (slice 1) ─────

    #[test]
    fn r1100_cross_window_release_redocks_at_target_window() {
        // Releasing a floating panel's drag over ANOTHER window's dock zone
        // (over_window=Some) emits the dock-at redock intent carrying the
        // target window + zone — NOT a same-window reorganize / escape-float.
        let mut ext = DockPanelExternal::new("inspector");
        let _ = ext.begin_drag();
        cross_detach(&mut ext, None, (640.0, 300.0)); // tear into a follower
        let mut drained: Vec<Intent> = Vec::new();
        ext.drain_intents(&mut |i| drained.push(i)); // consume the follow(s)
        ext.drag_release_at(
            &dummy_payload(),
            &upd(
                Some(drop_point("viewport", 0.25, 0.5)),
                (200.0, 100.0),
                Some("main"),
                false,
            ),
        );
        let mut after: Vec<Intent> = Vec::new();
        ext.drain_intents(&mut |i| after.push(i));
        assert_eq!(after.len(), 1, "exactly one dock-at redock intent");
        assert_eq!(after[0].tag.as_ref(), TEAR_OFF_REDOCK_AT_EVENT);
        let IntrospectValue::Json(p) = &after[0].payload else {
            panic!("the dock-at payload is JSON");
        };
        assert_eq!(p["panel"], "inspector");
        assert_eq!(p["window"], "main", "carries the TARGET window");
        assert_eq!(p["target"], "viewport", "carries the target dock zone tag");
        assert!(
            (p["x_rel"].as_f64().expect("x_rel") - 0.25).abs() < 1e-6,
            "carries the cursor normalised over the zone (for edge-vs-centre)",
        );
        assert!(
            !ext.detached(),
            "the dock-at consumed the floating follower"
        );
        // R1102 §2 #7 — the transient intent is also recorded on the persistent
        // `redock_at` slot so an AI (and the live demo) can observe the
        // cross-window redock after the intent has been drained.
        let Some(IntrospectValue::Json(observed)) = ext.query("redock_at") else {
            panic!("redock_at must surface the cross-window dock-at as JSON");
        };
        assert_eq!(observed["window"], "main");
        assert_eq!(observed["target"], "viewport");
        assert_eq!(
            ext.intervene("redock_at", IntrospectValue::Null),
            Err(InterveneError::ReadOnly),
            "redock_at is a read-only diagnostic",
        );
    }

    #[test]
    fn r1102_redock_at_is_null_before_and_resets_per_gesture() {
        // R1102 §2 #7 — the redock_at diagnostic is null before any cross-window
        // dock-at, carries the last one, and a fresh begin_drag clears it.
        let mut ext = DockPanelExternal::new("inspector");
        assert_eq!(
            ext.query("redock_at"),
            Some(IntrospectValue::Null),
            "redock_at is null before any cross-window redock"
        );
        let _ = ext.begin_drag();
        cross_detach(&mut ext, None, (640.0, 300.0));
        let mut drained: Vec<Intent> = Vec::new();
        ext.drain_intents(&mut |i| drained.push(i));
        ext.drag_release_at(
            &dummy_payload(),
            &upd(
                Some(drop_point("viewport", 0.5, 0.5)),
                (200.0, 100.0),
                Some("main"),
                false,
            ),
        );
        assert!(
            matches!(ext.query("redock_at"), Some(IntrospectValue::Json(_))),
            "redock_at carries the cross-window dock-at after the release"
        );
        // A fresh gesture clears the stale diagnostic.
        let _ = ext.begin_drag();
        assert_eq!(
            ext.query("redock_at"),
            Some(IntrospectValue::Null),
            "begin_drag resets redock_at for the new gesture"
        );
    }

    #[test]
    fn r1100_same_window_release_never_docks_at() {
        // over_window=None → the existing same-window path (escape-float here);
        // a same-window drop is NEVER a cross-window dock-at.
        let mut ext = DockPanelExternal::new("inspector");
        let _ = ext.begin_drag();
        cross_detach(&mut ext, None, (640.0, 300.0));
        let mut drained: Vec<Intent> = Vec::new();
        ext.drain_intents(&mut |i| drained.push(i));
        ext.drag_release_at(&dummy_payload(), &upd(None, (700.0, 320.0), None, false));
        let mut after: Vec<Intent> = Vec::new();
        ext.drain_intents(&mut |i| after.push(i));
        assert!(
            after
                .iter()
                .all(|i| i.tag.as_ref() != TEAR_OFF_REDOCK_AT_EVENT),
            "a same-window release never docks-at (got {after:?})",
        );
    }

    // ── R1129 §5.51.1 §5.38 §2 #5 — the DockPanelPolicy lifecycle chart ──
    //
    // STAGE 2 of the dock-panel SCXML-chart campaign: the External's persistent
    // `docked ↔ floating` lifecycle now lives in the R1127 `dock_panel.scxml`
    // chart (`Widget<DockPanelPolicy>`), not the imperative per-gesture
    // `detached` bool + hardcoded redock defaults (the §2 #5 drift). The chart is
    // the IO gate — a `dropped` / `dock_back` is inert while `docked`, so the
    // chart (not a bare bool) enforces "a panel that never floated cannot redock"
    // — and is AI-introspectable via `query("lifecycle")` (§2 #7).

    #[test]
    fn r1129_lifecycle_starts_docked() {
        let ext = DockPanelExternal::new("a");
        assert_eq!(
            ext.query("lifecycle"),
            Some(IntrospectValue::Text("Docked".to_string())),
            "a freshly registered panel is docked (the chart's SCXML initial)",
        );
    }

    #[test]
    fn r1129_tear_off_floats_then_redock_at_zone_docks() {
        // The full float → redock arc: escape every drop target (tear-off) →
        // `floating`; release back over another panel's zone → `docked`.
        let reorganizer = Rc::new(DockReorganizer::new(Rc::new(Signal::new(Some(
            ab_topology(),
        )))));
        let mut ext = DockPanelExternal::new("a").with_reorganizer(Rc::clone(&reorganizer));
        let _ = ext.begin_drag();
        cross_detach(&mut ext, None, (640.0, 300.0)); // escape → float
        assert_eq!(
            ext.query("lifecycle"),
            Some(IntrospectValue::Text("Floating".to_string())),
            "escaping every drop target floats the panel (escaped → floating)",
        );
        let mut drained: Vec<Intent> = Vec::new();
        ext.drain_intents(&mut |i| drained.push(i)); // consume the follow(s)
        // Release over sibling "b": redock-at-zone (dropped → docked).
        ext.drag_release(&dummy_payload(), Some(drop_point("b", 0.5, 0.5)));
        assert_eq!(
            ext.query("lifecycle"),
            Some(IntrospectValue::Text("Docked".to_string())),
            "redocking over a zone docks the panel (dropped → docked)",
        );
        let mut after: Vec<Intent> = Vec::new();
        ext.drain_intents(&mut |i| after.push(i));
        assert!(
            after
                .iter()
                .any(|i| i.tag.as_ref() == TEAR_OFF_REDOCK_EVENT),
            "a floated → docked redock removes the follow window (got {after:?})",
        );
    }

    #[test]
    fn r1129_snap_back_restores_and_redock_is_gated_by_the_chart() {
        // The KEY STAGE-2 property: the chart gates the restore IO. A tear-off
        // that returns home restores (dock_back → docked + removes the follow
        // window); a drag that NEVER floated is `docked`, so `dock_back` is inert
        // and NO restore fires — the chart, not a bare bool, owns the gate.
        // (a) floated then snapped home → restore IO fires.
        let mut floated = DockPanelExternal::new("a");
        let _ = floated.begin_drag();
        cross_detach(&mut floated, None, (640.0, 300.0)); // escape → float
        let mut drained: Vec<Intent> = Vec::new();
        floated.drain_intents(&mut |i| drained.push(i)); // consume the follow(s)
        floated.drag_release(&dummy_payload(), Some(drop_point("a", 0.5, 0.5))); // home
        assert_eq!(
            floated.query("lifecycle"),
            Some(IntrospectValue::Text("Docked".to_string())),
        );
        let mut after: Vec<Intent> = Vec::new();
        floated.drain_intents(&mut |i| after.push(i));
        assert!(
            after
                .iter()
                .any(|i| i.tag.as_ref() == TEAR_OFF_REDOCK_EVENT),
            "a floated panel snapping home restores (removes its follow window)",
        );
        // (b) never-floated drag back home → docked, NO spurious restore IO.
        let mut never = DockPanelExternal::new("a");
        let _ = never.begin_drag();
        never.drag_release(&dummy_payload(), Some(drop_point("a", 0.5, 0.5)));
        assert_eq!(
            never.query("lifecycle"),
            Some(IntrospectValue::Text("Docked".to_string())),
            "a never-floated drag stays docked",
        );
        let mut none: Vec<Intent> = Vec::new();
        never.drain_intents(&mut |i| none.push(i));
        assert!(
            none.iter().all(|i| i.tag.as_ref() != TEAR_OFF_REDOCK_EVENT),
            "dock_back is inert while docked — no spurious restore (got {none:?})",
        );
    }

    #[test]
    fn r1129_cancel_restores_floated_lifecycle() {
        // An OS-revoked drag that had floated restores home (dock_back → docked).
        let mut ext = DockPanelExternal::new("a");
        let _ = ext.begin_drag();
        cross_detach(&mut ext, None, (640.0, 300.0)); // escape → float
        assert_eq!(
            ext.query("lifecycle"),
            Some(IntrospectValue::Text("Floating".to_string())),
        );
        let mut drained: Vec<Intent> = Vec::new();
        ext.drain_intents(&mut |i| drained.push(i));
        ext.drag_cancel(&dummy_payload());
        assert_eq!(
            ext.query("lifecycle"),
            Some(IntrospectValue::Text("Docked".to_string())),
            "a cancelled float restores to docked",
        );
        let mut after: Vec<Intent> = Vec::new();
        ext.drain_intents(&mut |i| after.push(i));
        assert!(
            after
                .iter()
                .any(|i| i.tag.as_ref() == TEAR_OFF_REDOCK_EVENT),
            "the cancelled float removes its follow window",
        );
    }

    #[test]
    fn r1129_invoke_tear_off_floats_the_lifecycle() {
        // The AI-primary tear-off channel drives the same `escaped → floating`
        // transition as the pointer escape-drop, so the chart is consistent
        // across the pointer + invoke paths.
        let mut ext = DockPanelExternal::new("a");
        ext.invoke(TEAR_OFF_EVENT, IntrospectValue::Null)
            .expect("direct tear_off invoke");
        assert_eq!(
            ext.query("lifecycle"),
            Some(IntrospectValue::Text("Floating".to_string())),
        );
    }

    #[test]
    fn r1129_cross_window_redock_docks_the_lifecycle() {
        // A floating panel dropped into ANOTHER window's dock zone re-docks
        // (dropped → docked) via the settle path shared by the live
        // drag_release_at + the AI invoke.
        let mut ext = DockPanelExternal::new("inspector");
        let _ = ext.begin_drag();
        cross_detach(&mut ext, None, (640.0, 300.0)); // escape → float
        assert_eq!(
            ext.query("lifecycle"),
            Some(IntrospectValue::Text("Floating".to_string())),
        );
        let mut drained: Vec<Intent> = Vec::new();
        ext.drain_intents(&mut |i| drained.push(i));
        ext.drag_release_at(
            &dummy_payload(),
            &upd(
                Some(drop_point("viewport", 0.5, 0.5)),
                (200.0, 100.0),
                Some("main"),
                false,
            ),
        );
        assert_eq!(
            ext.query("lifecycle"),
            Some(IntrospectValue::Text("Docked".to_string())),
            "a cross-window dock-at docks the lifecycle",
        );
    }

    #[test]
    fn r1129_lifecycle_is_read_only() {
        // §2 #7 — the lifecycle is a chart-owned diagnostic; an AI drives it
        // through the gesture / invoke channels, not by poking the slot.
        let mut ext = DockPanelExternal::new("a");
        assert!(
            ext.query("lifecycle").is_some(),
            "lifecycle is introspectable"
        );
        assert!(matches!(
            ext.intervene("lifecycle", IntrospectValue::Text("Floating".to_string())),
            Err(InterveneError::ReadOnly),
        ));
    }

    #[test]
    fn r1129_lifecycle_persists_across_a_new_gesture() {
        // The chart is the PERSISTENT lifecycle (unlike `detached`, reset on
        // begin_drag): a panel that floated stays `floating` into the next
        // gesture, so a re-drag of an already-floating panel is not mistaken for
        // a fresh docked panel.
        let mut ext = DockPanelExternal::new("a");
        let _ = ext.begin_drag();
        cross_detach(&mut ext, None, (640.0, 300.0)); // escape → float
        assert_eq!(
            ext.query("lifecycle"),
            Some(IntrospectValue::Text("Floating".to_string())),
        );
        // A fresh gesture clears the per-gesture `detached` latch …
        let _ = ext.begin_drag();
        assert!(!ext.detached(), "begin_drag resets the per-gesture latch");
        // … but the persistent lifecycle is still floating.
        assert_eq!(
            ext.query("lifecycle"),
            Some(IntrospectValue::Text("Floating".to_string())),
            "the chart persists the float across gestures (not gesture-scoped)",
        );
    }

    #[test]
    fn r1130_invoke_tear_off_is_a_chart_directed_toggle() {
        // R1130 §5.51.1 — the AI-primary `tear_off` invoke is chart-directed: a
        // docked panel floats (Escaped + tear_off intent); a floating panel
        // dock-backs (DockBack + the directed tear_off_redock intent). This keeps
        // the chart consistent with the binding's window list across repeated
        // toggles — closing the R1129 defect where a 2nd tear_off drove the chart
        // Escaped->inert while the reducer's tear_off toggle docked the window.
        let mut ext = DockPanelExternal::new("a");
        // 1st invoke: docked -> float.
        ext.invoke(TEAR_OFF_EVENT, IntrospectValue::Null)
            .expect("tear_off invoke (float)");
        assert_eq!(
            ext.query("lifecycle"),
            Some(IntrospectValue::Text("Floating".to_string())),
            "a docked panel's tear_off floats it",
        );
        let mut first: Vec<Intent> = Vec::new();
        ext.drain_intents(&mut |i| first.push(i));
        assert_eq!(first.len(), 1);
        assert_eq!(
            first[0].tag.as_ref(),
            TEAR_OFF_EVENT,
            "the float direction emits the tear_off intent",
        );
        assert!(
            ext.tear_off_fired(),
            "the float direction sets tear_off_fired"
        );
        // 2nd invoke: floating -> dock-back (the directed restore).
        ext.invoke(TEAR_OFF_EVENT, IntrospectValue::Null)
            .expect("tear_off invoke (dock-back)");
        assert_eq!(
            ext.query("lifecycle"),
            Some(IntrospectValue::Text("Docked".to_string())),
            "a floating panel's tear_off dock-backs it (chart not desynced)",
        );
        let mut second: Vec<Intent> = Vec::new();
        ext.drain_intents(&mut |i| second.push(i));
        assert_eq!(second.len(), 1);
        assert_eq!(
            second[0].tag.as_ref(),
            TEAR_OFF_REDOCK_EVENT,
            "the dock-back direction emits the directed tear_off_redock intent",
        );
        assert!(!ext.tear_off_fired(), "a dock-back is not a tear-off");
    }

    #[test]
    fn r1133_with_initial_floating_rehydrates_the_chart() {
        // R1133 §5.51.1 — a reconstructed External re-hydrates its lifecycle from
        // the binding's float truth: with_initial_floating(true) drives the chart
        // to Floating (via the chart's own escaped transition), so a panel rebuilt
        // by the factory while its window exists is not reset to Docked. The
        // default (false / plain new) stays Docked.
        let floating = DockPanelExternal::new("a").with_initial_floating(true);
        assert_eq!(
            floating.query("lifecycle"),
            Some(IntrospectValue::Text("Floating".to_string())),
            "with_initial_floating(true) re-hydrates the chart to Floating",
        );
        let docked = DockPanelExternal::new("a").with_initial_floating(false);
        assert_eq!(
            docked.query("lifecycle"),
            Some(IntrospectValue::Text("Docked".to_string())),
            "with_initial_floating(false) leaves the chart Docked",
        );
        assert_eq!(
            DockPanelExternal::new("a").query("lifecycle"),
            Some(IntrospectValue::Text("Docked".to_string())),
            "plain new is Docked (the SCXML initial)",
        );
    }

    // ── R1134 §5.51.1 — FloatPolicy collapse timing through the external ──

    /// 3-panel fixture `a | (b | c)` behind a shared signal — `b` / `c` have a
    /// leaf-sibling home anchor so the collapse round-trip is exact.
    fn abc_signal() -> Rc<Signal<Option<DockTopology>>> {
        Rc::new(Signal::new(Some(DockTopology::new(
            DockNode::split_horizontal(
                "root_h",
                0.5,
                DockNode::leaf("a"),
                DockNode::split_horizontal(
                    "inner_h",
                    0.5,
                    DockNode::leaf("b"),
                    DockNode::leaf("c"),
                ),
            ),
        ))))
    }

    fn collapse_reorg(topo: &Rc<Signal<Option<DockTopology>>>) -> Rc<DockReorganizer> {
        Rc::new(DockReorganizer::new(Rc::clone(topo)).with_float_policy(FloatPolicy::Collapse))
    }

    #[test]
    fn r1134_invoke_tear_off_collapses_then_restores_home() {
        // Under Collapse the discrete tear_off invoke removes b's leaf (neighbours
        // reflow) and the chart floats; the dock-back invoke restores b at its home
        // anchor and the chart docks. The external surfaces the policy.
        let topo = abc_signal();
        let reorg = collapse_reorg(&topo);
        let mut ext = DockPanelExternal::new("b").with_reorganizer(Rc::clone(&reorg));
        assert_eq!(
            ext.query("float_policy"),
            Some(IntrospectValue::Text("collapse".to_string())),
            "the panel reports its coordinator's policy",
        );
        // Float (collapse out).
        ext.invoke(TEAR_OFF_EVENT, IntrospectValue::Null).unwrap();
        assert!(
            !topo.get().unwrap().panel_ids().contains(&"b"),
            "b collapsed out of the topology",
        );
        assert_eq!(
            ext.query("lifecycle"),
            Some(IntrospectValue::Text("Floating".to_string())),
            "the chart floated",
        );
        // Dock back (restore home).
        ext.invoke(TEAR_OFF_EVENT, IntrospectValue::Null).unwrap();
        let after = topo.get().unwrap();
        assert!(after.panel_ids().contains(&"b"), "b restored to the dock");
        assert_eq!(after.panel_ids().len(), 3, "all three panels present");
        assert_eq!(
            ext.query("lifecycle"),
            Some(IntrospectValue::Text("Docked".to_string())),
            "the chart docked back",
        );
    }

    #[test]
    fn r1134_placeholder_default_keeps_the_leaf_on_float() {
        // The default Placeholder policy leaves the leaf in the topology on a float
        // (bit-identical to pre-R1134) — the slot is preserved, no reflow.
        let topo = abc_signal();
        let reorg = Rc::new(DockReorganizer::new(Rc::clone(&topo))); // default Placeholder
        let mut ext = DockPanelExternal::new("b").with_reorganizer(reorg);
        assert_eq!(
            ext.query("float_policy"),
            Some(IntrospectValue::Text("placeholder".to_string())),
        );
        ext.invoke(TEAR_OFF_EVENT, IntrospectValue::Null).unwrap();
        assert!(
            topo.get().unwrap().panel_ids().contains(&"b"),
            "Placeholder keeps b's leaf (slot preserved)",
        );
        assert_eq!(topo.get().unwrap().panel_ids().len(), 3, "no reflow");
        assert_eq!(
            ext.query("lifecycle"),
            Some(IntrospectValue::Text("Floating".to_string())),
            "the chart still floats (the window side); only the leaf stays",
        );
    }

    #[test]
    fn r1134_live_drag_collapses_only_on_release_not_mid_drag() {
        // ★The release-as-floated timing: a mid-drag escape (drag_to_at) must NOT
        // remove the leaf — the slot stays put DURING the drag (so the live factory
        // is not disturbed) and collapses once on the release escape.
        let topo = abc_signal();
        let reorg = collapse_reorg(&topo);
        let mut ext = DockPanelExternal::new("b").with_reorganizer(reorg);
        let _ = ext.begin_drag();
        // Mid-drag escape: cursor left every same-window zone, the drag verdict landed.
        ext.drag_to_at(&dummy_payload(), &upd(None, (10.0, 10.0), None, true));
        assert!(
            topo.get().unwrap().panel_ids().contains(&"b"),
            "MID-DRAG: b's leaf is STILL docked (placeholder-style during the drag)",
        );
        assert_eq!(
            ext.query("lifecycle"),
            Some(IntrospectValue::Text("Floating".to_string())),
            "the chart floated mid-drag (the live follow), but the slot stayed",
        );
        // Release while escaped → settle as floated → collapse now.
        ext.drag_release(&dummy_payload(), None);
        assert!(
            !topo.get().unwrap().panel_ids().contains(&"b"),
            "ON RELEASE: b collapsed (release-as-floated)",
        );
    }

    #[test]
    fn r1134_two_layer_consistency_under_collapse() {
        // The R1131 2-layer invariant holds under collapse: the chart's Floating
        // state and the topology's absence of the leaf agree through a float / dock-
        // back cycle (chart Floating ⟺ leaf collapsed out).
        let topo = abc_signal();
        let reorg = collapse_reorg(&topo);
        let mut ext = DockPanelExternal::new("c").with_reorganizer(reorg);
        let docked = |ext: &DockPanelExternal| matches!(ext.query("lifecycle"), Some(IntrospectValue::Text(ref s)) if s == "Docked");
        assert!(
            docked(&ext) && topo.get().unwrap().panel_ids().contains(&"c"),
            "start: docked"
        );
        ext.invoke(TEAR_OFF_EVENT, IntrospectValue::Null).unwrap();
        assert!(
            !docked(&ext) && !topo.get().unwrap().panel_ids().contains(&"c"),
            "floated: chart Floating ⟺ leaf collapsed out",
        );
        ext.invoke(TEAR_OFF_EVENT, IntrospectValue::Null).unwrap();
        assert!(
            docked(&ext) && topo.get().unwrap().panel_ids().contains(&"c"),
            "docked back: chart Docked ⟺ leaf present",
        );
    }

    // ── R1136 §5.51 PR-39 — a floater WINDOW MOVE stays floating on release ──

    fn window_move_update(
        cursor: (f64, f64),
        over_window: Option<&'static str>,
    ) -> DragUpdate<'static> {
        DragUpdate {
            over: Some(drop_point("a", 0.5, 0.05)),
            cursor,
            over_window,
            source_window: Some("torn-a"),
            became_drag: true,
            press_cursor: (100.0, 10.0),
            source_window_origin: None,
        }
    }

    #[test]
    fn r1136_floater_window_move_release_stays_floating() {
        // ★A borderless floater's title-bar drag is a WINDOW MOVE; releasing it NOT
        // over another window's dock must leave it FLOATING (no snap-back home). The
        // R1116 move was wrongly docking the panel back on release (the cursor sits
        // over the floater's OWN header = a self-drop = the snap-back-home arm).
        let mut ext = DockPanelExternal::new("a")
            .with_floating_window("torn-a")
            .with_initial_floating(true);
        assert_eq!(
            ext.query("lifecycle"),
            Some(IntrospectValue::Text("Floating".to_string())),
            "the panel starts floating (it IS a floater)",
        );
        let _ = ext.begin_drag();
        ext.drag_to_at(&dummy_payload(), &window_move_update((90.0, 10.0), None));
        let mut moves: Vec<Intent> = Vec::new();
        ext.drain_intents(&mut |i| moves.push(i));
        assert!(
            moves.iter().any(|i| i.tag.as_ref() == WINDOW_MOVE_EVENT),
            "the move emits a window_move (not a tear-off): {moves:?}",
        );
        // Release NOT over another window (over_window None) — over the own header.
        ext.drag_release_at(&dummy_payload(), &window_move_update((80.0, 10.0), None));
        let mut after: Vec<Intent> = Vec::new();
        ext.drain_intents(&mut |i| after.push(i));
        assert!(
            !after
                .iter()
                .any(|i| i.tag.as_ref() == TEAR_OFF_REDOCK_EVENT),
            "★a floater window-move release must NOT snap back home: {after:?}",
        );
        assert_eq!(
            ext.query("lifecycle"),
            Some(IntrospectValue::Text("Floating".to_string())),
            "the chart stays Floating — the floater is still floating",
        );
        assert!(!ext.is_dragging(), "the gesture ended cleanly");
    }

    #[test]
    fn r1136_floater_release_over_another_window_still_redocks() {
        // The complement: a floater release that DID land on ANOTHER window's dock
        // (over_window Some) redocks via the cross-window dock-at — the move stays-
        // floating no-op applies ONLY when no redock target was hit.
        let mut ext = DockPanelExternal::new("a")
            .with_floating_window("torn-a")
            .with_initial_floating(true);
        let _ = ext.begin_drag();
        ext.drag_release_at(
            &dummy_payload(),
            &DragUpdate {
                over: Some(drop_point("viewport", 0.15, 0.5)),
                cursor: (50.0, 50.0),
                over_window: Some("main"),
                source_window: Some("torn-a"),
                became_drag: false,
                press_cursor: (50.0, 50.0),
                source_window_origin: None,
            },
        );
        assert!(
            matches!(ext.query("redock_at"), Some(IntrospectValue::Json(_))),
            "a release over another window's dock redocks (redock_at recorded)",
        );
    }

    #[test]
    fn r1136_docked_panel_snap_back_unaffected() {
        // The fix is floater-only: a DOCKED panel (no floating_window match — the
        // drag's source is the dock window) that floated mid-drag and snapped back
        // still restores home (the existing tear-off behaviour is untouched).
        let mut ext = DockPanelExternal::new("a").with_initial_floating(true);
        let _ = ext.begin_drag();
        // Release over the own panel (snap-back), source_window = the dock window
        // ("main"), NOT a floating window → the floater guard does NOT apply.
        ext.drag_release_at(
            &dummy_payload(),
            &DragUpdate {
                over: Some(drop_point("a", 0.5, 0.5)),
                cursor: (10.0, 10.0),
                over_window: None,
                source_window: Some("main"),
                became_drag: false,
                press_cursor: (10.0, 10.0),
                source_window_origin: None,
            },
        );
        let mut after: Vec<Intent> = Vec::new();
        ext.drain_intents(&mut |i| after.push(i));
        assert!(
            after
                .iter()
                .any(|i| i.tag.as_ref() == TEAR_OFF_REDOCK_EVENT),
            "a docked panel's floated snap-back still restores home: {after:?}",
        );
    }

    #[test]
    fn r1137_floater_move_over_zone_arms_then_disarms_the_chart() {
        // R1137 §5.51.1 — the holistic redesign: a floater's live move over another
        // window's dock zone ARMS the chart (`RedockArmed`, the binding paints the
        // preview); leaving every zone disarms it back to plain `Floating`. Driven
        // from the shell-resolved `over_window` during the window-move.
        let mut ext = DockPanelExternal::new("a")
            .with_floating_window("torn-a")
            .with_initial_floating(true);
        let _ = ext.begin_drag();
        // Move over another window's dock zone (over_window Some) → armed.
        ext.drag_to_at(
            &dummy_payload(),
            &window_move_update((90.0, 10.0), Some("main")),
        );
        assert_eq!(
            ext.query("lifecycle"),
            Some(IntrospectValue::Text("RedockArmed".to_string())),
            "a floater move over a dock zone ARMS the redock (preview shows)",
        );
        // is_floating stays true while armed (it is still a floating window).
        ext.drag_to_at(&dummy_payload(), &window_move_update((80.0, 10.0), None));
        assert_eq!(
            ext.query("lifecycle"),
            Some(IntrospectValue::Text("Floating".to_string())),
            "leaving every zone DISARMS back to plain floating",
        );
    }

    // ── R1103 §5.51 PR-33 — AI-primary cross-window redock invoke (slice 3) ──

    #[test]
    fn r1103_invoke_tear_off_redock_at_enqueues_the_dock_at_without_a_pointer_drag() {
        // The AI-primary driver: an `invoke("tear_off_redock_at", {window,
        // target, x_rel, y_rel})` emits the SAME dock-at intent + records the
        // SAME `redock_at` diagnostic the live `drag_release_at` cross-window
        // arm does — no pointer drag (the live floater→main path is blocked by
        // the R1095 moving-source follow-coordinate carry).
        let mut ext = DockPanelExternal::new("inspector");
        ext.invoke(
            TEAR_OFF_REDOCK_AT_EVENT,
            IntrospectValue::Json(serde_json::json!({
                "window": "main",
                "target": "property",
                "x_rel": 0.25,
                "y_rel": 0.75,
            })),
        )
        .expect("direct tear_off_redock_at invoke");
        let mut received: Vec<Intent> = Vec::new();
        ext.drain_intents(&mut |i| received.push(i));
        assert_eq!(received.len(), 1, "exactly one dock-at redock intent");
        assert_eq!(received[0].tag.as_ref(), TEAR_OFF_REDOCK_AT_EVENT);
        let IntrospectValue::Json(p) = &received[0].payload else {
            panic!("the dock-at payload is JSON");
        };
        assert_eq!(
            p["panel"], "inspector",
            "the panel id is the External's own"
        );
        assert_eq!(
            p["window"], "main",
            "carries the TARGET window from the args"
        );
        assert_eq!(p["target"], "property", "carries the target dock zone tag");
        assert!(
            (p["x_rel"].as_f64().expect("x_rel") - 0.25).abs() < 1e-6,
            "carries the supplied normalised cursor",
        );
        // The persistent diagnostic mirrors the fired intent.
        let Some(IntrospectValue::Json(observed)) = ext.query("redock_at") else {
            panic!("redock_at must surface the invoke-driven dock-at as JSON");
        };
        assert_eq!(observed["window"], "main");
        assert!(
            !ext.is_dragging(),
            "the invoke is a complete gesture, not in-flight"
        );
    }

    #[test]
    fn r1103_invoke_tear_off_redock_at_defaults_omitted_zone_to_centre() {
        // `target`/`x_rel`/`y_rel` are optional — a caller that only names the
        // window gets the zone centre (0.5, 0.5) + an empty target tag, so a
        // minimal AI redock still produces a well-formed payload.
        let mut ext = DockPanelExternal::new("inspector");
        ext.invoke(
            TEAR_OFF_REDOCK_AT_EVENT,
            IntrospectValue::Json(serde_json::json!({ "window": "main" })),
        )
        .expect("window-only redock invoke");
        let Some(IntrospectValue::Json(observed)) = ext.query("redock_at") else {
            panic!("redock_at recorded");
        };
        assert_eq!(observed["target"], "");
        assert!((observed["x_rel"].as_f64().expect("x_rel") - 0.5).abs() < 1e-6);
        assert!((observed["y_rel"].as_f64().expect("y_rel") - 0.5).abs() < 1e-6);
    }

    #[test]
    fn r1103_invoke_tear_off_redock_at_rejects_a_non_json_or_window_less_payload() {
        let mut ext = DockPanelExternal::new("inspector");
        assert_eq!(
            ext.invoke(TEAR_OFF_REDOCK_AT_EVENT, IntrospectValue::Null),
            Err(InvokeError::TypeMismatch),
            "a non-JSON payload is a type mismatch",
        );
        assert_eq!(
            ext.invoke(
                TEAR_OFF_REDOCK_AT_EVENT,
                IntrospectValue::Json(serde_json::json!({ "target": "property" })),
            ),
            Err(InvokeError::TypeMismatch),
            "a payload missing the required `window` is a type mismatch",
        );
        // A rejected invoke records nothing.
        assert_eq!(ext.query("redock_at"), Some(IntrospectValue::Null));
    }

    #[test]
    fn r1100_cross_window_with_no_zone_falls_through_to_float() {
        // over_window=Some but over=None (the cursor mapped into another window
        // but onto no drop zone) is NOT a dock-at — it falls through to the
        // same-window release path (escape-float), so a drop in another
        // window's dead space floats, it does not redock.
        let mut ext = DockPanelExternal::new("inspector");
        let _ = ext.begin_drag();
        cross_detach(&mut ext, None, (640.0, 300.0));
        let mut drained: Vec<Intent> = Vec::new();
        ext.drain_intents(&mut |i| drained.push(i));
        ext.drag_release_at(
            &dummy_payload(),
            &upd(None, (900.0, 50.0), Some("main"), false),
        );
        let mut after: Vec<Intent> = Vec::new();
        ext.drain_intents(&mut |i| after.push(i));
        assert!(
            after
                .iter()
                .all(|i| i.tag.as_ref() != TEAR_OFF_REDOCK_AT_EVENT),
            "no zone under the cursor → no dock-at (got {after:?})",
        );
    }

    #[test]
    fn r1097_threshold_move_detaches_into_a_follower() {
        // R1097 §5.51 PR-32 / R1101 / R1110 — once the router declares the press
        // a drag (`became_drag`) AND the cursor is over no same-window dock zone
        // (here `over` is `None` = escaped, the R1110 gate), the panel tears into
        // a live floating follower: latch `detached` + emit one ensure+position
        // follow carrying the panel id and cursor. The grab sample (router still
        // calls it a click) does not detach. R1101 sources the verdict from the
        // router (not a private distance latch); R1110 re-gates the float on
        // escape (a verdict move still over a zone stays docked — see
        // `r1110_threshold_move_over_a_panel_shows_preview_not_detach`).
        let mut ext = DockPanelExternal::new("a");
        let _ = ext.begin_drag();
        assert!(!ext.detached(), "fresh gesture has not detached");
        ext.drag_to_at(&dummy_payload(), &upd(None, (100.0, 100.0), None, false)); // grab
        assert!(!ext.detached(), "the grab sample alone does not detach");
        ext.drag_to_at(&dummy_payload(), &upd(None, (640.0, 300.0), None, true)); // > threshold
        assert!(ext.detached(), "the router's drag verdict detaches");
        let mut received: Vec<Intent> = Vec::new();
        ext.drain_intents(&mut |i| received.push(i));
        assert_eq!(received.len(), 1, "one follow on the move that detached");
        assert_eq!(received[0].tag.as_ref(), TEAR_OFF_FOLLOW_EVENT);
        assert_eq!(
            follow_fields(&received[0].payload),
            ("a".to_string(), 640.0, 300.0),
        );
    }

    #[test]
    fn r1101_detach_keys_off_router_verdict_not_distance() {
        // R1101 (F1 clearance) — the panel detaches from the router's
        // click-vs-drag verdict ([`DragUpdate::became_drag`]), NOT its own
        // distance computation. Proven by inverting the geometry against the
        // verdict: a FAR move the router still classifies a click
        // (`became_drag: false`) does NOT detach — the R1097 private
        // `detach_latch`, measuring its own distance from the first sample,
        // would have — and a NEAR move the router calls a drag
        // (`became_drag: true`) DOES detach, where that sub-threshold distance
        // would not. The router owns the SSOT (it sees the real press point);
        // the panel consumes it.
        let mut ext = DockPanelExternal::new("a");
        let _ = ext.begin_drag();
        // Far in pixels, but the router has not called this press a drag yet.
        ext.drag_to_at(&dummy_payload(), &upd(None, (5000.0, 5000.0), None, false));
        assert!(
            !ext.detached(),
            "a far move the router still calls a click does not detach",
        );
        let mut received: Vec<Intent> = Vec::new();
        ext.drain_intents(&mut |i| received.push(i));
        assert!(
            received.is_empty(),
            "no follow while the router calls it a click",
        );
        // A one-pixel move, but the router's verdict has flipped to drag.
        ext.drag_to_at(&dummy_payload(), &upd(None, (5001.0, 5000.0), None, true));
        assert!(
            ext.detached(),
            "a near move the router calls a drag detaches",
        );
        received.clear();
        ext.drain_intents(&mut |i| received.push(i));
        assert_eq!(received.len(), 1, "one follow once the verdict is a drag");
        assert_eq!(received[0].tag.as_ref(), TEAR_OFF_FOLLOW_EVENT);
    }

    #[test]
    fn r1110_threshold_move_over_a_panel_shows_preview_not_detach() {
        // R1110 PR-36 — detach-on-escape. A drag-verdict move whose cursor is
        // STILL over a same-window dock panel stays docked: no float, no follow,
        // and the reorganize/split preview is set (the release then applies the
        // in-dock SplitInsert/Tabify). This INVERTS the PR-32 behaviour (which
        // floated even over a zone — the in-dock reorder/split flicker R1110
        // fixes).
        let preview = Rc::new(Signal::new(None));
        let mut ext = DockPanelExternal::new("a").with_drop_preview(Rc::clone(&preview));
        let _ = ext.begin_drag();
        // Grab, then a drag-verdict move whose cursor is over panel "b"'s centre.
        ext.drag_to_at(
            &dummy_payload(),
            &upd(Some(drop_point("b", 0.5, 0.5)), (10.0, 10.0), None, false),
        );
        ext.drag_to_at(
            &dummy_payload(),
            &upd(Some(drop_point("b", 0.5, 0.5)), (160.0, 100.0), None, true),
        );
        assert!(
            !ext.detached(),
            "a drag verdict over a same-window zone does NOT float (R1110)",
        );
        let mut received: Vec<Intent> = Vec::new();
        ext.drain_intents(&mut |i| received.push(i));
        assert!(
            received.is_empty(),
            "no tear-off follow while docked over a zone, got {received:?}",
        );
        assert!(
            preview.get().is_some(),
            "the in-dock reorganize preview is shown over the zone",
        );
    }

    #[test]
    fn r1110_escape_after_over_a_panel_detaches() {
        // R1110 PR-36 — the float fires only once the cursor LEAVES every
        // same-window dock zone. A drag held over a panel (no float) that then
        // escapes to empty space (`over` None) tears into a follower at the
        // escape move, not before.
        let mut ext = DockPanelExternal::new("a");
        let _ = ext.begin_drag();
        ext.drag_to_at(
            &dummy_payload(),
            &upd(Some(drop_point("b", 0.5, 0.5)), (10.0, 10.0), None, true),
        );
        assert!(!ext.detached(), "over a zone: still docked");
        ext.drag_to_at(&dummy_payload(), &upd(None, (900.0, 900.0), None, true));
        assert!(ext.detached(), "escaping every zone floats the panel");
        let mut received: Vec<Intent> = Vec::new();
        ext.drain_intents(&mut |i| received.push(i));
        assert_eq!(received.len(), 1, "one follow on the escape move");
        assert_eq!(received[0].tag.as_ref(), TEAR_OFF_FOLLOW_EVENT);
    }

    #[test]
    fn r1110_cross_window_over_counts_as_escape_and_detaches() {
        // R1110 PR-36 — a cursor over ANOTHER window's dock zone
        // (`over_window: Some`) has left THIS window's dock surface, so it is an
        // escape and the panel floats (the PR-33 cross-window redock then
        // resolves at the target window). `same_window_over` is None for a
        // cross-window over, so the detach gate fires.
        let mut ext = DockPanelExternal::new("a");
        let _ = ext.begin_drag();
        ext.drag_to_at(&dummy_payload(), &upd(None, (10.0, 10.0), None, false)); // grab
        ext.drag_to_at(
            &dummy_payload(),
            &upd(
                Some(drop_point("b", 0.5, 0.5)),
                (700.0, 300.0),
                Some("other"),
                true,
            ),
        );
        assert!(
            ext.detached(),
            "a drag verdict over another window's zone floats (escape)",
        );
        let mut received: Vec<Intent> = Vec::new();
        ext.drain_intents(&mut |i| received.push(i));
        assert_eq!(received.len(), 1, "one follow on the cross-window escape");
        assert_eq!(received[0].tag.as_ref(), TEAR_OFF_FOLLOW_EVENT);
    }

    #[test]
    fn r1097_subthreshold_wobble_does_not_detach() {
        // A press with a tiny wobble under DRAG_CLICK_THRESHOLD_PX is still a
        // click, not a tear-off: no detach, no follow (the dead zone the
        // router's own click-vs-drag SSOT shares).
        let mut ext = DockPanelExternal::new("a");
        let _ = ext.begin_drag();
        ext.drag_to_at(&dummy_payload(), &upd(None, (100.0, 100.0), None, false)); // grab
        ext.drag_to_at(&dummy_payload(), &upd(None, (102.0, 101.0), None, false)); // < threshold
        assert!(!ext.detached(), "a sub-threshold wobble stays a click");
        let mut received: Vec<Intent> = Vec::new();
        ext.drain_intents(&mut |i| received.push(i));
        assert!(received.is_empty(), "no follow under the drag threshold");
    }

    #[test]
    fn r1097_each_move_after_detach_emits_a_follow_for_live_tracking() {
        // Per-move follow: once detached, every move re-emits the position so
        // the floating window tracks the cursor (the binding reducer dedups a
        // stationary cursor at `Signal::set`, not here). The grab sample, being
        // sub-threshold, emits no follow.
        let mut ext = DockPanelExternal::new("a");
        let _ = ext.begin_drag();
        ext.drag_to_at(&dummy_payload(), &upd(None, (10.0, 20.0), None, false)); // grab
        ext.drag_to_at(&dummy_payload(), &upd(None, (30.0, 40.0), None, true)); // detach + follow
        ext.drag_to_at(&dummy_payload(), &upd(None, (50.0, 60.0), None, true)); // follow
        let mut received: Vec<Intent> = Vec::new();
        ext.drain_intents(&mut |i| received.push(i));
        assert_eq!(received.len(), 2, "one follow per move after the grab");
        assert!(
            received
                .iter()
                .all(|i| i.tag.as_ref() == TEAR_OFF_FOLLOW_EVENT)
        );
        assert_eq!(
            follow_fields(&received[1].payload),
            ("a".to_string(), 50.0, 60.0),
            "the last follow carries the latest cursor",
        );
    }

    #[test]
    fn r1094_escape_release_with_cursor_emits_follow_not_toggle() {
        // The router path forwards a cursor (drag_release_at), so an
        // escape-drop emits the non-toggling follow (final position), NOT
        // the legacy `tear_off` toggle that would race the live follow
        // (the R1071-R1078 double-toggle lesson).
        let mut ext = DockPanelExternal::new("a");
        let _ = ext.begin_drag();
        cross_detach(&mut ext, None, (640.0, 300.0));
        ext.drag_release_at(&dummy_payload(), &upd(None, (700.0, 320.0), None, false));
        assert!(ext.tear_off_fired(), "escape-drop floats");
        assert!(ext.detached());
        let mut received: Vec<Intent> = Vec::new();
        ext.drain_intents(&mut |i| received.push(i));
        assert_eq!(received.len(), 2, "move follow then release follow");
        assert!(
            received
                .iter()
                .all(|i| i.tag.as_ref() == TEAR_OFF_FOLLOW_EVENT)
        );
        assert_eq!(
            follow_fields(&received[1].payload),
            ("a".to_string(), 700.0, 320.0),
            "the release follow carries the release cursor",
        );
    }

    #[test]
    fn r1094_cursorless_escape_release_keeps_the_legacy_toggle() {
        // The pre-R1093 cursor-less path (direct `drag_release`, no `_at`)
        // has no forwarded cursor, so an escape still floats via the legacy
        // `tear_off` toggle — backward compatible (the existing r1081
        // tests drive this path).
        let mut ext = DockPanelExternal::new("a");
        let _ = ext.begin_drag();
        ext.drag_release(&dummy_payload(), None);
        let mut received: Vec<Intent> = Vec::new();
        ext.drain_intents(&mut |i| received.push(i));
        assert_eq!(received.len(), 1);
        assert_eq!(
            received[0].tag.as_ref(),
            TEAR_OFF_EVENT,
            "cursor-less escape keeps the toggle",
        );
    }

    #[test]
    fn r1094_detached_then_snap_back_restores_via_redock() {
        // A drag that tore the panel off (escaped) then returned and
        // released over its own panel / a dead zone restores by removing
        // the floating window this gesture created.
        let mut ext = DockPanelExternal::new("a");
        let _ = ext.begin_drag();
        cross_detach(&mut ext, None, (640.0, 300.0)); // detach
        ext.drag_release_at(
            &dummy_payload(),
            &upd(Some(drop_point("a", 0.5, 0.5)), (120.0, 40.0), None, false),
        );
        assert!(!ext.detached(), "restore clears the latch");
        assert!(
            !ext.tear_off_fired(),
            "a restored snap-back is not a tear-off"
        );
        let mut received: Vec<Intent> = Vec::new();
        ext.drain_intents(&mut |i| received.push(i));
        assert_eq!(
            received.len(),
            2,
            "the escaped move's follow, then the redock"
        );
        assert_eq!(received[0].tag.as_ref(), TEAR_OFF_FOLLOW_EVENT);
        assert_eq!(received[1].tag.as_ref(), TEAR_OFF_REDOCK_EVENT);
        assert_eq!(received[1].payload.as_str(), Some("a"));
    }

    #[test]
    fn r1094_detached_then_cancel_restores_via_redock() {
        // An OS-cancelled drag that had detached restores (removes the
        // floating window).
        let mut ext = DockPanelExternal::new("a");
        let _ = ext.begin_drag();
        cross_detach(&mut ext, None, (640.0, 300.0)); // detach
        ext.drag_cancel(&dummy_payload());
        assert!(!ext.detached());
        let mut received: Vec<Intent> = Vec::new();
        ext.drain_intents(&mut |i| received.push(i));
        assert_eq!(received.len(), 2);
        assert_eq!(received[0].tag.as_ref(), TEAR_OFF_FOLLOW_EVENT);
        assert_eq!(received[1].tag.as_ref(), TEAR_OFF_REDOCK_EVENT);
    }

    #[test]
    fn r1094_never_detached_snap_back_does_not_redock() {
        // A plain snap-back (the drag never escaped) commits nothing — no
        // spurious redock that would remove an unrelated floating window.
        let mut ext = DockPanelExternal::new("a");
        let _ = ext.begin_drag();
        ext.drag_to_at(
            &dummy_payload(),
            &upd(Some(drop_point("a", 0.5, 0.5)), (100.0, 100.0), None, false),
        );
        ext.drag_release_at(
            &dummy_payload(),
            &upd(Some(drop_point("a", 0.5, 0.5)), (100.0, 100.0), None, false),
        );
        let mut received: Vec<Intent> = Vec::new();
        ext.drain_intents(&mut |i| received.push(i));
        assert!(received.is_empty(), "never-escaped snap-back is a no-op");
    }

    #[test]
    fn r1094_detached_redock_over_zone_removes_floating_then_docks() {
        // With a coordinator: a drag that detached then dropped on a dock
        // zone removes the floating window (redock) AND applies the dock.
        let topology = Rc::new(Signal::new(Some(ab_topology())));
        let reorganizer = Rc::new(DockReorganizer::new(Rc::clone(&topology)));
        let mut ext = DockPanelExternal::new("a").with_reorganizer(Rc::clone(&reorganizer));
        let _ = ext.begin_drag();
        cross_detach(&mut ext, None, (640.0, 300.0)); // detach
        // Drop on the centre of "b" → swap; the redock removes the floater
        // first so the reorganizer re-places a single panel.
        ext.drag_release_at(
            &dummy_payload(),
            &upd(Some(drop_point("b", 0.5, 0.5)), (300.0, 300.0), None, false),
        );
        assert!(!ext.detached());
        let mut received: Vec<Intent> = Vec::new();
        ext.drain_intents(&mut |i| received.push(i));
        assert_eq!(received.len(), 2, "follow (escape) then redock");
        assert_eq!(received[0].tag.as_ref(), TEAR_OFF_FOLLOW_EVENT);
        assert_eq!(received[1].tag.as_ref(), TEAR_OFF_REDOCK_EVENT);
        assert_eq!(
            reorganizer.last_outcome().as_deref(),
            Some("a -> b"),
            "the dock still applied after the redock",
        );
    }

    #[test]
    fn r1094_detached_query_is_read_only_and_resets_per_gesture() {
        let mut ext = DockPanelExternal::new("a");
        assert_eq!(
            ext.query("detached"),
            Some(IntrospectValue::Bool(false)),
            "detached is false before any drag",
        );
        let _ = ext.begin_drag();
        cross_detach(&mut ext, None, (640.0, 300.0));
        assert_eq!(
            ext.query("detached"),
            Some(IntrospectValue::Bool(true)),
            "detached is true after a threshold move",
        );
        // Framework-owned, not AI-writable.
        assert_eq!(
            ext.intervene("detached", IntrospectValue::Bool(false)),
            Err(InterveneError::ReadOnly),
        );
        // A fresh gesture resets it.
        let _ = ext.begin_drag();
        assert_eq!(
            ext.query("detached"),
            Some(IntrospectValue::Bool(false)),
            "begin_drag resets detached",
        );
    }

    #[test]
    fn r1095_no_tab_well_emits_no_tablist() {
        // R1095 §5.51 §5.27 — a Leaf/Split-only topology has no tab wells,
        // so it contributes no tablist a11y (a docked panel is not a tab).
        let nodes = dock_tablist_access_nodes(&ab_topology(), None);
        assert!(nodes.is_empty(), "no Tabs well → no tablist a11y nodes");
    }

    #[test]
    fn r1095_tab_well_emits_tablist_tabs_and_tabpanel() {
        let topo = DockTopology::new(DockNode::tabs(
            "well-1",
            vec!["a".into(), "b".into(), "c".into()],
            1,
        ));
        let nodes = dock_tablist_access_nodes(&topo, None);
        // The tablist names the well and owns the per-tab tags in order.
        let tablist = nodes
            .iter()
            .find(|n| n.role.aria_name() == "tablist")
            .expect("a tablist node");
        assert_eq!(tablist.tag, "well-1", "tablist tag is the well id");
        assert_eq!(
            tablist.children,
            vec![
                composite_tab_tag("well-1", 0),
                composite_tab_tag("well-1", 1),
                composite_tab_tag("well-1", 2),
            ],
            "tablist owns the per-tab composite tags in order",
        );
        // One tab node per panel; the active one is aria-selected.
        let tabs: Vec<&AccessNode> = nodes
            .iter()
            .filter(|n| n.role.aria_name() == "tab")
            .collect();
        assert_eq!(tabs.len(), 3, "one tab node per panel");
        assert_eq!(tabs[1].selected, Some(true), "the active tab is selected");
        assert_eq!(
            tabs[0].selected,
            Some(false),
            "an inactive tab is not selected"
        );
        assert_eq!(tabs[2].selected, Some(false));
        assert_eq!(
            tabs[0].size_of_set,
            Some(3),
            "aria-setsize is the tab count"
        );
        assert_eq!(tabs[0].position_in_set, Some(1), "aria-posinset is 1-based");
        assert_eq!(tabs[2].position_in_set, Some(3));
        assert_eq!(
            tabs[1].tag,
            composite_tab_tag("well-1", 1),
            "tab tag mirrors the painted composite tag",
        );
        // The tabpanel is the active panel (the header-suppressed content).
        let panel = nodes
            .iter()
            .find(|n| n.role.aria_name() == "tabpanel")
            .expect("a tabpanel node");
        assert_eq!(panel.tag, "b", "tabpanel tag is the active panel id");
    }

    #[test]
    fn r1095_tab_well_nested_in_split_is_walked() {
        let topo = DockTopology::new(DockNode::split_horizontal(
            "root",
            0.5,
            DockNode::leaf("side"),
            DockNode::tabs("well-x", vec!["a".into(), "b".into()], 0),
        ));
        let nodes = dock_tablist_access_nodes(&topo, None);
        assert!(
            nodes
                .iter()
                .any(|n| n.role.aria_name() == "tablist" && n.tag == "well-x"),
            "a tab well nested in a Split still emits a tablist",
        );
        assert_eq!(
            nodes
                .iter()
                .filter(|n| n.role.aria_name() == "tablist")
                .count(),
            1,
            "the Leaf sibling contributes no tablist",
        );
    }

    #[test]
    fn r1095_focused_strip_marks_active_tab_as_roving_descendant() {
        let topo = DockTopology::new(DockNode::tabs("well-1", vec!["a".into(), "b".into()], 1));
        // The strip owning focus makes its active tab the roving descendant.
        let focused = dock_tablist_access_nodes(&topo, Some("well-1"));
        let active = focused
            .iter()
            .find(|n| n.tag == composite_tab_tag("well-1", 1))
            .expect("active tab");
        assert!(
            active.state.focused,
            "the active tab is focused when the strip owns focus"
        );
        let inactive = focused
            .iter()
            .find(|n| n.tag == composite_tab_tag("well-1", 0))
            .expect("inactive tab");
        assert!(
            !inactive.state.focused,
            "an inactive tab is not the roving descendant"
        );
        // No strip focus → no tab is the roving descendant.
        let unfocused = dock_tablist_access_nodes(&topo, None);
        let a = unfocused
            .iter()
            .find(|n| n.tag == composite_tab_tag("well-1", 1))
            .expect("active tab");
        assert!(!a.state.focused, "no strip focus → no roving descendant");
    }

    #[test]
    fn r683_dock_panel_external_query_panel_id() {
        let ext = DockPanelExternal::new("my_panel");
        let val = ext.query("panel_id").expect("queryable");
        assert_eq!(val.as_str(), Some("my_panel"));
    }

    #[test]
    fn r683_dock_panel_external_query_tear_off_fired_starts_false() {
        let ext = DockPanelExternal::new("p1");
        let val = ext.query("tear_off_fired").expect("queryable");
        assert_eq!(val, IntrospectValue::Bool(false));
    }

    #[test]
    fn r1081_query_drop_preview_reflects_the_live_drag() {
        let preview = Rc::new(Signal::new(None));
        let mut ext = DockPanelExternal::new("a").with_drop_preview(Rc::clone(&preview));
        // No drag → null.
        assert_eq!(ext.query("drop_preview"), Some(IntrospectValue::Null));
        let _ = ext.begin_drag();
        ext.drag_to(&dummy_payload(), Some(drop_point("b", 0.5, 0.5)));
        let IntrospectValue::Json(obj) = ext.query("drop_preview").expect("queryable") else {
            panic!("drop_preview must be JSON mid-drag");
        };
        assert_eq!(obj.get("source").and_then(|v| v.as_str()), Some("a"));
        assert_eq!(obj.get("target").and_then(|v| v.as_str()), Some("b"));
        assert_eq!(obj.get("zone").and_then(|v| v.as_str()), Some("Center"));
    }

    #[test]
    fn r683_dock_panel_external_invoke_unknown_event_returns_err() {
        let mut ext = DockPanelExternal::new("p1");
        let res = ext.invoke("send", IntrospectValue::Text("UnknownEvent".to_string()));
        assert!(res.is_err());
    }

    #[test]
    fn r1081_view_dock_panel_root_opts_in_as_drop_target() {
        run_in_owner(|| {
            let style = DockPanelStyle::m3_default(PANEL_TAG);
            let scene = view_dock_panel("Title", empty_content(), &theme_light(), &style, None);
            let Scene::Container(outer) = &scene else {
                panic!()
            };
            assert!(
                outer.layout.drop_target,
                "every dock panel opts in as a drop target for the R742 router climb",
            );
        });
    }

    #[test]
    fn r1116_drop_target_style_default_true_override_false() {
        // R1116 §5.51 PR-38 — the drop_target lift (carry-② anticipated). Default
        // true (a docked panel); a sole-floater panel sets it false so its header
        // drag escapes into a window move instead of a self-redock.
        assert!(
            DockPanelStyle::m3_default(PANEL_TAG).drop_target,
            "default is a drop target"
        );
        let floater = DockPanelStyle::m3_default(PANEL_TAG).with_drop_target(false);
        assert!(!floater.drop_target, "with_drop_target(false) opts out");
        run_in_owner(|| {
            let Scene::Container(outer) =
                view_dock_panel("T", empty_content(), &theme_light(), &floater, None)
            else {
                panic!()
            };
            assert!(
                !outer.layout.drop_target,
                "a drop_target=false panel root is NOT a router drop target",
            );
        });
    }

    #[test]
    fn r1116_sole_floater_header_drag_is_grab_offset_window_move() {
        // R1116→R1120 §5.51 §5.15 PR-38/39 — a drag whose source_window is the
        // panel's OWN floating window (the title-bar drag of the borderless
        // floater) is a WINDOW MOVE: the widget emits a dedicated WINDOW_MOVE
        // carrying the DESKTOP-frame increment (`window_move_delta`), so the
        // binding's `pos += delta` keeps the grabbed point under the cursor
        // WITHOUT the moving-frame feedback that jittered the live drag (R1120).
        // The ACTUAL window origin cancels in the increment, so the deltas are the
        // same whatever the floater's position — window-INDEPENDENT.
        let mut floater = DockPanelExternal::new("viewport").with_floating_window("torn-viewport");
        let _ = floater.begin_drag();
        let press = (40.0, 8.0);
        // source_window_origin = the floater's ACTUAL outer origin (shell-stashed).
        let mv = |cursor: (f64, f64)| DragUpdate {
            over: None,
            cursor,
            over_window: None,
            source_window: Some("torn-viewport"),
            became_drag: true,
            press_cursor: press,
            source_window_origin: Some((10, 10)),
        };
        floater.drag_to_at(&dummy_payload(), &mv((100.0, 48.0)));
        floater.drag_to_at(&dummy_payload(), &mv((140.0, 88.0)));
        let mut got: Vec<Intent> = Vec::new();
        floater.drain_intents(&mut |i| got.push(i));
        assert_eq!(got.len(), 2, "two moves = two window-move intents");
        // R1118 — a window move emits the dedicated WINDOW_MOVE event (a
        // displacement), NOT tear_off_follow (a cursor). The wire form is honest.
        for i in &got {
            assert_eq!(
                i.tag.as_ref(),
                WINDOW_MOVE_EVENT,
                "the floater header drag is a window move, not a tear_off_follow",
            );
        }
        // The payload carries the DESKTOP-frame increment (dx, dy).
        let wm = |p: &IntrospectValue| -> (String, f64, f64) {
            let IntrospectValue::Json(v) = p else {
                panic!("window_move payload is Json");
            };
            (
                v.get("panel")
                    .and_then(serde_json::Value::as_str)
                    .expect("panel")
                    .to_string(),
                v.get("dx").and_then(serde_json::Value::as_f64).expect("dx"),
                v.get("dy").and_then(serde_json::Value::as_f64).expect("dy"),
            )
        };
        // First move: desktop cursor (10+100, 10+48)=(110,58) minus the grab's
        // desktop (10+40, 10+8)=(50,18) = (60,40). Second: increment from the
        // previous desktop (110,58) to (10+140, 10+88)=(150,98) = (40,40).
        assert_eq!(
            wm(&got[0].payload),
            ("viewport".into(), 60.0, 40.0),
            "first delta = desktop cursor - grab desktop (window-independent)",
        );
        assert_eq!(
            wm(&got[1].payload),
            ("viewport".into(), 40.0, 40.0),
            "later delta = desktop frame increment; the binding adds it to the window pos",
        );

        // Contrast: a docked tear-off (no floating_window declared) jumps to the
        // RAW cursor — no grab-offset, and on the tear_off_follow wire.
        let mut docked = DockPanelExternal::new("viewport");
        let _ = docked.begin_drag();
        docked.drag_to_at(
            &dummy_payload(),
            &DragUpdate {
                over: Some(drop_point("viewport", 0.5, 0.5)),
                cursor: (40.0, 8.0),
                over_window: None,
                source_window: Some("main"),
                became_drag: false,
                press_cursor: (40.0, 8.0),
                source_window_origin: None,
            },
        ); // grab over the panel's own zone — NOT a window move (no floating_window)
        docked.drag_to_at(
            &dummy_payload(),
            &DragUpdate {
                over: None,
                cursor: (140.0, 88.0),
                over_window: None,
                source_window: Some("main"),
                became_drag: true,
                press_cursor: (40.0, 8.0),
                source_window_origin: None,
            },
        ); // escape + follow
        let mut got2: Vec<Intent> = Vec::new();
        docked.drain_intents(&mut |i| got2.push(i));
        let follow = got2
            .iter()
            .find(|i| i.tag.as_ref() == TEAR_OFF_FOLLOW_EVENT)
            .expect("escape emits a follow");
        assert_eq!(
            follow_fields(&follow.payload),
            ("viewport".into(), 140.0, 88.0),
            "a docked tear-off jumps to the RAW cursor (no grab-offset)",
        );
    }

    /// R1119 PR-39 — closed-loop simulator for the borderless-floater title-bar
    /// window move. Models what the headless fixed-frame / stepwise-settle drives
    /// CANNOT: winit reports the cursor relative to the MOVING window, and the WM
    /// applies `set_outer_position` a few frames late — with the lag JITTERING
    /// frame-to-frame (a real compositor). Drives a desktop cursor trajectory
    /// through the loop and returns the per-frame WINDOW POSITION trace.
    ///
    /// `controller(actual_origin, cursor_local, declared) -> new_declared` is the
    /// position-update rule under test (it may capture per-gesture state).
    fn simulate_window_move<F: FnMut(Option<(f64, f64)>, (f64, f64), (f64, f64)) -> (f64, f64)>(
        cursor_path: &[(f64, f64)],
        o0: (f64, f64),
        lag_pattern: &[usize], // per-frame WM apply lag, cycled (real apply jitters)
        mut controller: F,
    ) -> Vec<(f64, f64)> {
        // declared[k] = the position commanded at frame k; actual_n = the position
        // commanded `lag` frames ago. The jitter is the perturbation that drives
        // the naive controller's marginal mode into a non-smooth (jittery) path.
        let mut declared_hist: Vec<(f64, f64)> = Vec::with_capacity(cursor_path.len());
        let mut declared = o0;
        for (n, &c) in cursor_path.iter().enumerate() {
            let lag = lag_pattern[n % lag_pattern.len()].max(1);
            let actual = if n >= lag { declared_hist[n - lag] } else { o0 };
            // winit reports the cursor RELATIVE to the actual on-screen window.
            let cursor_local = (c.0 - actual.0, c.1 - actual.1);
            declared = controller(Some(actual), cursor_local, declared);
            declared_hist.push(declared);
        }
        declared_hist
    }

    /// Max path "jerk" (|2nd difference| of the window position) over the moving
    /// frames `0..motion_end`. A SMOOTH move (constant cursor velocity) has jerk
    /// ~0; an oscillating/jittery move has large jerk. This is the jitter signal
    /// the handoff observed (per-frame deltas swinging ±, with big jumps).
    fn max_jerk(positions: &[(f64, f64)], motion_end: usize) -> f64 {
        (2..motion_end)
            .map(|n| {
                let jx = positions[n].0 - 2.0 * positions[n - 1].0 + positions[n - 2].0;
                let jy = positions[n].1 - 2.0 * positions[n - 1].1 + positions[n - 2].1;
                (jx * jx + jy * jy).sqrt()
            })
            .fold(0.0_f64, f64::max)
    }

    /// A realistic non-uniform WM apply-lag pattern (frames late, cycled). The
    /// jitter is what excites the naive controller's marginal oscillation.
    const APPLY_LAG_JITTER: &[usize] = &[1, 3, 1, 2, 4, 1, 3, 2];

    /// A constant-velocity desktop cursor path (the human's hand moving steadily
    /// — the SMOOTHEST possible input). 30 moving frames. Any jitter in the window
    /// path is therefore the controller's fault, not the input's.
    fn steady_drift(start: (f64, f64)) -> Vec<(f64, f64)> {
        (0..30)
            .map(|i| (start.0 + 15.0 * f64::from(i), start.1 + 10.0 * f64::from(i)))
            .collect()
    }

    #[test]
    fn r1119_naive_local_frame_window_move_jitters_under_apply_lag() {
        // The OLD (R1118) formula: new_declared = declared + (cursor_local −
        // press), measured in the MOVING frame → feedback → marginally-stable, so
        // the jittering apply-lag is never damped: even a perfectly STEADY cursor
        // produces a JITTERY window path (the PR-39 live defect). The fixed-frame
        // demo could not see it — it ran at effective apply-lag 0. This test
        // PROVES the harness catches the regression class.
        let grab = (40.0, 8.0);
        let press = grab;
        let path = steady_drift((100.0, 50.0));
        let o0 = (path[0].0 - grab.0, path[0].1 - grab.1); // grab under cursor at press
        let positions =
            simulate_window_move(&path, o0, APPLY_LAG_JITTER, |_actual, local, declared| {
                (
                    declared.0 + (local.0 - press.0),
                    declared.1 + (local.1 - press.1),
                )
            });
        let jerk = max_jerk(&positions, path.len());
        assert!(
            jerk > 10.0,
            "naive local-frame move JITTERS under apply-lag: window-path jerk {jerk} is large for a \
             perfectly steady cursor (a smooth move has jerk ~0)",
        );
    }

    #[test]
    fn r1119_desktop_frame_window_move_is_smooth_at_every_lag() {
        // The FIXED formula (`window_move_delta` desktop frame increment + the
        // binding's `pos += delta`). The desktop cursor (actual_origin +
        // cursor_local) is window-INDEPENDENT, so there is NO feedback — a steady
        // cursor yields a smooth window path (jerk ~0) at ANY apply-lag, including
        // the same jitter that breaks the naive controller.
        let grab = (40.0, 8.0);
        let path = steady_drift((100.0, 50.0));
        let o0 = (path[0].0 - grab.0, path[0].1 - grab.1);
        for pattern in [APPLY_LAG_JITTER, &[1], &[2], &[3], &[4]] {
            let mut prev: Option<(f64, f64)> = None;
            let positions = simulate_window_move(&path, o0, pattern, |actual, local, declared| {
                let (delta, new_prev) = window_move_delta(actual, local, grab, prev);
                prev = Some(new_prev);
                (declared.0 + delta.0, declared.1 + delta.1)
            });
            let jerk = max_jerk(&positions, path.len());
            assert!(
                jerk < 0.001,
                "desktop-frame move must be SMOOTH for lag {pattern:?}: jerk {jerk} (expected ~0)",
            );
        }
    }

    #[test]
    fn r1082_view_dock_panel_with_active_zone_appends_overlay_child() {
        run_in_owner(|| {
            let style = DockPanelStyle::m3_default(PANEL_TAG);
            // None → just header + content (no overlay), the static case.
            let Scene::Container(plain) =
                view_dock_panel("T", empty_content(), &theme_light(), &style, None)
            else {
                panic!()
            };
            assert_eq!(plain.children.len(), 2, "no active zone = no overlay");
            // Some(zone) → the overlay is an out-of-flow third child on top.
            let Scene::Container(active) = view_dock_panel(
                "T",
                empty_content(),
                &theme_light(),
                &style,
                Some(DockDropZone::Right),
            ) else {
                panic!()
            };
            assert_eq!(active.children.len(), 3, "active zone appends an overlay");
            let Scene::Container(overlay) = &active.children[2] else {
                panic!("overlay is the last child")
            };
            assert_eq!(overlay.layout.absolute_position, Some((0, 0)));
            assert!(overlay.layout.pointer_transparent);
            // None-zone is a no-op overlay (no extra child).
            let Scene::Container(none_zone) = view_dock_panel(
                "T",
                empty_content(),
                &theme_light(),
                &style,
                Some(DockDropZone::None),
            ) else {
                panic!()
            };
            assert_eq!(none_zone.children.len(), 2, "None zone paints no overlay");
        });
    }

    #[test]
    fn r1111_dock_drop_zone_highlight_shows_result_region() {
        use pinion_core::style::SizeValue;
        run_in_owner(|| {
            let theme = theme_light();
            // R1111 PR-37 — an edge zone highlights the RESULT half (the split
            // region the panel lands in), not the 25% classification band.
            let Scene::Container(left) = dock_drop_zone_highlight(DockDropZone::Left, &theme)
            else {
                panic!()
            };
            assert!(left.layout.pointer_transparent, "overlay never grabs input");
            assert_eq!(left.layout.absolute_position, Some((0, 0)));
            assert_eq!(left.children.len(), 1, "edge zone paints one strip");
            let Scene::Container(strip) = &left.children[0] else {
                panic!()
            };
            assert_eq!(
                strip.layout.size.width,
                SizeValue::Percent(super::DOCK_SPLIT_RESULT_PCT),
                "edge strip width is the split result half (50%), not the 25% band",
            );
            assert_eq!(
                strip.layout.size.height,
                SizeValue::Percent(100),
                "edge strip fills the cross axis",
            );
            // Centre tabify highlights the WHOLE target (the tab well covers it).
            let Scene::Container(center) = dock_drop_zone_highlight(DockDropZone::Center, &theme)
            else {
                panic!()
            };
            let Scene::Container(cstrip) = &center.children[0] else {
                panic!()
            };
            assert_eq!(
                cstrip.layout.size.width,
                SizeValue::Percent(100),
                "centre = whole target"
            );
            assert_eq!(
                cstrip.layout.size.height,
                SizeValue::Percent(100),
                "centre = whole target"
            );
            // None → an empty overlay (no band).
            let Scene::Container(none) = dock_drop_zone_highlight(DockDropZone::None, &theme)
            else {
                panic!()
            };
            assert!(none.children.is_empty(), "None paints no band");
        });
    }

    #[test]
    fn r1125_dock_drop_preview_overlay_result_region_in_absolute_px() {
        use pinion_core::style::Color;
        // R1125 §5.51 PR-33 — the shell-injected cross-window preview is a single
        // Box whose rect is the RESULT region in ABSOLUTE pixels (no Percent — it is
        // injected AFTER layout), positioned at the target panel's window-absolute
        // rect. A panel at (100, 40) sized 200x100: an edge takes the matching 50%
        // half, a centre the whole pane, a dead zone nothing.
        let panel = Rect::new(100, 40, 200, 100);
        let tint = Color::rgba(0x33, 0x66, 0xff, 0x66);
        let rect_of = |zone| match dock_drop_preview_overlay(panel, zone, tint) {
            Some(Scene::Box(b)) => {
                assert!(b.layout.pointer_transparent, "preview never grabs input");
                assert_eq!(
                    b.tag.as_deref(),
                    Some(DOCK_DROP_PREVIEW_TAG),
                    "carries the slot tag"
                );
                Some(b.rect)
            }
            Some(_) => panic!("preview is a Box"),
            None => None,
        };
        // half = 200*50/100 = 100 wide; 100*50/100 = 50 tall.
        assert_eq!(
            rect_of(DockDropZone::Left),
            Some(Rect::new(100, 40, 100, 100))
        );
        assert_eq!(
            rect_of(DockDropZone::Right),
            Some(Rect::new(200, 40, 100, 100))
        );
        assert_eq!(
            rect_of(DockDropZone::Top),
            Some(Rect::new(100, 40, 200, 50))
        );
        assert_eq!(
            rect_of(DockDropZone::Bottom),
            Some(Rect::new(100, 90, 200, 50))
        );
        assert_eq!(
            rect_of(DockDropZone::Center),
            Some(panel),
            "centre tabify covers the whole pane",
        );
        assert_eq!(
            rect_of(DockDropZone::None),
            None,
            "a dead zone paints nothing"
        );
    }

    #[test]
    fn r1139_redock_preview_overlay_carries_an_opaque_accent_border() {
        use pinion_core::style::Color;
        // R1139 §5.51 — the cross-window redock preview now outlines the result
        // region with an OPAQUE border derived from the fill tint, so it reads
        // over opaque floater content (the live-test "안 보임" failure) regardless
        // of the content hue. Structural guard; the rendered-pixel proof is the
        // GPU test r1139_redock_preview_is_boldly_visible_over_opaque_content.
        let tint = Color::rgba(0x1a, 0x73, 0xe8, super::DOCK_REDOCK_PREVIEW_ALPHA);
        let Some(Scene::Box(b)) =
            dock_drop_preview_overlay(Rect::new(0, 0, 200, 200), DockDropZone::Left, tint)
        else {
            panic!("left zone paints a Box");
        };
        let border = b.style.border.expect("redock preview carries a border");
        assert_eq!(
            border.color,
            tint.with_alpha(0xff),
            "border is the opaque tint hue",
        );
        assert_eq!(border.color.a, 0xff, "border is fully opaque (a hard edge)");
        assert_eq!(border.width, super::DOCK_REDOCK_PREVIEW_BORDER_PX);
        assert_eq!(
            b.style.fill, tint,
            "fill keeps the binding's (bold) tint alpha"
        );
    }

    #[test]
    fn r1139_redock_preview_tint_is_bolder_than_the_in_window_highlight() {
        // R1139 — the cross-window redock cue (drawn over opaque content) is more
        // opaque than the in-window split highlight (drawn over the SAME panel,
        // needs no shout). The two affordances are deliberately distinct.
        let theme = Theme::light();
        assert!(
            super::dock_redock_preview_tint(&theme).a > super::dock_drop_highlight_tint(&theme).a,
            "redock preview fill is more opaque than the in-window highlight",
        );
    }

    #[test]
    fn r683_composite_tag_format_matches_input_router_convention() {
        // R51.42 §5.35 — the composite-tag convention is
        // `{primary}#{suffix}`. The dock panel's header + content
        // tags both follow this format so the InputRouter's
        // deepest-tagged hit-test + dispatch_send wire route
        // PointerDown to the matching External.
        assert_eq!(
            composite_tag("panel_a", HEADER_TAG_SUFFIX),
            "panel_a#header"
        );
        assert_eq!(
            composite_tag("panel_a", CONTENT_TAG_SUFFIX),
            "panel_a#content"
        );
    }

    #[test]
    fn r683_dock_panel_external_panel_id_accessor_returns_construction_value() {
        let ext = DockPanelExternal::new("inspector");
        assert_eq!(ext.panel_id(), "inspector");
    }

    #[test]
    fn dock_split_result_pct_matches_ratio() {
        // R1111 PR-37 — the overlay's result-region percent must mirror the
        // split ratio the apply uses, so the previewed area and the actual
        // post-split occupancy cannot drift.
        assert!(
            (f32::from(super::DOCK_SPLIT_RESULT_PCT) / 100.0 - super::DEFAULT_REORGANIZE_RATIO)
                .abs()
                < f32::EPSILON,
        );
    }

    // ─────────────────────────────────────────────────────────────────
    // R684 §5.21 §5.16 — dock panel header cross-axis stretch fix.
    // Pre-R684 the header used `Size::px(0, h)` which forced taffy
    // to render the rect at exactly `padding-left + padding-right`
    // (16 px) because the explicit `Px(0)` width pre-empted the
    // outer Column container's `AlignItems::Stretch` resolution. The
    // R684 fix uses `Size::height_px(h)` (Auto width, Px height) so
    // the stretch path can promote the header to the dock panel's
    // full width.
    // ─────────────────────────────────────────────────────────────────

    #[test]
    fn r684_view_dock_panel_header_stretches_to_full_panel_width() {
        // R684 atomic 1 anchor — the header strip must paint at the
        // dock panel's full width. We build a dock panel and lay it
        // out inside a fixed 400×300 viewport via `compute_layout`;
        // the resulting header rect's width must equal the parent
        // panel's width (within taffy's u32 rounding). Pre-R684 this
        // assertion failed at width = 16 (padding-only).
        use pinion_runtime::layout::compute_layout;
        use pinion_text::LayoutCache;

        run_in_owner(|| {
            let style = DockPanelStyle::m3_default(PANEL_TAG);
            let panel = view_dock_panel("Inspector", empty_content(), &theme_light(), &style, None);
            let mut cache = LayoutCache::new();
            let mut scene = panel;
            let panel_w: u32 = 400;
            let panel_h: u32 = 300;
            compute_layout(&mut scene, &mut cache, panel_w, panel_h);
            let Scene::Container(outer) = &scene else {
                panic!("outer Container")
            };
            // Outer panel fills the viewport (Block default).
            assert_eq!(outer.rect.w, panel_w, "panel root fills viewport width");
            let Scene::Container(header) = &outer.children[0] else {
                panic!("header")
            };
            assert_eq!(
                header.rect.w, panel_w,
                "R684 atomic 1: header strip must stretch to full panel width \
                (was {} pre-R684 — padding-only collapse)",
                header.rect.w,
            );
            // Height stays pinned at the M3 default (28 px).
            assert_eq!(header.rect.h, style.header_height_px);
        });
    }

    #[test]
    fn r1109_dock_panel_content_shrinks_below_intrinsic_height() {
        // R1109 PR-35 forcing consumer — the first large-intrinsic dock
        // content (a reflow terminal grid is the real one). A box of definite
        // height 800 sits inside a 300px panel whose 28px header leaves 272px.
        // Pre-R1109 the content wrapper carried `with_flex_grow(1.0)` alone:
        // taffy's CSS automatic flex minimum pinned it to the 800px content,
        // overflowing the panel. The full `flex_basis:0 + flex_grow:1 +
        // min-height:0` idiom lets it shrink to the panel's leftover 272px,
        // while the cross axis still stretches to the full panel width.
        use pinion_core::scene::{BoxNode, Rect};
        use pinion_core::style::{Color, LayoutStyle, Size};
        use pinion_runtime::layout::compute_layout;
        use pinion_text::LayoutCache;

        run_in_owner(|| {
            let tall = Scene::Box(
                BoxNode::filled(Rect::default(), Color::default())
                    .with_layout(LayoutStyle::new().with_size(Size::px(100, 800))),
            );
            let style = DockPanelStyle::m3_default(PANEL_TAG);
            let panel = view_dock_panel("Terminal", tall, &theme_light(), &style, None);
            let mut cache = LayoutCache::new();
            let mut scene = panel;
            let panel_w: u32 = 400;
            let panel_h: u32 = 300;
            compute_layout(&mut scene, &mut cache, panel_w, panel_h);
            let Scene::Container(outer) = &scene else {
                panic!("outer Container")
            };
            let Scene::Container(content_wrapper) = &outer.children[1] else {
                panic!("content wrapper")
            };
            let expected_h = panel_h - style.header_height_px;
            assert_eq!(
                content_wrapper.rect.h, expected_h,
                "R1109 PR-35: content wrapper must shrink to the panel's \
                leftover height ({expected_h}px = {panel_h} - {} header), not \
                clamp to its 800px content (was {} = overflow)",
                style.header_height_px, content_wrapper.rect.h,
            );
            // Cross axis still fills the panel width (AlignItems::Stretch).
            assert_eq!(
                content_wrapper.rect.w, panel_w,
                "content wrapper still stretches to full panel width",
            );
        });
    }
}

#[cfg(test)]
mod topology_tests {
    //! R685 §5.16 §5.49 — `DockTopology` / `DockNode` pure-data
    //! substrate tests.
    //!
    //! These tests pin the load-bearing invariants the
    //! `view_dock_surface` walker (R685 atomic 1) and the binding-side
    //! `panel_handle` / `split_handle` callbacks rely on:
    //!
    //! 1. Constructor + builder API shapes (`leaf` /
    //!    `split_horizontal` / `split_vertical` / `single`).
    //! 2. Recursive traversal helpers (`leaf_count` / `split_count`
    //!    / `panel_ids` / `split_ids` depth-first pre-order).
    //! 3. JSON serde round-trip — every variant + nested topology
    //!    parses back to identity (bit-stable on-disk form).
    //! 4. Stable Split id semantics — every Split's id survives
    //!    serde + walker traversal without renumbering.

    use super::{DockNode, DockTopology};
    use crate::splitter::SplitterOrientation;

    /// 5-pane editor topology — the canonical R685 atomic 2 fixture
    /// shape (top toolbar + bottom console wrap a horizontal split
    /// of outliner + viewport + properties).
    ///
    /// Tree:
    /// ```text
    /// Vertical "outer" 0.10            (top: toolbar, rest: middle+bottom)
    /// ├── Leaf toolbar
    /// └── Vertical "inner_v" 0.80      (middle: panes, bottom: console)
    ///     ├── Horizontal "middle_h" 0.20  (left: outliner, rest: viewport+props)
    ///     │   ├── Leaf outliner
    ///     │   └── Horizontal "inner_h" 0.75   (left: viewport, right: properties)
    ///     │       ├── Leaf viewport
    ///     │       └── Leaf properties
    ///     └── Leaf console
    /// ```
    fn editor_topology() -> DockTopology {
        DockTopology::new(DockNode::split_vertical(
            "outer",
            0.10,
            DockNode::leaf("toolbar"),
            DockNode::split_vertical(
                "inner_v",
                0.80,
                DockNode::split_horizontal(
                    "middle_h",
                    0.20,
                    DockNode::leaf("outliner"),
                    DockNode::split_horizontal(
                        "inner_h",
                        0.75,
                        DockNode::leaf("viewport"),
                        DockNode::leaf("properties"),
                    ),
                ),
                DockNode::leaf("console"),
            ),
        ))
    }

    #[test]
    fn r685_dock_node_leaf_constructor_stores_panel_id() {
        let leaf = DockNode::leaf("viewport");
        let DockNode::Leaf { panel_id } = leaf else {
            panic!("expected Leaf");
        };
        assert_eq!(panel_id.as_ref(), "viewport");
    }

    #[test]
    fn r685_dock_node_split_horizontal_carries_stable_id_and_ratio() {
        let split =
            DockNode::split_horizontal("my_split", 0.42, DockNode::leaf("a"), DockNode::leaf("b"));
        let DockNode::Split {
            id,
            orientation,
            ratio,
            first,
            second,
        } = split
        else {
            panic!("expected Split");
        };
        assert_eq!(id.as_ref(), "my_split");
        assert_eq!(orientation, SplitterOrientation::Horizontal);
        assert!((ratio - 0.42).abs() < f32::EPSILON);
        let DockNode::Leaf { panel_id: a } = &*first else {
            panic!("first leaf");
        };
        let DockNode::Leaf { panel_id: b } = &*second else {
            panic!("second leaf");
        };
        assert_eq!(a.as_ref(), "a");
        assert_eq!(b.as_ref(), "b");
    }

    #[test]
    fn r685_dock_node_split_vertical_sets_orientation() {
        let split =
            DockNode::split_vertical("v_split", 0.5, DockNode::leaf("top"), DockNode::leaf("bot"));
        let DockNode::Split {
            id, orientation, ..
        } = split
        else {
            panic!("expected Split");
        };
        assert_eq!(id.as_ref(), "v_split");
        assert_eq!(orientation, SplitterOrientation::Vertical);
    }

    #[test]
    fn r685_dock_topology_single_panel_constructor() {
        let topology = DockTopology::single("viewport");
        assert_eq!(topology.leaf_count(), 1);
        assert_eq!(topology.split_count(), 0);
        assert_eq!(topology.panel_ids(), vec!["viewport"]);
        assert_eq!(topology.split_ids(), Vec::<&str>::new());
    }

    #[test]
    fn r685_dock_topology_leaf_count_walks_recursive_tree() {
        let topology = editor_topology();
        assert_eq!(topology.leaf_count(), 5, "5 panels in editor topology");
        assert_eq!(topology.split_count(), 4, "4 splits in editor topology");
    }

    #[test]
    fn r685_dock_topology_panel_ids_depth_first_first_before_second() {
        let topology = editor_topology();
        assert_eq!(
            topology.panel_ids(),
            vec!["toolbar", "outliner", "viewport", "properties", "console"],
            "depth-first first-before-second traversal order",
        );
    }

    #[test]
    fn r685_c_dock_topology_for_each_split_yields_id_orientation_ratio() {
        use crate::splitter::SplitterOrientation;
        let topology = editor_topology();
        let mut visits: Vec<(String, SplitterOrientation, f32)> = Vec::new();
        topology.for_each_split(|id, orient, ratio| {
            visits.push((id.to_string(), orient, ratio));
        });
        assert_eq!(
            visits,
            vec![
                ("outer".to_string(), SplitterOrientation::Vertical, 0.10),
                ("inner_v".to_string(), SplitterOrientation::Vertical, 0.80),
                (
                    "middle_h".to_string(),
                    SplitterOrientation::Horizontal,
                    0.20
                ),
                ("inner_h".to_string(), SplitterOrientation::Horizontal, 0.75),
            ],
            "for_each_split walks DF pre-order with id + orientation + initial ratio",
        );
    }

    #[test]
    fn r685_dock_topology_split_ids_depth_first_pre_order() {
        // Pre-order: outer (visit) → inner_v (visit) → middle_h (visit)
        // → inner_h (visit) → leaves only after this. The walker uses
        // this same order to dispatch split_handle callbacks.
        let topology = editor_topology();
        assert_eq!(
            topology.split_ids(),
            vec!["outer", "inner_v", "middle_h", "inner_h"],
            "split_ids walk = depth-first pre-order over Split nodes",
        );
    }

    #[test]
    fn r685_dock_node_leaf_serde_round_trip_through_json() {
        let leaf = DockNode::leaf("inspector");
        let serialized = serde_json::to_string(&leaf).expect("serialize leaf");
        assert!(serialized.contains("\"type\":\"Leaf\""));
        assert!(serialized.contains("\"panel_id\":\"inspector\""));
        // Pre-R685 atomic 5c form carried a `"slot":...` field —
        // R685 dropped the dead field, so it must NOT appear.
        assert!(!serialized.contains("\"slot\":"));
        let parsed: DockNode = serde_json::from_str(&serialized).expect("parse leaf");
        assert_eq!(parsed, leaf, "leaf round-trips through JSON identity");
    }

    #[test]
    fn r685_dock_node_split_serde_round_trip_through_json() {
        let split =
            DockNode::split_horizontal("h_split", 0.30, DockNode::leaf("a"), DockNode::leaf("b"));
        let serialized = serde_json::to_string(&split).expect("serialize split");
        assert!(serialized.contains("\"type\":\"Split\""));
        assert!(serialized.contains("\"id\":\"h_split\""));
        assert!(serialized.contains("\"orientation\":\"Horizontal\""));
        assert!(serialized.contains("\"ratio\":"));
        let parsed: DockNode = serde_json::from_str(&serialized).expect("parse split");
        assert_eq!(parsed, split, "split round-trips through JSON identity");
    }

    #[test]
    fn r685_dock_topology_full_editor_serde_round_trip() {
        let topology = editor_topology();
        let serialized = serde_json::to_string(&topology).expect("serialize editor topology");
        let parsed: DockTopology =
            serde_json::from_str(&serialized).expect("parse editor topology");
        assert_eq!(parsed, topology, "5-pane editor topology round-trips");
        assert_eq!(parsed.panel_ids(), topology.panel_ids());
        assert_eq!(parsed.split_ids(), topology.split_ids());
    }

    #[test]
    fn r685_dock_topology_split_count_pairs_with_signal_pool_size() {
        let topology = editor_topology();
        let signal_pool_size = topology.split_count();
        assert_eq!(signal_pool_size, 4, "4 splits = 4 ratio signals");
        assert_eq!(topology.split_ids().len(), signal_pool_size);
    }

    #[test]
    fn r685_dock_topology_serialized_form_stable_across_clones() {
        let a = editor_topology();
        let b = editor_topology();
        let sa = serde_json::to_string(&a).expect("a");
        let sb = serde_json::to_string(&b).expect("b");
        assert_eq!(sa, sb, "two equivalent topologies serialize identically");
    }

    // ─────────────────────────────────────────────────────────────────
    // R685.B atomic 2 — DockTopology::try_new validation tests
    // ─────────────────────────────────────────────────────────────────

    use super::TopologyError;

    #[test]
    fn r685_b_try_new_rejects_duplicate_panel_ids() {
        let root = DockNode::split_horizontal(
            "split",
            0.5,
            DockNode::leaf("dup_panel"),
            DockNode::leaf("dup_panel"),
        );
        let err = DockTopology::try_new(root).unwrap_err();
        assert_eq!(
            err,
            TopologyError::DuplicatePanelId("dup_panel".to_string())
        );
    }

    #[test]
    fn r685_b_try_new_rejects_duplicate_split_ids() {
        let root = DockNode::split_horizontal(
            "outer",
            0.5,
            DockNode::split_vertical(
                "outer", // duplicate
                0.5,
                DockNode::leaf("a"),
                DockNode::leaf("b"),
            ),
            DockNode::leaf("c"),
        );
        let err = DockTopology::try_new(root).unwrap_err();
        assert_eq!(err, TopologyError::DuplicateSplitId("outer".to_string()));
    }

    #[test]
    fn r685_b_try_new_rejects_nan_ratio() {
        let root =
            DockNode::split_horizontal("split", f32::NAN, DockNode::leaf("a"), DockNode::leaf("b"));
        let err = DockTopology::try_new(root).unwrap_err();
        let TopologyError::InvalidRatio { split_id, .. } = err else {
            panic!("expected InvalidRatio")
        };
        assert_eq!(split_id, "split");
    }

    #[test]
    fn r685_b_try_new_rejects_out_of_range_ratio() {
        let root =
            DockNode::split_horizontal("split", 1.5, DockNode::leaf("a"), DockNode::leaf("b"));
        let err = DockTopology::try_new(root).unwrap_err();
        let TopologyError::InvalidRatio { ratio, .. } = err else {
            panic!("expected InvalidRatio")
        };
        assert!((ratio - 1.5).abs() < f32::EPSILON);
    }

    #[test]
    fn r685_b_try_new_rejects_empty_panel_id() {
        let root = DockNode::leaf("");
        let err = DockTopology::try_new(root).unwrap_err();
        assert_eq!(err, TopologyError::EmptyId);
    }

    #[test]
    fn r685_b_try_new_rejects_empty_split_id() {
        let root = DockNode::split_horizontal("", 0.5, DockNode::leaf("a"), DockNode::leaf("b"));
        let err = DockTopology::try_new(root).unwrap_err();
        assert_eq!(err, TopologyError::EmptyId);
    }

    #[test]
    fn r685_b_try_new_accepts_valid_5_pane_editor() {
        let topology = editor_topology();
        // editor_topology uses DockTopology::new (panics on invalid);
        // re-validate via try_new to confirm valid.
        let cloned_root = topology.root().clone();
        assert!(DockTopology::try_new(cloned_root).is_ok());
    }

    #[test]
    fn r685_b_try_new_boundary_ratios_accepted() {
        // 0.0 and 1.0 are valid (degenerate but well-defined).
        let zero = DockNode::split_horizontal("z", 0.0, DockNode::leaf("a"), DockNode::leaf("b"));
        assert!(DockTopology::try_new(zero).is_ok());
        let one = DockNode::split_horizontal("o", 1.0, DockNode::leaf("a"), DockNode::leaf("b"));
        assert!(DockTopology::try_new(one).is_ok());
    }

    #[test]
    fn r685_b_topology_root_accessor_returns_inner_node() {
        let topology = DockTopology::single("only_panel");
        assert!(matches!(topology.root(), DockNode::Leaf { .. }));
    }

    // ─────────────────────────────────────────────────────────────────
    // R685.C atomic 1 — cross-namespace IdCollision validation.
    // ─────────────────────────────────────────────────────────────────

    #[test]
    fn r685_c_try_new_rejects_panel_id_split_id_collision_panel_first() {
        // panel_id "shared" appears first (outer Split's `first` child),
        // then the same string as a Split id deeper in the tree.
        let root = DockNode::split_horizontal(
            "wrapper",
            0.5,
            DockNode::leaf("shared"),
            DockNode::split_vertical(
                "shared", // collides with the panel_id above
                0.5,
                DockNode::leaf("a"),
                DockNode::leaf("b"),
            ),
        );
        let err = DockTopology::try_new(root).unwrap_err();
        assert_eq!(err, TopologyError::IdCollision("shared".to_string()));
    }

    #[test]
    fn r685_c_try_new_rejects_panel_id_split_id_collision_split_first() {
        // Split id "shared" appears first (the outer Split), then
        // the same string as a panel_id leaf.
        let root = DockNode::split_horizontal(
            "shared",
            0.5,
            DockNode::leaf("a"),
            DockNode::leaf("shared"), // collides with the outer Split id
        );
        let err = DockTopology::try_new(root).unwrap_err();
        assert_eq!(err, TopologyError::IdCollision("shared".to_string()));
    }

    #[test]
    fn r685_c_try_new_distinguishes_duplicate_kind_vs_cross_namespace() {
        // Same-kind duplicate (two panels with the same id) → DuplicatePanelId.
        let dup =
            DockNode::split_horizontal("outer", 0.5, DockNode::leaf("dup"), DockNode::leaf("dup"));
        assert_eq!(
            DockTopology::try_new(dup).unwrap_err(),
            TopologyError::DuplicatePanelId("dup".to_string()),
        );

        // Cross-namespace → IdCollision (NOT DuplicatePanelId).
        let cross = DockNode::split_horizontal(
            "cross",
            0.5,
            DockNode::leaf("cross"),
            DockNode::leaf("other"),
        );
        assert_eq!(
            DockTopology::try_new(cross).unwrap_err(),
            TopologyError::IdCollision("cross".to_string()),
        );
    }

    // ─────────────────────────────────────────────────────────────────
    // R686 atomic 1 — drag-to-reorganize mutation primitives.
    // ─────────────────────────────────────────────────────────────────

    use super::{DockSplitPosition, SplitterOrientation as Orient};

    #[test]
    fn r686_swap_leaves_exchanges_positions() {
        // toolbar (outer.first) ↔ console (inner_v.second).
        let swapped = editor_topology().swap_leaves("toolbar", "console").unwrap();
        // Depth-first panel order: the two ids trade slots; everyone
        // else keeps their place.
        assert_eq!(
            swapped.panel_ids(),
            vec!["console", "outliner", "viewport", "properties", "toolbar"],
        );
        // Tree shape (splits + ratios) is untouched by a swap.
        assert_eq!(
            swapped.split_ids(),
            vec!["outer", "inner_v", "middle_h", "inner_h"]
        );
        assert_eq!(swapped.split_count(), 4);
        assert_eq!(swapped.leaf_count(), 5);
    }

    #[test]
    fn r686_swap_leaves_self_is_identity() {
        let original = editor_topology();
        let same = original.swap_leaves("viewport", "viewport").unwrap();
        assert_eq!(same, original);
    }

    #[test]
    fn r686_swap_leaves_unknown_panel_errors() {
        let err = editor_topology()
            .swap_leaves("toolbar", "ghost")
            .unwrap_err();
        assert_eq!(err, TopologyError::PanelNotFound("ghost".to_string()));
        // First-arg miss is reported on the first arg.
        let err_a = editor_topology()
            .swap_leaves("ghost", "toolbar")
            .unwrap_err();
        assert_eq!(err_a, TopologyError::PanelNotFound("ghost".to_string()));
    }

    #[test]
    fn r686_split_leaf_into_second_position() {
        // Split "outliner" into a vertical pair, the new "assets" panel
        // taking the bottom (Second) slot.
        let grown = editor_topology()
            .split_leaf_into(
                "outliner",
                "assets",
                "outliner_v",
                Orient::Vertical,
                0.5,
                DockSplitPosition::Second,
            )
            .unwrap();
        assert_eq!(
            grown.panel_ids(),
            vec![
                "toolbar",
                "outliner",
                "assets",
                "viewport",
                "properties",
                "console"
            ],
        );
        assert_eq!(grown.split_count(), 5);
        assert!(grown.split_ids().contains(&"outliner_v"));
    }

    #[test]
    fn r686_split_leaf_into_first_position() {
        // Same split but the new "assets" panel takes the top (First)
        // slot, pushing "outliner" after it in depth-first order.
        let grown = editor_topology()
            .split_leaf_into(
                "outliner",
                "assets",
                "outliner_v",
                Orient::Vertical,
                0.5,
                DockSplitPosition::First,
            )
            .unwrap();
        assert_eq!(
            grown.panel_ids(),
            vec![
                "toolbar",
                "assets",
                "outliner",
                "viewport",
                "properties",
                "console"
            ],
        );
    }

    #[test]
    fn r686_split_leaf_into_unknown_target_errors() {
        let err = editor_topology()
            .split_leaf_into(
                "ghost",
                "assets",
                "s",
                Orient::Vertical,
                0.5,
                DockSplitPosition::First,
            )
            .unwrap_err();
        assert_eq!(err, TopologyError::PanelNotFound("ghost".to_string()));
    }

    #[test]
    fn r686_split_leaf_into_duplicate_panel_id_rejected() {
        // New leaf id collides with an existing panel → DuplicatePanelId
        // surfaced by the try_new gate (the live topology is unchanged).
        let err = editor_topology()
            .split_leaf_into(
                "outliner",
                "viewport",
                "s",
                Orient::Vertical,
                0.5,
                DockSplitPosition::First,
            )
            .unwrap_err();
        assert_eq!(err, TopologyError::DuplicatePanelId("viewport".to_string()));
    }

    #[test]
    fn r686_split_leaf_into_duplicate_split_id_rejected() {
        let err = editor_topology()
            .split_leaf_into(
                "outliner",
                "assets",
                "outer",
                Orient::Vertical,
                0.5,
                DockSplitPosition::First,
            )
            .unwrap_err();
        assert_eq!(err, TopologyError::DuplicateSplitId("outer".to_string()));
    }

    #[test]
    fn r686_split_leaf_into_id_collision_rejected() {
        // New *leaf* id "outer" collides with an existing *split* id.
        let err = editor_topology()
            .split_leaf_into(
                "outliner",
                "outer",
                "fresh_split",
                Orient::Vertical,
                0.5,
                DockSplitPosition::First,
            )
            .unwrap_err();
        assert_eq!(err, TopologyError::IdCollision("outer".to_string()));
    }

    #[test]
    fn r686_split_leaf_into_invalid_ratio_rejected() {
        let err = editor_topology()
            .split_leaf_into(
                "outliner",
                "assets",
                "s",
                Orient::Vertical,
                f32::NAN,
                DockSplitPosition::First,
            )
            .unwrap_err();
        let TopologyError::InvalidRatio { split_id, .. } = err else {
            panic!("expected InvalidRatio, got {err:?}");
        };
        assert_eq!(split_id, "s");
    }

    // ── R1132 §5.51.1 — home-anchor capture/restore (collapse-policy substrate) ──

    #[test]
    fn r1132_leaf_anchor_round_trips_a_leaf_sibling_exactly() {
        // Split{root_h, H, 0.5, Leaf a, Leaf b}: capture a's anchor, collapse
        // (remove a → layout reflows to just b), restore at the anchor == the
        // original topology. remove_leaf + insert_leaf_at_anchor is the exact
        // inverse for a leaf sibling — the collapse/dock-back round-trip.
        let topo = DockTopology::new(DockNode::split_horizontal(
            "root_h",
            0.5,
            DockNode::leaf("a"),
            DockNode::leaf("b"),
        ));
        let anchor = topo.leaf_anchor("a").expect("a has a parent-split anchor");
        assert_eq!(anchor.sibling, "b");
        assert_eq!(anchor.split_id, "root_h");
        assert_eq!(anchor.position, DockSplitPosition::First);
        let collapsed = topo.remove_leaf("a").expect("collapse a");
        assert_eq!(
            collapsed.panel_ids(),
            vec!["b"],
            "collapse reflows to just b"
        );
        let restored = collapsed
            .insert_leaf_at_anchor("a", &anchor)
            .expect("restore a at its anchor");
        assert_eq!(
            restored, topo,
            "remove_leaf + insert_leaf_at_anchor is identity for a leaf sibling",
        );
    }

    #[test]
    fn r1132_leaf_anchor_second_side_round_trips() {
        // The Second-side path (removed leaf was the split's `second` child).
        let topo = DockTopology::new(DockNode::split_horizontal(
            "root_h",
            0.3,
            DockNode::leaf("a"),
            DockNode::leaf("b"),
        ));
        let anchor = topo.leaf_anchor("b").expect("b anchor");
        assert_eq!(anchor.sibling, "a");
        assert_eq!(anchor.position, DockSplitPosition::Second);
        let restored = topo
            .remove_leaf("b")
            .unwrap()
            .insert_leaf_at_anchor("b", &anchor)
            .unwrap();
        assert_eq!(restored, topo, "Second-side round-trip is exact too");
    }

    #[test]
    fn r1132_leaf_anchor_none_for_root_pane() {
        // The sole-pane root has no parent Split to anchor against → None (the
        // binding cannot collapse the last pane; nothing to reflow).
        let single = DockTopology::new(DockNode::leaf("only"));
        assert!(
            single.leaf_anchor("only").is_none(),
            "the root leaf has no home anchor",
        );
        // An absent panel also has no anchor.
        assert!(single.leaf_anchor("ghost").is_none());
    }

    #[test]
    fn r1132_leaf_anchor_subtree_sibling_keeps_every_panel() {
        // Split{root, H, 0.5, Leaf a, Split{inner, V, 0.5, Leaf b, Leaf c}}:
        // a's sibling is the {b,c} SUBTREE (representative = b). Restore is not
        // pixel-exact for a subtree sibling (a returns adjacent to b, the
        // representative), but no panel is lost and a returns next to its
        // original neighbour — killing the hardcoded-default redock smell.
        let topo = DockTopology::new(DockNode::split_horizontal(
            "root",
            0.5,
            DockNode::leaf("a"),
            DockNode::split_vertical("inner", 0.5, DockNode::leaf("b"), DockNode::leaf("c")),
        ));
        let anchor = topo.leaf_anchor("a").expect("a anchor");
        assert_eq!(
            anchor.sibling, "b",
            "representative of the {{b,c}} sibling subtree",
        );
        let restored = topo
            .remove_leaf("a")
            .unwrap()
            .insert_leaf_at_anchor("a", &anchor)
            .unwrap();
        let mut ids = restored.panel_ids();
        ids.sort_unstable();
        assert_eq!(
            ids,
            vec!["a", "b", "c"],
            "no panel lost; a restored beside b"
        );
    }

    #[test]
    fn r1132_insert_at_anchor_errors_when_sibling_gone() {
        // If the captured sibling was itself removed before dock-back, restore
        // surfaces PanelNotFound so the binding can fall back to a default dock.
        let topo = DockTopology::new(DockNode::split_horizontal(
            "root_h",
            0.5,
            DockNode::leaf("a"),
            DockNode::leaf("b"),
        ));
        let anchor = topo.leaf_anchor("a").expect("a anchor"); // sibling = "b"
        // A topology where "b" no longer exists.
        let other = DockTopology::new(DockNode::leaf("z"));
        assert!(
            other.insert_leaf_at_anchor("a", &anchor).is_err(),
            "restore against a vanished sibling errors (caller falls back)",
        );
    }

    #[test]
    fn r686_remove_leaf_promotes_sibling_subtree() {
        // Remove "toolbar" (outer.first) → the "outer" Split disappears
        // and its sibling sub-tree (inner_v) becomes the new root.
        let pruned = editor_topology().remove_leaf("toolbar").unwrap();
        assert_eq!(
            pruned.panel_ids(),
            vec!["outliner", "viewport", "properties", "console"]
        );
        // "outer" is gone; the remaining splits keep their ids + ratios.
        assert_eq!(pruned.split_ids(), vec!["inner_v", "middle_h", "inner_h"]);
        assert!(matches!(pruned.root(), DockNode::Split { id, .. } if id.as_ref() == "inner_v"));
    }

    #[test]
    fn r686_remove_leaf_collapses_deep_split() {
        // Remove "properties" (inner_h.second) → inner_h collapses to
        // its surviving "viewport" leaf, which takes inner_h's slot.
        let pruned = editor_topology().remove_leaf("properties").unwrap();
        assert_eq!(
            pruned.panel_ids(),
            vec!["toolbar", "outliner", "viewport", "console"]
        );
        assert_eq!(pruned.split_ids(), vec!["outer", "inner_v", "middle_h"]);
    }

    #[test]
    fn r686_remove_leaf_unknown_panel_errors() {
        let err = editor_topology().remove_leaf("ghost").unwrap_err();
        assert_eq!(err, TopologyError::PanelNotFound("ghost".to_string()));
    }

    #[test]
    fn r686_remove_leaf_sole_panel_is_root_removal() {
        let err = DockTopology::single("only")
            .remove_leaf("only")
            .unwrap_err();
        assert_eq!(err, TopologyError::RootRemoval);
    }

    #[test]
    fn r686_remove_then_split_reparents_existing_panel() {
        // The drag-to-reparent composition: move "console" to sit left
        // of "viewport". Step 1 removes it; step 2 re-inserts it.
        let topology = editor_topology();
        let without_console = topology.remove_leaf("console").unwrap();
        assert!(!without_console.panel_ids().contains(&"console"));
        let reparented = without_console
            .split_leaf_into(
                "viewport",
                "console",
                "viewport_h",
                Orient::Horizontal,
                0.5,
                DockSplitPosition::First,
            )
            .unwrap();
        // console is back, now beside viewport; total leaf count restored.
        assert_eq!(reparented.leaf_count(), 5);
        assert!(reparented.panel_ids().contains(&"console"));
        assert!(reparented.split_ids().contains(&"viewport_h"));
    }
}

#[cfg(test)]
mod drop_zone_tests {
    //! R686 §5.16 §5.45 — `dock_drop_zone_for` geometry tests.
    //!
    //! Pure classification of a cursor over a panel rect into a
    //! [`DockDropZone`]. The drag-over External (R686 atomic 2) maps
    //! the zone to a topology edit; these tests pin only the geometry
    //! contract: edge-band proportions, centre rectangle, corner
    //! tiebreak precedence, half-open containment, and degenerate-rect
    //! handling.

    use super::{DOCK_EDGE_ZONE_FRAC, DockDropZone, dock_drop_zone_for, dock_drop_zone_normalized};
    use pinion_core::scene::Rect;

    /// Canonical 400×400 panel at offset (100, 100). With
    /// `DOCK_EDGE_ZONE_FRAC = 0.25` the edge bands are 100 px thick, so
    /// the centre rectangle spans (200, 200)..(400, 400).
    fn panel() -> Rect {
        Rect::new(100, 100, 400, 400)
    }

    #[test]
    fn r686_drop_zone_center_is_center() {
        // Dead centre — far from every edge.
        assert_eq!(
            dock_drop_zone_for(panel(), 300.0, 300.0),
            DockDropZone::Center
        );
    }

    #[test]
    fn r686_drop_zone_left_edge() {
        // 50 px in from the left → from_left = 0.125 < 0.25.
        assert_eq!(
            dock_drop_zone_for(panel(), 150.0, 300.0),
            DockDropZone::Left
        );
    }

    #[test]
    fn r686_drop_zone_right_edge() {
        assert_eq!(
            dock_drop_zone_for(panel(), 450.0, 300.0),
            DockDropZone::Right
        );
    }

    #[test]
    fn r686_drop_zone_top_edge() {
        assert_eq!(dock_drop_zone_for(panel(), 300.0, 150.0), DockDropZone::Top);
    }

    #[test]
    fn r686_drop_zone_bottom_edge() {
        assert_eq!(
            dock_drop_zone_for(panel(), 300.0, 450.0),
            DockDropZone::Bottom
        );
    }

    #[test]
    fn r686_drop_zone_corner_resolves_to_nearest_with_left_precedence() {
        // Top-left corner: from_left == from_top == 0.125 (exact tie).
        // Declaration-order precedence (Left → Right → Top → Bottom)
        // resolves the corner to Left.
        assert_eq!(
            dock_drop_zone_for(panel(), 150.0, 150.0),
            DockDropZone::Left
        );
        // Bottom-right corner: from_right == from_bottom tie → Right wins
        // over Bottom by precedence.
        assert_eq!(
            dock_drop_zone_for(panel(), 450.0, 450.0),
            DockDropZone::Right
        );
    }

    #[test]
    fn r686_drop_zone_band_boundary_belongs_to_center() {
        // Cursor exactly on the inner edge of the left band:
        // from_left = 0.25 == DOCK_EDGE_ZONE_FRAC. The band is half-open
        // on its inner side (>= frac → Center), so the boundary pixel is
        // Center, not Left.
        let on_boundary = 100.0 + DOCK_EDGE_ZONE_FRAC * 400.0;
        assert_eq!(
            dock_drop_zone_for(panel(), on_boundary, 300.0),
            DockDropZone::Center,
        );
    }

    #[test]
    fn r686_drop_zone_outside_is_none() {
        // Left / above of the rect.
        assert_eq!(dock_drop_zone_for(panel(), 50.0, 300.0), DockDropZone::None);
        assert_eq!(dock_drop_zone_for(panel(), 300.0, 50.0), DockDropZone::None);
    }

    #[test]
    fn r686_drop_zone_right_bottom_edges_are_half_open() {
        // x = 100 + 400 = 500 is the exclusive right edge → None.
        assert_eq!(
            dock_drop_zone_for(panel(), 500.0, 300.0),
            DockDropZone::None
        );
        // y = 100 + 400 = 500 is the exclusive bottom edge → None.
        assert_eq!(
            dock_drop_zone_for(panel(), 300.0, 500.0),
            DockDropZone::None
        );
    }

    #[test]
    fn r686_drop_zone_degenerate_rect_is_none() {
        // Zero width / zero height carry no pixels → never a target.
        assert_eq!(
            dock_drop_zone_for(Rect::new(0, 0, 0, 100), 0.0, 50.0),
            DockDropZone::None
        );
        assert_eq!(
            dock_drop_zone_for(Rect::new(0, 0, 100, 0), 50.0, 0.0),
            DockDropZone::None
        );
    }

    #[test]
    fn r1111_tabbing_off_suppresses_center_to_nearest_edge() {
        use super::dock_drop_zone_normalized_tabbing;
        // R1111 PR-37 — with tabbing on, the centre square is Center (tabify).
        assert_eq!(dock_drop_zone_normalized(0.5, 0.5), DockDropZone::Center);
        assert_eq!(
            dock_drop_zone_normalized_tabbing(0.5, 0.5, true),
            DockDropZone::Center,
        );
        // With tabbing OFF, the centre falls through to the nearest edge — a
        // split-only consumer never tabifies; dead-centre ties to Left.
        assert_eq!(
            dock_drop_zone_normalized_tabbing(0.5, 0.5, false),
            DockDropZone::Left,
        );
        // Off-centre under tabbing-off picks the nearest edge (no dead zone).
        assert_eq!(
            dock_drop_zone_normalized_tabbing(0.6, 0.5, false),
            DockDropZone::Right,
        );
        assert_eq!(
            dock_drop_zone_normalized_tabbing(0.5, 0.4, false),
            DockDropZone::Top,
        );
        // A clear edge hit classifies identically with tabbing on or off —
        // only the centre policy differs.
        assert_eq!(
            dock_drop_zone_normalized_tabbing(0.1, 0.5, false),
            DockDropZone::Left,
        );
        assert_eq!(
            dock_drop_zone_normalized_tabbing(0.1, 0.5, true),
            DockDropZone::Left,
        );
        // Out of bounds stays None regardless of tabbing.
        assert_eq!(
            dock_drop_zone_normalized_tabbing(1.0, 0.5, false),
            DockDropZone::None,
        );
    }

    // R1080 §5.51 — the normalised classifier the pointer drag coordinator
    // consumes directly (a `DropPoint` is already cursor-over-rect 0..1).

    #[test]
    fn r1080_drop_zone_normalized_classifies_center_and_edges() {
        // Dead centre (0.5, 0.5): nearest edge 0.5 >= 0.25 → Center.
        assert_eq!(dock_drop_zone_normalized(0.5, 0.5), DockDropZone::Center);
        // 0.125 in from each edge (< 0.25 band) on the mid-axis.
        assert_eq!(dock_drop_zone_normalized(0.125, 0.5), DockDropZone::Left);
        assert_eq!(dock_drop_zone_normalized(0.875, 0.5), DockDropZone::Right);
        assert_eq!(dock_drop_zone_normalized(0.5, 0.125), DockDropZone::Top);
        assert_eq!(dock_drop_zone_normalized(0.5, 0.875), DockDropZone::Bottom);
    }

    #[test]
    fn r1080_drop_zone_normalized_is_half_open_outside_is_none() {
        // Half-open [0.0, 1.0): below 0 or at/above 1 on either axis → None.
        assert_eq!(dock_drop_zone_normalized(-0.01, 0.5), DockDropZone::None);
        assert_eq!(dock_drop_zone_normalized(0.5, -0.01), DockDropZone::None);
        assert_eq!(dock_drop_zone_normalized(1.0, 0.5), DockDropZone::None);
        assert_eq!(dock_drop_zone_normalized(0.5, 1.0), DockDropZone::None);
        // 0.0 (left / top edge) is inside; just inside the right edge too.
        assert_eq!(dock_drop_zone_normalized(0.0, 0.5), DockDropZone::Left);
    }

    #[test]
    fn r1080_drop_zone_normalized_corner_tiebreak_is_left_then_right() {
        // Top-left corner tie (from_left == from_top) → Left precedence.
        assert_eq!(dock_drop_zone_normalized(0.1, 0.1), DockDropZone::Left);
        // Bottom-right corner tie (from_right == from_bottom) → Right.
        assert_eq!(dock_drop_zone_normalized(0.9, 0.9), DockDropZone::Right);
        // Band inner boundary is Center (half-open >= frac).
        assert_eq!(
            dock_drop_zone_normalized(DOCK_EDGE_ZONE_FRAC, 0.5),
            DockDropZone::Center
        );
    }

    #[test]
    fn r1080_drop_zone_for_equals_normalized_over_a_sweep() {
        // SSOT proof: the absolute classifier is exactly the normalised one
        // fed the cursor-over-rect coordinate, across a grid spanning inside,
        // the edges, and outside the canonical panel — so the two R742 drop
        // paths (absolute resolver, pointer DropPoint) can never disagree on
        // a zone. Steps land on band boundaries (0.25 of 400 = 100) and the
        // half-open right/bottom edges (x/y = 500).
        let rect = panel();
        let (x0, y0) = (f64::from(rect.x), f64::from(rect.y));
        let (width, height) = (f64::from(rect.w), f64::from(rect.h));
        for xi in (80..=520).step_by(20) {
            for yi in (80..=520).step_by(20) {
                let (cx, cy) = (f64::from(xi), f64::from(yi));
                assert_eq!(
                    dock_drop_zone_for(rect, cx, cy),
                    dock_drop_zone_normalized((cx - x0) / width, (cy - y0) / height),
                    "absolute and normalised disagree at ({cx}, {cy})"
                );
            }
        }
    }
}

#[cfg(test)]
mod reorganize_tests {
    //! R686 §5.16 §5.45 — drag-to-reorganize resolution + apply +
    //! `DockReorganizeExternal` invoke wire.
    //!
    //! Three layers:
    //! 1. [`resolve_dock_drop`] — cursor + layout → typed gesture.
    //! 2. [`DockReorganizeIntent::apply`] — gesture → mutated topology.
    //! 3. [`DockReorganizeExternal`] — the `scene/invoke` AI-native
    //!    wire: parse JSON payload, apply, mutate the shared topology
    //!    Signal, expose the result via `query`.

    use std::rc::Rc;

    use super::{
        DockDropPreview, DockDropZone, DockNode, DockReorganizeExternal, DockReorganizeIntent,
        DockReorganizer, DockSplitPosition, DockTopology, FloatPolicy, TabWellExternal,
        TopologyError, resolve_dock_drop,
    };
    use crate::splitter::SplitterOrientation as Orient;
    use pinion_core::external::{ExternalIntrospect, InterveneError, IntrospectValue, InvokeError};
    use pinion_core::reactive::Signal;
    use pinion_core::scene::Rect;

    /// 3-panel fixture: `a | (b | c)`. Panel ids depth-first [a, b, c].
    fn abc_topology() -> DockTopology {
        DockTopology::new(DockNode::split_horizontal(
            "root_h",
            0.5,
            DockNode::leaf("a"),
            DockNode::split_horizontal("inner_h", 0.5, DockNode::leaf("b"), DockNode::leaf("c")),
        ))
    }

    #[test]
    fn r1082_1_drop_preview_observable_on_canonical_reorganize_external() {
        // R1082.1 audit fix: the in-flight pointer drag must be observable
        // on the DockReorganizeExternal (the canonical AI reorganize tag),
        // not only on the per-panel externals.
        let topology = Rc::new(Signal::new(Some(abc_topology())));
        let preview = Rc::new(Signal::new(None));
        let reorganizer = Rc::new(DockReorganizer::new(topology));
        let ext = DockReorganizeExternal::from_reorganizer(reorganizer)
            .with_drop_preview(Rc::clone(&preview));
        // No drag → null.
        assert_eq!(ext.query("drop_preview"), Some(IntrospectValue::Null));
        // A panel's drag_to writes the shared signal → the canonical
        // external observes it via the same SSOT projection.
        preview.set(Some(DockDropPreview {
            source: "a".to_string(),
            target: "b".to_string(),
            zone: DockDropZone::Right,
        }));
        let IntrospectValue::Json(obj) = ext.query("drop_preview").expect("queryable") else {
            panic!("drop_preview must be JSON during a drag");
        };
        assert_eq!(obj.get("source").and_then(|v| v.as_str()), Some("a"));
        assert_eq!(obj.get("target").and_then(|v| v.as_str()), Some("b"));
        assert_eq!(obj.get("zone").and_then(|v| v.as_str()), Some("Right"));
        // The slot is declared in the schema.
        assert!(
            ext.schema()
                .fields
                .iter()
                .any(|(n, _)| *n == "drop_preview")
        );
    }

    #[test]
    fn r1082_1_apply_intent_records_rejection_outcome() {
        // R1082.1 audit fix: apply_intent is the SSOT for last_outcome on
        // BOTH success and rejection, so a failed pointer drag (which drops
        // the Result) still surfaces "rejected: …" to query("last_outcome").
        let topology = Rc::new(Signal::new(Some(abc_topology())));
        let reorganizer = DockReorganizer::new(topology);
        // Swap onto a stale panel id → TopologyError.
        let err = reorganizer
            .apply_intent(&DockReorganizeIntent::Swap {
                source: "a".to_string(),
                target: "ghost".to_string(),
            })
            .unwrap_err();
        let outcome = reorganizer.last_outcome().expect("rejection recorded");
        assert!(
            outcome.starts_with("rejected:"),
            "apply_intent records the rejection itself: {outcome}",
        );
        assert!(outcome.contains(&err.to_string()));
    }

    #[test]
    fn r1109_center_drop_with_absent_source_rejects() {
        // R1109 regression — a Center drop whose `source` names no leaf must
        // reject, NOT silently succeed. resolve_dock_drop classifies the
        // cursor over `b`'s centre into Tabify{ghost, b}; apply must then
        // reject the absent source uniformly with the SplitInsert/Swap arms
        // (both validate via remove_leaf/swap_leaves). This mirrors the
        // r686_dock_reorganize G.4 "stale source must reject" path that the
        // pre-R1109 Tabify arm let through. The r686 demo exercises the wire;
        // this pins the substrate invariant directly.
        let intent = resolve_dock_drop(&abc_rects(), "ghost", 300.0, 200.0)
            .expect("cursor over b's centre resolves to a gesture");
        assert!(
            matches!(intent, DockReorganizeIntent::Tabify { .. }),
            "a centre drop classifies as Tabify, got {intent:?}",
        );
        let topology = Rc::new(Signal::new(Some(abc_topology())));
        let reorganizer = DockReorganizer::new(topology);
        let result = reorganizer.apply_intent(&intent);
        assert!(
            result.is_err(),
            "Tabify with an absent source must reject (got {result:?})",
        );
    }

    /// (R1106) Do panels `x` and `y` form a 2-leaf split (direct siblings
    /// under one divider) anywhere in the tree?
    fn siblings_in_2leaf_split(node: &DockNode, x: &str, y: &str) -> bool {
        if let DockNode::Split { first, second, .. } = node {
            if let (DockNode::Leaf { panel_id: p }, DockNode::Leaf { panel_id: q }) =
                (first.as_ref(), second.as_ref())
            {
                let (p, q) = (p.as_ref(), q.as_ref());
                if (p == x && q == y) || (p == y && q == x) {
                    return true;
                }
            }
            return siblings_in_2leaf_split(first, x, y) || siblings_in_2leaf_split(second, x, y);
        }
        false
    }

    // (R1128 §5.51.1) `dock_panel_at_zone` is the SINGLE redock SSOT (retired
    // `apply_zone_redock`). These migrate the r1106 present-source (move) cases +
    // ADD the absent-source (insert) path the total primitive enables, and the
    // forcing consumers for `tabify_fresh` / `float_out_panel`.

    #[test]
    fn r1128_dock_panel_at_zone_edge_moves_present_source_beside_target() {
        // Placeholder policy: a PRESENT panel is MOVED to the dropped edge
        // (remove + re-split) — the r1106 zone-honoring behaviour, now via the SSOT.
        let topology = Rc::new(Signal::new(Some(abc_topology())));
        let reorganizer = DockReorganizer::new(Rc::clone(&topology));
        let before = topology.get().unwrap();
        assert!(
            siblings_in_2leaf_split(before.root(), "b", "c"),
            "before: b|c"
        );
        assert!(
            !siblings_in_2leaf_split(before.root(), "a", "c"),
            "before: a not beside c"
        );
        let outcome = reorganizer.dock_panel_at_zone("a", "c", 0.15, 0.5).unwrap();
        assert_eq!(outcome, "a -> c");
        let after = topology.get().unwrap();
        assert!(
            siblings_in_2leaf_split(after.root(), "a", "c"),
            "a moved beside c"
        );
        assert!(
            !siblings_in_2leaf_split(after.root(), "b", "c"),
            "c's old b-sibling changed"
        );
        assert_eq!(after.panel_ids().len(), 3, "a MOVE keeps the panel count");
    }

    #[test]
    fn r1128_dock_panel_at_zone_inserts_absent_source_at_edge() {
        // Collapse policy: a panel ABSENT from the topology (its leaf was floated
        // out) is INSERTED fresh at the dropped edge — no PanelNotFound (the
        // retired apply_zone_redock rejected an absent source). The total path.
        let topology = Rc::new(Signal::new(Some(abc_topology())));
        let reorganizer = DockReorganizer::new(Rc::clone(&topology));
        let outcome = reorganizer.dock_panel_at_zone("d", "c", 0.15, 0.5).unwrap();
        assert_eq!(outcome, "d -> c");
        let after = topology.get().unwrap();
        assert!(
            siblings_in_2leaf_split(after.root(), "d", "c"),
            "fresh d inserted beside c"
        );
        assert_eq!(
            after.panel_ids().len(),
            4,
            "an INSERT grows the panel count"
        );
    }

    #[test]
    fn r1128_dock_panel_at_zone_center_tabifies_present_or_absent_source() {
        // Centre = tabify, TOTAL over presence: present "a" re-tabs (move), absent
        // "d" tab-inserts fresh (tabify_fresh). One path, both policies.
        for (source, expect_count) in [("a", 3usize), ("d", 4usize)] {
            let topology = Rc::new(Signal::new(Some(abc_topology())));
            let reorganizer = DockReorganizer::new(Rc::clone(&topology));
            let outcome = reorganizer
                .dock_panel_at_zone(source, "c", 0.5, 0.5)
                .unwrap();
            assert_eq!(outcome, format!("{source} -> c"));
            let after = topology.get().unwrap();
            assert_eq!(
                after.tabs_well_count(),
                1,
                "a tab well formed (source={source})"
            );
            assert_eq!(
                after.panel_ids().len(),
                expect_count,
                "count (source={source})"
            );
        }
    }

    #[test]
    fn r1128_dock_panel_at_zone_own_slot_is_home_noop() {
        let topology = Rc::new(Signal::new(Some(abc_topology())));
        let reorganizer = DockReorganizer::new(Rc::clone(&topology));
        let before = topology.get().unwrap();
        let outcome = reorganizer.dock_panel_at_zone("a", "a", 0.15, 0.5).unwrap();
        assert!(
            outcome.contains("home redock"),
            "self-drop is a home return: {outcome}"
        );
        assert_eq!(topology.get().unwrap(), before, "topology unchanged");
    }

    #[test]
    fn r1128_dock_panel_at_zone_dead_zone_is_noop() {
        let topology = Rc::new(Signal::new(Some(abc_topology())));
        let reorganizer = DockReorganizer::new(Rc::clone(&topology));
        let before = topology.get().unwrap();
        let outcome = reorganizer.dock_panel_at_zone("a", "c", 1.5, 0.5).unwrap();
        assert!(
            outcome.contains("no actionable zone"),
            "dead zone is a no-op: {outcome}"
        );
        assert_eq!(topology.get().unwrap(), before, "topology unchanged");
    }

    #[test]
    fn r1128_tabify_fresh_stacks_a_new_panel_and_guards_ids() {
        // The fresh-tab INSERT primitive (the centre-zone collapse path).
        let topo = abc_topology();
        let after = topo.tabify_fresh("d", "c", "well-0").unwrap();
        assert_eq!(after.tabs_well_count(), 1, "a well minted from c + fresh d");
        assert_eq!(after.panel_ids().len(), 4, "d added");
        // A fresh insert must not duplicate a live panel, nor target an absent one.
        assert!(
            matches!(
                topo.tabify_fresh("a", "c", "well-1"),
                Err(TopologyError::DuplicatePanelId(_))
            ),
            "duplicate panel rejected",
        );
        assert!(
            matches!(
                topo.tabify_fresh("d", "z", "well-1"),
                Err(TopologyError::PanelNotFound(_))
            ),
            "absent target rejected",
        );
    }

    #[test]
    fn r1128_float_out_panel_collapses_the_leaf_idempotently() {
        // The COLLAPSE tear-off primitive: remove the leaf so siblings reclaim.
        let topology = Rc::new(Signal::new(Some(abc_topology())));
        let reorganizer = DockReorganizer::new(Rc::clone(&topology));
        let outcome = reorganizer.float_out_panel("a").unwrap();
        assert!(outcome.contains("floated out"), "outcome: {outcome}");
        let after = topology.get().unwrap();
        assert_eq!(after.panel_ids().len(), 2, "a removed; b + c reclaim");
        assert!(!after.panel_ids().contains(&"a"), "a is gone from the dock");
        // Idempotent: re-firing on the already-floated panel is a no-op.
        let again = reorganizer.float_out_panel("a").unwrap();
        assert!(again.contains("already floated"), "idempotent: {again}");
        assert_eq!(topology.get().unwrap().panel_ids().len(), 2, "still 2");
    }

    // ── R1134 §5.51.1 — FloatPolicy + collapse home-anchor round-trip ──

    #[test]
    fn r1134_float_policy_default_placeholder_and_runtime_settable() {
        let topology = Rc::new(Signal::new(Some(abc_topology())));
        let reorganizer = DockReorganizer::new(Rc::clone(&topology));
        assert_eq!(
            reorganizer.float_policy(),
            FloatPolicy::Placeholder,
            "default is Placeholder (bit-identical to pre-R1134)",
        );
        reorganizer.set_float_policy(FloatPolicy::Collapse);
        assert_eq!(
            reorganizer.float_policy(),
            FloatPolicy::Collapse,
            "runtime set"
        );
        // The construction-time builder picks it up too.
        let built =
            DockReorganizer::new(Rc::clone(&topology)).with_float_policy(FloatPolicy::Collapse);
        assert_eq!(
            built.float_policy(),
            FloatPolicy::Collapse,
            "with_float_policy"
        );
        // Wire name round-trip (the query / invoke value).
        assert_eq!(FloatPolicy::Collapse.as_str(), "collapse");
        assert_eq!(FloatPolicy::Placeholder.as_str(), "placeholder");
        assert_eq!(
            FloatPolicy::from_wire("collapse"),
            Some(FloatPolicy::Collapse)
        );
        assert_eq!(
            FloatPolicy::from_wire("placeholder"),
            Some(FloatPolicy::Placeholder)
        );
        assert_eq!(
            FloatPolicy::from_wire("bogus"),
            None,
            "unknown name rejected"
        );
    }

    #[test]
    fn r1134_float_out_panel_captures_home_anchor_and_restore_round_trips() {
        // The collapse home inverse: float b out (collapse), then dock it back HOME
        // restores b beside c under the SAME inner_h split — exact leaf-sibling
        // round-trip (the R1132 anchor, now driven by collapse policy).
        let topology = Rc::new(Signal::new(Some(abc_topology())));
        let reorganizer = DockReorganizer::new(Rc::clone(&topology));
        assert!(
            siblings_in_2leaf_split(topology.get().unwrap().root(), "b", "c"),
            "before: b|c siblings",
        );
        let out = reorganizer.float_out_panel("b").unwrap();
        assert!(out.contains("floated out"), "collapse outcome: {out}");
        assert!(
            !topology.get().unwrap().panel_ids().contains(&"b"),
            "b collapsed out"
        );
        assert_eq!(
            topology.get().unwrap().panel_ids().len(),
            2,
            "c reclaimed the space"
        );
        // Dock back home — restore at the captured anchor.
        let back = reorganizer.restore_panel_home("b").unwrap();
        assert_eq!(back, "b: docked home", "home restore outcome");
        let after = topology.get().unwrap();
        assert_eq!(after.panel_ids().len(), 3, "b is back");
        assert!(
            siblings_in_2leaf_split(after.root(), "b", "c"),
            "b restored beside c (home anchor)",
        );
        // The anchor is consumed by the restore — a second dock-back finds none.
        let again = reorganizer.restore_panel_home("b").unwrap();
        assert!(again.contains("no home anchor"), "anchor consumed: {again}");
    }

    #[test]
    fn r1134_restore_panel_home_without_anchor_is_noop() {
        // A panel that never collapsed has no stashed anchor → idempotent no-op
        // (placeholder mode + a stray dock-back are both harmless).
        let topology = Rc::new(Signal::new(Some(abc_topology())));
        let reorganizer = DockReorganizer::new(Rc::clone(&topology));
        let outcome = reorganizer.restore_panel_home("b").unwrap();
        assert!(
            outcome.contains("no home anchor"),
            "no-anchor no-op: {outcome}"
        );
        assert_eq!(
            topology.get().unwrap().panel_ids().len(),
            3,
            "topology unchanged"
        );
    }

    #[test]
    fn r1134_zone_redock_clears_stale_home_anchor() {
        // A zone redock lands the panel somewhere NEW, so its home anchor must be
        // dropped — a later home-restore then finds nothing (it is at the zone, not
        // home).
        let topology = Rc::new(Signal::new(Some(abc_topology())));
        let reorganizer = DockReorganizer::new(Rc::clone(&topology));
        reorganizer.float_out_panel("b").unwrap(); // stashes b's home anchor
        reorganizer.dock_panel_at_zone("b", "a", 0.15, 0.5).unwrap(); // lands beside a
        assert!(
            siblings_in_2leaf_split(topology.get().unwrap().root(), "b", "a"),
            "b docked beside a (the zone), not home",
        );
        let restore = reorganizer.restore_panel_home("b").unwrap();
        assert!(
            restore.contains("no home anchor"),
            "stash cleared by zone redock: {restore}"
        );
        assert_eq!(
            topology.get().unwrap().panel_ids().len(),
            3,
            "no resurrection"
        );
    }

    #[test]
    fn r1134_reorganize_external_set_float_policy_invoke_and_query() {
        // The §2 #2 AI-primary drive: the canonical reorganize surface exposes the
        // torn-slot policy as scene-as-data (`query`) + a `set_float_policy` invoke
        // toggle, so an AI flips collapse vs placeholder live + reads it back.
        let topology = Rc::new(Signal::new(Some(abc_topology())));
        let reorganizer = Rc::new(DockReorganizer::new(Rc::clone(&topology)));
        let mut ext = DockReorganizeExternal::from_reorganizer(Rc::clone(&reorganizer));
        assert_eq!(
            ext.query("float_policy"),
            Some(IntrospectValue::Text("placeholder".to_string())),
            "default is placeholder",
        );
        // Toggle to collapse — the invoke echoes the applied policy name.
        assert_eq!(
            ext.invoke(
                "set_float_policy",
                IntrospectValue::Text("collapse".to_string())
            )
            .unwrap(),
            IntrospectValue::Text("collapse".to_string()),
        );
        assert_eq!(
            reorganizer.float_policy(),
            FloatPolicy::Collapse,
            "coordinator updated"
        );
        assert_eq!(
            ext.query("float_policy"),
            Some(IntrospectValue::Text("collapse".to_string())),
            "query reflects it",
        );
        // Back to placeholder.
        ext.invoke(
            "set_float_policy",
            IntrospectValue::Text("placeholder".to_string()),
        )
        .unwrap();
        assert_eq!(reorganizer.float_policy(), FloatPolicy::Placeholder);
        // Unknown name = rejected; non-text arg = type mismatch.
        assert!(matches!(
            ext.invoke(
                "set_float_policy",
                IntrospectValue::Text("bogus".to_string())
            ),
            Err(InvokeError::Rejected),
        ));
        assert!(matches!(
            ext.invoke("set_float_policy", IntrospectValue::Null),
            Err(InvokeError::TypeMismatch),
        ));
        // Read-only to a direct intervene (the invoke is the write path).
        assert!(matches!(
            ext.intervene(
                "float_policy",
                IntrospectValue::Text("collapse".to_string())
            ),
            Err(InterveneError::ReadOnly),
        ));
    }

    /// Side-by-side layout for the three panels (each 200×400).
    fn abc_rects() -> Vec<(&'static str, Rect)> {
        vec![
            ("a", Rect::new(0, 0, 200, 400)),
            ("b", Rect::new(200, 0, 200, 400)),
            ("c", Rect::new(400, 0, 200, 400)),
        ]
    }

    #[test]
    fn r1083_resolve_center_drop_is_tabify() {
        // (R1083 §5.51) A centre drop now tabifies (supersedes the tab-less
        // v1 swap). Drop "a" onto b's centre (300, 200).
        let intent = resolve_dock_drop(&abc_rects(), "a", 300.0, 200.0).unwrap();
        assert_eq!(
            intent,
            DockReorganizeIntent::Tabify {
                source: "a".into(),
                target: "b".into()
            },
        );
    }

    #[test]
    fn r686_resolve_edge_drop_is_split_insert() {
        // Drop "a" onto b's left edge (210, 200) → dock left of b.
        let intent = resolve_dock_drop(&abc_rects(), "a", 210.0, 200.0).unwrap();
        assert_eq!(
            intent,
            DockReorganizeIntent::SplitInsert {
                source: "a".into(),
                target: "b".into(),
                orientation: Orient::Horizontal,
                position: DockSplitPosition::First,
            },
        );
    }

    #[test]
    fn r686_resolve_top_edge_maps_to_vertical_first() {
        // b's top edge (300, 10) → vertical split, source on top.
        let intent = resolve_dock_drop(&abc_rects(), "a", 300.0, 10.0).unwrap();
        let DockReorganizeIntent::SplitInsert {
            orientation,
            position,
            ..
        } = intent
        else {
            panic!("expected SplitInsert, got {intent:?}");
        };
        assert_eq!(orientation, Orient::Vertical);
        assert_eq!(position, DockSplitPosition::First);
    }

    #[test]
    fn r686_resolve_skips_source_panel() {
        // Cursor over "a" itself (the source) → no self-drop; nothing
        // else under the cursor → None.
        assert!(resolve_dock_drop(&abc_rects(), "a", 100.0, 200.0).is_none());
    }

    #[test]
    fn r686_resolve_outside_all_panels_is_none() {
        assert!(resolve_dock_drop(&abc_rects(), "a", 9999.0, 9999.0).is_none());
    }

    #[test]
    fn r686_intent_apply_swap_exchanges_panels() {
        let topo = abc_topology();
        let intent = DockReorganizeIntent::Swap {
            source: "a".into(),
            target: "c".into(),
        };
        let next = intent.apply(&topo, "unused", 0.5).unwrap();
        assert_eq!(next.panel_ids(), vec!["c", "b", "a"]);
        // Swap leaves the tree shape intact.
        assert_eq!(next.split_ids(), vec!["root_h", "inner_h"]);
    }

    #[test]
    fn r686_intent_apply_split_insert_moves_panel() {
        let topo = abc_topology();
        // Dock "a" to the right of "c".
        let intent = DockReorganizeIntent::SplitInsert {
            source: "a".into(),
            target: "c".into(),
            orientation: Orient::Horizontal,
            position: DockSplitPosition::Second,
        };
        let next = intent.apply(&topo, "new_split", 0.5).unwrap();
        // Leaf count invariant (a relocated, not duplicated).
        assert_eq!(next.leaf_count(), 3);
        assert!(next.split_ids().contains(&"new_split"));
        // "a" left its old slot beside the root; root_h collapsed.
        assert!(!next.split_ids().contains(&"root_h"));
    }

    #[test]
    fn r1083_external_invoke_center_tabifies_topology_signal() {
        // (R1083) A centre drop now TABIFIES (was a swap pre-R1083). The
        // panel_ids happen to coincide with a swap's ([b,a,c]), so this test
        // pins the STRUCTURAL distinction: a tabify produces a `Tabs` well
        // (a swap would leave three `Leaf`s) — otherwise a silent revert to
        // swap routing would slip past.
        let signal = Rc::new(Signal::new(Some(abc_topology())));
        let mut ext = DockReorganizeExternal::new(Rc::clone(&signal));
        let payload = IntrospectValue::Json(serde_json::json!({
            "source": "a", "target": "b", "zone": "Center",
        }));
        let result = ext.invoke("reorganize", payload).unwrap();
        assert!(matches!(result, IntrospectValue::Text(_)));
        let topo = signal.get().unwrap();
        let json = serde_json::to_string(&topo).expect("serialize topology");
        assert!(
            json.contains("\"type\":\"Tabs\""),
            "a centre drop must tabify into a well, not swap: {json}",
        );
        assert_eq!(topo.panel_ids(), vec!["b", "a", "c"]);
    }

    #[test]
    fn r749_with_undo_makes_reorganize_reversible() {
        use pinion_core::undo::UndoStack;
        let signal = Rc::new(Signal::new(Some(abc_topology())));
        let stack = Rc::new(UndoStack::new());
        let mut ext = DockReorganizeExternal::from_reorganizer(Rc::new(
            DockReorganizer::new(Rc::clone(&signal)).with_undo(Rc::clone(&stack)),
        ));
        assert_eq!(
            signal.get().unwrap().panel_ids(),
            vec!["a", "b", "c"],
            "boot layout"
        );
        // Swap a <-> b: recorded as one reversible topology edit.
        ext.invoke(
            "reorganize",
            IntrospectValue::Json(serde_json::json!({"source":"a","target":"b","zone":"Center"})),
        )
        .unwrap();
        assert_eq!(
            signal.get().unwrap().panel_ids(),
            vec!["b", "a", "c"],
            "reorganize applied"
        );
        assert_eq!(stack.len(), 1, "one recorded edit");
        // Undo restores the prior layout; redo re-applies it.
        assert!(stack.undo());
        assert_eq!(
            signal.get().unwrap().panel_ids(),
            vec!["a", "b", "c"],
            "undo restored the layout"
        );
        assert!(stack.redo());
        assert_eq!(
            signal.get().unwrap().panel_ids(),
            vec!["b", "a", "c"],
            "redo re-applied"
        );
    }

    #[test]
    fn r686_external_invoke_edge_split_inserts_and_bumps_seq() {
        let signal = Rc::new(Signal::new(Some(abc_topology())));
        let mut ext = DockReorganizeExternal::new(Rc::clone(&signal));
        assert_eq!(ext.split_seq(), 0);
        let payload = IntrospectValue::Json(serde_json::json!({
            "source": "a", "target": "c", "zone": "Right",
        }));
        ext.invoke("reorganize", payload).unwrap();
        // A split was minted → seq bumped; topology grew a reorg split.
        assert_eq!(ext.split_seq(), 1);
        assert!(
            signal
                .get()
                .unwrap()
                .split_ids()
                .iter()
                .any(|id| id.starts_with("reorg-split-"))
        );
        assert_eq!(signal.get().unwrap().leaf_count(), 3);
    }

    #[test]
    fn r686_external_invoke_swap_does_not_bump_seq() {
        let signal = Rc::new(Signal::new(Some(abc_topology())));
        let mut ext = DockReorganizeExternal::new(signal);
        ext.invoke(
            "reorganize",
            IntrospectValue::Json(serde_json::json!({"source":"a","target":"b","zone":"Center"})),
        )
        .unwrap();
        // Swap creates no split → seq stays 0.
        assert_eq!(ext.split_seq(), 0);
    }

    #[test]
    fn r686_external_invoke_non_json_payload_is_type_mismatch() {
        let signal = Rc::new(Signal::new(Some(abc_topology())));
        let mut ext = DockReorganizeExternal::new(signal);
        let err = ext
            .invoke("reorganize", IntrospectValue::Text("a:b:Center".into()))
            .unwrap_err();
        assert_eq!(err, InvokeError::TypeMismatch);
    }

    #[test]
    fn r686_external_invoke_unknown_zone_is_rejected() {
        let signal = Rc::new(Signal::new(Some(abc_topology())));
        let mut ext = DockReorganizeExternal::new(signal);
        let err = ext
            .invoke(
                "reorganize",
                IntrospectValue::Json(
                    serde_json::json!({"source":"a","target":"b","zone":"Diagonal"}),
                ),
            )
            .unwrap_err();
        assert_eq!(err, InvokeError::Rejected);
    }

    #[test]
    fn r686_external_invoke_stale_panel_rejected_topology_unchanged() {
        let signal = Rc::new(Signal::new(Some(abc_topology())));
        let mut ext = DockReorganizeExternal::new(Rc::clone(&signal));
        let before = signal.get();
        let err = ext
            .invoke(
                "reorganize",
                IntrospectValue::Json(
                    serde_json::json!({"source":"ghost","target":"b","zone":"Center"}),
                ),
            )
            .unwrap_err();
        assert_eq!(err, InvokeError::Rejected);
        // Live topology untouched on a rejected gesture.
        assert_eq!(signal.get(), before);
    }

    #[test]
    fn r686_external_unknown_action_is_unknown_path() {
        let signal = Rc::new(Signal::new(Some(abc_topology())));
        let mut ext = DockReorganizeExternal::new(signal);
        let err = ext.invoke("teleport", IntrospectValue::Null).unwrap_err();
        assert_eq!(err, InvokeError::UnknownPath);
    }

    /// Build the `drop` action's `panels` payload from `abc_rects`.
    fn abc_panels_json() -> serde_json::Value {
        serde_json::Value::Array(
            abc_rects()
                .into_iter()
                .map(|(tag, r)| {
                    serde_json::json!({
                        "tag": tag,
                        "rect": {"x": r.x, "y": r.y, "w": r.w, "h": r.h},
                    })
                })
                .collect(),
        )
    }

    #[test]
    fn r687_external_drop_center_resolves_swap_in_substrate() {
        // The client hands raw cursor + observed rects; the substrate
        // classifies the centre of "b" + swaps "a" onto it — no client
        // re-implements dock_drop_zone_for.
        let signal = Rc::new(Signal::new(Some(abc_topology())));
        let mut ext = DockReorganizeExternal::new(Rc::clone(&signal));
        let payload = IntrospectValue::Json(serde_json::json!({
            "source": "a",
            "cursor": {"x": 300.0, "y": 200.0},
            "panels": abc_panels_json(),
        }));
        let result = ext.invoke("drop", payload).unwrap();
        assert!(matches!(result, IntrospectValue::Text(_)));
        assert_eq!(signal.get().unwrap().panel_ids(), vec!["b", "a", "c"]);
        // Centre = swap → no split minted.
        assert_eq!(ext.split_seq(), 0);
    }

    #[test]
    fn r1112_external_drop_center_on_split_only_surface_splits_not_tabify() {
        // R1112 PR-37 — the `drop` RPC honours the dock surface's tabbing
        // policy. On a split-only surface (with_tabbing(false)) a CENTRE drop
        // that would tabify on a tab-docking surface instead resolves to the
        // nearest split edge — the RPC drive is uniform with the pointer drive
        // (it was NOT before R1112: the RPC path always classified Center).
        let signal = Rc::new(Signal::new(Some(abc_topology())));
        let reorganizer = Rc::new(DockReorganizer::new(Rc::clone(&signal)).with_tabbing(false));
        let mut ext = DockReorganizeExternal::from_reorganizer(reorganizer);
        // The surface policy is discoverable before classifying a drop (§2 #7).
        assert_eq!(ext.query("tabbing"), Some(IntrospectValue::Bool(false)));
        // Drop "a" onto the CENTRE of "b" (300, 200) — dead-centre of b.
        let payload = IntrospectValue::Json(serde_json::json!({
            "source": "a",
            "cursor": {"x": 300.0, "y": 200.0},
            "panels": abc_panels_json(),
        }));
        let result = ext.invoke("drop", payload).unwrap();
        assert!(
            matches!(result, IntrospectValue::Text(_)),
            "a split-insert applied"
        );
        // A split was minted, NOT a tabify: only SplitInsert bumps split_seq
        // (a tabify would leave it 0), and the move preserves all 3 panels.
        assert_eq!(
            ext.split_seq(),
            1,
            "centre on a split-only surface mints a split, not a tab well"
        );
        assert_eq!(
            signal.get().unwrap().panel_ids().len(),
            3,
            "a move preserves the panel count"
        );
    }

    #[test]
    fn r687_external_drop_edge_resolves_split_insert_in_substrate() {
        let signal = Rc::new(Signal::new(Some(abc_topology())));
        let mut ext = DockReorganizeExternal::new(Rc::clone(&signal));
        // Cursor near b's left edge (b spans x=200..400).
        let payload = IntrospectValue::Json(serde_json::json!({
            "source": "a",
            "cursor": {"x": 210.0, "y": 200.0},
            "panels": abc_panels_json(),
        }));
        ext.invoke("drop", payload).unwrap();
        assert_eq!(ext.split_seq(), 1, "edge drop mints a reorg split");
        assert_eq!(
            signal.get().unwrap().leaf_count(),
            3,
            "a relocated, not duplicated"
        );
        assert!(
            signal
                .get()
                .unwrap()
                .split_ids()
                .iter()
                .any(|id| id.starts_with("reorg-split-"))
        );
    }

    #[test]
    fn r687_external_drop_over_source_is_noop() {
        // Cursor over "a" itself (the source) → no valid target → cancel.
        let signal = Rc::new(Signal::new(Some(abc_topology())));
        let mut ext = DockReorganizeExternal::new(Rc::clone(&signal));
        let before = signal.get();
        let payload = IntrospectValue::Json(serde_json::json!({
            "source": "a",
            "cursor": {"x": 100.0, "y": 200.0},
            "panels": abc_panels_json(),
        }));
        let result = ext.invoke("drop", payload).unwrap();
        assert_eq!(result, IntrospectValue::Null, "no-target drop returns Null");
        assert_eq!(signal.get(), before, "topology unchanged on a no-op drop");
        assert_eq!(ext.split_seq(), 0);
    }

    #[test]
    fn r687_external_drop_malformed_payload_is_type_mismatch() {
        let signal = Rc::new(Signal::new(Some(abc_topology())));
        let mut ext = DockReorganizeExternal::new(signal);
        // Missing cursor field.
        let err = ext
            .invoke(
                "drop",
                IntrospectValue::Json(serde_json::json!({
                    "source": "a", "panels": abc_panels_json(),
                })),
            )
            .unwrap_err();
        assert_eq!(err, InvokeError::TypeMismatch);
    }

    #[test]
    fn r686_external_query_topology_returns_json() {
        let signal = Rc::new(Signal::new(Some(abc_topology())));
        let ext = DockReorganizeExternal::new(signal);
        let IntrospectValue::Json(value) = ext.query("topology").unwrap() else {
            panic!("topology query must return JSON");
        };
        // The serialized tree carries the root node's "type" tag.
        assert!(
            value.get("root").is_some(),
            "topology JSON exposes the root node"
        );
    }

    #[test]
    fn r686_external_intervene_slots_are_read_only() {
        let signal = Rc::new(Signal::new(Some(abc_topology())));
        let mut ext = DockReorganizeExternal::new(signal);
        assert_eq!(
            ext.intervene("topology", IntrospectValue::Null),
            Err(InterveneError::ReadOnly),
        );
        assert_eq!(
            ext.intervene("nonexistent", IntrospectValue::Null),
            Err(InterveneError::UnknownPath),
        );
    }

    // ── R1084 §5.51 — total over the empty (`None`) dock surface ────

    #[test]
    fn r1084_apply_intent_on_empty_surface_is_identity_no_op() {
        let signal: Rc<Signal<Option<DockTopology>>> = Rc::new(Signal::new(None));
        let reorganizer = DockReorganizer::new(Rc::clone(&signal));
        let outcome = reorganizer
            .apply_intent(&DockReorganizeIntent::Tabify {
                source: "a".into(),
                target: "b".into(),
            })
            .expect("an empty-surface reorganize is Ok(no-op), not Err");
        assert_eq!(outcome, "empty surface — no-op");
        assert!(signal.get().is_none(), "the empty surface stays empty");
        assert_eq!(reorganizer.split_seq(), 0, "no id minted on a no-op");
        assert_eq!(reorganizer.tabs_seq(), 0);
        assert_eq!(
            reorganizer.last_outcome().as_deref(),
            Some("empty surface — no-op"),
        );
    }

    #[test]
    fn r1084_apply_intent_with_undo_on_empty_surface_records_nothing() {
        let signal: Rc<Signal<Option<DockTopology>>> = Rc::new(Signal::new(None));
        let stack = Rc::new(pinion_core::undo::UndoStack::new());
        let reorganizer = DockReorganizer::new(Rc::clone(&signal)).with_undo(Rc::clone(&stack));
        reorganizer
            .apply_intent(&DockReorganizeIntent::Swap {
                source: "a".into(),
                target: "b".into(),
            })
            .expect("no-op Ok");
        assert!(!stack.can_undo(), "a no-op records no reversible edit");
        assert!(signal.get().is_none());
    }

    #[test]
    fn r1084_invoke_reorganize_on_empty_surface_is_no_op_not_error() {
        let signal: Rc<Signal<Option<DockTopology>>> = Rc::new(Signal::new(None));
        let mut ext = DockReorganizeExternal::new(Rc::clone(&signal));
        // An AI client may target an empty dock; the honest result is no-op.
        let res = ext.invoke(
            "reorganize",
            IntrospectValue::Json(serde_json::json!({
                "source": "a", "target": "b", "zone": "Center",
            })),
        );
        assert!(
            res.is_ok(),
            "empty-surface reorganize is a no-op, not an error"
        );
        assert!(signal.get().is_none());
    }

    #[test]
    fn r1084_query_topology_is_json_null_on_empty_surface() {
        let signal: Rc<Signal<Option<DockTopology>>> = Rc::new(Signal::new(None));
        let ext = DockReorganizeExternal::new(signal);
        // §2 #7 — the empty dock reads as JSON null, not a fabricated tree.
        assert_eq!(
            ext.query("topology"),
            Some(IntrospectValue::Json(serde_json::Value::Null)),
        );
    }

    #[test]
    fn r1084_some_surface_still_reorganizes_under_the_option_signal() {
        // The Some path is unchanged by the Option lift.
        let signal = Rc::new(Signal::new(Some(abc_topology())));
        let reorganizer = DockReorganizer::new(Rc::clone(&signal));
        reorganizer
            .apply_intent(&DockReorganizeIntent::Swap {
                source: "a".into(),
                target: "b".into(),
            })
            .expect("swap applies");
        assert_eq!(signal.get().unwrap().panel_ids(), vec!["b", "a", "c"]);
    }

    #[test]
    fn r1084_1_schema_and_query_expose_tabs_seq_symmetric_with_split_seq() {
        // (R1084.1) tab-well-mint progress is a first-class observable, like
        // split-mint progress — advertised in the schema + answerable.
        let signal = Rc::new(Signal::new(Some(abc_topology())));
        let reorganizer = Rc::new(DockReorganizer::new(Rc::clone(&signal)));
        let ext = DockReorganizeExternal::from_reorganizer(Rc::clone(&reorganizer));
        assert!(
            ext.schema().fields.iter().any(|(k, _)| *k == "tabs_seq"),
            "tabs_seq is advertised in the schema",
        );
        assert_eq!(ext.query("tabs_seq"), Some(IntrospectValue::Int(0)));
        // A tabify bumps it; an AI observes the mint count through the schema key.
        reorganizer
            .apply_intent(&DockReorganizeIntent::Tabify {
                source: "a".into(),
                target: "b".into(),
            })
            .expect("tabify applies");
        assert_eq!(ext.query("tabs_seq"), Some(IntrospectValue::Int(1)));
    }

    // ── R1085 §5.51 — tab-well navigation (`activate_tab`) ──────────

    /// `a | w0[x, y, z]@0` — a leaf beside a 3-tab well visible on `x`.
    fn well_topology() -> DockTopology {
        use std::borrow::Cow;
        DockTopology::new(DockNode::split_horizontal(
            "root_h",
            0.5,
            DockNode::leaf("a"),
            DockNode::tabs("w0", ["x", "y", "z"].map(Cow::from), 0),
        ))
    }

    /// `active` of the first [`DockNode::Tabs`] well in pre-order.
    fn first_well_active(node: &DockNode) -> Option<usize> {
        match node {
            DockNode::Tabs { active, .. } => Some(*active),
            DockNode::Leaf { .. } => None,
            DockNode::Split { first, second, .. } => {
                first_well_active(first).or_else(|| first_well_active(second))
            }
        }
    }

    #[test]
    fn r1085_activate_tab_invoke_flips_active_and_returns_summary() {
        let signal = Rc::new(Signal::new(Some(well_topology())));
        let mut ext = DockReorganizeExternal::new(Rc::clone(&signal));
        let out = ext
            .invoke(
                "activate_tab",
                IntrospectValue::Json(serde_json::json!({"well_id":"w0","index":2})),
            )
            .expect("activate ok");
        assert_eq!(out, IntrospectValue::Text("activate w0#2".to_string()));
        // The live signal (the SSOT query("topology") reads from) flipped.
        assert_eq!(first_well_active(signal.get().unwrap().root()), Some(2));
    }

    #[test]
    fn r1085_activate_tab_records_last_outcome_for_introspection() {
        let signal = Rc::new(Signal::new(Some(well_topology())));
        let mut ext = DockReorganizeExternal::new(Rc::clone(&signal));
        ext.invoke(
            "activate_tab",
            IntrospectValue::Json(serde_json::json!({"well_id":"w0","index":1})),
        )
        .expect("activate ok");
        // An AI confirms the gesture through the one outcome SSOT.
        assert_eq!(
            ext.query("last_outcome"),
            Some(IntrospectValue::Text("activate w0#1".to_string())),
        );
    }

    #[test]
    fn r1085_activate_tab_invoke_malformed_payload_is_type_mismatch() {
        let signal = Rc::new(Signal::new(Some(well_topology())));
        let mut ext = DockReorganizeExternal::new(signal);
        // Not a JSON object.
        assert_eq!(
            ext.invoke("activate_tab", IntrospectValue::Text("nope".into())),
            Err(InvokeError::TypeMismatch),
        );
        // Missing `index`.
        assert_eq!(
            ext.invoke(
                "activate_tab",
                IntrospectValue::Json(serde_json::json!({"well_id":"w0"})),
            ),
            Err(InvokeError::TypeMismatch),
        );
        // Missing `well_id`.
        assert_eq!(
            ext.invoke(
                "activate_tab",
                IntrospectValue::Json(serde_json::json!({"index":1})),
            ),
            Err(InvokeError::TypeMismatch),
        );
    }

    #[test]
    fn r1085_activate_tab_invoke_out_of_range_or_unknown_well_is_rejected() {
        let signal = Rc::new(Signal::new(Some(well_topology())));
        let mut ext = DockReorganizeExternal::new(Rc::clone(&signal));
        // Index past the well's end — a well-formed but rejected gesture.
        assert_eq!(
            ext.invoke(
                "activate_tab",
                IntrospectValue::Json(serde_json::json!({"well_id":"w0","index":9})),
            ),
            Err(InvokeError::Rejected),
        );
        // Unknown well id.
        assert_eq!(
            ext.invoke(
                "activate_tab",
                IntrospectValue::Json(serde_json::json!({"well_id":"nope","index":0})),
            ),
            Err(InvokeError::Rejected),
        );
        // The live topology is untouched by either rejection.
        assert_eq!(first_well_active(signal.get().unwrap().root()), Some(0));
        // ...and the rejection is observable through the outcome SSOT.
        let IntrospectValue::Text(outcome) = ext.query("last_outcome").unwrap() else {
            panic!("last_outcome is text after a rejection");
        };
        assert!(outcome.starts_with("rejected:"), "got {outcome:?}");
    }

    #[test]
    fn r1085_activate_tab_on_empty_surface_is_identity_noop() {
        let signal: Rc<Signal<Option<DockTopology>>> = Rc::new(Signal::new(None));
        let mut ext = DockReorganizeExternal::new(Rc::clone(&signal));
        let out = ext
            .invoke(
                "activate_tab",
                IntrospectValue::Json(serde_json::json!({"well_id":"w0","index":1})),
            )
            .expect("empty-surface activate is a no-op, not an error");
        assert_eq!(
            out,
            IntrospectValue::Text("empty surface — no-op".to_string()),
        );
        assert!(signal.get().is_none(), "empty surface stays empty");
    }

    #[test]
    fn r1085_activate_tab_with_undo_is_reversible() {
        use pinion_core::undo::UndoStack;
        let signal = Rc::new(Signal::new(Some(well_topology())));
        let stack = Rc::new(UndoStack::new());
        let reorganizer =
            Rc::new(DockReorganizer::new(Rc::clone(&signal)).with_undo(Rc::clone(&stack)));
        reorganizer.activate_tab("w0", 2).expect("activate ok");
        assert_eq!(first_well_active(signal.get().unwrap().root()), Some(2));
        assert_eq!(stack.len(), 1, "one recorded navigation edit");
        assert!(stack.undo());
        assert_eq!(
            first_well_active(signal.get().unwrap().root()),
            Some(0),
            "undo restored the prior active tab",
        );
        assert!(stack.redo());
        assert_eq!(first_well_active(signal.get().unwrap().root()), Some(2));
    }

    #[test]
    fn r1085_schema_advertises_activate_tab() {
        let signal = Rc::new(Signal::new(Some(well_topology())));
        let ext = DockReorganizeExternal::new(signal);
        assert!(
            ext.schema()
                .fields
                .iter()
                .any(|(k, _)| *k == "activate_tab"),
            "activate_tab is discoverable in the schema (AI-first primary)",
        );
    }

    // ── R1096 §5.51 — pointer click-to-switch (`TabWellExternal`) ──────

    /// Build a shared coordinator over `well_topology()` (well `w0`
    /// `[x, y, z]@0`) plus the `TabWellExternal` registered at `w0`.
    fn tab_well_fixture() -> (
        Rc<Signal<Option<DockTopology>>>,
        Rc<DockReorganizer>,
        TabWellExternal,
    ) {
        let signal = Rc::new(Signal::new(Some(well_topology())));
        let reorganizer = Rc::new(DockReorganizer::new(Rc::clone(&signal)));
        let ext = TabWellExternal::new("w0", Rc::clone(&reorganizer));
        (signal, reorganizer, ext)
    }

    #[test]
    fn r1096_tab_well_click_pointerup_activates_tab() {
        let (signal, _reorg, mut ext) = tab_well_fixture();
        // The router dispatches a click on the painted `w0#2` tab tag as
        // `"2:PointerUp"` (R51.42 `#`-split). The release edge activates it.
        let out = ext
            .invoke("send", IntrospectValue::Text("2:PointerUp".into()))
            .expect("click activates");
        assert_eq!(out, IntrospectValue::Text("activate w0#2".to_string()));
        // The shared topology Signal — the one SSOT — flipped.
        assert_eq!(first_well_active(signal.get().unwrap().root()), Some(2));
    }

    #[test]
    fn r1096_tab_well_non_release_edges_are_noop() {
        let (signal, _reorg, mut ext) = tab_well_fixture();
        // Press / hover transitions on a tab must not switch it — only the
        // release edge activates.
        for ev in [
            "2:PointerDown",
            "2:PointerEnter",
            "2:PointerLeave",
            "2:PointerCancel",
        ] {
            assert_eq!(
                ext.invoke("send", IntrospectValue::Text(ev.into())),
                Ok(IntrospectValue::Null),
                "{ev} is a no-op",
            );
        }
        assert_eq!(
            first_well_active(signal.get().unwrap().root()),
            Some(0),
            "no non-release edge changed the active tab",
        );
    }

    #[test]
    fn r1096_tab_well_already_active_click_does_not_churn_undo() {
        use pinion_core::undo::UndoStack;
        let signal = Rc::new(Signal::new(Some(well_topology())));
        let stack = Rc::new(UndoStack::new());
        let reorganizer =
            Rc::new(DockReorganizer::new(Rc::clone(&signal)).with_undo(Rc::clone(&stack)));
        let mut ext = TabWellExternal::new("w0", Rc::clone(&reorganizer));
        // Clicking the already-visible tab (`active == 0`) is a no-op — the
        // gesture-layer guard skips `activate_tab` so it mints no undo edit
        // (a direct `DockReorganizer::activate_tab(w0, 0)` WOULD commit, per
        // its idempotent-gesture contract — this guard is exactly why the
        // pointer drive lives here, not in the coordinator).
        assert_eq!(
            ext.invoke("send", IntrospectValue::Text("0:PointerUp".into())),
            Ok(IntrospectValue::Null),
        );
        assert_eq!(stack.len(), 0, "already-active click recorded no undo edit");
        assert_eq!(first_well_active(signal.get().unwrap().root()), Some(0));
        // A click on a *different* tab does commit (one reversible edit).
        ext.invoke("send", IntrospectValue::Text("1:PointerUp".into()))
            .expect("switch commits");
        assert_eq!(stack.len(), 1, "the real switch recorded one edit");
        assert_eq!(first_well_active(signal.get().unwrap().root()), Some(1));
    }

    #[test]
    fn r1096_tab_well_bare_pointerup_on_strip_background_is_noop() {
        let (signal, _reorg, mut ext) = tab_well_fixture();
        // A press on the strip background (tagged `w0`, not `w0#i`) arrives
        // as a bare `"PointerUp"` with no sub-index — it switches nothing.
        assert_eq!(
            ext.invoke("send", IntrospectValue::Text("PointerUp".into())),
            Ok(IntrospectValue::Null),
        );
        assert_eq!(first_well_active(signal.get().unwrap().root()), Some(0));
    }

    #[test]
    fn r1096_tab_well_out_of_range_or_unknown_well_click_rejected() {
        let (signal, reorg, mut ext) = tab_well_fixture();
        // An index past the well's end is a well-formed but rejected gesture.
        assert_eq!(
            ext.invoke("send", IntrospectValue::Text("9:PointerUp".into())),
            Err(InvokeError::Rejected),
        );
        assert_eq!(
            first_well_active(signal.get().unwrap().root()),
            Some(0),
            "a rejected click leaves the active tab untouched",
        );
        // An external bound to a non-existent well rejects every click.
        let mut ghost = TabWellExternal::new("ghost", Rc::clone(&reorg));
        assert_eq!(
            ghost.invoke("send", IntrospectValue::Text("0:PointerUp".into())),
            Err(InvokeError::Rejected),
        );
    }

    #[test]
    fn r1096_tab_well_query_and_intervene() {
        let (_signal, _reorg, mut ext) = tab_well_fixture();
        assert_eq!(
            ext.query("well_id"),
            Some(IntrospectValue::Text("w0".into()))
        );
        // `active` is a live read of the topology — not stored on the external.
        assert_eq!(ext.query("active"), Some(IntrospectValue::Int(0)));
        ext.invoke("send", IntrospectValue::Text("2:PointerUp".into()))
            .expect("activate");
        assert_eq!(ext.query("active"), Some(IntrospectValue::Int(2)));
        assert_eq!(ext.query("nope"), None);
        // The derived reads are not interveneable; tabs switch via the wire.
        assert_eq!(
            ext.intervene("active", IntrospectValue::Int(1)),
            Err(InterveneError::ReadOnly),
        );
        assert_eq!(
            ext.intervene("well_id", IntrospectValue::Text("x".into())),
            Err(InterveneError::ReadOnly),
        );
        assert_eq!(
            ext.intervene("zzz", IntrospectValue::Null),
            Err(InterveneError::UnknownPath),
        );
        // Only `send` is invokable; a non-string payload is malformed.
        assert_eq!(
            ext.invoke("bogus", IntrospectValue::Null),
            Err(InvokeError::UnknownPath)
        );
        assert_eq!(
            ext.invoke("send", IntrospectValue::Int(1)),
            Err(InvokeError::TypeMismatch),
        );
    }

    #[test]
    fn r1096_tab_well_query_active_null_for_missing_well() {
        let (_signal, reorg, _ext) = tab_well_fixture();
        // An external bound to a well that does not exist reads `active` as
        // null (the topology owns the value; there is none to project).
        let ghost = TabWellExternal::new("ghost", reorg);
        assert_eq!(ghost.query("active"), Some(IntrospectValue::Null));
    }

    #[test]
    fn r1096_tab_well_schema_advertises_send_and_active() {
        let (_signal, _reorg, ext) = tab_well_fixture();
        let keys: Vec<&str> = ext.schema().fields.iter().map(|(k, _)| *k).collect();
        for expected in ["well_id", "active", "send"] {
            assert!(keys.contains(&expected), "schema advertises {expected}");
        }
    }
}

#[cfg(test)]
mod tabify_tests {
    //! R1083 §5.51 — tabbed docking: the [`DockNode::Tabs`] well model,
    //! the [`DockTopology::tabify`] mutation, the `Center → Tabify`
    //! intent routing, and the [`view_dock_surface`] tab-strip render.
    //!
    //! The forcing consumer for the tab-well slice: there is no §7 RPC
    //! surface unique to tabs (the gesture rides the existing R1081/R1082
    //! reorganize coordinator), so these unit tests are the deliverable —
    //! exercising the model + routing + render directly, per
    //! [[needed-feature-test-as-forcing-consumer]].

    use std::borrow::Cow;
    use std::rc::Rc;

    use super::{
        DockDropZone, DockNode, DockReorganizeIntent, DockReorganizer, DockSplitState,
        DockTopology, TopologyError, intent_for_zone, view_dock_surface,
    };
    use pinion_core::reactive::{Owner, Signal};
    use pinion_core::scene::Scene;
    use pinion_core::theme::Theme;

    /// `a | b` — two side-by-side leaves.
    fn ab_topology() -> DockTopology {
        DockTopology::new(DockNode::split_horizontal(
            "root_h",
            0.5,
            DockNode::leaf("a"),
            DockNode::leaf("b"),
        ))
    }

    /// First [`DockNode::Tabs`] node found in pre-order, as
    /// `(id, panels, active)`.
    fn first_tabs(node: &DockNode) -> Option<(&str, &[Cow<'static, str>], usize)> {
        match node {
            DockNode::Tabs { id, panels, active } => Some((id.as_ref(), panels, *active)),
            DockNode::Leaf { .. } => None,
            DockNode::Split { first, second, .. } => {
                first_tabs(first).or_else(|| first_tabs(second))
            }
        }
    }

    fn collect_tags(scene: &Scene, out: &mut Vec<String>) {
        if let Scene::Container(c) = scene {
            if let Some(t) = &c.tag {
                out.push(t.to_string());
            }
            for child in &c.children {
                collect_tags(child, out);
            }
        }
    }

    fn has_tag(scene: &Scene, tag: &str) -> bool {
        let mut tags = Vec::new();
        collect_tags(scene, &mut tags);
        tags.iter().any(|t| t == tag)
    }

    // ── model: tabify ───────────────────────────────────────────────

    #[test]
    fn r1083_tabify_two_leaves_creates_well_source_relocated() {
        let topology = ab_topology();
        let next = topology.tabify("a", "b", "w0").expect("tabify ok");
        // The whole tree collapses to a single well (the root split is
        // gone — a was removed, b promoted, then b + a stacked).
        let (id, panels, active) = first_tabs(next.root()).expect("a well exists");
        assert_eq!(id, "w0");
        assert_eq!(
            panels.iter().map(Cow::as_ref).collect::<Vec<_>>(),
            vec!["b", "a"],
            "target first, dragged source stacked after",
        );
        assert_eq!(active, 1, "the dropped source becomes the visible tab");
        // Panel count invariant: one relocated, none created / destroyed.
        assert_eq!(next.leaf_count(), 1, "the well is one pane slot");
        assert_eq!(next.panel_count(), 2, "two panels total");
        assert_eq!(next.split_count(), 0);
    }

    #[test]
    fn r1083_tabify_into_existing_well_appends_and_activates() {
        // (b, c) already a well beside leaf a.
        let topology = DockTopology::new(DockNode::split_horizontal(
            "root_h",
            0.5,
            DockNode::leaf("a"),
            DockNode::tabs("w0", [Cow::from("b"), Cow::from("c")], 0),
        ));
        let next = topology.tabify("a", "b", "unused").expect("tabify ok");
        let (id, panels, active) = first_tabs(next.root()).expect("a well exists");
        assert_eq!(id, "w0", "joins the existing well, keeps its id");
        assert_eq!(
            panels.iter().map(Cow::as_ref).collect::<Vec<_>>(),
            vec!["b", "c", "a"],
            "source appended to the existing well",
        );
        assert_eq!(active, 2, "the newly-tabified panel is the visible tab");
        assert_eq!(next.panel_count(), 3);
    }

    #[test]
    fn r1083_tabify_self_is_noop() {
        let topology = ab_topology();
        let next = topology
            .tabify("a", "a", "w0")
            .expect("self-drop is a no-op");
        assert!(first_tabs(next.root()).is_none(), "no well minted");
        assert_eq!(next.panel_ids(), vec!["a", "b"]);
    }

    #[test]
    fn r1083_tabify_unknown_panel_errors() {
        let topology = ab_topology();
        assert_eq!(
            topology.tabify("zzz", "b", "w0"),
            Err(TopologyError::PanelNotFound("zzz".to_string())),
        );
        assert_eq!(
            topology.tabify("a", "zzz", "w0"),
            Err(TopologyError::PanelNotFound("zzz".to_string())),
        );
    }

    // ── model: remove from a well (shrink / collapse) ───────────────

    #[test]
    fn r1083_remove_from_well_shrinks_then_collapses_to_leaf() {
        let topology = DockTopology::new(DockNode::tabs("w0", ["a", "b", "c"].map(Cow::from), 2));
        // Remove the active (last) tab → shrinks, active clamps to new end.
        let shrunk = topology.remove_leaf("c").expect("shrink ok");
        let (id, panels, active) = first_tabs(shrunk.root()).expect("still a well");
        assert_eq!(id, "w0");
        assert_eq!(
            panels.iter().map(Cow::as_ref).collect::<Vec<_>>(),
            vec!["a", "b"]
        );
        assert_eq!(active, 1, "active 2 clamps to the new last index");
        // Removing again drops to one panel → collapses back to a Leaf.
        let collapsed = shrunk.remove_leaf("b").expect("collapse ok");
        assert!(
            matches!(collapsed.root(), DockNode::Leaf { panel_id } if panel_id == "a"),
            "a 1-panel well collapses to a Leaf",
        );
    }

    #[test]
    fn r1083_remove_before_active_shifts_active_down() {
        let topology = DockTopology::new(DockNode::tabs("w0", ["a", "b", "c"].map(Cow::from), 2));
        let next = topology.remove_leaf("a").expect("ok");
        let (_, panels, active) = first_tabs(next.root()).expect("well");
        assert_eq!(
            panels.iter().map(Cow::as_ref).collect::<Vec<_>>(),
            vec!["b", "c"]
        );
        assert_eq!(active, 1, "active 'c' tracked down from index 2 to 1");
    }

    // ── model: split a tabbed well as a whole ───────────────────────

    #[test]
    fn r1083_edge_split_targeting_a_well_keeps_it_intact() {
        let topology = DockTopology::new(DockNode::tabs("w0", ["a", "b"].map(Cow::from), 0));
        let next = topology
            .split_leaf_into(
                "a", // a lives in the well; the whole well splits
                "newp",
                "s0",
                super::SplitterOrientation::Horizontal,
                0.5,
                super::DockSplitPosition::First,
            )
            .expect("split ok");
        let DockNode::Split { first, second, .. } = next.root() else {
            panic!("expected a Split root");
        };
        assert!(
            matches!(first.as_ref(), DockNode::Leaf { panel_id } if panel_id == "newp"),
            "inserted leaf takes the First slot",
        );
        assert!(
            matches!(second.as_ref(), DockNode::Tabs { id, .. } if id == "w0"),
            "the whole well stays together as the sibling",
        );
        assert_eq!(next.panel_count(), 3);
    }

    // ── validation ──────────────────────────────────────────────────

    #[test]
    fn r1083_validate_rejects_too_few_tabs() {
        let err = DockTopology::try_new(DockNode::tabs("w0", [Cow::from("a")], 0)).unwrap_err();
        assert_eq!(
            err,
            TopologyError::TabsTooFew {
                tabs_id: "w0".into(),
                count: 1
            }
        );
    }

    #[test]
    fn r1083_validate_rejects_active_out_of_range() {
        let err =
            DockTopology::try_new(DockNode::tabs("w0", ["a", "b"].map(Cow::from), 5)).unwrap_err();
        assert_eq!(
            err,
            TopologyError::ActiveOutOfRange {
                tabs_id: "w0".into(),
                active: 5,
                count: 2
            },
        );
    }

    #[test]
    fn r1083_validate_rejects_duplicate_well_id() {
        let err = DockTopology::try_new(DockNode::split_horizontal(
            "root_h",
            0.5,
            DockNode::tabs("dup", ["a", "b"].map(Cow::from), 0),
            DockNode::tabs("dup", ["c", "d"].map(Cow::from), 0),
        ))
        .unwrap_err();
        assert_eq!(err, TopologyError::DuplicateTabsId("dup".to_string()));
    }

    #[test]
    fn r1083_validate_rejects_well_id_colliding_with_panel() {
        // The well's own id collides with a leaf panel id → IdCollision.
        let err = DockTopology::try_new(DockNode::split_horizontal(
            "root_h",
            0.5,
            DockNode::leaf("x"),
            DockNode::tabs("x", ["a", "b"].map(Cow::from), 0),
        ))
        .unwrap_err();
        assert_eq!(err, TopologyError::IdCollision("x".to_string()));
    }

    #[test]
    fn r1083_validate_rejects_panel_in_well_duplicating_a_leaf() {
        let err = DockTopology::try_new(DockNode::split_horizontal(
            "root_h",
            0.5,
            DockNode::leaf("a"),
            DockNode::tabs("w0", ["a", "b"].map(Cow::from), 0),
        ))
        .unwrap_err();
        assert_eq!(err, TopologyError::DuplicatePanelId("a".to_string()));
    }

    // ── accessors + swap with a well ────────────────────────────────

    #[test]
    fn r1083_panel_ids_includes_well_panels_in_tab_order() {
        let topology = DockTopology::new(DockNode::split_horizontal(
            "root_h",
            0.5,
            DockNode::leaf("a"),
            DockNode::tabs("w0", ["b", "c"].map(Cow::from), 0),
        ));
        assert_eq!(topology.panel_ids(), vec!["a", "b", "c"]);
        assert_eq!(topology.panel_count(), 3);
        assert_eq!(topology.leaf_count(), 2, "leaf a + the well = 2 pane slots");
        assert_eq!(topology.split_count(), 1);
    }

    #[test]
    fn r1083_swap_relabels_a_panel_inside_a_well() {
        let topology = DockTopology::new(DockNode::split_horizontal(
            "root_h",
            0.5,
            DockNode::leaf("a"),
            DockNode::tabs("w0", ["b", "c"].map(Cow::from), 0),
        ));
        let next = topology.swap_leaves("a", "c").expect("swap ok");
        let (_, panels, _) = first_tabs(next.root()).expect("well");
        assert_eq!(
            panels.iter().map(Cow::as_ref).collect::<Vec<_>>(),
            vec!["b", "a"],
            "c (in the well) swapped with the outer a",
        );
    }

    // ── routing: Center → Tabify SSOT ───────────────────────────────

    #[test]
    fn r1083_intent_for_zone_center_is_tabify() {
        assert_eq!(
            intent_for_zone("a", "b", DockDropZone::Center),
            Some(DockReorganizeIntent::Tabify {
                source: "a".into(),
                target: "b".into()
            }),
        );
        // Edges still split; None still maps to nothing.
        assert!(matches!(
            intent_for_zone("a", "b", DockDropZone::Left),
            Some(DockReorganizeIntent::SplitInsert { .. })
        ));
        assert_eq!(intent_for_zone("a", "b", DockDropZone::None), None);
    }

    // ── coordinator: live apply through the existing R1082 path ─────

    #[test]
    fn r1083_apply_intent_tabify_mints_well_id_and_bumps_only_tabs_seq() {
        let signal = Rc::new(Signal::new(Some(ab_topology())));
        let reorganizer = DockReorganizer::new(Rc::clone(&signal));
        let outcome = reorganizer
            .apply_intent(&DockReorganizeIntent::Tabify {
                source: "a".into(),
                target: "b".into(),
            })
            .expect("tabify applies");
        assert_eq!(outcome, "a -> b");
        let topo = signal.get().expect("topology present after tabify");
        let (id, panels, _) = first_tabs(topo.root()).expect("well minted");
        assert_eq!(
            id, "reorg-tabs-0",
            "fresh well id minted from the tabs sequence"
        );
        assert_eq!(
            panels.iter().map(Cow::as_ref).collect::<Vec<_>>(),
            vec!["b", "a"]
        );
        assert_eq!(reorganizer.tabs_seq(), 1, "tabs counter advanced");
        assert_eq!(
            reorganizer.split_seq(),
            0,
            "split counter untouched by a tabify"
        );
    }

    // ── render: the tab-well walker arm ─────────────────────────────

    fn split_state_for(initial_ratio: f32) -> DockSplitState {
        DockSplitState {
            ratio_signal: Rc::new(Signal::new(initial_ratio)),
            dragging: false,
        }
    }

    #[test]
    fn r1083_walker_renders_tab_strip_keyed_on_well_id_with_active_content() {
        Owner::new().run(|| {
            let topology = DockTopology::new(DockNode::tabs("w0", ["a", "b"].map(Cow::from), 1));
            let scene = view_dock_surface(
                &topology,
                |id| {
                    Scene::Container(
                        pinion_core::scene::ContainerNode::new(vec![])
                            .with_tag(format!("{id}_content")),
                    )
                },
                |_, _| split_state_for(0.5),
                |_| None,
                &Theme::light(),
            );
            // The tab strip is keyed on the well id, one composite tag per tab.
            assert!(has_tag(&scene, "w0"), "tab strip tagged with the well id");
            assert!(has_tag(&scene, "w0#0"), "tab 0 composite tag");
            assert!(has_tag(&scene, "w0#1"), "tab 1 composite tag");
            // Only the active panel (index 1 = "b") renders its content +
            // its drop-target panel root; the inactive tab's content is not
            // painted.
            assert!(has_tag(&scene, "b"), "active panel root present");
            assert!(has_tag(&scene, "b_content"), "active panel content present");
            assert!(
                !has_tag(&scene, "a_content"),
                "inactive tab content not painted"
            );
        });
    }

    #[test]
    fn r1083_walker_active_panel_suppresses_its_own_header() {
        Owner::new().run(|| {
            let topology = DockTopology::new(DockNode::tabs("w0", ["a", "b"].map(Cow::from), 0));
            let scene = view_dock_surface(
                &topology,
                |id| {
                    Scene::Container(
                        pinion_core::scene::ContainerNode::new(vec![])
                            .with_tag(format!("{id}_content")),
                    )
                },
                |_, _| split_state_for(0.5),
                |_| None,
                &Theme::light(),
            );
            // The tab strip is the title row → no per-panel `{tag}#header`.
            assert!(
                !has_tag(&scene, "a#header"),
                "the active tab-well panel suppresses its redundant header",
            );
            assert!(has_tag(&scene, "a#content"), "but keeps its content region");
        });
    }

    // ── persistence: the Tabs wire shape round-trips ────────────────

    #[test]
    fn r1083_tabs_node_serde_round_trips() {
        let node = DockNode::tabs("w0", ["a", "b", "c"].map(Cow::from), 1);
        let json = serde_json::to_string(&node).expect("serialize");
        let parsed: DockNode = serde_json::from_str(&json).expect("parse");
        assert_eq!(parsed, node);
        assert!(
            json.contains("\"type\":\"Tabs\""),
            "internally-tagged Tabs: {json}"
        );
    }

    // ── R1084.1 §5.51 — same-well tabify preserves the stable well id ──

    #[test]
    fn r1084_1_same_well_tabify_preserves_id_and_activates_source() {
        // A 2-panel well; tabify an INACTIVE member onto its well-mate. Only
        // reachable via the RPC path (a pointer drag can't grab an inactive
        // tab). Pre-fix this collapsed the well to a Leaf then re-promoted it
        // under a fresh id; the stable well id MUST be preserved.
        let topology = DockTopology::new(DockNode::tabs("w0", ["b", "c"].map(Cow::from), 0));
        let next = topology
            .tabify("c", "b", "reorg-tabs-0")
            .expect("tabify ok");
        let (id, panels, active) = first_tabs(next.root()).expect("still one well");
        assert_eq!(id, "w0", "the stable well id is preserved, not re-minted");
        assert_eq!(
            panels.iter().map(Cow::as_ref).collect::<Vec<_>>(),
            vec!["b", "c"],
            "panel order preserved (membership unchanged)",
        );
        assert_eq!(
            active, 1,
            "the dropped source (c) is brought forward / active"
        );
    }

    #[test]
    fn r1084_1_same_well_tabify_in_larger_well_preserves_id_and_order() {
        let topology = DockTopology::new(DockNode::tabs("w0", ["a", "b", "c"].map(Cow::from), 0));
        let next = topology
            .tabify("c", "a", "reorg-tabs-9")
            .expect("tabify ok");
        let (id, panels, active) = first_tabs(next.root()).expect("well");
        assert_eq!(id, "w0", "well id preserved");
        assert_eq!(
            panels.iter().map(Cow::as_ref).collect::<Vec<_>>(),
            vec!["a", "b", "c"],
            "order unchanged on an in-well activation",
        );
        assert_eq!(active, 2, "c activated in place");
    }

    #[test]
    fn r1084_1_cross_pane_tabify_still_relocates_source_out_of_its_well() {
        // The same-well guard is NARROW: a panel in well w0 tabified onto a
        // SEPARATE pane must still relocate (the remove+restack path), not be
        // mistaken for a same-well activation.
        let topology = DockTopology::new(DockNode::split_horizontal(
            "root_h",
            0.5,
            DockNode::leaf("a"),
            DockNode::tabs("w0", ["b", "c"].map(Cow::from), 0),
        ));
        // b leaves w0 (w0 had 2 → collapses to Leaf{c}) and stacks onto a.
        let next = topology
            .tabify("b", "a", "reorg-tabs-0")
            .expect("tabify ok");
        assert_eq!(next.panel_count(), 3, "no panel created/destroyed");
        let (id, panels, _) = first_tabs(next.root()).expect("a new well a+b exists");
        assert_eq!(
            id, "reorg-tabs-0",
            "cross-pane tabify mints the fresh well id"
        );
        assert_eq!(
            panels.iter().map(Cow::as_ref).collect::<Vec<_>>(),
            vec!["a", "b"],
        );
    }

    // ── model: set_active_tab (R1085 tab navigation) ────────────────

    /// `a | w0[x, y, z]@0` — a leaf beside a 3-tab well visible on `x`.
    fn leaf_beside_well() -> DockTopology {
        DockTopology::new(DockNode::split_horizontal(
            "root_h",
            0.5,
            DockNode::leaf("a"),
            DockNode::tabs("w0", ["x", "y", "z"].map(Cow::from), 0),
        ))
    }

    #[test]
    fn r1085_set_active_tab_flips_visible_tab() {
        let next = leaf_beside_well()
            .set_active_tab("w0", 2)
            .expect("activate ok");
        let (id, panels, active) = first_tabs(next.root()).expect("well");
        assert_eq!(id, "w0", "well id preserved");
        assert_eq!(
            panels.iter().map(Cow::as_ref).collect::<Vec<_>>(),
            vec!["x", "y", "z"],
            "panels + order unchanged (navigation is not a move)",
        );
        assert_eq!(active, 2, "z is now the visible tab");
    }

    #[test]
    fn r1085_set_active_tab_same_index_is_accepted_noop() {
        // Activating the already-visible tab is an accepted idempotent
        // no-op (mirrors swap_leaves(a, a) / tabify(s, s)).
        let next = leaf_beside_well()
            .set_active_tab("w0", 0)
            .expect("activate ok");
        let (_, _, active) = first_tabs(next.root()).expect("well");
        assert_eq!(active, 0);
    }

    #[test]
    fn r1085_set_active_tab_out_of_range_is_active_out_of_range() {
        // The single validation gate (try_new) catches an index past the
        // well's end — not a check re-implemented in set_active_tab.
        let err = leaf_beside_well()
            .set_active_tab("w0", 3)
            .expect_err("index 3 past a 3-panel well");
        assert_eq!(
            err,
            TopologyError::ActiveOutOfRange {
                tabs_id: "w0".to_string(),
                active: 3,
                count: 3,
            }
        );
    }

    #[test]
    fn r1085_set_active_tab_unknown_well_is_tabs_well_not_found() {
        let err = leaf_beside_well()
            .set_active_tab("nope", 0)
            .expect_err("no such well");
        assert_eq!(err, TopologyError::TabsWellNotFound("nope".to_string()));
    }

    #[test]
    fn r1085_set_active_tab_on_leaf_or_split_id_is_tabs_well_not_found() {
        // A panel id ("a", "x") or a Split id ("root_h") is in the same id
        // namespace but is NOT a tab well — set_active_tab rejects it
        // rather than mistaking it for one.
        let topo = leaf_beside_well();
        for non_well in ["a", "x", "root_h"] {
            assert_eq!(
                topo.set_active_tab(non_well, 0),
                Err(TopologyError::TabsWellNotFound(non_well.to_string())),
                "{non_well:?} is not a tab well",
            );
        }
    }

    #[test]
    fn r1085_set_active_tab_finds_well_nested_in_split_leaves_siblings_intact() {
        let next = leaf_beside_well()
            .set_active_tab("w0", 1)
            .expect("activate ok");
        // The well (nested in root_h.second) flipped to y...
        let (_, _, active) = first_tabs(next.root()).expect("well");
        assert_eq!(active, 1);
        // ...and the sibling leaf `a` + the split id are untouched.
        assert_eq!(next.panel_ids(), vec!["a", "x", "y", "z"]);
        assert_eq!(next.split_ids(), vec!["root_h"]);
    }

    // ── R1096 §5.51 — tab-well enumeration + active read ─────────────

    /// `(w0[p,q]@1) | w1[x,y,z]@0` — two wells under one split, so the
    /// enumeration order + per-well projections are observable.
    fn two_wells() -> DockTopology {
        DockTopology::new(DockNode::split_horizontal(
            "root_h",
            0.5,
            DockNode::tabs("w0", ["p", "q"].map(Cow::from), 1),
            DockNode::tabs("w1", ["x", "y", "z"].map(Cow::from), 0),
        ))
    }

    #[test]
    fn r1096_for_each_tabs_well_enumerates_wells_in_preorder() {
        let mut seen: Vec<(String, usize, usize)> = Vec::new();
        two_wells().for_each_tabs_well(|id, active, panels| {
            seen.push((id.to_string(), active, panels));
        });
        assert_eq!(
            seen,
            vec![("w0".to_string(), 1, 2), ("w1".to_string(), 0, 3),],
            "(well_id, active, panel_count) in depth-first pre-order",
        );
    }

    #[test]
    fn r1096_for_each_tabs_well_skips_a_well_less_topology() {
        // `a | (b | c)` — no wells; the walk visits nothing.
        let mut count = 0usize;
        abc_split().for_each_tabs_well(|_, _, _| count += 1);
        assert_eq!(count, 0);
    }

    #[test]
    fn r1096_tabs_well_count() {
        assert_eq!(two_wells().tabs_well_count(), 2);
        assert_eq!(leaf_beside_well().tabs_well_count(), 1);
        assert_eq!(abc_split().tabs_well_count(), 0);
    }

    #[test]
    fn r1096_tab_well_active_reads_well_and_misses_non_wells() {
        let t = two_wells();
        assert_eq!(t.tab_well_active("w0"), Some(1));
        assert_eq!(t.tab_well_active("w1"), Some(0));
        // An unknown id, a split id, and a stacked-panel id are all misses
        // (only a well's own id resolves).
        assert_eq!(t.tab_well_active("nope"), None);
        assert_eq!(
            t.tab_well_active("root_h"),
            None,
            "a split id is not a well"
        );
        assert_eq!(t.tab_well_active("p"), None, "a panel id is not a well id");
        // A leaf id in a well-less tree is also a miss.
        assert_eq!(abc_split().tab_well_active("a"), None);
    }

    /// `a | (b | c)` — a well-less 3-leaf tree (for the negative cases).
    fn abc_split() -> DockTopology {
        DockTopology::new(DockNode::split_horizontal(
            "root_h",
            0.5,
            DockNode::leaf("a"),
            DockNode::split_horizontal("inner", 0.5, DockNode::leaf("b"), DockNode::leaf("c")),
        ))
    }
}

#[cfg(test)]
mod placeholder_tests {
    //! R685 §5.16 §5.49 — `view_floating_placeholder` substrate
    //! lift tests. The helper was inlined in `hello-dock-panels`
    //! (R683.C 1st consumer); R685 lifts it to substrate on the
    //! 2nd-consumer signal (`hello-dock-panels-editor` round entry)
    //! per [[abstraction-needs-second-consumer]].

    use super::{FloatingPlaceholderStyle, PLACEHOLDER_TAG_SUFFIX, view_floating_placeholder};
    use pinion_core::scene::Scene;
    use pinion_core::theme::Theme;

    #[test]
    fn r685_view_floating_placeholder_tags_with_panel_id_suffix() {
        let theme = Theme::light();
        let scene =
            view_floating_placeholder("inspector", &theme, &FloatingPlaceholderStyle::m3_default());
        let Scene::Container(outer) = &scene else {
            panic!()
        };
        assert_eq!(
            outer.tag.as_deref(),
            Some(format!("inspector{PLACEHOLDER_TAG_SUFFIX}").as_str()),
        );
    }

    #[test]
    fn r1140_view_floating_placeholder_is_a_drop_target() {
        // R1140 §5.51 PR-39 — a torn slot must accept drops so a floater dragged
        // back over its OWN emptied home slot resolves here (the cross-window
        // resolver only finds opted-in drop targets) and the shell paints the
        // redock hint. Without this the self-home gesture showed no preview.
        let theme = Theme::light();
        let scene = view_floating_placeholder(
            "properties",
            &theme,
            &FloatingPlaceholderStyle::m3_default(),
        );
        let Scene::Container(outer) = &scene else {
            panic!()
        };
        assert!(
            outer.layout.drop_target,
            "the torn slot opts in as a drop target (self-home redock)",
        );
    }

    #[test]
    fn r685_view_floating_placeholder_contains_torn_off_label() {
        let theme = Theme::light();
        let scene =
            view_floating_placeholder("viewport", &theme, &FloatingPlaceholderStyle::m3_default());
        let Scene::Container(outer) = &scene else {
            panic!()
        };
        assert_eq!(outer.children.len(), 1, "single Text child");
        let Scene::Text(text) = &outer.children[0] else {
            panic!("expected Text")
        };
        assert!(
            text.content.contains("viewport") && text.content.contains("torn off"),
            "label '{}' contains panel id + 'torn off'",
            text.content,
        );
    }

    #[test]
    fn r685_view_floating_placeholder_default_font_is_14px() {
        // FloatingPlaceholderStyle::m3_default() should fix the
        // font size at 14 px (M3 Body Medium default). Pinned
        // here so a future style tweak doesn't silently drift the
        // hello-dock-panels visual (which calls
        // `with_label_font_size_px(PROPERTY_PANE_FONT_PX)` = 14
        // explicitly to preserve pre-R685 bit-identity).
        let style = FloatingPlaceholderStyle::m3_default();
        assert_eq!(style.label_font_size_px, 14);
    }

    #[test]
    fn r685_view_floating_placeholder_with_label_font_size_px_overrides() {
        let style = FloatingPlaceholderStyle::m3_default().with_label_font_size_px(18);
        assert_eq!(style.label_font_size_px, 18);
    }

    #[test]
    fn r685_floating_window_id_uses_prefix_concat() {
        use super::{DEFAULT_FLOATING_WINDOW_PREFIX, floating_window_id};
        assert_eq!(
            floating_window_id(DEFAULT_FLOATING_WINDOW_PREFIX, "inspector"),
            "torn-inspector",
        );
        // Custom prefix preserves the concat form.
        assert_eq!(
            floating_window_id("floating-", "panel-1"),
            "floating-panel-1"
        );
    }
}

#[cfg(test)]
mod surface_tests {
    //! R685.B §5.16 §5.49 — `view_dock_surface` recursive walker
    //! tests under the R685.B SSOT signature (topology owns all
    //! panel ids / split ids / orientations / initial ratios;
    //! callbacks supply only panel content + reactive split state).

    use super::{DockDropZone, DockNode, DockSplitState, DockTopology, view_dock_surface};
    use pinion_core::reactive::{Owner, Signal};
    use pinion_core::scene::{ContainerNode, Scene};
    use pinion_core::style::{BoxStyle, Color, FlexDirection};
    use pinion_core::theme::Theme;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// (R685.B test fixture) Minimal panel content — a transparent
    /// Container tagged for visual identification. The walker now
    /// auto-builds the [`DockPanelStyle`] from the leaf's `panel_id`,
    /// so tests supply only the inner content `Scene`.
    fn panel_content_for(panel_id: &str) -> Scene {
        Scene::Container(
            ContainerNode::new(vec![])
                .with_tag(format!("{panel_id}_content_marker"))
                .with_style(BoxStyle::filled(Color::rgba(0, 0, 0, 0))),
        )
    }

    /// (R685.B test fixture) Build a fresh [`DockSplitState`] with a
    /// new `Signal<f32>` seeded at the topology's declared initial
    /// ratio. The walker now auto-builds the [`SplitterStyle`] from
    /// the Split's `id` + `orientation`, so the fixture supplies
    /// only the reactive state pair.
    fn split_state_for(initial_ratio: f32) -> DockSplitState {
        DockSplitState {
            ratio_signal: Rc::new(Signal::new(initial_ratio)),
            dragging: false,
        }
    }

    fn theme_light() -> Theme {
        Theme::light()
    }
    fn run_in_owner<R>(f: impl FnOnce() -> R) -> R {
        Owner::new().run(f)
    }

    #[test]
    fn r685_dock_surface_single_leaf_emits_panel_no_splitter_wrap() {
        run_in_owner(|| {
            let topology = DockTopology::single("viewport");
            let scene = view_dock_surface(
                &topology,
                |id| {
                    assert_eq!(id, "viewport");
                    panel_content_for("viewport")
                },
                |_, _| panic!("split_state should not fire for single-leaf"),
                |_| None,
                &theme_light(),
            );
            let Scene::Container(outer) = &scene else {
                panic!()
            };
            assert_eq!(outer.tag.as_deref(), Some("viewport"));
            assert_eq!(outer.children.len(), 2);
        });
    }

    #[test]
    fn r1082_dock_surface_threads_drop_zone_to_the_targeted_panel() {
        run_in_owner(|| {
            // The walker hands each leaf its live zone via the drop_zone
            // closure → only the targeted panel paints the overlay.
            let topology = DockTopology::single("viewport");
            let scene = view_dock_surface(
                &topology,
                |_| panel_content_for("viewport"),
                |_, _| panic!("split_state should not fire for single-leaf"),
                |id| (id == "viewport").then_some(DockDropZone::Bottom),
                &theme_light(),
            );
            let Scene::Container(outer) = &scene else {
                panic!()
            };
            assert_eq!(
                outer.children.len(),
                3,
                "the targeted panel gains the drop-zone overlay",
            );
        });
    }

    #[test]
    fn r685_dock_surface_2_leaf_horizontal_dispatch_by_id() {
        run_in_owner(|| {
            let topology = DockTopology::new(DockNode::split_horizontal(
                "h_split",
                0.40,
                DockNode::leaf("left_panel"),
                DockNode::leaf("right_panel"),
            ));
            let calls: Rc<RefCell<Vec<(String, f32)>>> = Rc::new(RefCell::new(Vec::new()));
            let cc = Rc::clone(&calls);
            let scene = view_dock_surface(
                &topology,
                panel_content_for,
                |split_id, initial_ratio| {
                    cc.borrow_mut().push((split_id.to_string(), initial_ratio));
                    split_state_for(initial_ratio)
                },
                |_| None,
                &theme_light(),
            );
            let Scene::Container(outer) = &scene else {
                panic!()
            };
            assert_eq!(outer.layout.flex_direction, FlexDirection::Row);
            assert_eq!(outer.children.len(), 3);
            // (R685.B SSOT) Walker passes the topology's declared
            // initial_ratio to the callback — binding's Signal
            // constructor seeds from the same SoT.
            assert_eq!(*calls.borrow(), vec![("h_split".to_string(), 0.40)]);
        });
    }

    #[test]
    fn r685_dock_surface_2_leaf_vertical_dispatch_by_id() {
        run_in_owner(|| {
            let topology = DockTopology::new(DockNode::split_vertical(
                "v_split",
                0.30,
                DockNode::leaf("top_panel"),
                DockNode::leaf("bot_panel"),
            ));
            let scene = view_dock_surface(
                &topology,
                panel_content_for,
                |_split_id, initial_ratio| split_state_for(initial_ratio),
                |_| None,
                &theme_light(),
            );
            let Scene::Container(outer) = &scene else {
                panic!()
            };
            assert_eq!(outer.layout.flex_direction, FlexDirection::Column);
            assert_eq!(outer.children.len(), 3);
        });
    }

    #[test]
    fn r685_dock_surface_3_leaf_nested_dispatches_by_declared_id() {
        run_in_owner(|| {
            let topology = DockTopology::new(DockNode::split_horizontal(
                "outer",
                0.5,
                DockNode::split_vertical("inner", 0.3, DockNode::leaf("a"), DockNode::leaf("b")),
                DockNode::leaf("c"),
            ));
            let calls: Rc<RefCell<Vec<(String, f32)>>> = Rc::new(RefCell::new(Vec::new()));
            let cc = Rc::clone(&calls);
            let _scene = view_dock_surface(
                &topology,
                panel_content_for,
                |split_id, initial_ratio| {
                    cc.borrow_mut().push((split_id.to_string(), initial_ratio));
                    split_state_for(initial_ratio)
                },
                |_| None,
                &theme_light(),
            );
            assert_eq!(
                *calls.borrow(),
                vec![("outer".to_string(), 0.5), ("inner".to_string(), 0.3),],
                "DF pre-order with declared ids + topology-sourced ratios",
            );
        });
    }

    #[test]
    fn r685_dock_surface_4_leaf_2x2_grid_by_id() {
        run_in_owner(|| {
            let topology = DockTopology::new(DockNode::split_horizontal(
                "outer",
                0.5,
                DockNode::split_vertical(
                    "left_col",
                    0.5,
                    DockNode::leaf("tl"),
                    DockNode::leaf("bl"),
                ),
                DockNode::split_vertical(
                    "right_col",
                    0.5,
                    DockNode::leaf("tr"),
                    DockNode::leaf("br"),
                ),
            ));
            assert_eq!(topology.split_count(), 3);
            assert_eq!(topology.leaf_count(), 4);
            let calls: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
            let cc = Rc::clone(&calls);
            let _scene = view_dock_surface(
                &topology,
                panel_content_for,
                |split_id, initial_ratio| {
                    cc.borrow_mut().push(split_id.to_string());
                    split_state_for(initial_ratio)
                },
                |_| None,
                &theme_light(),
            );
            assert_eq!(
                *calls.borrow(),
                vec![
                    "outer".to_string(),
                    "left_col".to_string(),
                    "right_col".to_string()
                ],
            );
        });
    }

    #[test]
    fn r685_dock_surface_5_leaf_editor_dispatch_by_id() {
        run_in_owner(|| {
            let topology = DockTopology::new(DockNode::split_vertical(
                "outer",
                0.10,
                DockNode::leaf("toolbar"),
                DockNode::split_vertical(
                    "inner_v",
                    0.80,
                    DockNode::split_horizontal(
                        "middle_h",
                        0.20,
                        DockNode::leaf("outliner"),
                        DockNode::split_horizontal(
                            "inner_h",
                            0.75,
                            DockNode::leaf("viewport"),
                            DockNode::leaf("properties"),
                        ),
                    ),
                    DockNode::leaf("console"),
                ),
            ));
            assert_eq!(topology.split_count(), 4);
            let split_calls: Rc<RefCell<Vec<(String, f32)>>> = Rc::new(RefCell::new(Vec::new()));
            let panel_calls: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
            let sc = Rc::clone(&split_calls);
            let pc = Rc::clone(&panel_calls);
            let _scene = view_dock_surface(
                &topology,
                |id| {
                    pc.borrow_mut().push(id.to_string());
                    panel_content_for(id)
                },
                |split_id, initial_ratio| {
                    sc.borrow_mut().push((split_id.to_string(), initial_ratio));
                    split_state_for(initial_ratio)
                },
                |_| None,
                &theme_light(),
            );
            assert_eq!(
                *split_calls.borrow(),
                vec![
                    ("outer".to_string(), 0.10),
                    ("inner_v".to_string(), 0.80),
                    ("middle_h".to_string(), 0.20),
                    ("inner_h".to_string(), 0.75),
                ],
            );
            assert_eq!(
                *panel_calls.borrow(),
                vec![
                    "toolbar".to_string(),
                    "outliner".to_string(),
                    "viewport".to_string(),
                    "properties".to_string(),
                    "console".to_string()
                ],
            );
        });
    }

    #[test]
    fn r685_dock_surface_split_state_invoked_once_per_split() {
        run_in_owner(|| {
            let topology = DockTopology::new(DockNode::split_horizontal(
                "outer",
                0.5,
                DockNode::split_vertical("inner", 0.5, DockNode::leaf("a"), DockNode::leaf("b")),
                DockNode::leaf("c"),
            ));
            let count = Rc::new(RefCell::new(0_usize));
            let cc = Rc::clone(&count);
            let _ = view_dock_surface(
                &topology,
                panel_content_for,
                |_split_id, initial_ratio| {
                    *cc.borrow_mut() += 1;
                    split_state_for(initial_ratio)
                },
                |_| None,
                &theme_light(),
            );
            assert_eq!(*count.borrow(), topology.split_count());
        });
    }
}
