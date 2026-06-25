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
use std::collections::VecDeque;

use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, DragPayload, DropPoint, External, ExternalIntrospect,
    InterveneError, IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner, ThreadOwnership,
};
use pinion_core::input::PointerWireEvent;
use pinion_core::intent::Intent;
use pinion_core::scene::{ContainerNode, Rect, Scene, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, SizeValue, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme};
use std::rc::Rc;

use pinion_core::reactive::Signal;
use pinion_core::undo::{SignalEdit, UndoStack};

use crate::splitter::{SplitterOrientation, SplitterStyle, view_splitter};
use crate::tabs::{TabsStyle, view_tabs};

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
    // Degenerate rect carries no pixels → never a drop target.
    if panel_rect.w == 0 || panel_rect.h == 0 {
        return DockDropZone::None;
    }
    let x0 = f64::from(panel_rect.x);
    let y0 = f64::from(panel_rect.y);
    let w = f64::from(panel_rect.w);
    let h = f64::from(panel_rect.h);
    // Normalise the absolute cursor into the panel rect, then classify with
    // the shared SSOT [`dock_drop_zone_normalized`]. A cursor outside the
    // rect normalises to a coordinate outside `[0.0, 1.0)`, which that
    // classifier rejects with [`DockDropZone::None`] — exactly the half-open
    // `rect_contains` containment this function applied inline pre-R1080.
    dock_drop_zone_normalized((cursor_x - x0) / w, (cursor_y - y0) / h)
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
    // Half-open [0.0, 1.0): outside the panel on either axis → no zone.
    if !(0.0..1.0).contains(&x_rel) || !(0.0..1.0).contains(&y_rel) {
        return DockDropZone::None;
    }
    let from_left = x_rel;
    let from_right = 1.0 - from_left;
    let from_top = y_rel;
    let from_bottom = 1.0 - from_top;
    // Centre rectangle: at least one band-width clear of every edge.
    let nearest = from_left.min(from_right).min(from_top).min(from_bottom);
    if nearest >= DOCK_EDGE_ZONE_FRAC {
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
    for (panel_id, rect) in panel_rects {
        if *panel_id == source_panel_id {
            continue;
        }
        // Classify the cursor over this panel and map the zone to its
        // intent through the [`intent_for_zone`] SSOT (shared with the
        // symbolic `reorganize` invoke + the R742 pointer coordinator).
        // `DockDropZone::None` yields `None` → keep scanning the next
        // panel, exactly as the pre-R1081 per-arm `match` did.
        let zone = dock_drop_zone_for(*rect, cursor_x, cursor_y);
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

    /// (R1085 §5.51) Make tab `index` the visible tab of the
    /// [`DockNode::Tabs`] well `well_id` — the tab-well **navigation**
    /// gesture, shared by the `activate_tab` invoke (AI / RPC primary)
    /// and (R1086) the pointer tab-strip click. Distinct from
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
    /// behaviour; the pointer drive (R1086) guards the already-active click
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
                // path + the R1086 pointer click) surfaces `"rejected: …"`
                // through `query("last_outcome")` — one outcome SSOT.
                *self.last_outcome.borrow_mut() = Some(format!("rejected: {e}"));
                return Err(e);
            }
        };
        let summary = format!("activate {well_id}#{index}");
        Ok(self.commit(next, summary))
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
            "topology" | "split_seq" | "last_outcome" | "drop_preview" => {
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
                let Some(intent) = resolve_dock_drop(&panel_refs, source, cursor_x, cursor_y)
                else {
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
            _ => Err(InvokeError::UnknownPath),
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

/// R1094 §5.16 §5.41 §5.51 — symbolic event the [`DockPanelExternal`]
/// emits when a drag that had torn the panel into a live floating
/// follower ends back in the dock: released over a dock zone (redock) or
/// snapped back / cancelled (restore). **Remove-only** — the binding
/// reducer drops the panel's floating window if present, an idempotent
/// no-op otherwise. Payload is the panel id ([`IntrospectValue::Text`]).
pub const TEAR_OFF_REDOCK_EVENT: &str = "tear_off_redock";

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
        }
    }

    /// Override the header strip height in logical pixels. Touch
    /// surfaces want ≥ 44 px (Material touch-target floor).
    #[must_use]
    pub const fn with_header_height_px(mut self, height: u32) -> Self {
        self.header_height_px = height;
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
    let content_wrapper = Scene::Container(
        ContainerNode::new(vec![content])
            .with_tag(content_tag)
            .with_layout(LayoutStyle::new().with_flex_grow(1.0)),
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
                    // (R1080/R1081 §5.51) Every dock panel opts in as a drop
                    // target so the R742 router climbs a cursor over the
                    // panel's deeper content tag to THIS root, handing the
                    // pointer coordinator the panel id + a cursor normalised
                    // over the whole panel rect (the zone classifier's input).
                    .with_drop_target(true),
            ),
    )
}

fn composite_tag(panel_tag: &str, suffix: &'static str) -> String {
    format!("{panel_tag}#{suffix}")
}

/// (R1081 §5.51) Edge-band percentage for the drop-zone highlight
/// overlay — the integer-percent (`SizeValue::Percent`) mirror of
/// [`DOCK_EDGE_ZONE_FRAC`] (`0.25`) the strip sizing needs. Kept in sync
/// with the float fraction by `dock_edge_zone_pct_matches_frac`.
const DOCK_EDGE_ZONE_PCT: u8 = 25;

/// (R1081 §5.51) Alpha the drop-zone highlight tint is drawn at (~40% of
/// the [`ColorRole::Accent`] colour) so the docked-into band reads as a
/// translucent overlay, not an opaque fill that hides the panel content.
const DOCK_DROP_HIGHLIGHT_ALPHA: u8 = 0x66;

/// (R1081 §5.51) Build the drop-zone highlight overlay a dock panel
/// paints while it is the live drop target of an in-flight R742 drag — an
/// absolutely-positioned, pointer-transparent layer covering the whole
/// panel, tinting the band the drop would dock into:
/// `Left`/`Right`/`Top`/`Bottom` = a [`DOCK_EDGE_ZONE_FRAC`]-wide edge
/// strip, `Center` = the centre square. The band *fraction* is the
/// [`DOCK_EDGE_ZONE_FRAC`] the classifier uses, and the band painted is
/// always the one for the cursor's currently-classified zone — so the
/// highlighted band tracks the zone the drop would perform. (The strips
/// are rectangular bands, the conventional dock-overlay affordance; the
/// classifier's nearest-edge corner wedges are not drawn — only the
/// winning zone's full band is.)
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
    let edge = DOCK_EDGE_ZONE_PCT;
    let center = 100 - 2 * DOCK_EDGE_ZONE_PCT;
    let (dir, justify, align, w, h) = match zone {
        DockDropZone::None => {
            return Scene::Container(ContainerNode::new(vec![]).with_layout(overlay_layout()));
        }
        DockDropZone::Left => (
            FlexDirection::Row,
            JustifyContent::Start,
            AlignItems::Stretch,
            edge,
            100,
        ),
        DockDropZone::Right => (
            FlexDirection::Row,
            JustifyContent::End,
            AlignItems::Stretch,
            edge,
            100,
        ),
        DockDropZone::Top => (
            FlexDirection::Column,
            JustifyContent::Start,
            AlignItems::Stretch,
            100,
            edge,
        ),
        DockDropZone::Bottom => (
            FlexDirection::Column,
            JustifyContent::End,
            AlignItems::Stretch,
            100,
            edge,
        ),
        DockDropZone::Center => (
            FlexDirection::Row,
            JustifyContent::Center,
            AlignItems::Center,
            center,
            center,
        ),
    };
    let tint = theme
        .resolve(ColorRole::Accent)
        .with_alpha(DOCK_DROP_HIGHLIGHT_ALPHA);
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
            LayoutStyle::new()
                .flex(FlexDirection::Column)
                .with_justify(JustifyContent::Center)
                .with_align_items(AlignItems::Center),
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
fn tear_off_follow_payload(panel_id: &str, cursor: (f64, f64)) -> IntrospectValue {
    IntrospectValue::Json(serde_json::json!({
        "panel": panel_id,
        "x": cursor.0,
        "y": cursor.1,
    }))
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
    detached: Cell<bool>,
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
            reorganizer: None,
            drop_preview: None,
        }
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

    /// (R1081 §5.51) Classify a resolved [`DropPoint`] into the dock
    /// preview it implies: `None` when the cursor is over no panel, over
    /// this same panel (a self-drop is a no-op), or in a dead zone
    /// ([`DockDropZone::None`]); otherwise `Some` with the target panel +
    /// zone. The single classifier `drag_to` (preview) and `drag_release`
    /// (commit) share so the painted affordance and the applied edit
    /// cannot disagree.
    fn resolve_preview(&self, over: Option<&DropPoint>) -> Option<DockDropPreview> {
        let over = over?;
        // The drop target is the panel ROOT (R1080 marks only the root
        // `.drop_target`), but split defensively at `#` in case a future
        // nested target resolves to a composite tag.
        let target = over.tag.split('#').next().unwrap_or(over.tag.as_str());
        if target == self.panel_id.as_ref() {
            return None;
        }
        let zone = dock_drop_zone_normalized(f64::from(over.x_rel), f64::from(over.y_rel));
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
    fn enqueue_tear_off_follow(&self, cursor: (f64, f64)) {
        self.pending_intents.borrow_mut().push_back(Intent {
            tag: Cow::Borrowed(TEAR_OFF_FOLLOW_EVENT),
            payload: tear_off_follow_payload(&self.panel_id, cursor),
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
        // escaped a drop target).
        self.detached.set(false);
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
            // R1094 — if the drag had torn the panel into a live floating
            // follower before landing back on a dock zone, remove that
            // window first so the reorganizer re-places a single panel
            // (redock). A drag that never escaped leaves `detached` false
            // and skips the redock.
            if self.detached.get() {
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
        // cursor-less fallback (no `_at` ever ran: pre-R1093 unit paths /
        // direct `drag_release`) keeps the legacy `tear_off` toggle so an
        // escape still floats without a forwarded cursor.
        if over.is_none() {
            if let Some(cursor) = self.drag_cursor.get() {
                self.enqueue_tear_off_follow(cursor);
            } else {
                self.enqueue_tear_off();
            }
            self.detached.set(true);
            self.tear_off_fired.set(true);
            return;
        }
        // 3. Snapped back over a panel / dead zone / no coordinator. A drag
        // that had detached returns home → restore by removing the floating
        // window this gesture created; a drag that never escaped is the
        // plain snap-back (no commit, today's behaviour).
        if self.detached.get() {
            self.enqueue_tear_off_redock();
        }
        self.detached.set(false);
        self.tear_off_fired.set(false);
    }

    /// (R1093 §5.15 §5.51 §2 #7) Record the absolute window-logical cursor
    /// the router forwards, then delegate to the cursor-less
    /// [`drag_to`](Self::drag_to) so the existing preview/dock logic is
    /// unchanged. The recorded cursor is exposed as scene-as-data via
    /// `query("drag_cursor")`.
    fn drag_to_at(&mut self, payload: &DragPayload, over: Option<DropPoint>, cursor: (f64, f64)) {
        self.drag_cursor.set(Some(cursor));
        // R1094 — a move that has escaped every drop target tears the
        // panel into a live floating follower: latch `detached` and emit
        // the ensure+position follow so the floating window is created (on
        // the first escape) and tracks the cursor (on every subsequent
        // escaped move). Over a panel the existing preview path runs and no
        // follow fires — the panel is still a dock-drop candidate.
        if over.is_none() {
            self.detached.set(true);
            self.enqueue_tear_off_follow(cursor);
        }
        self.drag_to(payload, over);
    }

    /// (R1093 §5.15 §5.51 §2 #7) Record the release cursor, then delegate to
    /// the cursor-less [`drag_release`](Self::drag_release). The cursor
    /// persists after the gesture so an AI can read where the drop landed
    /// (reset on the next [`begin_drag`](Self::begin_drag)).
    fn drag_release_at(
        &mut self,
        payload: &DragPayload,
        over: Option<DropPoint>,
        cursor: (f64, f64),
    ) {
        self.drag_cursor.set(Some(cursor));
        self.drag_release(payload, over);
    }

    /// (R937.1 §5.51) R742 drag abort — the OS revoked the gesture.
    /// Discard it: clear the preview + diagnostics WITHOUT committing a
    /// dock or a tear-off.
    fn drag_cancel(&mut self, _payload: &DragPayload) {
        self.dragging.set(false);
        self.tear_off_fired.set(false);
        self.set_drop_preview(None);
        self.is_drag_armed.set(true);
        // R1094 — a cancelled drag that had torn the panel into a live
        // floating follower restores by removing that window.
        if self.detached.get() {
            self.enqueue_tear_off_redock();
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
            ("send", "string"),
            // R683.C §5.16 §5.49 — direct tear-off invoke channel.
            // See `invoke` rustdoc.
            (TEAR_OFF_EVENT, "string"),
        ])
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "panel_id" => Some(IntrospectValue::Text(self.panel_id.to_string())),
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
            "panel_id" | "dragging" | "tear_off_fired" | "drop_preview" | "drag_cursor"
            | "detached" => Err(InterveneError::ReadOnly),
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
        // clients drive the dock-back without depending on winit's paint
        // timing. Mirror of the R742 escape-drop tear-off — same intent,
        // same payload.
        if path == TEAR_OFF_EVENT {
            self.enqueue_tear_off();
            self.dragging.set(false);
            self.tear_off_fired.set(true);
            self.is_drag_armed.set(true);
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
        CONTENT_TAG_SUFFIX, DockDropZone, DockNode, DockPanelExternal, DockPanelStyle,
        DockReorganizer, DockTopology, HEADER_TAG_SUFFIX, TEAR_OFF_EVENT, TEAR_OFF_FOLLOW_EVENT,
        TEAR_OFF_REDOCK_EVENT, composite_tag, dock_drop_zone_highlight, view_dock_panel,
    };
    use pinion_core::external::{
        DragPayload, DropPoint, External, ExternalIntrospect, InterveneError, IntrospectValue,
    };
    use pinion_core::intent::Intent;
    use pinion_core::reactive::{Owner, Signal};
    use pinion_core::scene::{ContainerNode, Scene};
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
        ext.drag_to_at(&dummy_payload(), None, (123.0, 45.0));
        assert_eq!(
            ext.query("drag_cursor"),
            Some(IntrospectValue::Json(serde_json::json!([123.0, 45.0]))),
            "drag_cursor mirrors the forwarded move cursor"
        );
        // The release cursor overwrites it and persists post-gesture.
        ext.drag_release_at(&dummy_payload(), None, (200.0, 88.0));
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

    #[test]
    fn r1094_escaped_move_emits_follow_and_latches_detached() {
        // R1094 §5.16 §5.41 §5.51 — a drag move past every drop target
        // (over = None) tears the panel into a live floating follower:
        // latch `detached` + emit one ensure+position follow carrying the
        // panel id and the window-logical cursor.
        let mut ext = DockPanelExternal::new("a");
        let _ = ext.begin_drag();
        assert!(!ext.detached(), "fresh gesture has not detached");
        ext.drag_to_at(&dummy_payload(), None, (640.0, 300.0));
        assert!(ext.detached(), "an escaped move detaches");
        let mut received: Vec<Intent> = Vec::new();
        ext.drain_intents(&mut |i| received.push(i));
        assert_eq!(received.len(), 1, "one follow per escaped move");
        assert_eq!(received[0].tag.as_ref(), TEAR_OFF_FOLLOW_EVENT);
        assert_eq!(
            follow_fields(&received[0].payload),
            ("a".to_string(), 640.0, 300.0),
        );
    }

    #[test]
    fn r1094_each_escaped_move_emits_a_follow_for_live_tracking() {
        // Per-move follow: every escaped move re-emits the position so the
        // floating window tracks the cursor (the binding reducer dedups a
        // stationary cursor at `Signal::set`, not here).
        let mut ext = DockPanelExternal::new("a");
        let _ = ext.begin_drag();
        ext.drag_to_at(&dummy_payload(), None, (10.0, 20.0));
        ext.drag_to_at(&dummy_payload(), None, (30.0, 40.0));
        ext.drag_to_at(&dummy_payload(), None, (50.0, 60.0));
        let mut received: Vec<Intent> = Vec::new();
        ext.drain_intents(&mut |i| received.push(i));
        assert_eq!(received.len(), 3, "one follow per escaped move");
        assert!(
            received
                .iter()
                .all(|i| i.tag.as_ref() == TEAR_OFF_FOLLOW_EVENT)
        );
        assert_eq!(
            follow_fields(&received[2].payload),
            ("a".to_string(), 50.0, 60.0),
            "the last follow carries the latest cursor",
        );
    }

    #[test]
    fn r1094_move_over_a_panel_does_not_follow() {
        // A move that stays over a drop target is a dock candidate, not a
        // tear-off: no follow, `detached` stays false.
        let mut ext = DockPanelExternal::new("a");
        let _ = ext.begin_drag();
        ext.drag_to_at(
            &dummy_payload(),
            Some(drop_point("b", 0.5, 0.5)),
            (100.0, 100.0),
        );
        assert!(!ext.detached(), "a move over a panel does not detach");
        let mut received: Vec<Intent> = Vec::new();
        ext.drain_intents(&mut |i| received.push(i));
        assert!(received.is_empty(), "no follow over a drop target");
    }

    #[test]
    fn r1094_escape_release_with_cursor_emits_follow_not_toggle() {
        // The router path forwards a cursor (drag_release_at), so an
        // escape-drop emits the non-toggling follow (final position), NOT
        // the legacy `tear_off` toggle that would race the live follow
        // (the R1071-R1078 double-toggle lesson).
        let mut ext = DockPanelExternal::new("a");
        let _ = ext.begin_drag();
        ext.drag_to_at(&dummy_payload(), None, (640.0, 300.0));
        ext.drag_release_at(&dummy_payload(), None, (700.0, 320.0));
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
        ext.drag_to_at(&dummy_payload(), None, (640.0, 300.0)); // detach
        ext.drag_release_at(
            &dummy_payload(),
            Some(drop_point("a", 0.5, 0.5)),
            (120.0, 40.0),
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
        ext.drag_to_at(&dummy_payload(), None, (640.0, 300.0)); // detach
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
            Some(drop_point("a", 0.5, 0.5)),
            (100.0, 100.0),
        );
        ext.drag_release_at(
            &dummy_payload(),
            Some(drop_point("a", 0.5, 0.5)),
            (100.0, 100.0),
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
        ext.drag_to_at(&dummy_payload(), None, (640.0, 300.0)); // detach
        // Drop on the centre of "b" → swap; the redock removes the floater
        // first so the reorganizer re-places a single panel.
        ext.drag_release_at(
            &dummy_payload(),
            Some(drop_point("b", 0.5, 0.5)),
            (300.0, 300.0),
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
        ext.drag_to_at(&dummy_payload(), None, (640.0, 300.0));
        assert_eq!(
            ext.query("detached"),
            Some(IntrospectValue::Bool(true)),
            "detached is true after an escaped move",
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
    fn r1081_dock_drop_zone_highlight_paints_the_classified_band() {
        run_in_owner(|| {
            let theme = theme_light();
            // An edge zone → one tinted strip child sized to the band.
            let Scene::Container(left) = dock_drop_zone_highlight(DockDropZone::Left, &theme)
            else {
                panic!()
            };
            assert!(left.layout.pointer_transparent, "overlay never grabs input");
            assert_eq!(left.layout.absolute_position, Some((0, 0)));
            assert_eq!(left.children.len(), 1, "edge zone paints one strip");
            // None → an empty overlay (no band).
            let Scene::Container(none) = dock_drop_zone_highlight(DockDropZone::None, &theme)
            else {
                panic!()
            };
            assert!(none.children.is_empty(), "None paints no band");
        });
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
    fn dock_edge_zone_pct_matches_frac() {
        // The integer-percent overlay band must mirror the float zone
        // classifier fraction so the painted affordance and the
        // classified drop region cannot drift.
        assert!(
            (f64::from(super::DOCK_EDGE_ZONE_PCT) / 100.0 - super::DOCK_EDGE_ZONE_FRAC).abs()
                < f64::EPSILON,
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
        DockReorganizer, DockSplitPosition, DockTopology, resolve_dock_drop,
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
