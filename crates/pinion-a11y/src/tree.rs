//! R51.61 §5.40 — `accesskit::TreeUpdate` builder.
//!
//! [`AccessTreeBuilder`] collects a flat list of pinion-native
//! [`AccessNode`]s and lowers them into a single `accesskit::TreeUpdate`
//! that the platform Adapter (Windows UIA / macOS AX / Linux AT-SPI /
//! Android) consumes.
//!
//! ## Tree topology
//!
//! The emitted tree has a synthetic [`ROOT_NODE_ID`] window node whose
//! children are every widget tag that is not claimed as a composite
//! child by any other [`AccessNode`]. Composite widgets (`RadioGroup`)
//! list their internal children's tags in
//! [`AccessNode::children`]; the builder resolves those tags into
//! `accesskit::NodeId`s and attaches them under the composite parent
//! instead of the root.
//!
//! ## Tag → `NodeId` hashing
//!
//! [`tag_to_node_id`] runs the widget tag through the standard library
//! `DefaultHasher` and sets the high bit so the result never collides
//! with the reserved root [`ROOT_NODE_ID`] = `NodeId(1)`. The
//! deterministic mapping is required by AccessKit's invariant that the
//! same UI element keeps the same `NodeId` across `TreeUpdate`s — the
//! framework uses widget tags exactly for that stable identity, so the
//! hash carries the invariant through unchanged.

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

use accesskit::{Action, Node, NodeId, Rect as AccessRect, Role, Tree, TreeId, TreeUpdate};
use pinion_core::scene::Rect;

use crate::node::{AccessNode, AccessValue};
use crate::role::AriaRole;

/// Reserved root `NodeId` for the synthetic window node.
pub const ROOT_NODE_ID: NodeId = NodeId(1);

/// Stable widget-tag → `NodeId` mapping.
///
/// Uses `DefaultHasher` (FxHash-class throughput, not cryptographic).
/// High bit is set so the result never collides with the reserved
/// [`ROOT_NODE_ID`] = `NodeId(1)` and so two different `DefaultHasher`
/// runs (across Rust versions) cannot accidentally produce a node id
/// that aliases a reserved slot. The mapping is per-process
/// deterministic — AccessKit only requires within-process stability,
/// which `DefaultHasher` provides.
#[must_use]
pub fn tag_to_node_id(tag: &str) -> NodeId {
    let mut h = DefaultHasher::new();
    tag.hash(&mut h);
    NodeId(h.finish() | 0x8000_0000_0000_0000)
}

/// Convert pinion-core `Rect` (u32) into `accesskit::Rect` (f64).
fn rect_to_accesskit(r: Rect) -> AccessRect {
    AccessRect {
        x0: f64::from(r.x),
        y0: f64::from(r.y),
        x1: f64::from(r.x + r.w),
        y1: f64::from(r.y + r.h),
    }
}

/// Builder for `accesskit::TreeUpdate`.
///
/// Build pattern: `new()` → `add()*` → `focused(tag)?` → `build(...)`.
/// Holds a tag→`AccessNode` map internally so duplicate tags overwrite
/// (matches AccessKit's "later node with same id wins" semantic).
pub struct AccessTreeBuilder {
    nodes: HashMap<String, AccessNode>,
    insertion_order: Vec<String>,
    focused: Option<String>,
    active_descendants: HashMap<String, String>,
    initial: bool,
    dirty: Option<HashSet<String>>,
}

impl AccessTreeBuilder {
    /// New empty builder. The default emits the synthetic root
    /// every build — pass `initial(false)` after the first frame
    /// to skip the `Tree` field per AccessKit's "rarely-updated"
    /// guidance.
    #[must_use]
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            insertion_order: Vec::new(),
            focused: None,
            active_descendants: HashMap::new(),
            initial: true,
            dirty: None,
        }
    }

    /// Append a widget node. Duplicate tag overwrites the previous
    /// entry (last-write-wins, matching AccessKit semantics).
    ///
    /// R51.79 §5.40 — signature takes `&AccessNode` (not by-value)
    /// so callers can keep ownership of their `Vec<AccessNode>` past
    /// the builder build, hand the same Vec by-value to
    /// `ShellCore::commit_access_emit`, and move it
    /// straight into the per-tag cache without re-cloning. The
    /// builder still clones internally (`self.nodes` consumes the
    /// node) — moving the clone from caller to callee centralises
    /// the bookkeeping and eliminates the outer
    /// `nodes.clone()` the pre-R51.79 render path performed before
    /// every `update_if_active` closure.
    pub fn add(&mut self, node: &AccessNode) -> &mut Self {
        if !self.nodes.contains_key(&node.tag) {
            self.insertion_order.push(node.tag.clone());
        }
        self.nodes.insert(node.tag.clone(), node.clone());
        self
    }

    /// Mark the currently focused widget tag (or clear with `None`).
    pub fn focused(&mut self, tag: Option<&str>) -> &mut Self {
        self.focused = tag.map(str::to_owned);
        self
    }

    /// R51.72 §5.40 — restrict `build()` to emit only the listed
    /// widget tags (plus the synthetic root).
    ///
    /// AccessKit's incremental-update guidance: "an update should
    /// only include nodes that are new or changed". The shell
    /// caches the previous frame's `AccessNode` set, diffs against
    /// the current frame, and passes the changed tags here so the
    /// emitted `TreeUpdate::nodes` carries only what the AT
    /// actually needs to refresh — bandwidth-proportional to
    /// actual UI activity, not widget count.
    ///
    /// When `dirty_tags` is not called (or called with an empty
    /// set), `build()` falls back to emitting every node — the
    /// initial-frame behavior the conformance tests rely on. Tags
    /// in the set that don't correspond to any added node are
    /// silently ignored (the AT keeps its previous state for
    /// those).
    ///
    /// The synthetic root node is always emitted regardless of
    /// `dirty_tags`, because it carries the window bounds and
    /// children list — both of which may need to change between
    /// frames even when no widget body did (window resize, widget
    /// added/removed). Holding the root constant would leave the
    /// AT with stale geometry.
    pub fn dirty_tags(&mut self, tags: HashSet<String>) -> &mut Self {
        self.dirty = Some(tags);
        self
    }

    /// R51.71 §5.40 — declare the active descendant of `parent_tag`.
    ///
    /// ARIA Authoring Practices' roving-tabindex / `aria-active-
    /// descendant` model: the parent widget owns the tab stop and
    /// the AT reports the addressed child within. At build time, the
    /// parent's lowered `accesskit::Node` receives
    /// `set_active_descendant(NodeId(child_tag))`.
    ///
    /// Composite widgets call this from `AppShell::render` via the
    /// `WidgetView::access_focus_target` return value (composite
    /// variant of [`crate::AccessFocus`]). Atomic widgets do not
    /// invoke it — their `TreeUpdate::focus` already lands on their
    /// own `NodeId` and no descendant is meaningful.
    ///
    /// Later calls overwrite earlier ones for the same `parent_tag`
    /// (one active descendant per parent).
    pub fn active_descendant(&mut self, parent_tag: &str, child_tag: &str) -> &mut Self {
        self.active_descendants
            .insert(parent_tag.to_owned(), child_tag.to_owned());
        self
    }

    /// Set whether this `TreeUpdate` is the very first emission for
    /// the tree (default = `true`). After the first build,
    /// downstream callers pass `false` so the `tree` field is
    /// omitted per AccessKit's "rarely-updated" guidance.
    ///
    /// R51.84 §5.40 — signature is `&mut self -> &mut Self` to match
    /// every other [`AccessTreeBuilder`] setter (`add`, `focused`,
    /// `dirty_tags`, `active_descendant`). The pre-R51.84 by-value
    /// shape (`mut self -> Self`) forced the shell to write
    /// `builder = builder.initial(false)` instead of the plain
    /// `builder.initial(false)` form the other setters take.
    pub fn initial(&mut self, initial: bool) -> &mut Self {
        self.initial = initial;
        self
    }

    /// Reverse map for `ActionRequest::target_node` lookup. Includes
    /// the root id so an AT request against the window itself can
    /// still be answered.
    #[must_use]
    pub fn tag_map(&self) -> HashMap<NodeId, String> {
        let mut map = HashMap::with_capacity(self.nodes.len() + 1);
        map.insert(ROOT_NODE_ID, String::new()); // "" = window root
        for tag in self.nodes.keys() {
            map.insert(tag_to_node_id(tag), tag.clone());
        }
        map
    }

    /// Lower the collected nodes into an `accesskit::TreeUpdate`.
    ///
    /// `window_bounds` becomes the root node's bounds (`None` = no
    /// bounds; AT will fall back to native window geometry).
    #[must_use]
    pub fn build(self, window_bounds: Option<Rect>) -> TreeUpdate {
        self.build_with_scale(window_bounds, 1.0)
    }

    /// R1027 §5.40 — like [`Self::build`] but re-expresses the
    /// (logical-pixel) node bounds in AccessKit's physical-pixel
    /// coordinate space via a single root-node
    /// `Affine::scale(scale_factor)` transform.
    ///
    /// Since R1027 the paint scene is laid out in logical pixels (the
    /// shell applies the window `scale_factor` only at the GPU raster +
    /// pointer-input boundaries), so the `AccessNode` bounds collected
    /// from it are logical. AccessKit expects physical-pixel coordinates
    /// (`accesskit::Node::transform` doc), and a node's transform applies
    /// to its own `bounds` plus every descendant (descendants carry no
    /// transform of their own), so one scale on the synthetic root
    /// re-expresses the whole tree in physical pixels. `scale_factor`
    /// effectively `1.0` leaves the transform unset — byte-identical to
    /// the pre-R1027 (and non-`HiDPI`) output, matching AccessKit's
    /// "should be `None` for the identity transform" guidance.
    #[must_use]
    pub fn build_with_scale(self, window_bounds: Option<Rect>, scale_factor: f64) -> TreeUpdate {
        let claimed = collect_claimed_children(&self.nodes);
        let root_children: Vec<NodeId> = self
            .insertion_order
            .iter()
            .filter(|t| !claimed.contains(t.as_str()))
            .map(|t| tag_to_node_id(t))
            .collect();

        let mut nodes: Vec<(NodeId, Node)> = Vec::with_capacity(self.nodes.len() + 1);

        // 1. Synthetic root window node.
        let mut root = Node::new(Role::Window);
        if let Some(bounds) = window_bounds {
            root.set_bounds(rect_to_accesskit(bounds));
        }
        // R1027 §5.40 — map the logical-pixel tree into AccessKit's
        // physical-pixel space. The transform applies to the root's own
        // `bounds` and propagates to every (transform-less) descendant,
        // so a single scale on the root covers the whole tree. Skipped at
        // identity so non-`HiDPI` output is byte-identical (AccessKit asks
        // for `None`, not `Affine::scale(1.0)`, at identity). `!= 1.0`
        // would trip `clippy::float_cmp`; winit reports exact factors so
        // the `f64::EPSILON` margin only excludes a literal 1.0.
        if (scale_factor - 1.0).abs() > f64::EPSILON {
            root.set_transform(accesskit::Affine::scale(scale_factor));
        }
        for child_id in root_children {
            root.push_child(child_id);
        }
        nodes.push((ROOT_NODE_ID, root));

        // R51.94 §5.40 — `tag_to_node_id` injective verification
        // (debug builds only, zero release cost).
        //
        // Two distinct widget tags hashing to the same `NodeId`
        // would silently shadow each other (AccessKit's
        // "later node with same id wins") and AT-side actions would
        // route to the wrong widget. The hash space (63 effective
        // bits — `DefaultHasher::finish() | 0x8000_0000_0000_0000`)
        // makes a real collision astronomically unlikely
        // (P ≈ N²/2^64), but accidental tag-duplication bugs in
        // application code or future framework refactors that break
        // injectivity would surface here on the first debug-mode
        // build instead of as a baffling AT-routing report.
        //
        // `ROOT_NODE_ID` is pre-seeded; the high-bit set on every
        // hashed tag id guarantees no widget tag collides with the
        // reserved root.
        #[cfg(debug_assertions)]
        let mut seen_ids: HashSet<NodeId> = {
            let mut s = HashSet::with_capacity(self.insertion_order.len() + 1);
            s.insert(ROOT_NODE_ID);
            s
        };

        // 2. Per-widget nodes in insertion order.
        //    R51.72 §5.40 — when `dirty` is `Some`, emit only the
        //    tags it lists. The root above is always emitted so the
        //    AT-side window geometry stays current.
        for tag in &self.insertion_order {
            if let Some(dirty) = &self.dirty {
                if !dirty.contains(tag) {
                    continue;
                }
            }
            let access = &self.nodes[tag];
            let node_id = tag_to_node_id(tag);
            #[cfg(debug_assertions)]
            {
                debug_assert!(
                    seen_ids.insert(node_id),
                    "R51.94 §5.40 tag_to_node_id collision: tag {tag:?} hashes to \
                     a NodeId already emitted this frame. Pick a distinct widget \
                     tag string for one of the colliding widgets, or widen the \
                     hash output if this is a real production collision."
                );
            }
            let mut node = lower_access_node(access);
            // R51.71 §5.40 — apply roving-tabindex active descendant
            // when this tag was declared via `active_descendant`.
            // Child tag is hashed through the same `tag_to_node_id`
            // function so the AT sees a `NodeId` that resolves
            // through the tree's own children list (no out-of-band
            // node lookup needed at the AT side).
            if let Some(child_tag) = self.active_descendants.get(tag) {
                // R947.1 — symmetric with the `focus` existence filter below:
                // name an active-descendant ONLY when the tree actually emits
                // that node. A windowed / roving widget whose cursor row
                // scrolled out of the realized set (e.g. a wheel scroll that
                // moves the viewport without moving the cursor) would otherwise
                // advertise a dangling `aria-activedescendant` — a NodeId
                // absent from this frame's tree. Dropping it leaves the parent
                // focused (atomic), the correct virtualized posture: no active
                // descendant while the active row is not rendered. This is the
                // contract `pinion_shell`'s `access_node_for_window` doc already
                // promised but `build` only honored for the focus tag.
                if self.nodes.contains_key(child_tag.as_str()) {
                    node.set_active_descendant(tag_to_node_id(child_tag));
                }
            }
            nodes.push((node_id, node));
        }

        let focus = self
            .focused
            .as_deref()
            .filter(|t| self.nodes.contains_key(*t))
            .map_or(ROOT_NODE_ID, tag_to_node_id);

        TreeUpdate {
            nodes,
            tree: if self.initial {
                Some(Tree::new(ROOT_NODE_ID))
            } else {
                None
            },
            tree_id: TreeId::ROOT,
            focus,
        }
    }
}

impl Default for AccessTreeBuilder {
    fn default() -> Self {
        Self::new()
    }
}

fn collect_claimed_children(nodes: &HashMap<String, AccessNode>) -> HashSet<&str> {
    let mut claimed: HashSet<&str> = HashSet::new();
    for n in nodes.values() {
        for child in &n.children {
            claimed.insert(child.as_str());
        }
    }
    claimed
}

/// The three boolean flags a **container** carries, lowered together.
///
/// Split out at R1693 when `aria-busy` took `lower_access_node` past the
/// hundred-line bound. They belong together for a reason beyond arithmetic:
/// each is a claim about the container as a whole rather than about the node's
/// own content, and each is boolean-set — omitted when false, so a node that
/// says nothing about the axis keeps the attribute absent.
fn lower_container_flags(access: &AccessNode, node: &mut Node) {
    // R51.98 §5.40 — WAI-ARIA `aria-multiselectable`. Only meaningful on
    // container roles with a selection set (Listbox primarily; Grid / Tree /
    // TabList future).
    if access.multiselectable {
        node.set_multiselectable();
    }

    // R693 §5.40 — WAI-ARIA `aria-modal`. Set on the open `Dialog` root so AT
    // confines its virtual cursor to the dialog subtree (the AT-side mirror of
    // the shell focus trap).
    if access.modal {
        node.set_modal();
    }

    // R1693 §5.40 — WAI-ARIA `aria-busy`. A collection that is being populated
    // tells an assistive technology to wait rather than letting it announce a
    // subtree that is still arriving — and read the emptiness as the answer.
    if access.busy {
        node.set_busy();
    }
}

fn lower_access_node(access: &AccessNode) -> Node {
    let mut node = Node::new(access.role.to_accesskit());

    if let Some(name) = &access.name {
        node.set_label(name.clone());
    }

    // R1543 §5.40 — the mnemonic, as UIA `AccessKey` / HTML `accesskey`.
    if let Some(access_key) = &access.access_key {
        node.set_access_key(access_key.clone());
    }

    match &access.value {
        Some(AccessValue::Bool(b)) => {
            node.set_toggled(if *b {
                accesskit::Toggled::True
            } else {
                accesskit::Toggled::False
            });
        }
        Some(AccessValue::Float { value, min, max }) => {
            node.set_numeric_value(f64::from(*value));
            node.set_min_numeric_value(f64::from(*min));
            node.set_max_numeric_value(f64::from(*max));
        }
        Some(AccessValue::Text(t)) => {
            node.set_value(t.clone());
        }
        None => {}
    }

    // R739 §5.40 — WAI-ARIA `aria-valuetext`: a labeled-step range widget
    // (slider / spinbutton) sets a string value *alongside* the numeric
    // `AccessValue::Float` lowered above. AccessKit's `set_value` carries
    // the string; `set_numeric_value` carries the number; AT prefers the
    // string when present but keeps the numeric range for context. Lowered
    // after the value match so a `Float` + `value_text` node emits both.
    if let Some(text) = &access.value_text {
        node.set_value(text.clone());
    }

    if let Some(checked) = access.state.checked {
        node.set_toggled(if checked {
            accesskit::Toggled::True
        } else {
            accesskit::Toggled::False
        });
    }
    // R1229 §5.40 — WAI-ARIA `aria-checked="mixed"` (the HTML
    // `<input>.indeterminate` axis): an indeterminate tri-state checkbox. Lowered
    // AFTER `checked` / `AccessValue::Bool` so a mixed control overrides any
    // definite toggle — a multi-object checkbox whose members disagree announces
    // "mixed", not a misleading on/off.
    if access.state.mixed {
        node.set_toggled(accesskit::Toggled::Mixed);
    }
    // R1544 §5.40 — WAI-ARIA `aria-readonly`. Emitted only when set, so a node
    // that says nothing about editability stays silent about it rather than
    // asserting "editable" — the absent-vs-false distinction the property has
    // in ARIA. Orthogonal to `set_disabled`: a read-only node stays focusable.
    if access.state.read_only {
        node.set_read_only();
    }
    // R696 §5.40 — WAI-ARIA `aria-expanded` mapping for disclosure
    // controls (accordion header, future submenu title / tree twisty).
    // AccessKit's `Expanded` is a boolean flag: `set_expanded(true)` =
    // shown, `set_expanded(false)` = collapsed; we omit it (no call)
    // when the axis is `None` so non-disclosure roles keep the
    // attribute absent. An `AccessNode` field (mirror `selected` /
    // `modal`), distinct from `set_toggled` (`aria-checked`).
    if let Some(expanded) = access.expanded {
        node.set_expanded(expanded);
    }
    if access.state.disabled {
        node.set_disabled();
    }
    // R1668 §5.40 §5.39 — and WHY, as the node's **state description**, which
    // is the slot the accessibility layer defines for replacing the default
    // announcement of a state. `set_disabled` alone leaves a listener with
    // "dimmed"; the reference toolkit at 6.11 has exactly that bit and no slot
    // for a reason at all. The phrase is derived from the reason value, so the
    // announcement and `scene/disabled` cannot disagree.
    if let Some(reason) = &access.unavailable {
        node.set_state_description(reason.sentence());
    }

    // R51.98 §5.40 — WAI-ARIA `aria-selected` mapping. Distinct axis
    // from `aria-checked`: container-membership (Listbox option, Tab,
    // future grid cell) vs two-state truthy (Switch / CheckBox /
    // RadioButton). AccessKit treats `Selected` as a 3-state
    // `Option<bool>` flag — `Some(true)` = selected, `Some(false)` =
    // explicitly unselected (announced distinctly in multi-select
    // containers, per `bool_property_methods` doc), `None` = the
    // attribute is omitted (the role doesn't carry the axis).
    if let Some(v) = access.selected {
        node.set_selected(v);
    }

    lower_container_flags(access, &mut node);

    // R1609 §5.40 — WAI-ARIA `aria-live`. Emitted only when declared, so a node that
    // says nothing about liveness keeps the attribute absent rather than
    // asserting `Off` — the same absent-vs-explicit distinction `aria-readonly` has, and it
    // matters here because `Off` is a meaningful opt-out inside a live ancestor.
    // The toolkit's peer is a fired accessible announcement event, which no
    // widget in `the toolkit's widget module/src/widgets` fires; a declaration is also the only form §2 #7 can
    // report, since a fired event leaves nothing to read back.
    if let Some(live) = access.live {
        node.set_live(live.to_accesskit());
    }

    if let Some(bounds) = access.bounds {
        node.set_bounds(rect_to_accesskit(bounds));
    }

    // R674 §5.40 — WAI-ARIA hierarchical axes (`aria-level` /
    // `aria-posinset` / `aria-setsize`). AccessKit's attribute names
    // collapse `aria-level` to bare `level`; the per-attribute
    // setters mirror the WAI-ARIA literals 1:1 otherwise. Custom-
    // widget roles (`role="treeitem"`, etc.) require these from the
    // author per WAI-ARIA 1.2 §6.6.8 / §6.6.9 / §6.6.10 — AT does NOT
    // infer them from DOM nesting on `non-native` roles. Pinion's
    // paint scenes are flat row sequences (composite-tag stamped per
    // row), so the binding is the sole source of truth.
    // AccessKit's `set_level` / `set_position_in_set` /
    // `set_size_of_set` take `usize` natively (the lib treats these
    // axes as platform-word-sized indices). `u32` widens losslessly
    // on every supported target (32-bit pointer + 64-bit pointer
    // both satisfy `u32 ≤ usize`); the saturating `try_from` keeps
    // the conversion explicit and survives a hypothetical
    // 16-bit-pointer target without panic.
    if let Some(level) = access.level {
        node.set_level(usize::try_from(level).unwrap_or(usize::MAX));
    }
    if let Some(pos) = access.position_in_set {
        node.set_position_in_set(usize::try_from(pos).unwrap_or(usize::MAX));
    }
    if let Some(size) = access.size_of_set {
        node.set_size_of_set(usize::try_from(size).unwrap_or(usize::MAX));
    }
    lower_table_axes(&mut node, access);

    for child_tag in &access.children {
        node.push_child(tag_to_node_id(child_tag));
    }

    // R695 §5.40 — WAI-ARIA `aria-describedby`. The related tag is
    // hashed through the same `tag_to_node_id` the children list uses,
    // so the AT resolves the description target through a NodeId that
    // already exists in this frame's tree (the tooltip node) — no
    // out-of-band lookup. Omitted when absent to keep the payload
    // minimal.
    if let Some(desc_tag) = &access.described_by {
        node.push_described_by(tag_to_node_id(desc_tag));
    }

    // R714 §5.40 — WAI-ARIA `aria-controls` (combobox → listbox). The
    // controlled tag is hashed through the same `tag_to_node_id` so the
    // AT resolves the popup through a NodeId already present in this
    // frame's tree. Omitted when absent (non-combobox roles).
    if let Some(controls_tag) = &access.controls {
        node.push_controlled(tag_to_node_id(controls_tag));
    }

    // R717 §5.40 — WAI-ARIA `aria-autocomplete` (editable combobox).
    // `Some(mode)` lowers through the `AutoComplete` bridge; `None`
    // omits the property (`aria-autocomplete="none"`).
    if let Some(mode) = access.auto_complete {
        node.set_auto_complete(mode.to_accesskit());
    }

    // R730 §5.40 — WAI-ARIA `aria-sort` on a sortable column header.
    // `Some(dir)` lowers through the `SortDirection` bridge; `None` omits
    // the property (`aria-sort="none"`).
    if let Some(dir) = access.sort {
        node.set_sort_direction(dir.to_accesskit());
    }

    // R731 §5.40 — WAI-ARIA `aria-current` on the current element of a set
    // (the breadcrumb's current crumb). `None` omits it (`aria-current="false"`).
    if let Some(kind) = access.current {
        node.set_aria_current(kind.to_accesskit());
    }
    // R985 §5.40 — WAI-ARIA `aria-haspopup` on a popup-owning trigger (the
    // submenu parent menuitem). `None` omits it (`aria-haspopup="false"`).
    if let Some(kind) = access.has_popup {
        node.set_has_popup(kind.to_accesskit());
    }

    add_actions_for_role(&mut node, access.role);
    node
}

/// R1560 §5.40 — the two tabular axes and the two spans, lifted out of
/// [`lower_access_node`]'s body.
///
/// R1559's `text_snapshot_into_json` precedent: an axis whose lowering is this
/// many independent properties earns its own function, and the alternative was
/// an `allow` on the whole writer, which would raise the length bound for every
/// other property it holds too.
fn lower_table_axes(node: &mut Node, access: &AccessNode) {
    // R1523 §5.40 §5.27 — the column axis' extent pair (`aria-colcount` /
    // `aria-colindex`), which a column-windowed grid needs for the same reason
    // the row axis needs setsize/posinset: the tree holds a slice, so the slice
    // has to say what it is a slice of.
    //
    // Carried through as the one-based ARIA value pinion stores. AccessKit
    // documents no base for `ColumnIndex`, and **no platform adapter in the
    // pinned generation reads it** — accesskit 0.24 / accesskit_winit 0.33 map
    // only `ColumnIndexText` (Windows), so there is nothing to calibrate the
    // base against yet. If a future adapter bump surfaces `ColumnIndex` and
    // announces it off by one, this is the single line that changes; pinion's
    // own tree (and the RPC introspection over it — invariant #2's primary
    // path) is where the value is verified today.
    if let Some(columns) = access.column_count {
        node.set_column_count(usize::try_from(columns).unwrap_or(usize::MAX));
    }
    if let Some(col) = access.column_index {
        node.set_column_index(usize::try_from(col).unwrap_or(usize::MAX));
    }
    // R1560 §5.40 §5.36 — the row axis and the two spans. Same caveat as the
    // column axis above: the value stored here is what pinion's own tree and
    // the §7 introspection over it are verified against.
    if let Some(rows) = access.row_count {
        node.set_row_count(usize::try_from(rows).unwrap_or(usize::MAX));
    }
    if let Some(row) = access.row_index {
        node.set_row_index(usize::try_from(row).unwrap_or(usize::MAX));
    }
    if let Some(span) = access.row_span {
        node.set_row_span(usize::try_from(span).unwrap_or(usize::MAX));
    }
    if let Some(span) = access.column_span {
        node.set_column_span(usize::try_from(span).unwrap_or(usize::MAX));
    }
}

fn add_actions_for_role(node: &mut Node, role: AriaRole) {
    match role {
        AriaRole::Button
        | AriaRole::CheckBox
        | AriaRole::RadioButton
        // R51.96.1 §5.40 — `ListBoxOption` behaves like the other
        // commit-class atomic roles for AT-side actions (Click =
        // activate, Focus = move active descendant). Distinct from
        // `RadioButton` only at the role surface; the action set is
        // the same.
        | AriaRole::ListBoxOption
        // R673 §5.40 — `TreeItem` rows are commit-class atomic at
        // the AT-action surface (Click to expand/activate, Focus to
        // move AT cursor). The role identity stays distinct
        // (`Role::TreeItem` → screen readers announce hierarchy +
        // level + posinset), but the action set matches Button /
        // ListBoxOption / etc.
        | AriaRole::TreeItem
        // R690 §5.40 — `Tab` is commit-class atomic at the AT-action
        // surface (Click selects the tab, Focus moves the AT cursor
        // for roving-tabindex navigation). The role identity stays
        // distinct (`Role::Tab` → screen readers announce "tab,
        // selected, N of M") but the action set matches
        // RadioButton / ListBoxOption.
        | AriaRole::Tab
        // R691 §5.40 — `MenuItem` is commit-class atomic at the
        // AT-action surface (Click activates the command, Focus moves
        // the AT cursor / active descendant). The role identity stays
        // distinct (`Role::MenuItem` → screen readers announce "menu
        // item") but the action set matches Button / Tab — a base
        // `menuitem` is a one-shot command, not a selection.
        | AriaRole::MenuItem
        // R805 §5.40 — `MenuItemCheckbox` shares the commit-class action
        // set (Click toggles + activates, Focus moves the AT cursor); the
        // toggled state surfaces separately through `aria-checked`.
        | AriaRole::MenuItemCheckbox
        // R704 §5.40 — `GridCell` (a date-picker day cell) is commit-
        // class atomic: Click activates (selects the day), Focus moves
        // the AT cursor for the grid's two-dimensional roving model. The
        // role identity stays distinct (`Role::GridCell` → screen readers
        // announce "cell, selected, N of M") but the action set matches
        // ListBoxOption / Tab.
        | AriaRole::GridCell
        // R714 §5.40 — `ComboBox` (the select-only trigger) is
        // commit-class atomic at the AT-action surface: Click opens /
        // toggles the listbox popup, Focus moves the AT cursor (the
        // active option surfaces as the combobox's
        // `aria-activedescendant`). Mirrors the Button / Disclosure
        // action set — the role identity (`Role::ComboBox` → screen
        // readers announce "combo box, collapsed/expanded") stays
        // distinct while the actions match.
        | AriaRole::ComboBox
        // R731 §5.40 — `Link` (a breadcrumb crumb) is commit-class atomic
        // at the AT-action surface: Click navigates, Focus moves the AT
        // cursor. The role identity stays distinct (`Role::Link` → screen
        // readers announce "link") but the action set matches Button.
        | AriaRole::Link
        | AriaRole::Switch => {
            node.add_action(Action::Click);
            node.add_action(Action::Focus);
        }
        // R734 §5.40 — `SpinButton` shares `Slider`'s *operable* numeric
        // action set: Focus (Tab in) + Increment / Decrement (the AT
        // step-up / step-down actions a screen reader maps to ArrowUp /
        // ArrowDown). Distinct from the passive `ProgressBar`, which
        // reports a `Float` value but receives no actions.
        AriaRole::Slider | AriaRole::SpinButton => {
            node.add_action(Action::Focus);
            node.add_action(Action::Increment);
            node.add_action(Action::Decrement);
        }
        // R51.96.1 §5.40 — `Listbox` composite parent supports
        // `Focus` (move the AT-side cursor to the listbox itself,
        // letting `active_descendant` surface the focused option).
        // Mirrors `RadioGroup`'s container-action set.
        //
        // R656 §5.40 — `List` and `ListItem` (WAI-ARIA 1.2 §5.3.5 / §5.3.6)
        // are passive AT containers. They share the `Focus`-
        // only action set with `Generic` / `RadioGroup` / `Listbox`:
        // AT cursor can land on the container/item to read its name,
        // but interactive children (delete buttons, edit handles)
        // own their own action sets through separate `AccessNode`
        // entries. This matches the WAI-ARIA authoring guide
        // recommendation for ungrouped lists (a non-selectable
        // collection of items, e.g. todomvc).
        // R673 §5.40 — `Tree` joins the focus-only container set
        // (parallel to `Listbox` / `RadioGroup` / `List`). Per-row
        // `TreeItem` AT events land in the commit-class arm above.
        // R690 §5.40 — `TabList` joins the focus-only container set
        // (parallel to `Tree` / `Listbox` / `RadioGroup`); its per-tab
        // `Tab` children land in the commit-class arm above.
        // `TabPanel` is the active tab's content region — focusable
        // (Tab key lands on it when it carries no focusable content)
        // so it shares the same `Focus`-only action set.
        // R691 §5.40 — `MenuBar` (the persistent title bar) and `Menu`
        // (the open dropdown container) join the focus-only container
        // set (parallel to `TabList` / `Tree` / `Listbox`); their
        // per-command `MenuItem` children land in the commit-class arm
        // above. The active dropdown item is surfaced as the menu's
        // `aria-activedescendant`, not as a focus of the container.
        // R692 §5.40 — `Toolbar` is the roving-tabindex control strip;
        // it owns the single Tab stop and surfaces the roving cursor as
        // its `aria-activedescendant`, so it joins the focus-only
        // container set. Its command / toggle `Button` children land in
        // the commit-class arm above.
        // R704 §5.40 — `Grid` is the date-picker calendar container. It
        // owns the single Tab stop and surfaces the roving day cell as
        // its `aria-activedescendant`, so it joins the focus-only
        // container set (parallel to `Listbox` / `TabList` / `Tree`). Its
        // `GridCell` children land in the commit-class arm above.
        // R863 §5.40 — `TreeGrid` (the columned-outliner container) is the
        // grid analogue of `Tree`: it owns the single Tab stop and surfaces
        // the roving row as its `aria-activedescendant`, so it joins the
        // focus-only container set. Its `Row` / `RowHeader` / `GridCell`
        // children are structural (the passive arm below).
        AriaRole::RadioGroup
        | AriaRole::Listbox
        | AriaRole::List
        | AriaRole::ListItem
        | AriaRole::Tree
        | AriaRole::TabList
        | AriaRole::TabPanel
        | AriaRole::MenuBar
        | AriaRole::Menu
        | AriaRole::Toolbar
        | AriaRole::Dialog
        | AriaRole::Grid
        | AriaRole::TreeGrid
        | AriaRole::Generic => {
            node.add_action(Action::Focus);
        }
        // R56.1.b.1 §5.40 — `TextInput` (single-line textbox) accepts
        // `Focus` (Tab into the field) and `Click` (place caret on
        // press). Edit actions (insert / delete / set-value) are
        // dispatched as `AccessAction::SetValue` events on R56.1.f+
        // accessibility carry — the action set here matches
        // WAI-ARIA 1.2 §4.3 textbox baseline.
        //
        // R717 §5.40 — `EditableComboBox` shares the textbox action set:
        // its trigger *is* a single-line text input (Focus to Tab in,
        // Click to place the caret). The popup-open behaviour rides on
        // the same surface (typing / ArrowDown open it); no extra AT
        // action token is needed beyond the textbox baseline.
        AriaRole::TextInput | AriaRole::EditableComboBox => {
            node.add_action(Action::Focus);
            node.add_action(Action::Click);
        }
        // R695 §5.40 — `Tooltip` (WAI-ARIA 1.2 §3.7) is a passive
        // description region with **no** AT actions: it never receives
        // focus and is not clickable. AT reaches it only through the
        // trigger's `aria-describedby` relation (the trigger announces
        // the tooltip text as its description). So the node carries
        // zero actions — distinct from every other role.
        //
        // R704 §5.40 — `ColumnHeader` (weekday header) and `Row` are
        // structural grid roles: AT reads their names to describe the
        // grid's column/row structure, but neither receives focus nor is
        // clickable, so they carry zero actions (the same passive arm as
        // `Tooltip`).
        //
        // R718 §5.40 — `ProgressBar` is a descriptive status widget: AT
        // announces its `aria-valuenow` as a read-only progress reading,
        // but it never receives focus and is not operable (a slider is —
        // hence `ProgressBar` is passive, not a `Slider`). Zero actions,
        // the same passive arm as `Tooltip` / `ColumnHeader` / `Row`.
        // R725 §5.40 — `Status` (the snackbar/toast live region) is a
        // passive polite region: AT announces its content when it
        // appears but it never receives focus and is not clickable, so
        // it carries zero actions (the same passive arm as `Tooltip` /
        // `ProgressBar`). Any in-snackbar action label is a separate
        // `Button` child carrying its own Click/Focus action set.
        // R731 §5.40 — `Navigation` (WAI-ARIA §3.5) is a passive landmark:
        // AT lists it among the page regions but it never receives focus
        // and is not clickable (its `Link` children carry the
        // interaction), so it joins the zero-action arm.
        // R733 §5.40 — `Group` (WAI-ARIA §3.6) is a passive labelled
        // container: it never receives focus and is not clickable (its
        // toggle-`Button` children each carry their own Click/Focus
        // action set and own Tab stop), so it joins the zero-action arm.
        // Distinct from `RadioGroup` / `Toolbar` (focus-only above): those
        // own the single Tab stop + roving cursor, whereas a multi-select
        // toggle-button group leaves each button independently tabbable.
        // R863 §5.40 — `RowHeader` (the tree-grid name column's label cell)
        // is a structural header like `ColumnHeader`: AT reads its name to
        // label the row, but it owns no AT-action surface (the tree-grid's
        // pointer click routes through hit-test, not an AT action; a future
        // keyboard-roving model would surface expand/collapse on the `Row`,
        // not the header cell). Joins the zero-action arm.
        // R1551 §5.40 — a `heading` is structural: AT reads its name and
        // `aria-level` to build the document outline a screen-reader user
        // navigates by, and it owns no AT action (a heading is not activatable).
        // The level itself is carried by the shared `set_level` call above,
        // which the `TreeItem` depth axis already drives.
        // R1560 §5.40 — a document `table` and its `cell`s are content, not a
        // widget: they own no AT action, which is precisely the difference
        // from `grid` / `gridcell` above and the reason the two role pairs
        // were kept separate rather than reused.
        AriaRole::Tooltip
        | AriaRole::Table
        | AriaRole::Cell
        | AriaRole::ColumnHeader
        | AriaRole::RowHeader
        | AriaRole::Row
        | AriaRole::ProgressBar
        | AriaRole::Status
        | AriaRole::Navigation
        | AriaRole::Heading
        | AriaRole::Group => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::AccessState;

    #[test]
    fn tag_to_node_id_is_deterministic() {
        let a = tag_to_node_id("main_btn");
        let b = tag_to_node_id("main_btn");
        assert_eq!(a, b);
    }

    #[test]
    fn tag_to_node_id_distinct_for_distinct_tags() {
        let a = tag_to_node_id("a");
        let b = tag_to_node_id("b");
        assert_ne!(a, b);
    }

    #[test]
    fn tag_to_node_id_never_collides_with_root() {
        for t in ["a", "b", "main_btn", "main_group", "", "  ", "1"] {
            assert_ne!(tag_to_node_id(t), ROOT_NODE_ID);
        }
    }

    #[test]
    fn empty_builder_emits_root_only() {
        let update = AccessTreeBuilder::new().build(None);
        assert_eq!(update.nodes.len(), 1);
        assert_eq!(update.nodes[0].0, ROOT_NODE_ID);
        assert_eq!(update.focus, ROOT_NODE_ID);
        assert!(update.tree.is_some());
    }

    #[test]
    fn single_atomic_widget_attaches_to_root() {
        let mut b = AccessTreeBuilder::new();
        b.add(&AccessNode::new("main_btn", AriaRole::Button));
        let update = b.build(None);
        assert_eq!(update.nodes.len(), 2);
        // root first, then widget
        assert_eq!(update.nodes[0].0, ROOT_NODE_ID);
        assert_eq!(update.nodes[1].0, tag_to_node_id("main_btn"));
    }

    #[test]
    fn composite_children_not_at_root() {
        let mut b = AccessTreeBuilder::new();
        b.add(
            &AccessNode::new("main_group", AriaRole::RadioGroup)
                .with_child("r0")
                .with_child("r1"),
        );
        b.add(&AccessNode::new("r0", AriaRole::RadioButton));
        b.add(&AccessNode::new("r1", AriaRole::RadioButton));
        let update = b.build(None);
        // RadioGroup is at root; r0/r1 are not direct root children
        // (they live under RadioGroup via the composite topology).
        // Inspect the root node's children count: only 1 (RadioGroup).
        // We can't easily inspect Node internals without accesskit
        // private API, but the build must succeed and emit 4 nodes
        // (root + group + r0 + r1).
        assert_eq!(update.nodes.len(), 4);
    }

    #[test]
    fn focused_falls_back_to_root_when_tag_missing() {
        let mut b = AccessTreeBuilder::new();
        b.add(&AccessNode::new("main_btn", AriaRole::Button));
        b.focused(Some("nonexistent"));
        let update = b.build(None);
        assert_eq!(update.focus, ROOT_NODE_ID);
    }

    #[test]
    fn focused_resolves_to_widget_when_present() {
        let mut b = AccessTreeBuilder::new();
        b.add(&AccessNode::new("main_btn", AriaRole::Button));
        b.focused(Some("main_btn"));
        let update = b.build(None);
        assert_eq!(update.focus, tag_to_node_id("main_btn"));
    }

    #[test]
    fn initial_false_omits_tree_field() {
        let mut b = AccessTreeBuilder::new();
        b.initial(false);
        let update = b.build(None);
        assert!(update.tree.is_none());
    }

    #[test]
    fn window_bounds_sets_root_bounds() {
        // We can't introspect Node bounds without accesskit private
        // API, so just verify build succeeds with bounds passed.
        let update = AccessTreeBuilder::new().build(Some(Rect::new(0, 0, 1024, 768)));
        assert_eq!(update.nodes.len(), 1);
    }

    #[test]
    fn r1027_build_with_scale_sets_root_transform() {
        // R1027 §5.40 — at non-identity scale the synthetic root node
        // carries an `Affine::scale(scale_factor)` transform so the
        // logical-pixel bounds + descendants map into AccessKit's
        // physical-pixel space (the only place the scale enters the tree).
        let update =
            AccessTreeBuilder::new().build_with_scale(Some(Rect::new(0, 0, 480, 320)), 2.0);
        let (_, root) = update
            .nodes
            .iter()
            .find(|(id, _)| *id == ROOT_NODE_ID)
            .expect("synthetic root node emitted");
        assert_eq!(
            root.transform(),
            Some(&accesskit::Affine::scale(2.0)),
            "root transform re-expresses the logical tree in physical pixels"
        );
    }

    #[test]
    fn r1027_build_with_scale_identity_leaves_transform_none() {
        // R1027 §5.40 — at scale 1.0 the root carries NO transform
        // (AccessKit asks for `None`, not `Affine::scale(1.0)`, at
        // identity), so non-`HiDPI` output is byte-identical to `build()`.
        let update =
            AccessTreeBuilder::new().build_with_scale(Some(Rect::new(0, 0, 480, 320)), 1.0);
        let (_, root) = update
            .nodes
            .iter()
            .find(|(id, _)| *id == ROOT_NODE_ID)
            .expect("synthetic root node emitted");
        assert_eq!(
            root.transform(),
            None,
            "identity scale leaves the root transform unset"
        );
        // And it matches the plain `build()` root (no transform divergence).
        let plain = AccessTreeBuilder::new().build(Some(Rect::new(0, 0, 480, 320)));
        let (_, plain_root) = plain
            .nodes
            .iter()
            .find(|(id, _)| *id == ROOT_NODE_ID)
            .expect("synthetic root node emitted");
        assert_eq!(plain_root.transform(), None);
    }

    #[test]
    fn duplicate_tag_overwrites_previous() {
        let mut b = AccessTreeBuilder::new();
        b.add(&AccessNode::new("btn", AriaRole::Button).with_name("First"));
        b.add(&AccessNode::new("btn", AriaRole::Button).with_name("Second"));
        let update = b.build(None);
        // 1 widget node + 1 root = 2
        assert_eq!(update.nodes.len(), 2);
    }

    #[test]
    fn tag_map_includes_root_and_widgets() {
        let mut b = AccessTreeBuilder::new();
        b.add(&AccessNode::new("main_btn", AriaRole::Button));
        b.add(&AccessNode::new("main_cb", AriaRole::CheckBox));
        let map = b.tag_map();
        assert_eq!(map.get(&ROOT_NODE_ID).map(String::as_str), Some(""));
        assert_eq!(
            map.get(&tag_to_node_id("main_btn")).map(String::as_str),
            Some("main_btn"),
        );
        assert_eq!(
            map.get(&tag_to_node_id("main_cb")).map(String::as_str),
            Some("main_cb"),
        );
    }

    #[test]
    fn checkbox_with_value_and_state_lowers() {
        let state = AccessState {
            focused: true,
            checked: Some(true),
            ..AccessState::default()
        };
        let node = AccessNode::new("cb", AriaRole::CheckBox)
            .with_name("Enable")
            .with_value(AccessValue::Bool(true))
            .with_state(state)
            .with_bounds(Rect::new(10, 20, 100, 30));
        let mut b = AccessTreeBuilder::new();
        b.add(&node);
        b.focused(Some("cb"));
        let update = b.build(None);
        assert_eq!(update.focus, tag_to_node_id("cb"));
        assert_eq!(update.nodes.len(), 2);
    }

    #[test]
    fn r733_button_with_checked_lowers_as_toggle_button() {
        // R733 §5.40 — a `button` role carrying `checked = Some(_)` is a
        // toggle button: it lowers through the same `set_toggled` path a
        // checkbox uses, but AT announces it as `aria-pressed` (not
        // `aria-checked`) purely because the role is `Button`. The first
        // toggle-button consumer is the multi-select segmented button
        // (`hello-segmented-multi`). AccessKit exposes no public
        // `toggled()` getter, so we pin the lowering by node count +
        // role; the pressed-vs-checked semantic distinction lives in the
        // role (verified at the binding's `access_node` level).
        let pressed = AccessNode::new("seg", AriaRole::Button)
            .with_name("Photos")
            .with_state(AccessState {
                checked: Some(true),
                ..AccessState::default()
            })
            .with_bounds(Rect::new(0, 0, 80, 32));
        let mut b = AccessTreeBuilder::new();
        b.add(&pressed);
        let update = b.build(None);
        // root + the toggle button.
        assert_eq!(update.nodes.len(), 2);
        // A plain (non-toggle) button carries `checked = None` — the
        // distinguishing field is the `AccessNode` state, not the role.
        let plain = AccessNode::new("btn", AriaRole::Button);
        assert_eq!(plain.state.checked, None);
        assert_eq!(pressed.state.checked, Some(true));
    }

    #[test]
    fn r1229_checkbox_with_mixed_lowers_indeterminate() {
        // R1229 §5.40 — a tri-state checkbox marked mixed carries the
        // indeterminate axis (`aria-checked="mixed"` / accesskit `Toggled::Mixed`),
        // the HTML `<input>.indeterminate` property. AccessKit exposes no public
        // `toggled()` getter (see r733), so we pin the `AccessNode`-level state +
        // that the lowering branch builds; the mixed leg is a separate opt-in that
        // takes precedence over any definite `checked` / `AccessValue::Bool`.
        let mixed = AccessNode::new("cb", AriaRole::CheckBox)
            .with_name("Visible")
            .with_mixed()
            .with_bounds(Rect::new(0, 0, 40, 20));
        assert!(mixed.state.mixed, "with_mixed sets the indeterminate axis");
        // A definite `checked` alongside `mixed` still carries the mixed axis
        // (the lowering resolves it last → Toggled::Mixed wins).
        let both = AccessNode::new("cb2", AriaRole::CheckBox)
            .with_value(AccessValue::Bool(true))
            .with_mixed()
            .with_bounds(Rect::new(0, 0, 40, 20));
        assert!(both.state.mixed);
        // The tree builds (the mixed lowering branch runs) without panic.
        let mut b = AccessTreeBuilder::new();
        b.add(&mixed);
        b.add(&both);
        let update = b.build(None);
        assert_eq!(update.nodes.len(), 3, "root + 2 checkboxes");
        // A default checkbox is NOT mixed.
        assert!(!AccessNode::new("cb3", AriaRole::CheckBox).state.mixed);
    }

    #[test]
    fn slider_emits_float_range() {
        let node = AccessNode::new("sl", AriaRole::Slider)
            .with_value(AccessValue::Float {
                value: 50.0,
                min: 0.0,
                max: 100.0,
            })
            .with_bounds(Rect::new(0, 0, 200, 24));
        let mut b = AccessTreeBuilder::new();
        b.add(&node);
        let update = b.build(None);
        assert_eq!(update.nodes.len(), 2);
    }

    #[test]
    fn active_descendant_does_not_alter_focus_or_node_count() {
        let mut b = AccessTreeBuilder::new();
        b.add(
            &AccessNode::new("main_group", AriaRole::RadioGroup)
                .with_child("main_group#0")
                .with_child("main_group#1"),
        );
        b.add(&AccessNode::new("main_group#0", AriaRole::RadioButton));
        b.add(&AccessNode::new("main_group#1", AriaRole::RadioButton));
        b.focused(Some("main_group"));
        b.active_descendant("main_group", "main_group#1");
        let update = b.build(None);
        // TreeUpdate.focus stays on the parent — ARIA Authoring
        // Practices roving-tabindex model.
        assert_eq!(update.focus, tag_to_node_id("main_group"));
        // 1 root + group + 2 radios = 4 nodes; active_descendant is a
        // node attribute, not a separate tree node.
        assert_eq!(update.nodes.len(), 4);
    }

    #[test]
    fn active_descendant_for_unknown_parent_is_silent() {
        let mut b = AccessTreeBuilder::new();
        b.add(&AccessNode::new("main_btn", AriaRole::Button));
        b.active_descendant("nonexistent_parent", "main_btn");
        let update = b.build(None);
        // No panic, no spurious node — the declaration applies only
        // to lowered nodes whose tag matches.
        assert_eq!(update.nodes.len(), 2);
    }

    #[test]
    fn r947_1_active_descendant_present_in_nodes_is_set() {
        // A realized cursor row (in the emitted node set) is named as the
        // parent's aria-activedescendant — the normal roving case.
        let mut b = AccessTreeBuilder::new();
        b.add(&AccessNode::new("tree", AriaRole::Tree).with_child("tree#0"));
        b.add(&AccessNode::new("tree#0", AriaRole::TreeItem));
        b.focused(Some("tree"));
        b.active_descendant("tree", "tree#0");
        let update = b.build(None);
        let (_, tree_node) = update
            .nodes
            .iter()
            .find(|(id, _)| *id == tag_to_node_id("tree"))
            .expect("tree node emitted");
        assert_eq!(
            tree_node.active_descendant(),
            Some(tag_to_node_id("tree#0")),
            "a realized active-descendant child is named"
        );
    }

    #[test]
    fn r947_1_active_descendant_absent_from_nodes_is_dropped_not_dangling() {
        // R947.1 regression: a roving widget whose active-descendant child is
        // NOT in the emitted node set (a virtualized cursor row scrolled
        // off-window by a wheel scroll that did not move the cursor) must NOT
        // advertise a dangling aria-activedescendant. The builder drops it —
        // symmetric with the focus filter — so the parent stays focused
        // atomically with no descendant (the correct virtualized posture).
        let mut b = AccessTreeBuilder::new();
        b.add(&AccessNode::new("tree", AriaRole::Tree).with_child("tree#0"));
        b.add(&AccessNode::new("tree#0", AriaRole::TreeItem));
        b.focused(Some("tree"));
        // The cursor (#9) is off the rendered window: declared but never added.
        b.active_descendant("tree", "tree#9");
        let update = b.build(None);
        let (_, tree_node) = update
            .nodes
            .iter()
            .find(|(id, _)| *id == tag_to_node_id("tree"))
            .expect("tree node emitted");
        assert_eq!(
            tree_node.active_descendant(),
            None,
            "an absent active-descendant is dropped, never a dangling NodeId"
        );
        // Focus still lands on the realized parent (atomic).
        assert_eq!(update.focus, tag_to_node_id("tree"));
    }

    #[test]
    fn dirty_tags_filters_to_named_widgets_only() {
        let mut b = AccessTreeBuilder::new();
        b.add(&AccessNode::new("main_btn", AriaRole::Button));
        b.add(&AccessNode::new("main_cb", AriaRole::CheckBox));
        b.add(&AccessNode::new("main_sl", AriaRole::Slider));
        let dirty: HashSet<String> = ["main_cb".to_owned()].into_iter().collect();
        b.dirty_tags(dirty);
        let update = b.build(None);
        // Root + 1 dirty widget = 2 emitted.
        assert_eq!(update.nodes.len(), 2);
        assert_eq!(update.nodes[0].0, ROOT_NODE_ID);
        assert_eq!(update.nodes[1].0, tag_to_node_id("main_cb"));
    }

    #[test]
    fn dirty_tags_empty_set_emits_root_only() {
        let mut b = AccessTreeBuilder::new();
        b.add(&AccessNode::new("main_btn", AriaRole::Button));
        b.dirty_tags(HashSet::new());
        let update = b.build(None);
        assert_eq!(update.nodes.len(), 1);
        assert_eq!(update.nodes[0].0, ROOT_NODE_ID);
    }

    #[test]
    fn dirty_tags_unknown_tag_silently_skipped() {
        let mut b = AccessTreeBuilder::new();
        b.add(&AccessNode::new("main_btn", AriaRole::Button));
        let dirty: HashSet<String> = ["nonexistent".to_owned(), "main_btn".to_owned()]
            .into_iter()
            .collect();
        b.dirty_tags(dirty);
        let update = b.build(None);
        // The unknown tag is silently dropped; only main_btn lowered.
        assert_eq!(update.nodes.len(), 2);
    }

    #[test]
    fn unset_dirty_emits_every_widget() {
        let mut b = AccessTreeBuilder::new();
        b.add(&AccessNode::new("a", AriaRole::Button));
        b.add(&AccessNode::new("b", AriaRole::Button));
        b.add(&AccessNode::new("c", AriaRole::Button));
        // No call to `dirty_tags` — equivalent to "all dirty".
        let update = b.build(None);
        assert_eq!(update.nodes.len(), 4); // root + 3
    }

    #[test]
    fn r51_98_listbox_with_multiselectable_lowers() {
        // Smoke test: build with a multi-selectable Listbox parent +
        // two ListBoxOption children, one selected. We can't read
        // accesskit::Node internals from outside the crate, so we
        // only verify build succeeds and the node count is right.
        let mut b = AccessTreeBuilder::new();
        b.add(
            &AccessNode::new("list", AriaRole::Listbox)
                .with_multiselectable()
                .with_child("list#0")
                .with_child("list#1"),
        );
        b.add(&AccessNode::new("list#0", AriaRole::ListBoxOption).with_selected(true));
        b.add(&AccessNode::new("list#1", AriaRole::ListBoxOption).with_selected(false));
        let update = b.build(None);
        // root + list + 2 options = 4
        assert_eq!(update.nodes.len(), 4);
    }

    #[test]
    fn r51_98_listbox_option_with_selected_none_lowers() {
        // `selected: None` is the legacy path — pre-R51.98 hello-listbox
        // didn't set the axis at all. Build must still succeed.
        let mut b = AccessTreeBuilder::new();
        b.add(&AccessNode::new("opt", AriaRole::ListBoxOption));
        let update = b.build(None);
        assert_eq!(update.nodes.len(), 2);
    }

    #[test]
    fn r693_modal_dialog_with_action_buttons_lowers() {
        // Smoke test: an open modal Dialog root with aria-modal + two
        // action Button children. AccessKit node internals are opaque
        // from outside the crate, so we verify build succeeds with the
        // right node count (root + dialog + 2 actions).
        let mut b = AccessTreeBuilder::new();
        b.add(
            &AccessNode::new("dialog", AriaRole::Dialog)
                .with_modal()
                .with_name("Confirm")
                .with_child("dialog_ok")
                .with_child("dialog_cancel"),
        );
        b.add(&AccessNode::new("dialog_ok", AriaRole::Button).with_name("OK"));
        b.add(&AccessNode::new("dialog_cancel", AriaRole::Button).with_name("Cancel"));
        let update = b.build(None);
        assert_eq!(update.nodes.len(), 4);
    }

    #[test]
    fn r696_disclosure_header_with_expanded_lowers() {
        // Smoke test: a disclosure header Button carrying aria-expanded
        // (the WAI-ARIA disclosure pattern's primary requirement).
        // AccessKit node internals are opaque from outside the crate,
        // so we verify build succeeds for both expanded and collapsed
        // forms. The content panel is a plain grouping container (the
        // APG marks `role=region` optional, so no dedicated role is
        // emitted — see [[abstraction-needs-second-consumer]]).
        let mut open = AccessTreeBuilder::new();
        open.add(
            &AccessNode::new("section_hdr", AriaRole::Button)
                .with_name("Section")
                .with_expanded(true),
        );
        assert_eq!(open.build(None).nodes.len(), 2); // root + header

        let mut closed = AccessTreeBuilder::new();
        closed.add(
            &AccessNode::new("section_hdr", AriaRole::Button)
                .with_name("Section")
                .with_expanded(false),
        );
        assert_eq!(closed.build(None).nodes.len(), 2); // root + header
    }

    #[test]
    fn r714_combobox_controls_listbox_lowers() {
        // Smoke test: a ComboBox trigger controlling a Listbox popup
        // (the WAI-ARIA §4.5 combobox pairing). AccessKit node internals
        // are opaque from outside the crate, so we verify the build
        // succeeds with both nodes present (root + combobox + listbox).
        let mut b = AccessTreeBuilder::new();
        b.add(
            &AccessNode::new("size_combo", AriaRole::ComboBox)
                .with_name("Size")
                .with_expanded(true)
                .with_controls("size_options"),
        );
        b.add(&AccessNode::new("size_options", AriaRole::Listbox).with_name("Size options"));
        let update = b.build(None);
        assert_eq!(update.nodes.len(), 3);
    }

    #[test]
    fn r717_editable_combobox_autocomplete_lowers() {
        // Smoke test: an EditableComboBox input controlling a filtered
        // Listbox popup with aria-autocomplete=list (the WAI-ARIA §4.5
        // editable-combobox pairing). AccessKit node internals are opaque
        // from outside the crate, so we verify the build succeeds with
        // all nodes present (root + combobox input + listbox).
        let mut b = AccessTreeBuilder::new();
        b.add(
            &AccessNode::new("fruit_input", AriaRole::EditableComboBox)
                .with_name("Fruit")
                .with_expanded(true)
                .with_controls("fruit_options")
                .with_auto_complete(crate::role::AutoComplete::List),
        );
        b.add(&AccessNode::new("fruit_options", AriaRole::Listbox).with_name("Fruit options"));
        let update = b.build(None);
        assert_eq!(update.nodes.len(), 3);
    }

    #[test]
    fn r718_progress_bar_numeric_value_lowers() {
        // Smoke test: a determinate ProgressBar carrying a normalized
        // AccessValue::Float (aria-valuenow/min/max). AccessKit node
        // internals are opaque from outside the crate, so we verify the
        // build succeeds with the node present (root + progressbar) and
        // the same numeric lowering path Slider exercises.
        let mut b = AccessTreeBuilder::new();
        b.add(
            &AccessNode::new("dl_progress", AriaRole::ProgressBar)
                .with_name("Download")
                .with_value(crate::AccessValue::Float {
                    value: 0.4,
                    min: 0.0,
                    max: 1.0,
                }),
        );
        let update = b.build(None);
        assert_eq!(update.nodes.len(), 2);
    }

    #[test]
    fn r739_labeled_slider_lowers_valuetext_and_numeric() {
        // R739 §5.40 — a labeled-step slider lowers BOTH the numeric range
        // (aria-valuenow/min/max from AccessValue::Float) AND the string
        // aria-valuetext (from value_text). AccessKit keeps them as separate
        // node properties; AT prefers the string but retains the numeric
        // range for context. Call the private lowering directly so we can
        // read both back via the AccessKit getters.
        let labeled = AccessNode::new("sl", AriaRole::Slider)
            .with_value(AccessValue::Float {
                value: 0.5,
                min: 0.0,
                max: 1.0,
            })
            .with_value_text("Medium");
        let node = lower_access_node(&labeled);
        assert_eq!(
            node.value(),
            Some("Medium"),
            "aria-valuetext string lowered"
        );
        assert_eq!(
            node.numeric_value(),
            Some(0.5),
            "aria-valuenow still lowered"
        );
        assert_eq!(
            node.min_numeric_value(),
            Some(0.0),
            "aria-valuemin retained"
        );
        assert_eq!(
            node.max_numeric_value(),
            Some(1.0),
            "aria-valuemax retained"
        );

        // A plain numeric slider (no value_text) omits the string value.
        let plain = AccessNode::new("sl", AriaRole::Slider).with_value(AccessValue::Float {
            value: 0.5,
            min: 0.0,
            max: 1.0,
        });
        let plain_node = lower_access_node(&plain);
        assert_eq!(
            plain_node.value(),
            None,
            "plain numeric slider omits aria-valuetext"
        );
        assert_eq!(
            plain_node.numeric_value(),
            Some(0.5),
            "numeric value still present"
        );
    }

    #[test]
    fn r695_tooltip_describedby_lowers() {
        // Smoke test: a Button trigger describing-by a Tooltip node.
        // AccessKit node internals are opaque from outside the crate,
        // so we verify build succeeds with both nodes present (root +
        // trigger + tooltip).
        let mut b = AccessTreeBuilder::new();
        b.add(
            &AccessNode::new("save_btn", AriaRole::Button)
                .with_name("Save")
                .with_described_by("save_tip"),
        );
        b.add(&AccessNode::new("save_tip", AriaRole::Tooltip).with_name("Saves the file"));
        let update = b.build(None);
        assert_eq!(update.nodes.len(), 3);
    }

    #[test]
    fn r1609_a_live_region_declaration_reaches_accesskit_and_absence_stays_absent() {
        // Not a smoke test: `accesskit::Node::live()` is a real getter, so the
        // lowered value is asserted rather than inferred from the node count.
        let mut b = AccessTreeBuilder::new();
        b.add(
            &AccessNode::new("board", AriaRole::Group)
                .with_name("Tile dashboard")
                .with_live(crate::node::AccessLive::Polite),
        );
        b.add(&AccessNode::new("quiet", AriaRole::Group).with_name("Not live"));
        b.add(
            &AccessNode::new("urgent", AriaRole::Group)
                .with_live(crate::node::AccessLive::Assertive),
        );
        // `Off` is why this axis is three-valued where the toolkit's politeness
        // is two: a fired event has no "off", a declared region nested in a
        // live ancestor does.
        b.add(&AccessNode::new("optout", AriaRole::Group).with_live(crate::node::AccessLive::Off));
        let update = b.build(None);

        let live_of = |tag: &str| {
            update
                .nodes
                .iter()
                .find(|(id, _)| *id == tag_to_node_id(tag))
                .map(|(_, node)| node.live())
                .expect("the node is in the update")
        };
        assert_eq!(live_of("board"), Some(accesskit::Live::Polite));
        assert_eq!(live_of("urgent"), Some(accesskit::Live::Assertive));
        assert_eq!(live_of("optout"), Some(accesskit::Live::Off));
        assert_eq!(
            live_of("quiet"),
            None,
            "a node that says nothing about liveness keeps the attribute ABSENT \
             rather than asserting Off — the distinction `aria-readonly` has too"
        );
    }

    #[test]
    fn active_descendant_last_call_wins_per_parent() {
        let mut b = AccessTreeBuilder::new();
        b.add(
            &AccessNode::new("g", AriaRole::RadioGroup)
                .with_child("g#a")
                .with_child("g#b"),
        );
        b.add(&AccessNode::new("g#a", AriaRole::RadioButton));
        b.add(&AccessNode::new("g#b", AriaRole::RadioButton));
        b.active_descendant("g", "g#a");
        b.active_descendant("g", "g#b");
        let update = b.build(None);
        // Single-active-descendant per parent — overwriting is the
        // documented semantic.
        assert_eq!(update.nodes.len(), 4);
    }
}
