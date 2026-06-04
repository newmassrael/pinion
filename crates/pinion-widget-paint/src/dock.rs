//! R683.B §5.16 §5.41 — backend-agnostic Dock-panel primitive +
//! drag-to-tear-off [`External`].
//!
//! ## Role
//!
//! A **`DockPanel`** is the atomic unit of a multi-pane DCC / IDE /
//! CAD layout (the Phase B → D north star surface). Each panel
//! carries a header strip the user can grab + drag past a threshold
//! to **tear it off** into a new floating window — the canonical
//! pro-tool authoring affordance every Photoshop / Figma / Unreal
//! Editor / `VSCode` panel system ships.
//!
//! v1 ships the **panel primitive** + tear-off detection only. The
//! topology composition (recursive split tree with `Horizontal` /
//! `Vertical` orientations and nested
//! splitters) is application-level for v1, composed via
//! [`splitter::view_splitter`](crate::splitter::view_splitter) +
//! [`view_dock_panel`]. The substrate-as-topology lift is deferred
//! per [[abstraction-needs-second-consumer]] — R683.B atomic 4's
//! `hello-dock-panels` is the 1st consumer; a 2nd consumer with a
//! different topology shape (e.g. an `editor` binding with main
//! viewport + outliner + properties + console + asset browser) will
//! surface the topology-level abstraction's actual contract.
//!
//! ## Tear-off wire
//!
//! [`DockPanelExternal`] captures the pointer on `PointerDown`
//! against the panel's header tag. Each `pointer_move` under
//! capture lock checks the cursor distance from the press-time
//! frame against [`DockPanelStyle::tear_off_threshold_frac`]; when
//! the threshold is crossed the external emits a `tear_off` intent
//! with the panel id as `IntrospectValue::Text` payload. The
//! intent fires exactly once per drag (subsequent moves past the
//! threshold do not re-fire).
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
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner, ThreadOwnership,
};
use pinion_core::input::PointerWireEvent;
use pinion_core::intent::Intent;
use pinion_core::scene::{ContainerNode, Rect, Scene, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme};
use std::rc::Rc;

use pinion_core::reactive::Signal;
use pinion_core::undo::{SignalEdit, UndoStack};

use crate::splitter::{view_splitter, SplitterOrientation, SplitterStyle};

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

    /// (R685 §5.16) Count of [`DockNode::Leaf`] nodes in the
    /// sub-tree rooted at `self`. Useful for `panel_views` callback
    /// validation + persistence size limits.
    #[must_use]
    pub fn leaf_count(&self) -> usize {
        match self {
            Self::Leaf { .. } => 1,
            Self::Split { first, second, .. } => first.leaf_count() + second.leaf_count(),
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
            Self::Leaf { .. } => 0,
            Self::Split { first, second, .. } => 1 + first.split_count() + second.split_count(),
        }
    }

    /// (R685 §5.16) Walk all leaf panel ids in depth-first order
    /// (first child before second). Order is stable across
    /// serialization round-trips so `panel_views` callback indices
    /// align with `panel_ids()[i]` deterministically.
    #[must_use]
    pub fn panel_ids(&self) -> Vec<&str> {
        let mut out = Vec::with_capacity(self.leaf_count());
        self.collect_panel_ids(&mut out);
        out
    }

    fn collect_panel_ids<'a>(&'a self, out: &mut Vec<&'a str>) {
        match self {
            Self::Leaf { panel_id, .. } => out.push(panel_id.as_ref()),
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
                write!(f, "split {split_id:?} has invalid ratio {ratio}; must be finite in [0.0, 1.0]")
            }
            Self::EmptyId => write!(f, "empty id (panel_id or split id) — empty tags collide with InputRouter dispatch"),
            Self::PanelNotFound(id) => write!(f, "panel_id {id:?} not found in topology"),
            Self::RootRemoval => write!(f, "cannot remove the topology's sole panel (an empty topology has no valid layout)"),
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
        Self::try_new(root).expect("DockTopology::new: invariant violation; use try_new for fallible construction")
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

    /// (R685 §5.16) Count of leaf nodes. Equals
    /// `self.panel_ids().len()`.
    #[must_use]
    pub fn leaf_count(&self) -> usize {
        self.root.leaf_count()
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
    // the binding holds a `Signal<DockTopology>`, computes the next
    // value, and `set`s it (or discards it on `Err`), so the reactive
    // re-render is a clean swap. Every result flows back through
    // [`Self::try_new`] so an invalid intermediate tree (a generated id
    // colliding with an existing one, say) surfaces as a typed error
    // instead of corrupting the live topology.
    // ─────────────────────────────────────────────────────────────────

    /// (R686 §5.16 §5.45) Swap the two named leaves' positions in the
    /// tree — the panel previously at `panel_id_a`'s location now sits
    /// where `panel_id_b` was, and vice versa. The tree *shape* (every
    /// Split, every ratio, every split id) is unchanged; only which
    /// panel occupies which leaf slot changes.
    ///
    /// This is the [`DockDropZone::Center`] gesture: dragging panel A
    /// onto panel B's centre swaps them (v1 has no tab-stacking, so a
    /// centre drop is a swap).
    ///
    /// `panel_id_a == panel_id_b` (dropping a panel on itself) is a
    /// well-defined no-op that returns the topology unchanged.
    ///
    /// # Errors
    ///
    /// [`TopologyError::PanelNotFound`] if either id names no leaf.
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
            let ins = insertion
                .take()
                .expect("split_leaf_rec: target leaf visited more than once (unique-id invariant broken)");
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
        DockNode::Leaf { .. } => node.clone(),
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

/// (R685.C atomic 1 §5.16) Discriminator for the unified id-namespace
/// validator — tracks whether an id was first seen as a panel or
/// a Split, so duplicate detection produces the right error
/// variant (same-kind → `DuplicatePanelId` / `DuplicateSplitId`;
/// cross-kind → `IdCollision`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NodeKind {
    Panel,
    Split,
}

/// (R685.B / R685.C §5.16) Internal recursive validator. Walks the
/// node tree depth-first pre-order, accumulating panel + split ids
/// into a unified `HashMap<String, NodeKind>` for duplicate +
/// cross-namespace collision detection. Validates every Split's
/// ratio for finiteness + bounds; rejects empty ids on first
/// encounter.
fn validate_node(
    node: &DockNode,
    seen: &mut std::collections::HashMap<String, NodeKind>,
) -> Result<(), TopologyError> {
    match node {
        DockNode::Leaf { panel_id } => {
            if panel_id.is_empty() {
                return Err(TopologyError::EmptyId);
            }
            match seen.insert(panel_id.to_string(), NodeKind::Panel) {
                None => Ok(()),
                Some(NodeKind::Panel) => {
                    Err(TopologyError::DuplicatePanelId(panel_id.to_string()))
                }
                Some(NodeKind::Split) => {
                    Err(TopologyError::IdCollision(panel_id.to_string()))
                }
            }
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
                Some(NodeKind::Panel) => {
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    /// Cursor is in the panel's centre rectangle — swap the dragged
    /// panel with the target (no tabs in v1, so centre = swap).
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
    // Half-open containment (mirror of `rect_contains`): the right /
    // bottom edges belong to the next panel over.
    if cursor_x < x0 || cursor_y < y0 || cursor_x >= x0 + w || cursor_y >= y0 + h {
        return DockDropZone::None;
    }
    // Normalised distance from each edge, in [0.0, 1.0).
    let from_left = (cursor_x - x0) / w;
    let from_right = 1.0 - from_left;
    let from_top = (cursor_y - y0) / h;
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

/// (R686 §5.16 §5.45) Map an edge [`DockDropZone`] to the
/// `(orientation, position)` a [`DockTopology::split_leaf_into`] needs:
/// a left/right drop splits the target **horizontally**, a top/bottom
/// drop splits it **vertically**; left/top place the dragged panel in
/// the `first` slot, right/bottom in `second`. Returns `None` for the
/// non-edge zones ([`DockDropZone::Center`] / [`DockDropZone::None`]),
/// which are not split gestures.
fn zone_split_geometry(
    zone: DockDropZone,
) -> Option<(SplitterOrientation, DockSplitPosition)> {
    match zone {
        DockDropZone::Left => Some((SplitterOrientation::Horizontal, DockSplitPosition::First)),
        DockDropZone::Right => Some((SplitterOrientation::Horizontal, DockSplitPosition::Second)),
        DockDropZone::Top => Some((SplitterOrientation::Vertical, DockSplitPosition::First)),
        DockDropZone::Bottom => Some((SplitterOrientation::Vertical, DockSplitPosition::Second)),
        DockDropZone::Center | DockDropZone::None => None,
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

/// (R687 §5.16 §5.45) Parse a `{"x","y","w","h"}` JSON object (as
/// `scene/layout` emits each node's integer rect) into a [`Rect`].
/// Returns `None` if any field is missing or not a non-negative
/// integer in `u32` range — the [`DockReorganizeExternal`] `drop`
/// action surfaces that as [`InvokeError::TypeMismatch`].
fn parse_json_rect(v: &serde_json::Value) -> Option<Rect> {
    let field = |k: &str| -> Option<u32> { u32::try_from(v.get(k)?.as_u64()?).ok() };
    Some(Rect::new(field("x")?, field("y")?, field("w")?, field("h")?))
}

/// (R686 §5.16 §5.45) A resolved drag-to-reorganize gesture — the
/// topology edit a panel-drag drop produces, fully decided (no
/// geometry / zone ambiguity left). Built by [`resolve_dock_drop`]
/// from a cursor position over a layout, or by the
/// [`DockReorganizeExternal`] from an AI client's invoke payload.
///
/// The two variants mirror the two drop outcomes a tab-less v1 dock
/// supports:
/// * `Swap` — the cursor landed in a panel's **centre**; the dragged
///   panel and the target trade places ([`DockTopology::swap_leaves`]).
/// * `SplitInsert` — the cursor landed near a panel's **edge**; the
///   dragged panel docks to that side, splitting the target. The
///   `orientation` + `position` are pre-resolved from the edge zone
///   (so the intent carries no invalid-zone state), and applying it
///   moves the dragged panel via [`DockTopology::remove_leaf`] then
///   re-inserts it via [`DockTopology::split_leaf_into`].
///
/// `#[non_exhaustive]` so a future tab-stack outcome can land without
/// breaking downstream `match` arms.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DockReorganizeIntent {
    /// Swap the dragged `source` panel with the `target` panel
    /// (centre drop).
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
}

impl DockReorganizeIntent {
    /// The panel being dragged.
    #[must_use]
    pub fn source(&self) -> &str {
        match self {
            Self::Swap { source, .. } | Self::SplitInsert { source, .. } => source,
        }
    }

    /// The panel dropped onto.
    #[must_use]
    pub fn target(&self) -> &str {
        match self {
            Self::Swap { target, .. } | Self::SplitInsert { target, .. } => target,
        }
    }

    /// (R686 §5.16 §5.45) Apply this gesture to `topology`, producing a
    /// new validated topology. `new_split_id` is the stable id for the
    /// divider a `SplitInsert` creates (ignored by `Swap`); `ratio` is
    /// the initial split fraction (typically
    /// [`DEFAULT_REORGANIZE_RATIO`]).
    ///
    /// `SplitInsert` is a **move**: the source panel is removed from
    /// its current slot (collapsing its old parent split) and
    /// re-inserted beside the target. Composing the two mutation
    /// primitives this way keeps the leaf count invariant (one panel
    /// relocated, none created or destroyed).
    ///
    /// # Errors
    ///
    /// Propagates the underlying mutation primitives' errors:
    /// [`TopologyError::PanelNotFound`] if `source` or `target` names
    /// no leaf, [`TopologyError::RootRemoval`] if `source` is the sole
    /// panel, or a duplicate/collision error if `new_split_id` clashes
    /// with an existing id.
    pub fn apply(
        &self,
        topology: &DockTopology,
        new_split_id: impl Into<Cow<'static, str>>,
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
                new_split_id,
                *orientation,
                ratio,
                *position,
            ),
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
/// produces [`DockReorganizeIntent::Swap`], an edge hit a
/// [`DockReorganizeIntent::SplitInsert`] with the edge mapped to a
/// split orientation + position.
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
        match dock_drop_zone_for(*rect, cursor_x, cursor_y) {
            DockDropZone::None => {}
            DockDropZone::Center => {
                return Some(DockReorganizeIntent::Swap {
                    source: source_panel_id.to_string(),
                    target: (*panel_id).to_string(),
                });
            }
            // `edge` is one of Left/Right/Top/Bottom here (None + Center
            // handled above), so `zone_split_geometry` always returns
            // `Some`. The `if let` keeps the SSOT mapping in one place
            // without a panic path — a hypothetical future non-edge zone
            // would simply be skipped rather than crashing the resolver.
            edge => {
                if let Some((orientation, position)) = zone_split_geometry(edge) {
                    return Some(DockReorganizeIntent::SplitInsert {
                        source: source_panel_id.to_string(),
                        target: (*panel_id).to_string(),
                        orientation,
                        position,
                    });
                }
            }
        }
    }
    None
}

/// (R686 §5.16 §5.45) AI-native drag-to-reorganize handle — the
/// [`External`] a dock editor registers (via
/// [`WidgetCore::create_extra_externals`](pinion_core::WidgetCore::create_extra_externals))
/// to apply topology edits through the
/// [`scene/invoke`](pinion_core::external::ExternalIntrospect::invoke)
/// channel.
///
/// ## Why invoke-driven, not pointer-driven
///
/// A live mouse drag-to-reorganize needs three things at drop time: the
/// **absolute** cursor position, the layout rects of **every** panel,
/// and the dragged panel's identity. The `InputRouter`'s capture lock
/// (R51.34) routes all pointer events to the **source** panel
/// exclusively while a drag is in flight, and an [`External`] only ever
/// sees rect-relative coordinates — so no single pointer-receiving
/// widget can resolve the drop on its own. The entity that *does* hold
/// the full picture is the AI client (or shell drag session), which
/// reads `scene/layout` for the rects and owns the cursor. That client
/// classifies the drop with [`resolve_dock_drop`] / [`dock_drop_zone_for`]
/// and applies it here — the §2 #2 RPC-as-primary-path contract. A
/// pointer-driven mouse gesture with a live drop-zone overlay is a
/// thin consumer that layers on top (a future round) once the shell
/// grows a drag-session that feeds absolute cursor + layout to a
/// resolver.
///
/// ## State
///
/// Holds a shared `Rc<Signal<DockTopology>>` — the live topology the
/// dock editor's view fn reads. A successful reorganize calls
/// `Signal::set` with the mutated topology, and the view fn's reactive
/// subscription re-renders the new layout. The external also exposes
/// the current topology as queryable JSON (`query("topology")`) for
/// §2 #7 scene-as-data introspection.
pub struct DockReorganizeExternal {
    /// Live topology the dock editor view fn reads. Mutated via
    /// `Signal::set` on a successful reorganize → reactive re-render.
    topology: Rc<Signal<DockTopology>>,
    /// Monotonic counter feeding the stable id of each generated
    /// split (`reorg-split-{seq}`). Bumped only when a `SplitInsert`
    /// actually lands, so ids stay gap-minimal + collision-free.
    split_seq: Cell<u64>,
    /// Initial ratio each generated split seeds (even split by default).
    reorganize_ratio: f32,
    /// Last gesture outcome, surfaced via `query("last_outcome")` for
    /// AI clients to confirm an apply succeeded / why it was rejected.
    last_outcome: RefCell<Option<String>>,
    /// (R749 §5.52) When attached via [`with_undo`](Self::with_undo) each
    /// applied reorganize is recorded as a reversible
    /// [`SignalEdit<DockTopology>`] onto this stack (the **third**
    /// [`UndoCommand`](pinion_core::undo::UndoCommand) consumer — editor
    /// workspace history), instead of mutating the topology signal
    /// directly. `None` = the R686 direct-mutate behavior.
    undo: Option<Rc<UndoStack>>,
}

impl core::fmt::Debug for DockReorganizeExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DockReorganizeExternal")
            .field("split_seq", &self.split_seq.get())
            .field("reorganize_ratio", &self.reorganize_ratio)
            .field("last_outcome", &self.last_outcome.borrow())
            .finish_non_exhaustive()
    }
}

impl DockReorganizeExternal {
    /// Construct a reorganize handle over a shared topology signal.
    /// The editor binding creates the `Rc<Signal<DockTopology>>` (via
    /// `Owner::cache`) and hands a clone here so the external + the
    /// view fn share one source of truth.
    #[must_use]
    pub fn new(topology: Rc<Signal<DockTopology>>) -> Self {
        Self {
            topology,
            split_seq: Cell::new(0),
            reorganize_ratio: DEFAULT_REORGANIZE_RATIO,
            last_outcome: RefCell::new(None),
            undo: None,
        }
    }

    /// (R749 §5.52) Record every applied reorganize onto `stack` as a
    /// reversible [`SignalEdit<DockTopology>`], so `invoke "undo"` /
    /// `"redo"` on the stack step the whole layout back and forth — the
    /// editor's workspace history (Phase D seed). Without this the
    /// external mutates the topology signal directly (the R686 behavior).
    #[must_use]
    pub fn with_undo(mut self, stack: Rc<UndoStack>) -> Self {
        self.undo = Some(stack);
        self
    }

    /// Diagnostic: how many splits this external has minted so far.
    #[must_use]
    pub fn split_seq(&self) -> u64 {
        self.split_seq.get()
    }

    /// Apply a resolved [`DockReorganizeIntent`] to the live topology,
    /// returning a human-readable outcome summary on success. Shared
    /// by the `invoke("reorganize", …)` wire and exposed for direct
    /// use by an in-process drag session.
    ///
    /// # Errors
    ///
    /// Returns the [`TopologyError`] from the underlying mutation when
    /// the gesture cannot apply (stale panel id, root removal, id
    /// collision). The live topology is left unchanged on error.
    pub fn apply_intent(
        &self,
        intent: &DockReorganizeIntent,
    ) -> Result<String, TopologyError> {
        let current = self.topology.get();
        let seq = self.split_seq.get();
        let new_split_id = format!("{REORG_SPLIT_ID_PREFIX}{seq}");
        let next = intent.apply(&current, new_split_id, self.reorganize_ratio)?;
        if matches!(intent, DockReorganizeIntent::SplitInsert { .. }) {
            self.split_seq.set(seq + 1);
        }
        let summary = format!("{} -> {}", intent.source(), intent.target());
        *self.last_outcome.borrow_mut() = Some(summary.clone());
        // (R749 §5.52) When an undo stack is attached, record the topology
        // change as a reversible edit (which applies it); else mutate the
        // signal directly (the R686 path).
        if let Some(stack) = &self.undo {
            stack.record(SignalEdit::to(&self.topology, next, summary.clone()));
        } else {
            self.topology.set(next);
        }
        Ok(summary)
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
            ("last_outcome", "string"),
            ("drop", "json"),
            ("reorganize", "json"),
        ])
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "topology" => {
                // §2 #7 scene-as-data — the live topology as queryable
                // JSON. serde cannot fail for the well-formed topology
                // tree; fall back to Null defensively rather than panic.
                serde_json::to_value(self.topology.get())
                    .ok()
                    .map(IntrospectValue::Json)
            }
            "split_seq" => Some(IntrospectValue::Int(i64::try_from(self.split_seq.get()).unwrap_or(i64::MAX))),
            "last_outcome" => Some(match self.last_outcome.borrow().as_deref() {
                Some(s) => IntrospectValue::Text(s.to_string()),
                None => IntrospectValue::Null,
            }),
            _ => None,
        }
    }

    fn intervene(
        &mut self,
        path: &str,
        _value: IntrospectValue,
    ) -> Result<(), InterveneError> {
        match path {
            // Topology mutation flows through `invoke("drop", …)` /
            // `invoke("reorganize", …)`, not direct slot writes — every
            // edit must pass the mutation primitives' validation gate.
            "topology" | "split_seq" | "last_outcome" => Err(InterveneError::ReadOnly),
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
    ///
    /// Both return the outcome summary `"<source> -> <target>"` on a
    /// successful edit and leave the live topology unchanged on
    /// [`InvokeError::Rejected`] (stale id / root removal / id
    /// collision); [`InvokeError::TypeMismatch`] for a malformed
    /// payload.
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
                    *self.last_outcome.borrow_mut() = Some("no drop target".to_string());
                    return Ok(IntrospectValue::Null);
                };
                match self.apply_intent(&intent) {
                    Ok(summary) => Ok(IntrospectValue::Text(summary)),
                    Err(e) => {
                        *self.last_outcome.borrow_mut() = Some(format!("rejected: {e}"));
                        Err(InvokeError::Rejected)
                    }
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
                let intent = match zone {
                    DockDropZone::Center => DockReorganizeIntent::Swap {
                        source: source.to_string(),
                        target: target.to_string(),
                    },
                    DockDropZone::None => return Err(InvokeError::Rejected),
                    edge => {
                        let (orientation, position) =
                            zone_split_geometry(edge).ok_or(InvokeError::Rejected)?;
                        DockReorganizeIntent::SplitInsert {
                            source: source.to_string(),
                            target: target.to_string(),
                            orientation,
                            position,
                        }
                    }
                };
                match self.apply_intent(&intent) {
                    Ok(summary) => Ok(IntrospectValue::Text(summary)),
                    Err(e) => {
                        *self.last_outcome.borrow_mut() =
                            Some(format!("rejected: {e}"));
                        Err(InvokeError::Rejected)
                    }
                }
            }
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

/// R683.B §5.16 — symbolic event name the
/// [`DockPanelExternal`] emits when the user's drag exceeds the
/// configured threshold. Constant (not raw literal) so binding-side
/// reducer match arms can spell the dotted intent tag via
/// [`intent_tag!`](pinion_core::intent_tag) without duplicating the
/// literal: `intent_tag!(PANEL_TAG, dock::TEAR_OFF_EVENT)`.
pub const TEAR_OFF_EVENT: &str = "tear_off";

/// R683.B §5.16 — sidecar carrying [`view_dock_panel`]'s
/// binding-local visual + behavioural constants. `#[non_exhaustive]`
/// so future axes (resize handles, close button, collapse arrow)
/// land via builders without breaking the constructor surface.
///
/// Use [`Self::m3_default`] for the M3-canonical 28-px header strip
/// plus a 0.5 tear-off-threshold-fraction (the user must drag the header across
/// half its own width before the tear-off intent fires — matches the
/// `VSCode` / `IntelliJ` pane tear-off feel).
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct DockPanelStyle {
    /// Header strip extent (logical pixels) along the cross axis of
    /// the panel (height for the default `FlexDirection::Column`
    /// layout). Material 3 list / app-bar dense-row convention is
    /// 28 px; pro-tool authoring surfaces (DCC / IDE panels) use
    /// 24-32 px for compactness.
    pub header_height_px: u32,
    /// Fraction of the header extent the cursor must travel from
    /// the press point before [`TEAR_OFF_EVENT`] fires. Default
    /// `0.5` — half the header width matches the implicit
    /// `VSCode` / `JetBrains` feel (the user has to commit to the
    /// drag).
    ///
    /// Cursor delta is computed via the L∞ norm (`max(|Δx_rel|,
    /// |Δy_rel|)`) so diagonal drag past either axis fires. Pure
    /// horizontal or pure vertical drag through `Δx_rel =
    /// tear_off_threshold_frac` is the canonical UX trigger.
    pub tear_off_threshold_frac: f32,
    /// Paint-side tag the panel's outer
    /// [`Scene::Container`] carries. The header strip is tagged
    /// `{tag}#header` (composite-tag convention R51.42); the
    /// content area is tagged `{tag}#content`. The
    /// [`DockPanelExternal`] is registered against the header tag
    /// so deepest-tagged hit-test routes `PointerDown` on the
    /// header to it.
    pub tag: Cow<'static, str>,
    /// Font size for the header title text. M3 label-medium token
    /// = 12 sp by default; reads tightly against the 28-px header
    /// strip.
    pub header_font_size_px: u32,
}

impl DockPanelStyle {
    /// (R683.B §5.16) M3-canonical default: 28-px header, 0.5
    /// tear-off fraction, 12-px header font.
    #[must_use]
    pub fn m3_default(tag: impl Into<Cow<'static, str>>) -> Self {
        Self {
            header_height_px: 28,
            tear_off_threshold_frac: 0.5,
            tag: tag.into(),
            header_font_size_px: 12,
        }
    }

    /// Override the tear-off threshold fraction. Floor `0.0` makes
    /// the tear-off fire on the very first `pointer_move`; ceiling
    /// `1.0` requires the cursor to drag a full header-extent past
    /// the press point before firing. Out-of-range inputs degrade
    /// the UX but do not abort (the L∞ delta saturates at `1.0`
    /// inside the header rect; under capture lock `x_rel` / `y_rel`
    /// can exceed `[0.0, 1.0]`).
    #[must_use]
    pub fn with_tear_off_threshold_frac(mut self, frac: f32) -> Self {
        self.tear_off_threshold_frac = frac;
        self
    }

    /// Override the header strip height in logical pixels. Touch
    /// surfaces want ≥ 44 px (Material touch-target floor).
    #[must_use]
    pub const fn with_header_height_px(mut self, height: u32) -> Self {
        self.header_height_px = height;
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
/// The header strip is the drag handle for tear-off: the
/// [`DockPanelExternal`] the binding registers against the
/// `{tag}#header` composite-tag receives `PointerDown` + tracks
/// drag distance + emits [`TEAR_OFF_EVENT`] past the threshold.
///
/// # Panics
///
/// Never panics on its own — `title` is borrowed verbatim into a
/// `TextNode`; `content` is moved into the content container
/// without inspection.
#[must_use]
pub fn view_dock_panel(
    title: &str,
    content: Scene,
    theme: &Theme,
    style: &DockPanelStyle,
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
    Scene::Container(
        ContainerNode::new(vec![header, content_wrapper])
            .with_tag(style.tag.clone())
            .with_style(BoxStyle::filled(
                theme.resolve(ColorRole::SurfaceContainer),
            ))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Stretch),
            ),
    )
}

fn composite_tag(panel_tag: &str, suffix: &'static str) -> String {
    format!("{panel_tag}#{suffix}")
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
        .with_style(BoxStyle::filled(theme.resolve(ColorRole::SurfaceContainerLow)))
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
#[must_use]
pub fn view_dock_surface<P, S>(
    topology: &DockTopology,
    panel_content: P,
    split_state: S,
    theme: &Theme,
) -> Scene
where
    P: Fn(&str) -> Scene,
    S: Fn(&str, f32) -> DockSplitState,
{
    view_dock_surface_node(topology.root(), &panel_content, &split_state, theme)
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
fn view_dock_surface_node<P, S>(
    node: &DockNode,
    panel_content: &P,
    split_state: &S,
    theme: &Theme,
) -> Scene
where
    P: Fn(&str) -> Scene,
    S: Fn(&str, f32) -> DockSplitState,
{
    match node {
        DockNode::Leaf { panel_id } => {
            let content = panel_content(panel_id.as_ref());
            // Walker builds the panel style from the topology's
            // panel_id — no caller drift possible (SSOT).
            let style = DockPanelStyle::m3_default(panel_id.clone());
            view_dock_panel(panel_id.as_ref(), content, theme, &style)
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
                view_dock_surface_node(first, panel_content, split_state, theme);
            let second_scene =
                view_dock_surface_node(second, panel_content, split_state, theme);
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

/// (R683.B §5.16) Cursor snapshot captured on the first
/// `pointer_move` under capture lock. The drag distance is computed
/// as `(x_rel - cursor_x_at_press, y_rel - cursor_y_at_press)` each
/// subsequent frame; the L∞ norm (`max(|Δx|, |Δy|)`) crosses the
/// `tear_off_threshold_frac` to fire the intent.
#[derive(Debug, Clone, Copy)]
struct DockDragStart {
    cursor_x: f32,
    cursor_y: f32,
}

/// (R683.B §5.16, R683.C `InputRouter` routing fix)
/// Drag-to-tear-off External for the [`view_dock_panel`] header strip.
/// Registered by the binding via
/// [`WidgetCore::create_extra_externals`](pinion_core::WidgetCore::create_extra_externals)
/// against the **panel root tag** (e.g. `"inspector"`, the
/// [`DockPanelStyle::tag`]), NOT the composite `"inspector#header"`
/// tag the paint emits on the header strip. The R51.42 `dispatch_send`
/// and `forward_pointer_move` paths always split a composite paint tag
/// at `#` and look up the state-scene External by the primary half, so
/// the External must live at the panel root tag for the `InputRouter`
/// to route events to it. R683.B's first-cut emit registered the
/// External at the composite tag, which made `dispatch_send` silently
/// skip every press: no `pointer_move` ever reached the External and
/// the tear-off arc was unobservable from the `InputRouter` side.
/// R683.C surfaced and corrected the mismatch.
///
/// ## Wire
///
/// `wants_pointer_capture = true` so the cursor lock survives the
/// press → drag → release span (the user can drag well past the
/// header strip before tear-off fires). Each `pointer_move` under
/// capture lock checks the L∞ delta from the press-time frame
/// against [`DockPanelStyle::tear_off_threshold_frac`]; on first
/// crossing the external emits a [`TEAR_OFF_EVENT`] intent with the
/// panel id as the `IntrospectValue::Text` payload + sets the
/// `fired_for_drag` guard so subsequent moves do not re-fire.
///
/// `PointerUp` / `PointerCancel` (delivered via the
/// [`ExternalIntrospect::invoke`] `"send"` channel) clear the drag
/// snapshot + the `fired_for_drag` guard so the next press starts a
/// fresh cycle.
///
/// ## Sub-region discriminator
///
/// Because the External lives at the panel root tag, the InputRouter
/// dispatches `send` events for clicks on EITHER the header OR the
/// content sub-region (both paint tags split to the same primary).
/// The R51.42 wire-payload prefix is the sub-index: `"header:PointerDown"`,
/// `"content:PointerDown"`, etc. Only `"header:*"` events arm the
/// drag — `"content:*"` clears `is_drag_armed` so a press on the
/// content area's empty space cannot accidentally fire `tear_off` on a
/// drag past the threshold. The pre-R683.C unit tests that called
/// `invoke("send", Text("PointerUp"))` with bare event names continue
/// to work for the teardown half (bare events split to no sub-index
/// and clear the drag state too); the production invoking-via-
/// InputRouter path always carries a sub-index prefix.
///
/// ## Pattern of operations
///
/// 1. Construct: `DockPanelExternal::new(panel_id, threshold_frac)`.
/// 2. Application's `create_extra_externals` registers the External
///    against the **panel root tag** (matching the view fn's panel
///    root Container tag — typically the same `panel_id` passed to
///    `DockPanelExternal::new`).
/// 3. User presses on the header + drags past the threshold — the
///    InputRouter dispatches `"header:PointerDown"` (arms the drag)
///    + `pointer_move` frames; the External emits the `tear_off`
///    intent.
/// 4. Binding's `WidgetCore::update` reducer catches the dotted
///    intent `{panel_tag}.tear_off` (the runtime walker prefixes the
///    bare `TEAR_OFF_EVENT` with the registered External tag, which is
///    now the panel root tag, not the composite header tag) + pushes
///    a fresh `WindowSpec` onto its `Signal<Vec<WindowSpec>>`.
/// 5. R683.A `reconcile_windows` Effect picks up the signal change +
///    spawns the new floating window with the torn-off panel's content.
#[allow(clippy::doc_markdown, clippy::doc_lazy_continuation)]
pub struct DockPanelExternal {
    /// Stable panel identifier carried into the tear-off intent
    /// payload. The binding's reducer + the
    /// `Signal<Vec<WindowSpec>>` push use this to determine which
    /// panel was torn off + what content the new window should
    /// host.
    panel_id: Cow<'static, str>,
    /// Tear-off threshold as a fraction of the header rect extent
    /// (matches [`DockPanelStyle::tear_off_threshold_frac`]). The
    /// external receives a copy of the style's value at
    /// construction so the threshold can be queried + introspected.
    tear_off_threshold_frac: f32,
    /// Drag-start snapshot. `None` between presses; `Some` once
    /// the first `pointer_move` under capture lock calibrates.
    drag_start: Cell<Option<DockDragStart>>,
    /// Whether the `tear_off` intent has already been emitted for
    /// the current drag. Guards against multi-fire (every
    /// `pointer_move` past the threshold would otherwise re-fire,
    /// pushing N+1 `WindowSpec`s per single user drag).
    fired_for_drag: Cell<bool>,
    /// Pending intents waiting for the framework's
    /// [`External::drain_intents`] poll. v1 fires exactly one
    /// `tear_off` per drag, so the queue depth is `≤ 1` in steady
    /// state, but the `VecDeque` shape leaves room for future
    /// multi-event drags (e.g. an `tear_off_armed` precursor +
    /// `tear_off` final).
    pending_intents: RefCell<VecDeque<Intent>>,
    /// R683.C §5.16 — drag-arm flag. Set to `true` on
    /// `invoke("send", "header:PointerDown")` (the InputRouter
    /// dispatches this when the user presses on the header strip
    /// because the External lives at the panel root tag + the R51.42
    /// payload format prefixes the sub-index). Cleared on
    /// `"content:PointerDown"` (a press on the content area must NOT
    /// drive tear-off), `"PointerUp"` / `"PointerCancel"` (drag
    /// teardown), or via direct construction default. `pointer_move`
    /// gates its drag math on this flag so content-area drags do not
    /// fire `tear_off` accidentally.
    ///
    /// Defaults to `true` for backward-compat with the R683.B unit
    /// tests that called `pointer_move` directly without simulating
    /// the press arc — production flows always go through the
    /// InputRouter's `dispatch_send` which sets the flag explicitly.
    is_drag_armed: Cell<bool>,
}

impl core::fmt::Debug for DockPanelExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DockPanelExternal")
            .field("panel_id", &self.panel_id)
            .field("tear_off_threshold_frac", &self.tear_off_threshold_frac)
            .field("drag_start", &self.drag_start.get())
            .field("fired_for_drag", &self.fired_for_drag.get())
            .finish_non_exhaustive()
    }
}

impl DockPanelExternal {
    /// Construct a dock-panel tear-off External for the given
    /// panel id + threshold. The threshold must match
    /// [`DockPanelStyle::tear_off_threshold_frac`] the view fn
    /// uses — they are paired (visual + drag detection) for the
    /// canonical UX.
    #[must_use]
    pub fn new(
        panel_id: impl Into<Cow<'static, str>>,
        tear_off_threshold_frac: f32,
    ) -> Self {
        Self {
            panel_id: panel_id.into(),
            tear_off_threshold_frac,
            drag_start: Cell::new(None),
            fired_for_drag: Cell::new(false),
            pending_intents: RefCell::new(VecDeque::new()),
            is_drag_armed: Cell::new(true),
        }
    }

    /// Read the panel id this external carries — the payload the
    /// `tear_off` intent ships.
    #[must_use]
    pub fn panel_id(&self) -> &str {
        &self.panel_id
    }

    /// Read the tear-off threshold fraction.
    #[must_use]
    pub fn tear_off_threshold_frac(&self) -> f32 {
        self.tear_off_threshold_frac
    }

    /// Diagnostic: drag-in-progress flag. `true` between the
    /// press-time `pointer_move` calibration and the `PointerUp`
    /// / `PointerCancel` clear.
    #[must_use]
    pub fn is_dragging(&self) -> bool {
        self.drag_start.get().is_some()
    }

    /// Diagnostic: whether the `tear_off` intent has fired for the
    /// current drag. `false` until the threshold is crossed; back
    /// to `false` after the release clears the cycle.
    #[must_use]
    pub fn tear_off_fired(&self) -> bool {
        self.fired_for_drag.get()
    }

    /// Pure projection: compute the L∞ cursor delta against the
    /// press-time snapshot. Returns `None` before the drag
    /// calibrates. Exposed `pub(crate)` for unit tests; not part of
    /// the public surface.
    pub(crate) fn cursor_delta_l_inf(&self, x_rel: f32, y_rel: f32) -> Option<f32> {
        let snapshot = self.drag_start.get()?;
        let dx = (x_rel - snapshot.cursor_x).abs();
        let dy = (y_rel - snapshot.cursor_y).abs();
        Some(dx.max(dy))
    }

    /// Enqueue the `tear_off` intent. Internal helper —
    /// `pointer_move` calls this exactly once per drag when the
    /// threshold is crossed.
    fn enqueue_tear_off(&self) {
        self.pending_intents.borrow_mut().push_back(Intent {
            tag: Cow::Borrowed(TEAR_OFF_EVENT),
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

    /// Capture lock so the cursor stays pinned to the header strip
    /// for the duration of the press, even when the cursor strays
    /// outside the header rect (the natural tear-off path —
    /// dragging the panel out of its dock slot toward a new window
    /// position).
    fn wants_pointer_capture(&self) -> bool {
        true
    }

    /// R51.34 §5.15 + §5.35 — calibrate drag-start on the first
    /// frame, accumulate delta on subsequent frames, fire
    /// [`TEAR_OFF_EVENT`] intent once when the L∞ delta crosses
    /// [`DockPanelStyle::tear_off_threshold_frac`].
    ///
    /// R683.C drag-arm gate: production flows that route through the
    /// `InputRouter`'s `forward_pointer_move` always carry a press
    /// arc (`"header:PointerDown"` or `"content:PointerDown"`) that
    /// sets [`Self::is_drag_armed`]. Pressing on the content area
    /// disarms the drag so the tear-off intent does not fire on a
    /// drag through the panel's content body. Pre-R683.C unit tests
    /// that exercise this method without simulating the press arc
    /// continue to work because the construction default is `armed =
    /// true`.
    fn pointer_move(&mut self, x_rel: f32, y_rel: f32) {
        if !self.is_drag_armed.get() {
            return;
        }
        if self.drag_start.get().is_none() {
            self.drag_start.set(Some(DockDragStart {
                cursor_x: x_rel,
                cursor_y: y_rel,
            }));
            return;
        }
        if self.fired_for_drag.get() {
            // Tear-off already fired — no more work for this drag.
            // The binding's reducer will have already pushed a new
            // WindowSpec; subsequent cursor jitter must not re-fire.
            return;
        }
        let Some(delta) = self.cursor_delta_l_inf(x_rel, y_rel) else {
            return;
        };
        if delta >= self.tear_off_threshold_frac {
            self.enqueue_tear_off();
            self.fired_for_drag.set(true);
        }
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
            ("tear_off_threshold_frac", "float"),
            ("dragging", "bool"),
            ("tear_off_fired", "bool"),
            ("send", "string"),
            // R683.C §5.16 §5.49 — direct tear-off invoke channel.
            // See `invoke` rustdoc.
            (TEAR_OFF_EVENT, "string"),
        ])
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "panel_id" => Some(IntrospectValue::Text(self.panel_id.to_string())),
            "tear_off_threshold_frac" => {
                Some(IntrospectValue::Float(f64::from(self.tear_off_threshold_frac)))
            }
            "dragging" => Some(IntrospectValue::Bool(self.is_dragging())),
            "tear_off_fired" => Some(IntrospectValue::Bool(self.tear_off_fired())),
            _ => None,
        }
    }

    fn intervene(
        &mut self,
        path: &str,
        _value: IntrospectValue,
    ) -> Result<(), InterveneError> {
        match path {
            // Every slot is framework-owned or construction-time
            // fixed. AI clients drive the tear-off through the
            // `invoke("send", ...)` channel + the binding's
            // reducer + the windows_signal push — not by
            // intervening on dragging / tear_off_fired directly.
            "panel_id" | "tear_off_threshold_frac" | "dragging" | "tear_off_fired" => {
                Err(InterveneError::ReadOnly)
            }
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
    ///   / `"PointerCancel"` — drag teardown. Clears `drag_start`,
    ///   `fired_for_drag`, and re-arms (`is_drag_armed = true`) so
    ///   the next press starts a fresh cycle.
    /// * `"header:PointerLeave"` / `"content:PointerLeave"` / bare
    ///   `"PointerLeave"` — no-op (the hover-leave does not clear
    ///   capture).
    /// * Other / unknown event names — `InvokeError::UnknownPath`.
    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        // R683.C §5.16 §5.49 — direct tear-off invoke. The drag path
        // requires a populated `InputRouter::last_paint_scene` for the
        // addressed window; on freshly-spawned floating windows under
        // headless RPC, the router has no paint scene until winit's
        // next paint cycle fires `finalize_frame_for_window`. The
        // direct `tear_off` invoke bypasses the drag simulation so AI
        // clients can drive the dock-back gesture without depending on
        // winit's paint timing. Mirror of [`TEAR_OFF_EVENT`] enqueue —
        // same intent, same payload, same single-fire guard.
        if path == TEAR_OFF_EVENT {
            // Idempotent on the firing guard: re-invoking on an
            // already-fired drag is allowed (the binding's reducer
            // toggles `is_panel_floating(...)`, so a second call when
            // already torn off becomes the dock-back). Reset the drag
            // bookkeeping (drag_start / fired_for_drag / is_drag_armed)
            // to the same fresh state a `PointerUp` / `PointerCancel`
            // would produce so subsequent pointer-driven drags start
            // a fresh calibration cycle.
            self.enqueue_tear_off();
            self.drag_start.set(None);
            self.fired_for_drag.set(false);
            self.is_drag_armed.set(true);
            return Ok(IntrospectValue::Null);
        }
        if path != "send" {
            return Err(InvokeError::UnknownPath);
        }
        let raw = args.as_str().ok_or(InvokeError::TypeMismatch)?;
        // Split `"sub_index:Event"` into `(Some(sub_index), Event)` or
        // `(None, raw_event)` if no `:` separator is present.
        let (sub_index, event_name) = match raw.split_once(':') {
            Some((sub, ev)) => (Some(sub), ev),
            None => (None, raw),
        };
        match PointerWireEvent::from_wire_name(event_name) {
            Some(PointerWireEvent::Up | PointerWireEvent::Cancel) => {
                self.drag_start.set(None);
                self.fired_for_drag.set(false);
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
    //! R683.B §5.16 — Dock-panel paint + tear-off wire tests.
    //!
    //! Pins the load-bearing invariants the
    //! [`hello-dock-panels`](crate) + future `DockSurface` consumers
    //! rely on:
    //!
    //! 1. **Paint shape**: outer Container carries `tag` + 2
    //!    children (header strip + content wrapper). Header tagged
    //!    `{tag}#header`, content tagged `{tag}#content`.
    //! 2. **Header height**: header child's layout matches
    //!    `header_height_px` style.
    //! 3. **Header text**: header contains a `TextNode` with the
    //!    supplied title.
    //! 4. **Tear-off threshold default**: 0.5 (M3 default).
    //! 5. **Drag calibration**: first `pointer_move` snapshots; no
    //!    intent fires.
    //! 6. **Threshold crossing**: drag past threshold fires exactly
    //!    one `tear_off` intent with the panel id payload.
    //! 7. **Single-fire guard**: subsequent moves past threshold do
    //!    not re-fire.
    //! 8. **`PointerUp` clears state**: `drag_start` + fired guard
    //!    both reset on the canonical release.
    //! 9. **Threshold not reached**: short drag → release → no
    //!    intent fired.
    //! 10. **L∞ delta semantics**: diagonal drag fires when EITHER
    //!     axis crosses the threshold.
    //! 11. **Introspect schema + query**: `panel_id` / threshold /
    //!     `dragging` / `tear_off_fired` all queryable.
    //! 12. **Composite tag format**: `{tag}#header` /
    //!     `{tag}#content`.

    use super::{
        composite_tag, view_dock_panel, DockPanelExternal, DockPanelStyle,
        CONTENT_TAG_SUFFIX, HEADER_TAG_SUFFIX, TEAR_OFF_EVENT,
    };
    use pinion_core::external::{External, ExternalIntrospect, IntrospectValue};
    use pinion_core::intent::Intent;
    use pinion_core::reactive::Owner;
    use pinion_core::scene::{ContainerNode, Scene};
    use pinion_core::theme::Theme;

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
            let scene = view_dock_panel("My Panel", empty_content(), &theme_light(), &style);
            let Scene::Container(outer) = &scene else { panic!() };
            assert_eq!(outer.tag.as_deref(), Some(PANEL_TAG));
            assert_eq!(outer.children.len(), 2);
        });
    }

    #[test]
    fn r683_view_dock_panel_header_tagged_with_composite_suffix() {
        run_in_owner(|| {
            let style = DockPanelStyle::m3_default(PANEL_TAG);
            let scene = view_dock_panel("Title", empty_content(), &theme_light(), &style);
            let Scene::Container(outer) = &scene else { panic!() };
            let Scene::Container(header) = &outer.children[0] else { panic!() };
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
            let scene = view_dock_panel("Title", empty_content(), &theme_light(), &style);
            let Scene::Container(outer) = &scene else { panic!() };
            let Scene::Container(content) = &outer.children[1] else { panic!() };
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
            let scene = view_dock_panel("Title", empty_content(), &theme_light(), &style);
            let Scene::Container(outer) = &scene else { panic!() };
            let Scene::Container(header) = &outer.children[0] else { panic!() };
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
            let scene = view_dock_panel("Inspector", empty_content(), &theme_light(), &style);
            let Scene::Container(outer) = &scene else { panic!() };
            let Scene::Container(header) = &outer.children[0] else { panic!() };
            // Header has exactly one child: the title TextNode.
            assert_eq!(header.children.len(), 1);
            let Scene::Text(text) = &header.children[0] else { panic!() };
            assert_eq!(text.content, "Inspector");
        });
    }

    #[test]
    fn r683_dock_panel_style_m3_default_carries_canonical_defaults() {
        let style = DockPanelStyle::m3_default(PANEL_TAG);
        assert_eq!(style.header_height_px, 28);
        assert!((style.tear_off_threshold_frac - 0.5).abs() < f32::EPSILON);
        assert_eq!(style.header_font_size_px, 12);
        assert_eq!(style.tag.as_ref(), PANEL_TAG);
    }

    #[test]
    fn r683_dock_panel_style_with_tear_off_threshold_override() {
        let style = DockPanelStyle::m3_default(PANEL_TAG).with_tear_off_threshold_frac(0.25);
        assert!((style.tear_off_threshold_frac - 0.25).abs() < f32::EPSILON);
    }

    #[test]
    fn r683_dock_panel_external_first_pointer_move_calibrates_no_intent() {
        let mut ext = DockPanelExternal::new("inspector_panel", 0.5);
        ext.pointer_move(0.3, 0.5);
        assert!(ext.is_dragging());
        assert!(!ext.tear_off_fired());
        let mut received: Vec<Intent> = Vec::new();
        ext.drain_intents(&mut |i| received.push(i));
        assert!(
            received.is_empty(),
            "press-time frame must not enqueue any intent",
        );
    }

    #[test]
    fn r683_dock_panel_external_drag_past_threshold_fires_tear_off() {
        let mut ext = DockPanelExternal::new("inspector_panel", 0.5);
        // Press at (0.3, 0.5); move to (0.85, 0.5) — Δx = 0.55, past 0.5.
        ext.pointer_move(0.3, 0.5);
        ext.pointer_move(0.85, 0.5);
        assert!(ext.tear_off_fired());
        let mut received: Vec<Intent> = Vec::new();
        ext.drain_intents(&mut |i| received.push(i));
        assert_eq!(received.len(), 1, "exactly one tear_off per drag");
        assert_eq!(received[0].tag.as_ref(), TEAR_OFF_EVENT);
        assert_eq!(received[0].payload.as_str(), Some("inspector_panel"));
    }

    #[test]
    fn r683_dock_panel_external_subsequent_moves_past_threshold_do_not_refire() {
        let mut ext = DockPanelExternal::new("p1", 0.3);
        ext.pointer_move(0.0, 0.5);
        ext.pointer_move(0.5, 0.5); // crosses threshold, fires once
        // Continue dragging further — must NOT re-fire.
        ext.pointer_move(0.7, 0.5);
        ext.pointer_move(0.9, 0.5);
        let mut received: Vec<Intent> = Vec::new();
        ext.drain_intents(&mut |i| received.push(i));
        assert_eq!(
            received.len(),
            1,
            "multi-fire guard must keep total tear_offs = 1 per drag",
        );
    }

    #[test]
    fn r683_dock_panel_external_pointer_up_clears_drag_state() {
        let mut ext = DockPanelExternal::new("p1", 0.5);
        ext.pointer_move(0.3, 0.5);
        ext.pointer_move(0.85, 0.5); // fires
        assert!(ext.tear_off_fired());
        // Drain the intent off the queue (mirror of framework's
        // per-frame drain) before checking state — the drain
        // empties the queue but does not clear the
        // tear_off_fired guard (the guard clears on PointerUp).
        let mut received: Vec<Intent> = Vec::new();
        ext.drain_intents(&mut |i| received.push(i));
        assert_eq!(received.len(), 1);
        // PointerUp clears both.
        ext.invoke("send", IntrospectValue::Text("PointerUp".to_string()))
            .expect("invoke send PointerUp returns Ok");
        assert!(!ext.is_dragging());
        assert!(!ext.tear_off_fired());
    }

    #[test]
    fn r683_dock_panel_external_pointer_cancel_also_clears() {
        let mut ext = DockPanelExternal::new("p1", 0.5);
        ext.pointer_move(0.3, 0.5);
        assert!(ext.is_dragging());
        ext.invoke("send", IntrospectValue::Text("PointerCancel".to_string()))
            .expect("invoke send PointerCancel returns Ok");
        assert!(!ext.is_dragging());
    }

    #[test]
    fn r683_dock_panel_external_short_drag_no_intent() {
        let mut ext = DockPanelExternal::new("p1", 0.5);
        ext.pointer_move(0.5, 0.5);
        // Move only 0.2 — under threshold 0.5.
        ext.pointer_move(0.7, 0.5);
        let mut received: Vec<Intent> = Vec::new();
        ext.drain_intents(&mut |i| received.push(i));
        assert!(
            received.is_empty(),
            "drag below threshold must not enqueue tear_off",
        );
        assert!(!ext.tear_off_fired());
    }

    #[test]
    fn r683_dock_panel_external_l_inf_diagonal_drag_fires_on_y_axis_too() {
        // L∞ norm: max(|Δx|, |Δy|). Pure y drag past threshold
        // must fire (the canonical "drag panel down out of slot"
        // gesture).
        let mut ext = DockPanelExternal::new("p1", 0.4);
        ext.pointer_move(0.5, 0.0);
        // Cursor moves down by 0.5 (above threshold 0.4) but x
        // unchanged.
        ext.pointer_move(0.5, 0.5);
        assert!(ext.tear_off_fired(), "y-axis drag past threshold must fire");
    }

    #[test]
    fn r683_dock_panel_external_cursor_delta_l_inf_pre_calibration_is_none() {
        let ext = DockPanelExternal::new("p1", 0.5);
        // Before any pointer_move call, no snapshot exists.
        assert!(ext.cursor_delta_l_inf(0.5, 0.5).is_none());
    }

    #[test]
    fn r683_dock_panel_external_introspect_schema_includes_canonical_paths() {
        let ext = DockPanelExternal::new("p1", 0.5);
        let schema = ext.schema();
        let fields: Vec<&str> = schema.fields.iter().map(|(n, _)| *n).collect();
        for needed in [
            "panel_id",
            "tear_off_threshold_frac",
            "dragging",
            "tear_off_fired",
            "send",
        ] {
            assert!(fields.contains(&needed), "schema must include {needed}");
        }
    }

    #[test]
    fn r683_dock_panel_external_query_panel_id() {
        let ext = DockPanelExternal::new("my_panel", 0.5);
        let val = ext.query("panel_id").expect("queryable");
        assert_eq!(val.as_str(), Some("my_panel"));
    }

    #[test]
    fn r683_dock_panel_external_query_tear_off_fired_starts_false() {
        let ext = DockPanelExternal::new("p1", 0.5);
        let val = ext.query("tear_off_fired").expect("queryable");
        assert_eq!(val, IntrospectValue::Bool(false));
    }

    #[test]
    fn r683_dock_panel_external_invoke_unknown_event_returns_err() {
        let mut ext = DockPanelExternal::new("p1", 0.5);
        let res = ext.invoke("send", IntrospectValue::Text("UnknownEvent".to_string()));
        assert!(res.is_err());
    }

    #[test]
    fn r683_composite_tag_format_matches_input_router_convention() {
        // R51.42 §5.35 — the composite-tag convention is
        // `{primary}#{suffix}`. The dock panel's header + content
        // tags both follow this format so the InputRouter's
        // deepest-tagged hit-test + dispatch_send wire route
        // PointerDown to the matching External.
        assert_eq!(composite_tag("panel_a", HEADER_TAG_SUFFIX), "panel_a#header");
        assert_eq!(composite_tag("panel_a", CONTENT_TAG_SUFFIX), "panel_a#content");
    }

    #[test]
    fn r683_dock_panel_external_panel_id_accessor_returns_construction_value() {
        let ext = DockPanelExternal::new("inspector", 0.5);
        assert_eq!(ext.panel_id(), "inspector");
    }

    #[test]
    fn r683_dock_panel_external_threshold_accessor() {
        let ext = DockPanelExternal::new("p1", 0.42);
        assert!((ext.tear_off_threshold_frac() - 0.42).abs() < f32::EPSILON);
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
            let panel = view_dock_panel("Inspector", empty_content(), &theme_light(), &style);
            let mut cache = LayoutCache::new();
            let mut scene = panel;
            let panel_w: u32 = 400;
            let panel_h: u32 = 300;
            compute_layout(&mut scene, &mut cache, panel_w, panel_h);
            let Scene::Container(outer) = &scene else { panic!("outer Container") };
            // Outer panel fills the viewport (Block default).
            assert_eq!(outer.rect.w, panel_w, "panel root fills viewport width");
            let Scene::Container(header) = &outer.children[0] else { panic!("header") };
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
        let split = DockNode::split_horizontal(
            "my_split",
            0.42,
            DockNode::leaf("a"),
            DockNode::leaf("b"),
        );
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
        let split = DockNode::split_vertical(
            "v_split",
            0.5,
            DockNode::leaf("top"),
            DockNode::leaf("bot"),
        );
        let DockNode::Split { id, orientation, .. } = split else {
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
                ("middle_h".to_string(), SplitterOrientation::Horizontal, 0.20),
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
        let parsed: DockNode =
            serde_json::from_str(&serialized).expect("parse leaf");
        assert_eq!(parsed, leaf, "leaf round-trips through JSON identity");
    }

    #[test]
    fn r685_dock_node_split_serde_round_trip_through_json() {
        let split = DockNode::split_horizontal(
            "h_split",
            0.30,
            DockNode::leaf("a"),
            DockNode::leaf("b"),
        );
        let serialized = serde_json::to_string(&split).expect("serialize split");
        assert!(serialized.contains("\"type\":\"Split\""));
        assert!(serialized.contains("\"id\":\"h_split\""));
        assert!(serialized.contains("\"orientation\":\"Horizontal\""));
        assert!(serialized.contains("\"ratio\":"));
        let parsed: DockNode =
            serde_json::from_str(&serialized).expect("parse split");
        assert_eq!(parsed, split, "split round-trips through JSON identity");
    }

    #[test]
    fn r685_dock_topology_full_editor_serde_round_trip() {
        let topology = editor_topology();
        let serialized =
            serde_json::to_string(&topology).expect("serialize editor topology");
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
        assert_eq!(err, TopologyError::DuplicatePanelId("dup_panel".to_string()));
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
        let root = DockNode::split_horizontal(
            "split",
            f32::NAN,
            DockNode::leaf("a"),
            DockNode::leaf("b"),
        );
        let err = DockTopology::try_new(root).unwrap_err();
        let TopologyError::InvalidRatio { split_id, .. } = err else {
            panic!("expected InvalidRatio")
        };
        assert_eq!(split_id, "split");
    }

    #[test]
    fn r685_b_try_new_rejects_out_of_range_ratio() {
        let root = DockNode::split_horizontal(
            "split",
            1.5,
            DockNode::leaf("a"),
            DockNode::leaf("b"),
        );
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
        let root = DockNode::split_horizontal(
            "",
            0.5,
            DockNode::leaf("a"),
            DockNode::leaf("b"),
        );
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
        let zero = DockNode::split_horizontal(
            "z",
            0.0,
            DockNode::leaf("a"),
            DockNode::leaf("b"),
        );
        assert!(DockTopology::try_new(zero).is_ok());
        let one = DockNode::split_horizontal(
            "o",
            1.0,
            DockNode::leaf("a"),
            DockNode::leaf("b"),
        );
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
        let dup = DockNode::split_horizontal(
            "outer",
            0.5,
            DockNode::leaf("dup"),
            DockNode::leaf("dup"),
        );
        assert_eq!(
            DockTopology::try_new(dup).unwrap_err(),
            TopologyError::DuplicatePanelId("dup".to_string()),
        );

        // Cross-namespace → IdCollision (NOT DuplicatePanelId).
        let cross = DockNode::split_horizontal(
            "cross", 0.5,
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
        assert_eq!(swapped.split_ids(), vec!["outer", "inner_v", "middle_h", "inner_h"]);
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
        let err = editor_topology().swap_leaves("toolbar", "ghost").unwrap_err();
        assert_eq!(err, TopologyError::PanelNotFound("ghost".to_string()));
        // First-arg miss is reported on the first arg.
        let err_a = editor_topology().swap_leaves("ghost", "toolbar").unwrap_err();
        assert_eq!(err_a, TopologyError::PanelNotFound("ghost".to_string()));
    }

    #[test]
    fn r686_split_leaf_into_second_position() {
        // Split "outliner" into a vertical pair, the new "assets" panel
        // taking the bottom (Second) slot.
        let grown = editor_topology()
            .split_leaf_into("outliner", "assets", "outliner_v", Orient::Vertical, 0.5, DockSplitPosition::Second)
            .unwrap();
        assert_eq!(
            grown.panel_ids(),
            vec!["toolbar", "outliner", "assets", "viewport", "properties", "console"],
        );
        assert_eq!(grown.split_count(), 5);
        assert!(grown.split_ids().contains(&"outliner_v"));
    }

    #[test]
    fn r686_split_leaf_into_first_position() {
        // Same split but the new "assets" panel takes the top (First)
        // slot, pushing "outliner" after it in depth-first order.
        let grown = editor_topology()
            .split_leaf_into("outliner", "assets", "outliner_v", Orient::Vertical, 0.5, DockSplitPosition::First)
            .unwrap();
        assert_eq!(
            grown.panel_ids(),
            vec!["toolbar", "assets", "outliner", "viewport", "properties", "console"],
        );
    }

    #[test]
    fn r686_split_leaf_into_unknown_target_errors() {
        let err = editor_topology()
            .split_leaf_into("ghost", "assets", "s", Orient::Vertical, 0.5, DockSplitPosition::First)
            .unwrap_err();
        assert_eq!(err, TopologyError::PanelNotFound("ghost".to_string()));
    }

    #[test]
    fn r686_split_leaf_into_duplicate_panel_id_rejected() {
        // New leaf id collides with an existing panel → DuplicatePanelId
        // surfaced by the try_new gate (the live topology is unchanged).
        let err = editor_topology()
            .split_leaf_into("outliner", "viewport", "s", Orient::Vertical, 0.5, DockSplitPosition::First)
            .unwrap_err();
        assert_eq!(err, TopologyError::DuplicatePanelId("viewport".to_string()));
    }

    #[test]
    fn r686_split_leaf_into_duplicate_split_id_rejected() {
        let err = editor_topology()
            .split_leaf_into("outliner", "assets", "outer", Orient::Vertical, 0.5, DockSplitPosition::First)
            .unwrap_err();
        assert_eq!(err, TopologyError::DuplicateSplitId("outer".to_string()));
    }

    #[test]
    fn r686_split_leaf_into_id_collision_rejected() {
        // New *leaf* id "outer" collides with an existing *split* id.
        let err = editor_topology()
            .split_leaf_into("outliner", "outer", "fresh_split", Orient::Vertical, 0.5, DockSplitPosition::First)
            .unwrap_err();
        assert_eq!(err, TopologyError::IdCollision("outer".to_string()));
    }

    #[test]
    fn r686_split_leaf_into_invalid_ratio_rejected() {
        let err = editor_topology()
            .split_leaf_into("outliner", "assets", "s", Orient::Vertical, f32::NAN, DockSplitPosition::First)
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
        assert_eq!(pruned.panel_ids(), vec!["outliner", "viewport", "properties", "console"]);
        // "outer" is gone; the remaining splits keep their ids + ratios.
        assert_eq!(pruned.split_ids(), vec!["inner_v", "middle_h", "inner_h"]);
        assert!(matches!(pruned.root(), DockNode::Split { id, .. } if id.as_ref() == "inner_v"));
    }

    #[test]
    fn r686_remove_leaf_collapses_deep_split() {
        // Remove "properties" (inner_h.second) → inner_h collapses to
        // its surviving "viewport" leaf, which takes inner_h's slot.
        let pruned = editor_topology().remove_leaf("properties").unwrap();
        assert_eq!(pruned.panel_ids(), vec!["toolbar", "outliner", "viewport", "console"]);
        assert_eq!(pruned.split_ids(), vec!["outer", "inner_v", "middle_h"]);
    }

    #[test]
    fn r686_remove_leaf_unknown_panel_errors() {
        let err = editor_topology().remove_leaf("ghost").unwrap_err();
        assert_eq!(err, TopologyError::PanelNotFound("ghost".to_string()));
    }

    #[test]
    fn r686_remove_leaf_sole_panel_is_root_removal() {
        let err = DockTopology::single("only").remove_leaf("only").unwrap_err();
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
            .split_leaf_into("viewport", "console", "viewport_h", Orient::Horizontal, 0.5, DockSplitPosition::First)
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

    use super::{dock_drop_zone_for, DockDropZone, DOCK_EDGE_ZONE_FRAC};
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
        assert_eq!(dock_drop_zone_for(panel(), 300.0, 300.0), DockDropZone::Center);
    }

    #[test]
    fn r686_drop_zone_left_edge() {
        // 50 px in from the left → from_left = 0.125 < 0.25.
        assert_eq!(dock_drop_zone_for(panel(), 150.0, 300.0), DockDropZone::Left);
    }

    #[test]
    fn r686_drop_zone_right_edge() {
        assert_eq!(dock_drop_zone_for(panel(), 450.0, 300.0), DockDropZone::Right);
    }

    #[test]
    fn r686_drop_zone_top_edge() {
        assert_eq!(dock_drop_zone_for(panel(), 300.0, 150.0), DockDropZone::Top);
    }

    #[test]
    fn r686_drop_zone_bottom_edge() {
        assert_eq!(dock_drop_zone_for(panel(), 300.0, 450.0), DockDropZone::Bottom);
    }

    #[test]
    fn r686_drop_zone_corner_resolves_to_nearest_with_left_precedence() {
        // Top-left corner: from_left == from_top == 0.125 (exact tie).
        // Declaration-order precedence (Left → Right → Top → Bottom)
        // resolves the corner to Left.
        assert_eq!(dock_drop_zone_for(panel(), 150.0, 150.0), DockDropZone::Left);
        // Bottom-right corner: from_right == from_bottom tie → Right wins
        // over Bottom by precedence.
        assert_eq!(dock_drop_zone_for(panel(), 450.0, 450.0), DockDropZone::Right);
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
        assert_eq!(dock_drop_zone_for(panel(), 500.0, 300.0), DockDropZone::None);
        // y = 100 + 400 = 500 is the exclusive bottom edge → None.
        assert_eq!(dock_drop_zone_for(panel(), 300.0, 500.0), DockDropZone::None);
    }

    #[test]
    fn r686_drop_zone_degenerate_rect_is_none() {
        // Zero width / zero height carry no pixels → never a target.
        assert_eq!(dock_drop_zone_for(Rect::new(0, 0, 0, 100), 0.0, 50.0), DockDropZone::None);
        assert_eq!(dock_drop_zone_for(Rect::new(0, 0, 100, 0), 50.0, 0.0), DockDropZone::None);
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
        resolve_dock_drop, DockNode, DockReorganizeExternal, DockReorganizeIntent, DockSplitPosition,
        DockTopology,
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

    /// Side-by-side layout for the three panels (each 200×400).
    fn abc_rects() -> Vec<(&'static str, Rect)> {
        vec![
            ("a", Rect::new(0, 0, 200, 400)),
            ("b", Rect::new(200, 0, 200, 400)),
            ("c", Rect::new(400, 0, 200, 400)),
        ]
    }

    #[test]
    fn r686_resolve_center_drop_is_swap() {
        // Drop "a" onto b's centre (300, 200).
        let intent = resolve_dock_drop(&abc_rects(), "a", 300.0, 200.0).unwrap();
        assert_eq!(
            intent,
            DockReorganizeIntent::Swap { source: "a".into(), target: "b".into() },
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
        let DockReorganizeIntent::SplitInsert { orientation, position, .. } = intent else {
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
        let intent = DockReorganizeIntent::Swap { source: "a".into(), target: "c".into() };
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
    fn r686_external_invoke_center_swaps_topology_signal() {
        let signal = Rc::new(Signal::new(abc_topology()));
        let mut ext = DockReorganizeExternal::new(Rc::clone(&signal));
        let payload = IntrospectValue::Json(serde_json::json!({
            "source": "a", "target": "b", "zone": "Center",
        }));
        let result = ext.invoke("reorganize", payload).unwrap();
        assert!(matches!(result, IntrospectValue::Text(_)));
        // The shared signal now holds the swapped topology.
        assert_eq!(signal.get().panel_ids(), vec!["b", "a", "c"]);
    }

    #[test]
    fn r749_with_undo_makes_reorganize_reversible() {
        use pinion_core::undo::UndoStack;
        let signal = Rc::new(Signal::new(abc_topology()));
        let stack = Rc::new(UndoStack::new());
        let mut ext = DockReorganizeExternal::new(Rc::clone(&signal)).with_undo(Rc::clone(&stack));
        assert_eq!(signal.get().panel_ids(), vec!["a", "b", "c"], "boot layout");
        // Swap a <-> b: recorded as one reversible topology edit.
        ext.invoke(
            "reorganize",
            IntrospectValue::Json(serde_json::json!({"source":"a","target":"b","zone":"Center"})),
        )
        .unwrap();
        assert_eq!(signal.get().panel_ids(), vec!["b", "a", "c"], "reorganize applied");
        assert_eq!(stack.len(), 1, "one recorded edit");
        // Undo restores the prior layout; redo re-applies it.
        assert!(stack.undo());
        assert_eq!(signal.get().panel_ids(), vec!["a", "b", "c"], "undo restored the layout");
        assert!(stack.redo());
        assert_eq!(signal.get().panel_ids(), vec!["b", "a", "c"], "redo re-applied");
    }

    #[test]
    fn r686_external_invoke_edge_split_inserts_and_bumps_seq() {
        let signal = Rc::new(Signal::new(abc_topology()));
        let mut ext = DockReorganizeExternal::new(Rc::clone(&signal));
        assert_eq!(ext.split_seq(), 0);
        let payload = IntrospectValue::Json(serde_json::json!({
            "source": "a", "target": "c", "zone": "Right",
        }));
        ext.invoke("reorganize", payload).unwrap();
        // A split was minted → seq bumped; topology grew a reorg split.
        assert_eq!(ext.split_seq(), 1);
        assert!(signal.get().split_ids().iter().any(|id| id.starts_with("reorg-split-")));
        assert_eq!(signal.get().leaf_count(), 3);
    }

    #[test]
    fn r686_external_invoke_swap_does_not_bump_seq() {
        let signal = Rc::new(Signal::new(abc_topology()));
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
        let signal = Rc::new(Signal::new(abc_topology()));
        let mut ext = DockReorganizeExternal::new(signal);
        let err = ext.invoke("reorganize", IntrospectValue::Text("a:b:Center".into())).unwrap_err();
        assert_eq!(err, InvokeError::TypeMismatch);
    }

    #[test]
    fn r686_external_invoke_unknown_zone_is_rejected() {
        let signal = Rc::new(Signal::new(abc_topology()));
        let mut ext = DockReorganizeExternal::new(signal);
        let err = ext
            .invoke(
                "reorganize",
                IntrospectValue::Json(serde_json::json!({"source":"a","target":"b","zone":"Diagonal"})),
            )
            .unwrap_err();
        assert_eq!(err, InvokeError::Rejected);
    }

    #[test]
    fn r686_external_invoke_stale_panel_rejected_topology_unchanged() {
        let signal = Rc::new(Signal::new(abc_topology()));
        let mut ext = DockReorganizeExternal::new(Rc::clone(&signal));
        let before = signal.get();
        let err = ext
            .invoke(
                "reorganize",
                IntrospectValue::Json(serde_json::json!({"source":"ghost","target":"b","zone":"Center"})),
            )
            .unwrap_err();
        assert_eq!(err, InvokeError::Rejected);
        // Live topology untouched on a rejected gesture.
        assert_eq!(signal.get(), before);
    }

    #[test]
    fn r686_external_unknown_action_is_unknown_path() {
        let signal = Rc::new(Signal::new(abc_topology()));
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
        let signal = Rc::new(Signal::new(abc_topology()));
        let mut ext = DockReorganizeExternal::new(Rc::clone(&signal));
        let payload = IntrospectValue::Json(serde_json::json!({
            "source": "a",
            "cursor": {"x": 300.0, "y": 200.0},
            "panels": abc_panels_json(),
        }));
        let result = ext.invoke("drop", payload).unwrap();
        assert!(matches!(result, IntrospectValue::Text(_)));
        assert_eq!(signal.get().panel_ids(), vec!["b", "a", "c"]);
        // Centre = swap → no split minted.
        assert_eq!(ext.split_seq(), 0);
    }

    #[test]
    fn r687_external_drop_edge_resolves_split_insert_in_substrate() {
        let signal = Rc::new(Signal::new(abc_topology()));
        let mut ext = DockReorganizeExternal::new(Rc::clone(&signal));
        // Cursor near b's left edge (b spans x=200..400).
        let payload = IntrospectValue::Json(serde_json::json!({
            "source": "a",
            "cursor": {"x": 210.0, "y": 200.0},
            "panels": abc_panels_json(),
        }));
        ext.invoke("drop", payload).unwrap();
        assert_eq!(ext.split_seq(), 1, "edge drop mints a reorg split");
        assert_eq!(signal.get().leaf_count(), 3, "a relocated, not duplicated");
        assert!(signal
            .get()
            .split_ids()
            .iter()
            .any(|id| id.starts_with("reorg-split-")));
    }

    #[test]
    fn r687_external_drop_over_source_is_noop() {
        // Cursor over "a" itself (the source) → no valid target → cancel.
        let signal = Rc::new(Signal::new(abc_topology()));
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
        let signal = Rc::new(Signal::new(abc_topology()));
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
        let signal = Rc::new(Signal::new(abc_topology()));
        let ext = DockReorganizeExternal::new(signal);
        let IntrospectValue::Json(value) = ext.query("topology").unwrap() else {
            panic!("topology query must return JSON");
        };
        // The serialized tree carries the root node's "type" tag.
        assert!(value.get("root").is_some(), "topology JSON exposes the root node");
    }

    #[test]
    fn r686_external_intervene_slots_are_read_only() {
        let signal = Rc::new(Signal::new(abc_topology()));
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
}

#[cfg(test)]
mod placeholder_tests {
    //! R685 §5.16 §5.49 — `view_floating_placeholder` substrate
    //! lift tests. The helper was inlined in `hello-dock-panels`
    //! (R683.C 1st consumer); R685 lifts it to substrate on the
    //! 2nd-consumer signal (`hello-dock-panels-editor` round entry)
    //! per [[abstraction-needs-second-consumer]].

    use super::{
        view_floating_placeholder, FloatingPlaceholderStyle, PLACEHOLDER_TAG_SUFFIX,
    };
    use pinion_core::scene::Scene;
    use pinion_core::theme::Theme;

    #[test]
    fn r685_view_floating_placeholder_tags_with_panel_id_suffix() {
        let theme = Theme::light();
        let scene =
            view_floating_placeholder("inspector", &theme, &FloatingPlaceholderStyle::m3_default());
        let Scene::Container(outer) = &scene else { panic!() };
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
        let Scene::Container(outer) = &scene else { panic!() };
        assert_eq!(outer.children.len(), 1, "single Text child");
        let Scene::Text(text) = &outer.children[0] else { panic!("expected Text") };
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
        use super::{floating_window_id, DEFAULT_FLOATING_WINDOW_PREFIX};
        assert_eq!(
            floating_window_id(DEFAULT_FLOATING_WINDOW_PREFIX, "inspector"),
            "torn-inspector",
        );
        // Custom prefix preserves the concat form.
        assert_eq!(floating_window_id("floating-", "panel-1"), "floating-panel-1");
    }
}

#[cfg(test)]
mod surface_tests {
    //! R685.B §5.16 §5.49 — `view_dock_surface` recursive walker
    //! tests under the R685.B SSOT signature (topology owns all
    //! panel ids / split ids / orientations / initial ratios;
    //! callbacks supply only panel content + reactive split state).

    use super::{view_dock_surface, DockNode, DockSplitState, DockTopology};
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

    fn theme_light() -> Theme { Theme::light() }
    fn run_in_owner<R>(f: impl FnOnce() -> R) -> R { Owner::new().run(f) }

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
                &theme_light(),
            );
            let Scene::Container(outer) = &scene else { panic!() };
            assert_eq!(outer.tag.as_deref(), Some("viewport"));
            assert_eq!(outer.children.len(), 2);
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
                &theme_light(),
            );
            let Scene::Container(outer) = &scene else { panic!() };
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
                &theme_light(),
            );
            let Scene::Container(outer) = &scene else { panic!() };
            assert_eq!(outer.layout.flex_direction, FlexDirection::Column);
            assert_eq!(outer.children.len(), 3);
        });
    }

    #[test]
    fn r685_dock_surface_3_leaf_nested_dispatches_by_declared_id() {
        run_in_owner(|| {
            let topology = DockTopology::new(DockNode::split_horizontal(
                "outer", 0.5,
                DockNode::split_vertical(
                    "inner", 0.3,
                    DockNode::leaf("a"),
                    DockNode::leaf("b"),
                ),
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
                &theme_light(),
            );
            assert_eq!(
                *calls.borrow(),
                vec![
                    ("outer".to_string(), 0.5),
                    ("inner".to_string(), 0.3),
                ],
                "DF pre-order with declared ids + topology-sourced ratios",
            );
        });
    }

    #[test]
    fn r685_dock_surface_4_leaf_2x2_grid_by_id() {
        run_in_owner(|| {
            let topology = DockTopology::new(DockNode::split_horizontal(
                "outer", 0.5,
                DockNode::split_vertical("left_col", 0.5,
                    DockNode::leaf("tl"), DockNode::leaf("bl")),
                DockNode::split_vertical("right_col", 0.5,
                    DockNode::leaf("tr"), DockNode::leaf("br")),
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
                &theme_light(),
            );
            assert_eq!(
                *calls.borrow(),
                vec!["outer".to_string(), "left_col".to_string(), "right_col".to_string()],
            );
        });
    }

    #[test]
    fn r685_dock_surface_5_leaf_editor_dispatch_by_id() {
        run_in_owner(|| {
            let topology = DockTopology::new(DockNode::split_vertical(
                "outer", 0.10, DockNode::leaf("toolbar"),
                DockNode::split_vertical("inner_v", 0.80,
                    DockNode::split_horizontal("middle_h", 0.20,
                        DockNode::leaf("outliner"),
                        DockNode::split_horizontal("inner_h", 0.75,
                            DockNode::leaf("viewport"),
                            DockNode::leaf("properties"))),
                    DockNode::leaf("console")),
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
                    sc.borrow_mut()
                        .push((split_id.to_string(), initial_ratio));
                    split_state_for(initial_ratio)
                },
                &theme_light(),
            );
            assert_eq!(
                *split_calls.borrow(),
                vec![
                    ("outer".to_string(),    0.10),
                    ("inner_v".to_string(),  0.80),
                    ("middle_h".to_string(), 0.20),
                    ("inner_h".to_string(),  0.75),
                ],
            );
            assert_eq!(
                *panel_calls.borrow(),
                vec!["toolbar".to_string(),"outliner".to_string(),"viewport".to_string(),
                     "properties".to_string(),"console".to_string()],
            );
        });
    }

    #[test]
    fn r685_dock_surface_split_state_invoked_once_per_split() {
        run_in_owner(|| {
            let topology = DockTopology::new(DockNode::split_horizontal(
                "outer", 0.5,
                DockNode::split_vertical("inner", 0.5,
                    DockNode::leaf("a"), DockNode::leaf("b")),
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
                &theme_light(),
            );
            assert_eq!(*count.borrow(), topology.split_count());
        });
    }
}

