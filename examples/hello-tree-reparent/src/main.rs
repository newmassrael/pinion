// R935 §5.16 — example bindings tolerate looser doc-markdown lints than
// substrate crates; the architectural narrative carries many proper-noun
// identifiers (TreeNode, DropPoint, WAI-ARIA, …).
#![allow(clippy::doc_markdown)]

//! `hello-tree-reparent` — R935 §5.51 — **scene-outliner drag-to-reparent**.
//!
//! ## Why this binding exists
//!
//! `hello-dnd` (R742) and `hello-tab-reorder` (R743) reorder a **flat**
//! collection. A scene outliner / layer tree — the self-hosted editor's core
//! navigation surface — needs the *hierarchical* move: drag a node **onto**
//! another to make it that node's child (reparent), or **between** two siblings
//! to reorder. This is the third consumer of the R742 drag-session
//! [`External`] hooks (`begin_drag` /
//! `drag_to` / `drag_release`) and the second consumer of the
//! [`tree_nav`](pinion_core::widgets::tree_nav) structural-mutation ops
//! ([`remove_subtree`] / [`insert_subtree`]) — lifted from `hello-tree-grid`
//! this round because a wrong-depth `Vec::remove`/`insert` silently corrupts a
//! tree, so the two consumers must share one correct copy.
//!
//! ## Three-zone drop classification
//!
//! When the cursor is over a row, the row's height is split into thirds along
//! `DropPoint::y_rel`:
//!
//! - top third (`< 0.25`) → **Before** the target (reorder as the preceding
//!   sibling),
//! - middle (`0.25..0.75`) → **Onto** the target (reparent: become its last
//!   child; the target auto-expands so the moved node is visible),
//! - bottom third (`>= 0.75`) → **After** the target (reorder as the following
//!   sibling).
//!
//! A Before/After drop draws a horizontal insertion line at the gap; an Onto
//! drop rings the target row.
//!
//! ## Cycle prevention
//!
//! A node may not be dropped into its own subtree (onto itself or any
//! descendant) — that would detach the subtree from the tree. The commit
//! rejects such a drop (`is_descendant` guard) and leaves the tree unchanged,
//! the canonical outliner behaviour.
//!
//! ## State & reactivity
//!
//! The retained tree, the keyboard cursor, the armed drag source, and the live
//! drop preview are reactive [`Signal`]s in
//! [`Owner::cache`](pinion_core::reactive::Owner::cache) (`use_outliner`), so
//! the view, the a11y tree, and the drag-hook `External` all share one
//! instance — the `hello-tree-grid` pattern. `WidgetCore::State` is `()`; the
//! view reads the cache directly.
//!
//! ## AI clients (§2 #7)
//!
//! `query("/external/row_count")`, `id_at.<n>` / `depth_at.<n>` (the visible
//! flattening), `parent_of.<id>` (verify a reparent), and `preview` (the
//! in-flight drop). The drag is driven by the existing `scene/drag` RPC — no
//! new method. See `tools/demos/r935_tree_reparent.py`.

use std::borrow::Cow;

use pinion_a11y::{
    AccessAction, AccessFocus, AccessNode, WidgetA11y, tree_access_nodes, tree_row_tag,
};
use pinion_core::composite_tag::{parse_send_payload, split_subindex};
use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, DragPayload, DropPoint, External, ExternalIntrospect,
    InterveneError, IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner, ThreadOwnership,
};
use pinion_core::input::PointerWireEvent;
use pinion_core::reactive::{Owner, Signal};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, Border, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{ColorRole, use_theme};
use pinion_core::widgets::tree_nav::{
    MutableTreeNode, TreeNode, VisibleRow, apply_tree_key, find_node, find_node_mut, flat_visible,
    insert_subtree, remove_subtree,
};
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::{WidgetView, vello_renderer_impl};
use serde::{Deserialize, Serialize};
use std::rc::Rc;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloTreeReparentRenderer, HelloTreeReparentRendererError);

const WIN_W: u32 = 340;
const WIN_H: u32 = 460;
const THEME_TAG: &str = "app";
/// Root `External` + paint-root tag; rows hang off it as `outliner#<id>`.
const TAG: &str = "outliner";
/// Fixed visible-list height container so the insertion line is an absolute
/// overlay that never reflows the rows (the `hello-dnd` precedent).
const LIST_H: u32 = 11 * ROW_H;
/// Uniform row height (logical px).
const ROW_H: u32 = 30;
/// Per-depth indentation (logical px).
const INDENT: u32 = 20;
/// Insertion-line thickness for a Before/After drop.
const LINE_H: u32 = 3;
/// Keyboard page size (PageUp/Down rows).
const NAV_PAGE: usize = 8;
/// Owner::cache key for the shared outliner state.
const STATE_KEY: &str = "tree_reparent::state";

/// The drag-payload kind the dragged node travels under.
const DRAG_KIND: &str = "tree-node";

/// Where a drop lands relative to the target row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum DropZone {
    /// Insert as the preceding sibling of the target.
    Before,
    /// Reparent: insert as the target's last child.
    Onto,
    /// Insert as the following sibling of the target.
    After,
}

impl DropZone {
    /// The wire token reported through `query("preview")`.
    fn as_str(self) -> &'static str {
        match self {
            DropZone::Before => "before",
            DropZone::Onto => "onto",
            DropZone::After => "after",
        }
    }

    /// Classify a `DropPoint::y_rel` into a zone (top/middle/bottom thirds).
    fn from_y(y_rel: f32) -> Self {
        if y_rel < 0.25 {
            DropZone::Before
        } else if y_rel >= 0.75 {
            DropZone::After
        } else {
            DropZone::Onto
        }
    }
}

/// The in-flight drop preview the view reads to dim the source + draw the
/// insertion line / reparent ring.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct DropPreview {
    /// Stable id of the node being dragged.
    drag_id: String,
    /// Stable id of the row the cursor is over.
    target: String,
    /// Where the drop would land relative to `target`.
    zone: DropZone,
}

// ─── tree model ────────────────────────────────────────────────────────────

/// One outliner node — a scene folder or object. Carries its own `expanded`
/// flag (retained flag-on-node storage); `serde` + `PartialEq` satisfy the
/// §5.22 `Signal<T>` introspect bound.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct OutlinerNode {
    id: String,
    label: String,
    expanded: bool,
    children: Vec<OutlinerNode>,
}

impl OutlinerNode {
    fn leaf(id: &str, label: &str) -> Self {
        Self {
            id: id.to_owned(),
            label: label.to_owned(),
            expanded: false,
            children: Vec::new(),
        }
    }
    fn branch(id: &str, label: &str, expanded: bool, children: Vec<OutlinerNode>) -> Self {
        Self {
            id: id.to_owned(),
            label: label.to_owned(),
            expanded,
            children,
        }
    }
}

impl TreeNode for OutlinerNode {
    fn id(&self) -> &str {
        &self.id
    }
    fn label(&self) -> &str {
        &self.label
    }
    fn expanded(&self) -> bool {
        self.expanded
    }
    fn children(&self) -> &[Self] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut [Self] {
        &mut self.children
    }
    fn set_expanded(&mut self, expanded: bool) {
        self.expanded = expanded;
    }
}

impl MutableTreeNode for OutlinerNode {
    fn children_vec_mut(&mut self) -> &mut Vec<Self> {
        &mut self.children
    }
}

/// The boot scene graph: a small two-to-three-level outliner.
fn initial_nodes() -> Vec<OutlinerNode> {
    vec![
        OutlinerNode::branch(
            "world",
            "World",
            true,
            vec![
                OutlinerNode::leaf("terrain", "Terrain"),
                OutlinerNode::branch(
                    "player",
                    "Player",
                    true,
                    vec![
                        OutlinerNode::leaf("camera", "Camera"),
                        OutlinerNode::leaf("light", "Key Light"),
                    ],
                ),
                OutlinerNode::branch(
                    "props",
                    "Props",
                    false,
                    vec![OutlinerNode::leaf("crate", "Crate")],
                ),
            ],
        ),
        OutlinerNode::branch("ui", "UI", true, vec![OutlinerNode::leaf("hud", "HUD")]),
    ]
}

// ─── example-local tree queries (1-consumer; not lifted) ────────────────────

/// Whether `candidate` lies strictly inside `ancestor`'s subtree (a proper
/// descendant) — the cycle guard a reparent consults.
fn is_descendant(nodes: &[OutlinerNode], ancestor: &str, candidate: &str) -> bool {
    fn contains(node: &OutlinerNode, id: &str) -> bool {
        node.children.iter().any(|c| c.id == id || contains(c, id))
    }
    find_node(nodes, ancestor).is_some_and(|a| contains(a, candidate))
}

/// Locate `id`'s parent id (`None` = root) and its index among siblings.
fn locate(nodes: &[OutlinerNode], id: &str) -> Option<(Option<String>, usize)> {
    fn rec(
        nodes: &[OutlinerNode],
        id: &str,
        parent: Option<&str>,
    ) -> Option<(Option<String>, usize)> {
        for (i, n) in nodes.iter().enumerate() {
            if n.id == id {
                return Some((parent.map(str::to_owned), i));
            }
            if let Some(found) = rec(&n.children, id, Some(&n.id)) {
                return Some(found);
            }
        }
        None
    }
    rec(nodes, id, None)
}

/// The parent id of `id` (`None` = root or absent).
fn parent_of(nodes: &[OutlinerNode], id: &str) -> Option<String> {
    locate(nodes, id).and_then(|(p, _)| p)
}

/// Move the subtree `drag_id` to `target`+`zone`, returning whether the tree
/// changed. Rejects (no-op) a drop into the dragged node's own subtree (cycle)
/// or a drop onto its current position. The reparent composition is
/// example-local (one consumer); it stands on the lifted
/// [`remove_subtree`] / [`insert_subtree`] primitives.
fn reparent(nodes: &mut Vec<OutlinerNode>, drag_id: &str, target: &str, zone: DropZone) -> bool {
    // The target must exist OUTSIDE the dragged subtree: a drop onto self, a
    // descendant, or a sibling *inside* the moved subtree would orphan it.
    if drag_id == target || is_descendant(nodes, drag_id, target) {
        return false;
    }
    let Some(node) = remove_subtree(nodes, drag_id) else {
        return false;
    };
    // Recompute the insertion against the POST-removal tree (a sibling index
    // shifts when the dragged node was an earlier sibling under the same parent).
    let (parent, index) = match zone {
        DropZone::Onto => {
            let count = find_node(nodes, target).map_or(0, |n| n.children.len());
            (Some(target.to_owned()), count)
        }
        DropZone::Before | DropZone::After => {
            let (tp, ti) = locate(nodes, target).unwrap_or((None, 0));
            let idx = if matches!(zone, DropZone::After) {
                ti + 1
            } else {
                ti
            };
            (tp, idx)
        }
    };
    insert_subtree(nodes, parent.as_deref(), index, node);
    // Auto-expand the new parent so the moved node is visible.
    if let Some(pid) = &parent {
        if let Some(p) = find_node_mut(nodes, pid) {
            p.set_expanded(true);
        }
    }
    true
}

// ─── shared reactive state ──────────────────────────────────────────────────

/// The outliner's reactive state, shared by the view, the a11y tree, and the
/// drag-hook `External` through [`Owner::cache`].
struct Outliner {
    nodes: Signal<Vec<OutlinerNode>>,
    /// Armed drag source (set on PointerDown, read by `begin_drag`).
    pressed: Signal<Option<String>>,
    /// In-flight drop preview.
    preview: Signal<Option<DropPreview>>,
    /// Keyboard cursor / AT active descendant (node id).
    focused: Signal<Option<String>>,
}

fn use_outliner() -> Rc<Outliner> {
    Owner::current()
        .expect("use_outliner requires an active Owner scope")
        .cache(STATE_KEY, || Outliner {
            nodes: Signal::new(initial_nodes()),
            pressed: Signal::new(None),
            preview: Signal::new(None),
            focused: Signal::new(None),
        })
}

// Row composite tags use the shared `pinion_a11y::tree_row_tag(TAG, id)` SSOT
// (`"outliner#<id>"`) so the painted row tag, the a11y treeitem tag, and the
// `DropPoint` the router reports back all derive from one scheme.

// ─── drag-hook External ─────────────────────────────────────────────────────

/// R935 §5.51 — the outliner coordinator. A plain `External` (no statechart;
/// the drag session lives in the router) holding the shared [`Outliner`]
/// signals. The InputRouter dispatches the press as a `send`/`PointerDown`
/// (arming `pressed`), then calls `begin_drag` / `drag_to` / `drag_release`.
struct OutlinerExternal {
    state: Rc<Outliner>,
}

impl OutlinerExternal {
    fn new() -> Self {
        Self {
            state: use_outliner(),
        }
    }

    /// Classify the cursor's drop into `(target id, zone)`; `None` over no row.
    fn classify(over: Option<&DropPoint>) -> Option<(String, DropZone)> {
        let p = over?;
        let target = split_subindex(&p.tag).1?;
        Some((target.to_owned(), DropZone::from_y(p.y_rel)))
    }
}

impl core::fmt::Debug for OutlinerExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("OutlinerExternal")
            .field("pressed", &self.state.pressed.get())
            .field("preview", &self.state.preview.get())
            .finish()
    }
}

impl External for OutlinerExternal {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui], BackendFallback::Skip)
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

    /// Arm a drag from the pressed node, carrying its stable id as the payload.
    fn begin_drag(&self) -> Option<DragPayload> {
        let id = self.state.pressed.get()?;
        Some(DragPayload {
            kind: Cow::Borrowed(DRAG_KIND),
            value: IntrospectValue::Text(id),
        })
    }

    /// Live update: classify the drop and store the preview the view reads.
    /// Over no row, hold the last preview rather than snapping (the `hello-dnd`
    /// hold-last behaviour) so the indicator does not flicker over gaps.
    fn drag_to(&mut self, _payload: &DragPayload, over: Option<DropPoint>) {
        let Some(drag_id) = self.state.pressed.get() else {
            return;
        };
        // Over a row, refresh the preview; over no row, hold the last one (no
        // snapping over inter-row gaps — the `hello-dnd` hold-last behaviour).
        if let Some((target, zone)) = Self::classify(over.as_ref()) {
            self.state.preview.set(Some(DropPreview {
                drag_id,
                target,
                zone,
            }));
        }
    }

    /// Commit: reparent the dragged subtree to the released drop (honouring the
    /// held preview when the release is over no row), then clear the drag state.
    fn drag_release(&mut self, _payload: &DragPayload, over: Option<DropPoint>) {
        if let Some(drag_id) = self.state.pressed.get() {
            let drop = Self::classify(over.as_ref())
                .or_else(|| self.state.preview.get().map(|p| (p.target, p.zone)));
            if let Some((target, zone)) = drop {
                let mut tree = self.state.nodes.get();
                if reparent(&mut tree, &drag_id, &target, zone) {
                    self.state.nodes.set(tree);
                }
            }
        }
        self.state.preview.set(None);
        self.state.pressed.set(None);
    }
}

impl ExternalIntrospect for OutlinerExternal {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(&[
            ("row_count", "int"),
            ("id_at", "string"),
            ("depth_at", "string"),
            ("parent_of", "string"),
            ("preview", "json"),
            ("focused", "string"),
            ("send", "string"),
        ])
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        let tree = self.state.nodes.get();
        if path == "row_count" {
            let n = flat_visible(&tree).len();
            return Some(IntrospectValue::Int(i64::try_from(n).unwrap_or(i64::MAX)));
        }
        if let Some(n) = path.strip_prefix("id_at.") {
            let rows = flat_visible(&tree);
            let idx: usize = n.parse().ok()?;
            return Some(match rows.get(idx) {
                Some(r) => IntrospectValue::Text(r.id.clone()),
                None => IntrospectValue::Null,
            });
        }
        if let Some(n) = path.strip_prefix("depth_at.") {
            let rows = flat_visible(&tree);
            let idx: usize = n.parse().ok()?;
            return Some(match rows.get(idx) {
                Some(r) => IntrospectValue::Int(i64::from(r.depth)),
                None => IntrospectValue::Null,
            });
        }
        if let Some(id) = path.strip_prefix("parent_of.") {
            // Absent node → Null; root node → empty string (present, no parent).
            return Some(match find_node(&tree, id) {
                Some(_) => IntrospectValue::Text(parent_of(&tree, id).unwrap_or_default()),
                None => IntrospectValue::Null,
            });
        }
        match path {
            "preview" => Some(match self.state.preview.get() {
                Some(p) => IntrospectValue::Json(serde_json::json!({
                    "drag_id": p.drag_id,
                    "target": p.target,
                    "zone": p.zone.as_str(),
                })),
                None => IntrospectValue::Null,
            }),
            "focused" => Some(match self.state.focused.get() {
                Some(id) => IntrospectValue::Text(id),
                None => IntrospectValue::Null,
            }),
            _ => None,
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            "focused" => {
                let IntrospectValue::Text(id) = value else {
                    return Err(InterveneError::TypeMismatch);
                };
                if find_node(&self.state.nodes.get(), &id).is_none() {
                    return Err(InterveneError::OutOfRange);
                }
                self.state.focused.set(Some(id));
                Ok(())
            }
            _ => Err(InterveneError::UnknownPath),
        }
    }

    fn invoke(
        &mut self,
        method: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match method {
            // The router dispatches the composite press as `send` with the
            // "{node_id}:{Event}" wire form; a PointerDown arms `begin_drag`.
            "send" => {
                let IntrospectValue::Text(payload) = args else {
                    return Err(InvokeError::TypeMismatch);
                };
                let (id, event, _): (String, &str, _) =
                    parse_send_payload(&payload).ok_or(InvokeError::Rejected)?;
                if event == PointerWireEvent::Down.as_wire_name()
                    && find_node(&self.state.nodes.get(), &id).is_some()
                {
                    self.state.pressed.set(Some(id));
                }
                Ok(IntrospectValue::Null)
            }
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

// ─── view ───────────────────────────────────────────────────────────────────

/// One outliner row: an indented, tagged container with a branch chevron +
/// label. The dragged row dims; the Onto target rings; the focused row rings.
fn row_node(
    row: &VisibleRow,
    preview: Option<&DropPreview>,
    focused: Option<&str>,
    theme: &pinion_core::theme::Theme,
) -> Scene {
    let dragged = preview.is_some_and(|p| p.drag_id == row.id);
    let onto = preview.is_some_and(|p| p.zone == DropZone::Onto && p.target == row.id);
    let is_focused = focused == Some(row.id.as_str());

    let glyph = if row.has_children {
        if row.expanded {
            "\u{25BE} "
        } else {
            "\u{25B8} "
        }
    } else {
        "\u{2022} "
    };
    let fg = if dragged {
        theme.resolve(ColorRole::OnSurfaceMuted)
    } else {
        theme.resolve(ColorRole::OnSurface)
    };
    let label = Scene::Text(TextNode::styled(
        format!("{glyph}{}", row.label),
        Rect::default(),
        TextStyle::new().with_size_px(14).with_fg(fg),
    ));

    let indent = INDENT * row.depth;
    let mut style = BoxStyle::filled(theme.resolve(ColorRole::Surface));
    if onto {
        // Reparent target: a filled accent ring drawn inside the row rect.
        style = style
            .with_border(Border::new(theme.resolve(ColorRole::Accent), 2))
            .with_corner_radius(4);
    } else if is_focused {
        style = style.with_border(Border::new(theme.resolve(ColorRole::OnSurface), 1));
    }
    Scene::Container(
        ContainerNode::new(vec![label])
            .with_tag(tree_row_tag(TAG, &row.id))
            .with_style(style)
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(WIN_W - 24, ROW_H))
                    .with_padding(Rect::new(8 + indent, 0, 8, 0)),
            ),
    )
}

/// The horizontal insertion line for a Before/After drop, absolutely
/// positioned at the gap above visible row `visual` so it never reflows rows.
fn insertion_line(visual: usize, theme: &pinion_core::theme::Theme) -> Scene {
    let top = u32::try_from(visual).unwrap_or(0) * ROW_H;
    let top = top
        .saturating_sub(LINE_H / 2)
        .min(LIST_H.saturating_sub(LINE_H));
    Scene::Container(
        ContainerNode::new(vec![])
            .with_tag("outliner_insert")
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Accent)).with_corner_radius(2))
            .with_layout(
                LayoutStyle::new()
                    .with_absolute_position(0, top)
                    .with_size(Size::px(WIN_W - 24, LINE_H)),
            ),
    )
}

/// view-fn (§6.3): pure sync `() -> Scene`. Reads the shared outliner cache and
/// renders the visible flattening with depth indentation, the drag preview
/// (dimmed source + insertion line / reparent ring), and the keyboard cursor.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(_state: (), _frame: &Frame) -> Scene {
    let st = use_outliner();
    let theme = use_theme(THEME_TAG).theme_animated();
    let nodes = st.nodes.get();
    let rows = flat_visible(&nodes);
    let preview = st.preview.get();
    let focused = st.focused.get();

    let mut kids: Vec<Scene> = Vec::with_capacity(rows.len() + 1);
    for row in &rows {
        kids.push(row_node(row, preview.as_ref(), focused.as_deref(), &theme));
    }
    // Before/After insertion line overlay (Onto is shown as a row ring).
    if let Some(p) = &preview {
        if p.zone != DropZone::Onto {
            if let Some(vis) = rows.iter().position(|r| r.id == p.target) {
                let gap = if p.zone == DropZone::After {
                    vis + 1
                } else {
                    vis
                };
                kids.push(insertion_line(gap, &theme));
            }
        }
    }

    let list = Scene::Container(
        ContainerNode::new(kids).with_tag(TAG).with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Column)
                .with_align_items(AlignItems::Stretch)
                .with_size(Size::px(WIN_W - 24, LIST_H))
                .with_focusable(true),
        ),
    );

    let title = Scene::Text(TextNode::styled(
        "Scene outliner — drag to reparent",
        Rect::default(),
        TextStyle::new()
            .with_size_px(15)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ));
    let footer = Scene::Text(TextNode::styled(
        "drag onto a row = child   between rows = reorder",
        Rect::default(),
        TextStyle::new()
            .with_size_px(12)
            .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
    ));

    Scene::Container(
        ContainerNode::new(vec![title, list, footer])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Start)
                    .with_align_items(AlignItems::Start)
                    .with_size(Size::px(WIN_W, WIN_H))
                    .with_gap(10)
                    .with_padding(Rect::new(12, 12, 12, 12)),
            ),
    )
}

struct TreeReparentView;

impl WidgetCore for TreeReparentView {
    type State = ();
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(OutlinerExternal::new())
    }

    fn tag() -> &'static str {
        TAG
    }

    fn read_state(_scene: &Scene) {}

    fn view(state: (), frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-tree-reparent (R935 §5.51)"
    }

    fn keybinding(_key: &str) -> Option<()> {
        None
    }

    /// Keyboard navigation: the WAI-ARIA tree cursor (Arrow Up/Down move,
    /// Left/Right collapse/expand/descend, Home/End, type-ahead) via the lifted
    /// [`apply_tree_key`]. Keyboard *reparent* (cut/paste move) is a deferred
    /// axis — the pointer / `scene/drag` path is the v1 reparent.
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        if focused != Some(TAG) {
            return false;
        }
        let _ = scene;
        let st = use_outliner();
        apply_tree_key(&st.nodes, &st.focused, key, NAV_PAGE)
    }

    fn fmt_state_log(_state: &()) -> String {
        "outliner (tree state in Owner::cache)".to_string()
    }
}

impl WidgetA11y for TreeReparentView {
    /// WAI-ARIA tree via the shared
    /// [`tree_access_nodes`] builder (the same
    /// SSOT `hello-tree-view` feeds): a `tree` root over one `treeitem` per
    /// visible row, each with `aria-level` / `aria-posinset` / `aria-setsize`
    /// and `aria-expanded` on branches. No selection model (`selected_id =
    /// None`); the keyboard cursor is the `aria-activedescendant` when the tree
    /// holds focus.
    fn access_node(_state: &(), focused: Option<&str>) -> Vec<AccessNode> {
        let st = use_outliner();
        let rows = flat_visible(&st.nodes.get());
        let cursor = st.focused.get();
        let focused_id = if focused == Some(TAG) {
            cursor.as_deref()
        } else {
            None
        };
        tree_access_nodes(TAG, TAG, Some("Scene outliner"), &rows, None, focused_id)
    }

    /// Composite focus: when the tree holds focus, the cursor row is the
    /// `aria-activedescendant`.
    fn access_focus_target(_state: &(), focused: Option<&str>) -> Option<AccessFocus> {
        if focused == Some(TAG) {
            let st = use_outliner();
            let cursor = st.focused.get().unwrap_or_else(|| {
                flat_visible(&st.nodes.get())
                    .first()
                    .map(|r| r.id.clone())
                    .unwrap_or_default()
            });
            Some(AccessFocus::composite(TAG, tree_row_tag(TAG, &cursor)))
        } else {
            focused.map(AccessFocus::atomic)
        }
    }

    /// AT actions on a row: `Focus` / `Click` parks the cursor on it.
    fn access_child_invoke(
        _scene: &mut Scene,
        _parent_tag: &str,
        sub_tag: &str,
        action: AccessAction,
    ) -> bool {
        let st = use_outliner();
        if find_node(&st.nodes.get(), sub_tag).is_none() {
            return false;
        }
        match action {
            AccessAction::Focus | AccessAction::Click | AccessAction::Default => {
                st.focused.set(Some(sub_tag.to_owned()));
                true
            }
            _ => false,
        }
    }
}

impl WidgetView for TreeReparentView {
    type Renderer = HelloTreeReparentRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<TreeReparentView>();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Visible-row id order of a tree (after expanding nothing extra).
    fn visible_ids(nodes: &[OutlinerNode]) -> Vec<String> {
        flat_visible(nodes).into_iter().map(|r| r.id).collect()
    }

    #[test]
    fn boot_tree_flattens_to_the_expected_visible_order() {
        let ids = visible_ids(&initial_nodes());
        // world(exp) > terrain, player(exp) > camera, light, props(collapsed);
        // ui(exp) > hud. props' child `crate` is hidden (collapsed).
        assert_eq!(
            ids,
            [
                "world", "terrain", "player", "camera", "light", "props", "ui", "hud"
            ]
        );
    }

    #[test]
    fn reparent_onto_makes_the_node_a_child_and_expands_target() {
        let mut tree = initial_nodes();
        // Drop "light" ONTO "props" (currently collapsed) → light becomes
        // props' child and props auto-expands so it shows.
        assert!(reparent(&mut tree, "light", "props", DropZone::Onto));
        assert_eq!(parent_of(&tree, "light").as_deref(), Some("props"));
        assert!(
            find_node(&tree, "props").unwrap().expanded,
            "target auto-expands"
        );
        assert_eq!(
            parent_of(&tree, "camera").as_deref(),
            Some("player"),
            "sibling untouched"
        );
    }

    #[test]
    fn reparent_before_and_after_reorder_within_siblings() {
        let mut tree = initial_nodes();
        // Move "light" BEFORE "camera" (both children of player) → light first.
        assert!(reparent(&mut tree, "light", "camera", DropZone::Before));
        let player = find_node(&tree, "player").unwrap();
        let kids: Vec<&str> = player.children.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(kids, ["light", "camera"], "light reordered before camera");
        // Now move "light" AFTER "camera" → back to camera, light.
        assert!(reparent(&mut tree, "light", "camera", DropZone::After));
        let player = find_node(&tree, "player").unwrap();
        let kids: Vec<&str> = player.children.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(kids, ["camera", "light"]);
    }

    #[test]
    fn reparent_promotes_a_child_to_root_level() {
        let mut tree = initial_nodes();
        // Drop "camera" AFTER "ui" at root → camera becomes a root sibling.
        assert!(reparent(&mut tree, "camera", "ui", DropZone::After));
        assert_eq!(parent_of(&tree, "camera"), None, "camera now at root");
        let roots: Vec<String> = tree.iter().map(|n| n.id.clone()).collect();
        assert_eq!(roots, ["world", "ui", "camera"]);
    }

    #[test]
    fn cycle_drop_into_own_subtree_is_rejected() {
        let mut tree = initial_nodes();
        let before = visible_ids(&tree);
        // Drop "player" ONTO "camera" (its own child) → rejected, no change.
        assert!(!reparent(&mut tree, "player", "camera", DropZone::Onto));
        assert_eq!(
            visible_ids(&tree),
            before,
            "tree unchanged after a cycle drop"
        );
        // Drop "world" onto "light" (a deep descendant) → also rejected.
        assert!(!reparent(&mut tree, "world", "light", DropZone::Onto));
        assert_eq!(visible_ids(&tree), before);
    }

    #[test]
    fn drop_onto_self_is_a_noop() {
        let mut tree = initial_nodes();
        assert!(!reparent(&mut tree, "terrain", "terrain", DropZone::Onto));
        assert_eq!(parent_of(&tree, "terrain").as_deref(), Some("world"));
    }

    #[test]
    fn is_descendant_distinguishes_subtree_membership() {
        let tree = initial_nodes();
        assert!(
            is_descendant(&tree, "world", "camera"),
            "camera is under world"
        );
        assert!(is_descendant(&tree, "player", "light"));
        assert!(
            !is_descendant(&tree, "player", "world"),
            "ancestor is not a descendant"
        );
        assert!(!is_descendant(&tree, "ui", "camera"), "different subtree");
        assert!(
            !is_descendant(&tree, "camera", "camera"),
            "a node is not its own descendant"
        );
    }

    // ── External drag-hook integration (the scene/drag path) ──────────────

    fn drop_point(id: &str, y_rel: f32) -> DropPoint {
        DropPoint {
            tag: tree_row_tag(TAG, id),
            x_rel: 0.5,
            y_rel,
        }
    }

    #[test]
    fn external_send_then_drag_reparents_through_the_hooks() {
        Owner::new().run(|| {
            let mut ext = OutlinerExternal::new();
            // Press "light" (arms the drag), then drag onto "props" (middle).
            ext.invoke("send", IntrospectValue::Text("light:PointerDown".into()))
                .expect("send accepted");
            let payload = ext.begin_drag().expect("armed after press");
            assert_eq!(
                payload.value,
                IntrospectValue::Text("light".into()),
                "payload carries id"
            );
            ext.drag_to(&payload, Some(drop_point("props", 0.5)));
            assert!(
                matches!(ext.query("preview"), Some(IntrospectValue::Json(_))),
                "preview is in flight (scene-as-data)"
            );
            ext.drag_release(&payload, Some(drop_point("props", 0.5)));
            // light reparented under props; drag state cleared.
            assert_eq!(
                ext.query("parent_of.light"),
                Some(IntrospectValue::Text("props".into()))
            );
            assert_eq!(
                ext.query("preview"),
                Some(IntrospectValue::Null),
                "preview cleared"
            );
            assert!(ext.begin_drag().is_none(), "press cleared on release");
        });
    }

    #[test]
    fn external_query_reports_structure_and_rejects_cycle_drag() {
        Owner::new().run(|| {
            let mut ext = OutlinerExternal::new();
            assert_eq!(ext.query("row_count"), Some(IntrospectValue::Int(8)));
            assert_eq!(
                ext.query("id_at.0"),
                Some(IntrospectValue::Text("world".into()))
            );
            assert_eq!(
                ext.query("depth_at.3"),
                Some(IntrospectValue::Int(2)),
                "camera is depth 2"
            );
            assert_eq!(
                ext.query("parent_of.world"),
                Some(IntrospectValue::Text(String::new())),
                "root"
            );
            assert_eq!(
                ext.query("parent_of.nope"),
                Some(IntrospectValue::Null),
                "absent → Null"
            );
            // Drag "player" onto its own child "camera" → rejected, no change.
            ext.invoke("send", IntrospectValue::Text("player:PointerDown".into()))
                .expect("send");
            let p = ext.begin_drag().expect("armed");
            ext.drag_release(&p, Some(drop_point("camera", 0.5)));
            assert_eq!(
                ext.query("parent_of.camera"),
                Some(IntrospectValue::Text("player".into())),
                "cycle drop rejected — camera still under player",
            );
        });
    }

    #[test]
    fn external_send_rejects_unknown_node() {
        Owner::new().run(|| {
            let mut ext = OutlinerExternal::new();
            ext.invoke("send", IntrospectValue::Text("ghost:PointerDown".into()))
                .expect("send ok");
            assert!(
                ext.begin_drag().is_none(),
                "unknown node does not arm a drag"
            );
        });
    }

    #[test]
    fn r55_g20_view_contains_paint_root_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<TreeReparentView>(
            (),
            &Frame::new(),
        );
    }
}
