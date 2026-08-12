//! R1007 §5.27 §5.40 — **master-detail row dissection** over a Model/View
//! dataset.
//!
//! The Model/View-at-scale campaign's earlier slices act on the *row set*: the
//! filter axis (R997 [`GridFilter`](crate::widgets::grid_sort::GridFilter)) removes
//! rows, the R998 coloring engine tints them, the R1004 search cursor walks
//! them. **Master-detail** turns the other way — it takes the *one* selected
//! row and explodes its structured payload into a recursive tree of fields.
//! This is the analyser's packet-detail pane below the packet list, the
//! dlt-viewer message-detail tree, the inspector panel that follows a
//! selection: pick a row up top, read its inner structure below.
//!
//! ## What is genuinely new here (the transducer, not the tree)
//!
//! The keyboard-navigable tree substrate ([`TreeNode`] / `flat_visible` /
//! `toggle_expanded` / `view_virtual_tree`) is mature and reused verbatim, and
//! `pinion_widget_paint::devtools::scene_to_tree_item` already projects one
//! structured value — a [`Scene`](crate::scene::Scene) — into that tree. What
//! does **not** exist is the peer transducer for the Model/View *data* domain:
//! [`dissect_row`] recursively expands a row's structured payload (a
//! [`serde_json::Value`], the shape an [`IntrospectValue::Json`] already
//! carries) into [`DissectNode`]s — one branch per object field, one per array
//! element, a leaf per scalar — with a stable path id per node. That value→tree
//! transducer, plus the selection-reactive re-derivation in
//! [`RowDissectionState`], is the new substrate this module adds. Everything
//! downstream (collapse, keyboard nav, paint, AT) is the existing tree
//! machinery.
//!
//! ## State (retained tree) vs. derivation (on select)
//!
//! [`RowDissectionState`] holds the materialized source rows, the selected
//! master index, and the **retained** detail tree — rebuilt from the selected
//! row's payload on every [`select`](RowDissectionState::select), then mutated
//! in place by [`toggle_expanded`] as the user collapses branches. This is the same retained-`Signal<Vec<N>>`
//! flag store `hello-tree-view` and the property grid drive, so collapse,
//! keyboard nav and the flat walk all reuse the R820 helpers. The detail tree
//! is small (one row's fields), so it is rebuilt rather than diffed.
//!
//! ## The AI-first witness (§2 #7 scene-as-data)
//!
//! `query("selected")` is the master row the detail tree dissects;
//! `query("node_count")` the number of *visible* detail rows (collapsed
//! branches omitted); `query("name.<i>")` / `query("value.<i>")` /
//! `query("kind.<i>")` / `query("depth.<i>")` / `query("path.<i>")` the i-th
//! visible node's field name, scalar value, structural kind, tree depth and
//! stable path. `invoke "select" <row>` re-dissects, `invoke "toggle"/"expand"/
//! "collapse" <path>` drive a branch, `invoke "clear"` deselects — the whole
//! dissection is observable and driveable without a pixel (see
//! `tools/demos/r1007_row_dissect.py`).

use std::rc::Rc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::external::{
    ExternalIntrospect, InterveneError, IntrospectSchema, IntrospectValue, InvokeError,
    ReadRefusal, SchemaArg, SchemaField, query_proxy_external_impl,
};
use crate::reactive::{Owner, Signal, batch};
use crate::widgets::tree_nav::{TreeNode, set_expanded_in, toggle_expanded};

/// R1007 §5.27 §5.40 — the structural class of a dissected value, so a
/// consumer can render or color a node by its kind (a string leaf differently
/// from a number, an object branch differently from an array) and an AI client
/// can read the field's type without parsing its rendered text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DissectKind {
    /// A JSON `null`.
    Null,
    /// A JSON boolean.
    Bool,
    /// A JSON number with no fractional part (`is_i64() || is_u64()`).
    Int,
    /// A JSON number with a fractional part.
    Float,
    /// A JSON string.
    Text,
    /// A JSON array — a branch whose children are its elements.
    Array,
    /// A JSON object — a branch whose children are its fields.
    Object,
}

impl DissectKind {
    /// The lower-case wire name (`"int"`, `"object"`, …) — the `kind.<i>`
    /// introspection token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Bool => "bool",
            Self::Int => "int",
            Self::Float => "float",
            Self::Text => "text",
            Self::Array => "array",
            Self::Object => "object",
        }
    }

    /// Whether this kind is a branch (an `Object` or `Array`, the only kinds
    /// with children).
    #[must_use]
    pub const fn is_branch(self) -> bool {
        matches!(self, Self::Array | Self::Object)
    }

    /// Classify a [`serde_json::Value`].
    #[must_use]
    pub fn of(value: &Value) -> Self {
        match value {
            Value::Null => Self::Null,
            Value::Bool(_) => Self::Bool,
            Value::Number(n) => {
                if n.is_i64() || n.is_u64() {
                    Self::Int
                } else {
                    Self::Float
                }
            }
            Value::String(_) => Self::Text,
            Value::Array(_) => Self::Array,
            Value::Object(_) => Self::Object,
        }
    }
}

/// R1007 §5.27 §5.40 — one node of a dissected structured value: a stable
/// `path` id, the field `name`, the rendered scalar `value` (empty for a
/// branch), the structural `kind`, the `expanded` flag, and the recursive
/// `children`.
///
/// Implements [`TreeNode`] (`id` = `path`, `label` = `name`), so the whole
/// keyboard-navigation / collapse / flat-walk substrate drives it unchanged. A
/// branch's `value` is empty — its data lives in its children; use
/// [`display`](Self::display) for the painted row text.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DissectNode {
    /// Stable node id — the path from the row root (`"headers.host"`,
    /// `"items[0].name"`). The nav cursor key + composite-tag suffix.
    pub path: String,
    /// Field name (an object key, an `[i]` array index, or `value` for a
    /// scalar root). The [`TreeNode::label`] + type-ahead key.
    pub name: String,
    /// Rendered scalar text; empty for an `Object` / `Array` branch.
    pub value: String,
    /// Structural kind of the underlying value.
    pub kind: DissectKind,
    /// Whether this branch is expanded (a leaf is always `false`).
    pub expanded: bool,
    /// Child nodes — object fields / array elements; empty for a leaf.
    pub children: Vec<DissectNode>,
}

impl DissectNode {
    /// The painted row text: `"name: value"` for a leaf, `"name  (n)"` for a branch (n = child
    /// count) — the analyser "field: value" / "Protocol (count)" detail row.
    /// The substrate's [`TreeNode::label`] stays the bare `name` (the type-ahead key); this is
    /// what the binding feeds the painter.
    #[must_use]
    pub fn display(&self) -> String {
        if self.kind.is_branch() {
            format!("{}  ({})", self.name, self.children.len())
        } else {
            format!("{}: {}", self.name, self.value)
        }
    }
}

impl TreeNode for DissectNode {
    fn id(&self) -> &str {
        &self.path
    }
    fn label(&self) -> &str {
        &self.name
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

/// The rendered scalar text for a value — empty for a container, whose
/// children carry the data.
fn render_scalar(value: &Value) -> String {
    match value {
        Value::Null => "null".to_owned(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        Value::Array(_) | Value::Object(_) => String::new(),
    }
}

/// Dissect one `value` into a node named `name` at `path`, recursing into
/// object fields and array elements. A branch starts expanded only at
/// `depth == 0`, so a freshly selected row shows its top-level structure one
/// level deep and the user expands further — the analyser/the toolkit default.
fn dissect_node(name: &str, value: &Value, path: &str, depth: u32) -> DissectNode {
    let kind = DissectKind::of(value);
    let children = match value {
        Value::Object(map) => map
            .iter()
            .map(|(k, v)| dissect_node(k, v, &format!("{path}.{k}"), depth + 1))
            .collect(),
        Value::Array(items) => items
            .iter()
            .enumerate()
            .map(|(i, v)| dissect_node(&format!("[{i}]"), v, &format!("{path}[{i}]"), depth + 1))
            .collect(),
        _ => Vec::new(),
    };
    DissectNode {
        path: path.to_owned(),
        name: name.to_owned(),
        value: render_scalar(value),
        kind,
        expanded: kind.is_branch() && depth == 0,
        children,
    }
}

/// R1007 §5.27 §5.40 — **the new substrate**: dissect a row's structured
/// payload into its top-level field nodes.
///
/// An object yields one [`DissectNode`] per field (keyed by field name); an
/// array one node per element (keyed `[i]`); a bare scalar a single `value`
/// node. Each node carries a stable path so it round-trips through the
/// composite-tag / `path.<i>` wire. The peer of
/// `pinion_widget_paint::devtools::scene_to_tree_item` for the Model/View data
/// domain — that transduces a `Scene`, this a `serde_json::Value`.
///
/// Recursion depth follows the value's JSON nesting. `serde_json`'s parser caps
/// nesting at its own `RECURSION_LIMIT` (128) for *parsed* input, so a `Value`
/// from `from_str` is bounded; a consumer that programmatically builds a far
/// deeper `Value` (well past any real packet/message detail) should pre-cap it
/// — a depth-bounded transducer is a follow-up for the first such source.
#[must_use]
pub fn dissect_row(value: &Value) -> Vec<DissectNode> {
    match value {
        Value::Object(map) => map.iter().map(|(k, v)| dissect_node(k, v, k, 0)).collect(),
        Value::Array(items) => items
            .iter()
            .enumerate()
            .map(|(i, v)| dissect_node(&format!("[{i}]"), v, &format!("[{i}]"), 0))
            .collect(),
        scalar => vec![dissect_node("value", scalar, "value", 0)],
    }
}

/// R1007 §5.27 §5.40 — one row of the depth-first flattening of the *visible*
/// dissection tree, carrying the dissection's data (name / value / kind) the
/// generic [`VisibleRow`](crate::widgets::tree_nav::VisibleRow) does not.
///
/// The traversal rule is exactly `flat_visible`'s — depth-first preorder,
/// descend a branch only when expanded — so the row sequence matches what the
/// tree painter and the keyboard nav walk (a test pins the two to agree). This
/// is the dissection's introspection + paint-source flattening.
#[derive(Clone, Debug, PartialEq)]
pub struct DissectRow {
    /// Stable node path (the [`DissectNode::path`]).
    pub path: String,
    /// Field name.
    pub name: String,
    /// Rendered scalar value; empty for a branch.
    pub value: String,
    /// Structural kind.
    pub kind: DissectKind,
    /// Zero-based tree depth (top-level rows = 0).
    pub depth: u32,
    /// Whether this node is a branch (has children).
    pub has_children: bool,
    /// Whether a branch is currently expanded.
    pub expanded: bool,
}

fn walk_flat(nodes: &[DissectNode], depth: u32, out: &mut Vec<DissectRow>) {
    for n in nodes {
        let has_children = !n.children.is_empty();
        out.push(DissectRow {
            path: n.path.clone(),
            name: n.name.clone(),
            value: n.value.clone(),
            kind: n.kind,
            depth,
            has_children,
            expanded: n.expanded,
        });
        if has_children && n.expanded {
            walk_flat(&n.children, depth + 1, out);
        }
    }
}

/// R1007 §5.27 §5.40 — flatten the *visible* dissection tree into its
/// depth-first row sequence (collapsed branches' descendants omitted) — the
/// paint source + the `node.<i>` introspection list.
#[must_use]
pub fn dissect_flat(nodes: &[DissectNode]) -> Vec<DissectRow> {
    let mut out = Vec::new();
    walk_flat(nodes, 0, &mut out);
    out
}

/// R1007 §5.27 §5.40 — the reactive **single source of truth** for a
/// master-detail dissection: the materialized source rows, the selected master
/// index, and the retained detail tree derived from the selected row.
///
/// Created once via [`use_row_dissection`] and shared — the same `Rc` — by the
/// [`RowDissectionExternal`] (which selects / toggles) and the view (which
/// reads the selection + tree). Reading [`selected`](Self::selected) /
/// [`tree`](Self::tree) inside a view-fn auto-subscribes, so a selection or a
/// collapse repaints exactly like any other reactive change.
pub struct RowDissectionState {
    tag: Option<&'static str>,
    /// The source rows' structured payloads, materialized once.
    rows: Vec<Value>,
    /// Selected master row, or `None` when nothing is selected.
    selected: Signal<Option<usize>>,
    /// The retained detail tree of the selected row — rebuilt on
    /// [`select`](Self::select), mutated in place by the collapse helpers.
    tree: Signal<Vec<DissectNode>>,
}

impl RowDissectionState {
    /// Construct over the given source row payloads. Starts with no selection
    /// and an empty detail tree.
    #[must_use]
    pub fn new(rows: Vec<Value>) -> Self {
        Self {
            tag: None,
            rows,
            selected: Signal::new(None),
            tree: Signal::new(Vec::new()),
        }
    }

    /// As [`new`](Self::new) but records the [`use_row_dissection`] cache key.
    #[must_use]
    pub fn with_tag(key: &'static str, rows: Vec<Value>) -> Self {
        Self {
            tag: Some(key),
            ..Self::new(rows)
        }
    }

    /// The [`use_row_dissection`] cache key, or `None` when constructed
    /// directly.
    #[must_use]
    pub fn tag(&self) -> Option<&'static str> {
        self.tag
    }

    /// Source (master) row count.
    #[must_use]
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// The structured payload of master `row`, or `None` out of range.
    #[must_use]
    pub fn row_value(&self, row: usize) -> Option<&Value> {
        self.rows.get(row)
    }

    /// The selected master row, or `None`. Subscribes when read in a view-fn.
    #[must_use]
    pub fn selected(&self) -> Option<usize> {
        self.selected.get()
    }

    /// The current detail tree (a clone of the retained nodes). Subscribes
    /// when read in a view-fn.
    #[must_use]
    pub fn tree(&self) -> Vec<DissectNode> {
        self.tree.get()
    }

    /// The visible detail rows (the flattened tree). Subscribes (tree) when
    /// read in a view-fn.
    #[must_use]
    pub fn flat(&self) -> Vec<DissectRow> {
        dissect_flat(&self.tree.get())
    }

    /// Number of visible detail rows. Subscribes when read in a view-fn.
    #[must_use]
    pub fn node_count(&self) -> usize {
        self.flat().len()
    }

    /// Select master `row` (`None` deselects) and rebuild the detail tree from
    /// that row's payload. An out-of-range index deselects (empty tree), never
    /// panics. The two writes share one [`batch`] so a subscribed view sees the
    /// new selection and the new tree in one repaint.
    pub fn select(&self, row: Option<usize>) {
        let valid = row.filter(|&r| r < self.rows.len());
        batch(|| {
            self.selected.set(valid);
            let tree = valid
                .map(|r| dissect_row(&self.rows[r]))
                .unwrap_or_default();
            self.tree.set(tree);
        });
    }

    /// Flip branch `path`'s expanded flag in the retained detail tree (the
    /// click / Space-Enter toggle). A leaf / unknown path is a no-op — the R820
    /// [`toggle_expanded`] helper.
    pub fn toggle_path(&self, path: &str) {
        toggle_expanded(&self.tree, path);
    }

    /// Set branch `path`'s expanded flag explicitly (the `expand` / `collapse`
    /// channels) — the R820 [`set_expanded_in`] helper.
    pub fn set_expanded_path(&self, path: &str, expanded: bool) {
        set_expanded_in(&self.tree, path, expanded);
    }
}

/// R1007 §5.27 §5.40 — resolve the shared [`RowDissectionState`] for `key`,
/// building it once via `data` (the source row payloads). Mirrors
/// [`use_row_search`](crate::widgets::row_search::use_row_search): the
/// `External` and the view both call this with the same `key` and receive the
/// same `Rc`, so the selection is one source of truth.
///
/// # Panics
///
/// Panics if no current [`Owner`] is set (call from within a `view` / a
/// `create_extra_externals` hook — both run inside a `root_owner.run`).
#[must_use]
pub fn use_row_dissection(
    key: &'static str,
    data: impl FnOnce() -> Vec<Value>,
) -> Rc<RowDissectionState> {
    Owner::current()
        .expect("use_row_dissection requires an active Owner scope")
        .cache(key, || RowDissectionState::with_tag(key, data()))
}

/// R1007 §5.27 §5.40 — the dissection **coordinator** External: a thin adapter
/// surfacing the shared [`RowDissectionState`] to the §5.12 `scene/query` /
/// `scene/intervene` / `scene/invoke` paths.
///
/// Like the search / sort-filter proxies it owns no interaction statechart and
/// emits **no** §5.20 intent — the selection / tree `Signal` writes already
/// repaint every subscribed view, so the widget is never independently dirty.
#[derive(Clone)]
pub struct RowDissectionExternal {
    state: Rc<RowDissectionState>,
}

impl core::fmt::Debug for RowDissectionExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("RowDissectionExternal")
            .field("selected", &self.state.selected())
            .field("node_count", &self.state.node_count())
            .finish_non_exhaustive()
    }
}

impl RowDissectionExternal {
    /// Wrap the shared [`RowDissectionState`] (from [`use_row_dissection`]).
    #[must_use]
    pub fn new(state: Rc<RowDissectionState>) -> Self {
        Self { state }
    }

    /// The shared state handle (the view reaches the same `Rc` via
    /// [`use_row_dissection`]).
    #[must_use]
    pub fn state(&self) -> &Rc<RowDissectionState> {
        &self.state
    }

    /// `node_count` as an `IntrospectValue::Int` — the uniform return for the
    /// selection / collapse `invoke` paths.
    fn node_count_value(&self) -> IntrospectValue {
        IntrospectValue::Int(i64::try_from(self.state.node_count()).unwrap_or(i64::MAX))
    }

    /// Resolve a `path.<i>` / `name.<i>` / … query: the `pick` of the i-th
    /// visible detail row, or `Null` (present-but-empty) when `i` is malformed
    /// or out of range.
    fn flat_field(
        &self,
        rest: &str,
        pick: impl Fn(&DissectRow) -> IntrospectValue,
    ) -> IntrospectValue {
        rest.parse::<usize>()
            .ok()
            .and_then(|i| self.state.flat().get(i).map(&pick))
            .unwrap_or(IntrospectValue::Null)
    }
}

// The shared display-config-proxy `External` skeleton (R847 SSOT): no §5.20
// intent — the selection / tree `Signal` writes already repaint every
// subscribed view, so the widget is never independently dirty.
query_proxy_external_impl!(RowDissectionExternal);

impl ExternalIntrospect for RowDissectionExternal {
    fn schema(&self) -> IntrospectSchema {
        // `selected`   — selected master row, or Null (query + intervene).
        // `row_count`  — master row count (query only).
        // `node_count` — visible detail-row count (query only).
        // `path`/`name`/`value`/`kind`/`depth` — `<token>.<i>` per visible row.
        // `select`/`toggle`/`expand`/`collapse`/`clear` — invoke channels.
        IntrospectSchema::new(
            const {
                &[
                    SchemaField::new("selected", "int"),
                    SchemaField::new("row_count", "int"),
                    SchemaField::new("node_count", "int"),
                    SchemaField::parametric(
                        "path.<idx>",
                        "string",
                        const { &[SchemaArg::index("idx", "node_count")] },
                    ),
                    SchemaField::parametric(
                        "name.<idx>",
                        "string",
                        const { &[SchemaArg::index("idx", "node_count")] },
                    ),
                    SchemaField::parametric(
                        "value.<idx>",
                        "string",
                        const { &[SchemaArg::index("idx", "node_count")] },
                    ),
                    SchemaField::parametric(
                        "kind.<idx>",
                        "string",
                        const { &[SchemaArg::index("idx", "node_count")] },
                    ),
                    SchemaField::parametric(
                        "depth.<idx>",
                        "int",
                        const { &[SchemaArg::index("idx", "node_count")] },
                    ),
                    SchemaField::action("select", "int"),
                    SchemaField::action("toggle", "string"),
                    SchemaField::action("expand", "string"),
                    SchemaField::action("collapse", "string"),
                    SchemaField::action("clear", "string"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Result<IntrospectValue, ReadRefusal> {
        // Per-visible-row dissection fields: `<token>.<i>`.
        if let Some(rest) = path.strip_prefix("path.") {
            return Ok(self.flat_field(rest, |r| IntrospectValue::Text(r.path.clone())));
        }
        if let Some(rest) = path.strip_prefix("name.") {
            return Ok(self.flat_field(rest, |r| IntrospectValue::Text(r.name.clone())));
        }
        if let Some(rest) = path.strip_prefix("value.") {
            return Ok(self.flat_field(rest, |r| IntrospectValue::Text(r.value.clone())));
        }
        if let Some(rest) = path.strip_prefix("kind.") {
            return Ok(self.flat_field(rest, |r| IntrospectValue::Text(r.kind.as_str().to_owned())));
        }
        if let Some(rest) = path.strip_prefix("depth.") {
            return Ok(self.flat_field(rest, |r| IntrospectValue::Int(i64::from(r.depth))));
        }
        match path {
            "selected" => Ok(self
                .state
                .selected()
                .and_then(|r| i64::try_from(r).ok())
                .map_or(IntrospectValue::Null, IntrospectValue::Int)),
            "row_count" => Ok(IntrospectValue::Int(
                i64::try_from(self.state.row_count()).unwrap_or(i64::MAX),
            )),
            "node_count" => Ok(self.node_count_value()),
            _ => Err(ReadRefusal::UnknownPath),
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            // Admin / restore: select a master row (or Null to deselect).
            "selected" => match value {
                IntrospectValue::Null => {
                    self.state.select(None);
                    Ok(())
                }
                IntrospectValue::Int(i) => {
                    self.state.select(usize::try_from(i).ok());
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            "row_count" | "node_count" | "path" | "name" | "value" | "kind" | "depth" => {
                Err(InterveneError::ReadOnly)
            }
            _ => Err(InterveneError::UnknownPath),
        }
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match path {
            // Select a master row and re-dissect; returns the new node_count.
            "select" => match args {
                IntrospectValue::Int(i) => {
                    self.state.select(usize::try_from(i).ok());
                    Ok(self.node_count_value())
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            // Flip / set a branch's collapse state; returns the new node_count.
            "toggle" => match args {
                IntrospectValue::Text(ref p) => {
                    self.state.toggle_path(p);
                    Ok(self.node_count_value())
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            "expand" => match args {
                IntrospectValue::Text(ref p) => {
                    self.state.set_expanded_path(p, true);
                    Ok(self.node_count_value())
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            "collapse" => match args {
                IntrospectValue::Text(ref p) => {
                    self.state.set_expanded_path(p, false);
                    Ok(self.node_count_value())
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            // Deselect: empty detail tree, node_count 0.
            "clear" => {
                self.state.select(None);
                Ok(self.node_count_value())
            }
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::tree_nav::flat_visible;
    use serde_json::json;

    /// A small HTTP-request-log row: nested object (headers) + array (tags) +
    /// scalars of every kind, so the dissection exercises each branch/leaf.
    fn request_row() -> Value {
        json!({
            "method": "GET",
            "status": 200,
            "secure": true,
            "latency": 12.5,
            "note": null,
            "headers": { "host": "example.com", "accept": "*/*" },
            "tags": ["api", "v1"]
        })
    }

    fn rows() -> Vec<Value> {
        vec![
            request_row(),
            json!({ "method": "POST", "status": 404 }),
            json!([1, 2, 3]),
        ]
    }

    fn ext() -> RowDissectionExternal {
        RowDissectionExternal::new(Rc::new(RowDissectionState::new(rows())))
    }

    #[test]
    fn dissect_kind_classifies_each_json_type() {
        assert_eq!(DissectKind::of(&json!(null)), DissectKind::Null);
        assert_eq!(DissectKind::of(&json!(true)), DissectKind::Bool);
        assert_eq!(DissectKind::of(&json!(7)), DissectKind::Int);
        assert_eq!(DissectKind::of(&json!(-3)), DissectKind::Int);
        assert_eq!(DissectKind::of(&json!(1.5)), DissectKind::Float);
        assert_eq!(DissectKind::of(&json!("x")), DissectKind::Text);
        assert_eq!(DissectKind::of(&json!([1])), DissectKind::Array);
        assert_eq!(DissectKind::of(&json!({"a": 1})), DissectKind::Object);
        assert!(DissectKind::Object.is_branch() && DissectKind::Array.is_branch());
        assert!(!DissectKind::Int.is_branch());
    }

    #[test]
    fn dissect_row_object_yields_field_nodes_with_paths() {
        let nodes = dissect_row(&request_row());
        // serde_json::Map preserves insertion order (preserve_order is off by
        // default → BTreeMap-ish alphabetical); assert by lookup, not position.
        let by_name = |name: &str| {
            nodes
                .iter()
                .find(|n| n.name == name)
                .expect("field present")
                .clone()
        };

        let method = by_name("method");
        assert_eq!(method.kind, DissectKind::Text);
        assert_eq!(method.value, "GET");
        assert_eq!(method.path, "method");
        assert!(method.children.is_empty());

        let status = by_name("status");
        assert_eq!(status.kind, DissectKind::Int);
        assert_eq!(status.value, "200");

        let headers = by_name("headers");
        assert_eq!(headers.kind, DissectKind::Object);
        assert!(
            headers.expanded,
            "a top-level (depth 0) branch starts expanded"
        );
        let host = headers.children.iter().find(|n| n.name == "host").unwrap();
        assert_eq!(host.path, "headers.host", "child path is parent.field");
        assert_eq!(host.value, "example.com");

        let tags = by_name("tags");
        assert_eq!(tags.kind, DissectKind::Array);
        assert_eq!(tags.children.len(), 2);
        assert_eq!(tags.children[0].name, "[0]");
        assert_eq!(tags.children[0].path, "tags[0]");
        assert_eq!(tags.children[0].value, "api");
    }

    #[test]
    fn dissect_row_array_and_scalar_roots() {
        let arr = dissect_row(&json!([10, 20]));
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0].name, "[0]");
        assert_eq!(arr[0].path, "[0]");
        assert_eq!(arr[1].value, "20");

        let scalar = dissect_row(&json!("lonely"));
        assert_eq!(scalar.len(), 1);
        assert_eq!(scalar[0].name, "value");
        assert_eq!(scalar[0].value, "lonely");
        assert_eq!(scalar[0].kind, DissectKind::Text);
    }

    #[test]
    fn display_renders_leaf_and_branch_rows() {
        let nodes = dissect_row(&request_row());
        let method = nodes.iter().find(|n| n.name == "method").unwrap();
        assert_eq!(method.display(), "method: GET");
        let headers = nodes.iter().find(|n| n.name == "headers").unwrap();
        assert_eq!(
            headers.display(),
            "headers  (2)",
            "branch shows its child count"
        );
    }

    #[test]
    fn dissect_flat_omits_collapsed_descendants() {
        let mut nodes = dissect_row(&json!({ "a": { "b": 1 } }));
        // `a` is a depth-0 object → expanded → its child `a.b` is visible.
        let visible_expanded = dissect_flat(&nodes);
        assert_eq!(visible_expanded.len(), 2, "a + a.b");
        assert_eq!(visible_expanded[1].path, "a.b");
        assert_eq!(visible_expanded[1].depth, 1);

        // Collapse `a` → only `a` is visible.
        nodes[0].set_expanded(false);
        let visible_collapsed = dissect_flat(&nodes);
        assert_eq!(visible_collapsed.len(), 1, "a only");
        assert!(!visible_collapsed[0].expanded);
        assert!(visible_collapsed[0].has_children);
    }

    #[test]
    fn dissect_flat_agrees_with_flat_visible() {
        // The dissection's richer flatten must walk the same visible rows as
        // the generic tree-nav flatten (divergence would desync paint vs RPC).
        // Checked both fully expanded AND with a branch collapsed, so the
        // `&& n.expanded` descend guard in walk_flat is actually exercised — a
        // fixture with only expanded branches would pass even if that guard were
        // dropped.
        fn assert_agrees(nodes: &[DissectNode]) {
            let rich = dissect_flat(nodes);
            let generic = flat_visible(nodes);
            assert_eq!(rich.len(), generic.len(), "same visible row count");
            for (r, g) in rich.iter().zip(generic.iter()) {
                assert_eq!(r.path, g.id, "path == id");
                assert_eq!(r.name, g.label, "name == label");
                assert_eq!(r.depth, g.depth);
                assert_eq!(r.has_children, g.has_children);
                assert_eq!(r.expanded, g.expanded);
            }
        }
        let mut nodes = dissect_row(&request_row());
        let full = dissect_flat(&nodes).len();
        assert_agrees(&nodes);
        // Collapse the headers branch and re-check: both flattens must drop its
        // two children (the collapsed-descend path), and still agree.
        let headers = nodes
            .iter_mut()
            .find(|n| n.name == "headers")
            .expect("headers branch");
        assert!(
            headers.expanded && !headers.children.is_empty(),
            "headers is an expanded branch"
        );
        headers.set_expanded(false);
        assert_eq!(
            dissect_flat(&nodes).len(),
            full - 2,
            "collapsing headers drops its 2 children"
        );
        assert_agrees(&nodes);
    }

    #[test]
    fn state_select_builds_tree_and_counts() {
        Owner::new().run(|| {
            let s = RowDissectionState::new(rows());
            assert_eq!(s.row_count(), 3);
            assert_eq!(s.selected(), None);
            assert_eq!(s.node_count(), 0, "no selection → empty detail tree");

            s.select(Some(0));
            assert_eq!(s.selected(), Some(0));
            // Top-level fields (7) + expanded headers' 2 children + tags' 2 = 11.
            assert_eq!(s.node_count(), 11);
        });
    }

    #[test]
    fn state_select_out_of_range_deselects() {
        Owner::new().run(|| {
            let s = RowDissectionState::new(rows());
            s.select(Some(0));
            assert_eq!(s.selected(), Some(0));
            s.select(Some(99));
            assert_eq!(s.selected(), None, "an out-of-range select deselects");
            assert_eq!(s.node_count(), 0);
        });
    }

    #[test]
    fn state_toggle_collapses_and_expands() {
        Owner::new().run(|| {
            let s = RowDissectionState::new(rows());
            s.select(Some(0));
            let full = s.node_count();
            // Collapse the headers branch → its 2 children drop out.
            s.toggle_path("headers");
            assert_eq!(
                s.node_count(),
                full - 2,
                "collapsing headers hides its children"
            );
            // Re-expand → back to full.
            s.toggle_path("headers");
            assert_eq!(s.node_count(), full);
            // Explicit collapse of tags → its 2 children drop out.
            s.set_expanded_path("tags", false);
            assert_eq!(s.node_count(), full - 2);
            // A leaf / unknown path is a no-op.
            s.toggle_path("method");
            s.toggle_path("no-such-path");
            assert_eq!(s.node_count(), full - 2);
        });
    }

    #[test]
    fn external_query_surfaces_selection_and_nodes() {
        Owner::new().run(|| {
            let mut e = ext();
            assert_eq!(e.query("row_count"), Ok(IntrospectValue::Int(3)));
            assert_eq!(e.query("selected"), Ok(IntrospectValue::Null));
            assert_eq!(e.query("node_count"), Ok(IntrospectValue::Int(0)));

            e.invoke("select", IntrospectValue::Int(0)).unwrap();
            assert_eq!(e.query("selected"), Ok(IntrospectValue::Int(0)));
            assert_eq!(e.query("node_count"), Ok(IntrospectValue::Int(11)));

            // Row 0, alphabetical field order: headers, latency, method, note,
            // secure, status, tags. headers is index 0 and is expanded.
            assert_eq!(
                e.query("name.0"),
                Ok(IntrospectValue::Text("headers".into()))
            );
            assert_eq!(
                e.query("kind.0"),
                Ok(IntrospectValue::Text("object".into()))
            );
            assert_eq!(e.query("depth.0"), Ok(IntrospectValue::Int(0)));
            assert_eq!(
                e.query("path.1"),
                Ok(IntrospectValue::Text("headers.accept".into()))
            );
            assert_eq!(e.query("depth.1"), Ok(IntrospectValue::Int(1)));
            assert_eq!(e.query("value.1"), Ok(IntrospectValue::Text("*/*".into())));
            assert_eq!(
                e.query("name.99"),
                Ok(IntrospectValue::Null),
                "out-of-range node is present-but-empty",
            );
            assert_eq!(
                e.query("nope"),
                Err(ReadRefusal::UnknownPath),
                "undeclared path is genuinely absent"
            );
        });
    }

    #[test]
    fn external_invoke_select_toggle_expand_collapse_clear() {
        Owner::new().run(|| {
            let mut e = ext();
            assert_eq!(
                e.invoke("select", IntrospectValue::Int(0)),
                Ok(IntrospectValue::Int(11)),
                "select returns node_count",
            );
            // Collapse headers (index 0): 11 - 2 children = 9.
            assert_eq!(
                e.invoke("collapse", IntrospectValue::Text("headers".into())),
                Ok(IntrospectValue::Int(9)),
            );
            assert_eq!(
                e.invoke("expand", IntrospectValue::Text("headers".into())),
                Ok(IntrospectValue::Int(11)),
            );
            assert_eq!(
                e.invoke("toggle", IntrospectValue::Text("tags".into())),
                Ok(IntrospectValue::Int(9)),
                "toggle collapses tags",
            );
            assert_eq!(
                e.invoke("clear", IntrospectValue::Null),
                Ok(IntrospectValue::Int(0)),
                "clear deselects",
            );
            assert_eq!(e.query("selected"), Ok(IntrospectValue::Null));
            assert_eq!(
                e.invoke("bogus", IntrospectValue::Null),
                Err(InvokeError::UnknownPath)
            );
            assert_eq!(
                e.invoke("select", IntrospectValue::Text("x".into())),
                Err(InvokeError::TypeMismatch),
            );
        });
    }

    #[test]
    fn external_intervene_selects_and_guards_readonly() {
        Owner::new().run(|| {
            let mut e = ext();
            e.intervene("selected", IntrospectValue::Int(1))
                .expect("select row 1");
            assert_eq!(e.state().selected(), Some(1));
            // Row 1 is { method, status } → 2 visible nodes.
            assert_eq!(e.query("node_count"), Ok(IntrospectValue::Int(2)));
            e.intervene("selected", IntrospectValue::Null)
                .expect("deselect");
            assert_eq!(e.state().selected(), None);
            assert_eq!(
                e.intervene("node_count", IntrospectValue::Int(1)),
                Err(InterveneError::ReadOnly),
            );
            assert_eq!(
                e.intervene("selected", IntrospectValue::Text("x".into())),
                Err(InterveneError::TypeMismatch),
            );
            assert_eq!(
                e.intervene("nope", IntrospectValue::Null),
                Err(InterveneError::UnknownPath),
            );
        });
    }
}
