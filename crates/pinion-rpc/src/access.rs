//! R979 §5.40 §2 #7 — `scene/access`: the accessibility tree as data.
//!
//! Every widget already builds a WAI-ARIA [`AccessNode`] tree (the
//! `WidgetView::access_node` family), which the shell enriches with names
//! and resolves bounds for, then hands to the platform AccessKit adapter
//! for a screen reader. Until this method that tree was reachable *only*
//! through a live AT client (or an in-process unit test) — the AI-first
//! JSON-RPC path (§2 #7 "scene-as-data, queryable as text") could see the
//! paint scene and the introspect schema but **not** the a11y projection.
//! A reset button, a slider's value range, a tree row's depth — all the
//! semantics an assistive client reads — were invisible to an AI agent
//! driving pinion headlessly.
//!
//! `scene/access` closes that gap: it serializes the same enriched,
//! bounds-resolved [`AccessNode`] list (plus the [`AccessFocus`] target
//! the AT would land on) the AccessKit adapter receives, so an AI client
//! introspects the accessibility tree exactly as a screen reader would.
//! The wire vocabulary is the WAI-ARIA vocabulary itself — every role /
//! sort / autocomplete / current token is the type's own
//! [`AriaRole::aria_name`](pinion_a11y::AriaRole::aria_name) (the single
//! source of truth the AccessKit lowering also speaks), never a parallel
//! string table.
//!
//! Default-valued fields are omitted (an atomic `Button` with no set state
//! serializes to just `{tag, role}`), mirroring the snapshot serializers,
//! so the dump stays compact and a present key always carries meaning.

use pinion_a11y::{AccessFocus, AccessNode, AccessState, AccessValue};
use pinion_core::scene::Rect;
use serde_json::{Map, Value};

/// Serialize the enriched access tree (nodes + the AT focus target) to the
/// `scene/access` result envelope: `{ count, focus, nodes }`. `focus` is
/// JSON `null` when no node holds focus.
#[must_use]
pub fn access_to_json(nodes: &[AccessNode], focus: Option<&AccessFocus>) -> Value {
    let mut obj = Map::new();
    obj.insert("count".to_string(), Value::from(nodes.len()));
    obj.insert(
        "focus".to_string(),
        focus.map_or(Value::Null, access_focus_to_json),
    );
    obj.insert(
        "nodes".to_string(),
        Value::Array(nodes.iter().map(access_node_to_json).collect()),
    );
    Value::Object(obj)
}

/// Wire fields for one [`AccessNode`]. `tag` and `role` are always present;
/// every other field is emitted only when it carries a non-default value
/// (the snapshot-serializer convention), so a present key always means
/// "the binding set this".
fn access_node_to_json(node: &AccessNode) -> Value {
    let mut obj = Map::new();
    obj.insert("tag".to_string(), Value::String(node.tag.clone()));
    obj.insert(
        "role".to_string(),
        Value::String(node.role.aria_name().to_string()),
    );
    if let Some(name) = &node.name {
        obj.insert("name".to_string(), Value::String(name.clone()));
    }
    if let Some(value) = &node.value {
        obj.insert("value".to_string(), access_value_to_json(value));
    }
    if let Some(text) = &node.value_text {
        obj.insert("value_text".to_string(), Value::String(text.clone()));
    }
    if let Some(state) = access_state_to_json(node.state) {
        obj.insert("state".to_string(), state);
    }
    if let Some(bounds) = &node.bounds {
        obj.insert("bounds".to_string(), rect_to_json(*bounds));
    }
    if !node.children.is_empty() {
        obj.insert(
            "children".to_string(),
            Value::Array(
                node.children
                    .iter()
                    .map(|c| Value::String(c.clone()))
                    .collect(),
            ),
        );
    }
    if let Some(selected) = node.selected {
        obj.insert("selected".to_string(), Value::Bool(selected));
    }
    if node.multiselectable {
        obj.insert("multiselectable".to_string(), Value::Bool(true));
    }
    if let Some(level) = node.level {
        obj.insert("level".to_string(), Value::from(level));
    }
    if let Some(pos) = node.position_in_set {
        obj.insert("position_in_set".to_string(), Value::from(pos));
    }
    if let Some(size) = node.size_of_set {
        obj.insert("size_of_set".to_string(), Value::from(size));
    }
    table_axes_to_json(node, &mut obj);
    if node.modal {
        obj.insert("modal".to_string(), Value::Bool(true));
    }
    if let Some(expanded) = node.expanded {
        obj.insert("expanded".to_string(), Value::Bool(expanded));
    }
    if let Some(described_by) = &node.described_by {
        obj.insert(
            "described_by".to_string(),
            Value::String(described_by.clone()),
        );
    }
    if let Some(controls) = &node.controls {
        obj.insert("controls".to_string(), Value::String(controls.clone()));
    }
    if let Some(ac) = node.auto_complete {
        obj.insert(
            "auto_complete".to_string(),
            Value::String(ac.aria_name().to_string()),
        );
    }
    if let Some(sort) = node.sort {
        obj.insert(
            "sort".to_string(),
            Value::String(sort.aria_name().to_string()),
        );
    }
    if let Some(current) = node.current {
        obj.insert(
            "current".to_string(),
            Value::String(current.aria_name().to_string()),
        );
    }
    if let Some(has_popup) = node.has_popup {
        obj.insert(
            "haspopup".to_string(),
            Value::String(has_popup.aria_name().to_string()),
        );
    }
    // R1543 §5.40 §5.39 — the mnemonic, spelled as HTML's `accesskey` (the
    // attribute name AccessKit lowers to UIA `AccessKey`). The AI client's
    // spelling of the fact `scene/mnemonics` publishes as a map and the AT
    // receives as a node property: three readings of one declaration, none of
    // them a second source.
    if let Some(access_key) = &node.access_key {
        obj.insert("accesskey".to_string(), Value::String(access_key.clone()));
    }
    Value::Object(obj)
}

/// Wire form of an [`AccessValue`]: a single-key object whose key names the
/// variant (`bool` / `float` / `text`), so the discriminant is unambiguous
/// without a separate tag field. A `float` carries the WAI-ARIA
/// `valuenow` / `valuemin` / `valuemax` triple — the value-range that was
/// previously RPC-invisible for sliders / spin buttons / progress bars.
fn access_value_to_json(value: &AccessValue) -> Value {
    match value {
        AccessValue::Bool(b) => serde_json::json!({ "bool": b }),
        AccessValue::Float { value, min, max } => serde_json::json!({
            "float": {
                "value": finite(*value),
                "min": finite(*min),
                "max": finite(*max),
            }
        }),
        AccessValue::Text(text) => serde_json::json!({ "text": text }),
    }
}

/// Wire form of the interaction [`AccessState`]: an object carrying only
/// the flags that are set (`checked` is emitted whenever the widget has a
/// two-state value, including explicit `false`). Returns `None` when no
/// flag is set, so the caller omits the `state` key entirely.
fn access_state_to_json(state: AccessState) -> Option<Value> {
    let mut obj = Map::new();
    if state.focused {
        obj.insert("focused".to_string(), Value::Bool(true));
    }
    if state.disabled {
        obj.insert("disabled".to_string(), Value::Bool(true));
    }
    if state.hovered {
        obj.insert("hovered".to_string(), Value::Bool(true));
    }
    if state.pressed {
        obj.insert("pressed".to_string(), Value::Bool(true));
    }
    if let Some(checked) = state.checked {
        obj.insert("checked".to_string(), Value::Bool(checked));
    }
    // R1544 — `aria-checked="mixed"`, absent from this wire form since R1229
    // added the axis: the indeterminate leg reached AccessKit but not the
    // introspection surface, so an agent reading `scene/access` saw a
    // tri-state checkbox as whatever `checked` happened to say. Found while
    // adding `read_only` below, which is the same omission one round later.
    if state.mixed {
        obj.insert("mixed".to_string(), Value::Bool(true));
    }
    // R1544 — `aria-readonly`: emitted only when set, so an unmarked node
    // stays silent about editability rather than asserting "editable" (the
    // absent-vs-false distinction the property has in WAI-ARIA).
    if state.read_only {
        obj.insert("read_only".to_string(), Value::Bool(true));
    }
    (!obj.is_empty()).then_some(Value::Object(obj))
}

/// Wire form of the [`AccessFocus`] target: the focused node's `tag` plus,
/// for a composite (roving / active-descendant) widget, the `active_descendant`
/// child tag the AT virtual cursor sits on.
fn access_focus_to_json(focus: &AccessFocus) -> Value {
    let mut obj = Map::new();
    obj.insert("tag".to_string(), Value::String(focus.focus_tag.clone()));
    if let Some(child) = &focus.active_descendant {
        obj.insert(
            "active_descendant".to_string(),
            Value::String(child.clone()),
        );
    }
    Value::Object(obj)
}

/// Wire form of a resolved hit-test [`Rect`] — `{x, y, w, h}`, the same
/// shape `scene/locate_region` and the cache-stats damage region speak.
fn rect_to_json(rect: Rect) -> Value {
    serde_json::json!({ "x": rect.x, "y": rect.y, "w": rect.w, "h": rect.h })
}

/// A finite `f32` as a JSON number, or `null` for NaN / infinity (which
/// JSON cannot represent). Range widgets never carry non-finite stops, so
/// `null` is a defensive floor, not an expected value.
fn finite(f: f32) -> Value {
    serde_json::Number::from_f64(f64::from(f)).map_or(Value::Null, Value::Number)
}

/// R1560 §5.40 — the two tabular axes and the two spans, lifted out of
/// [`access_node_to_json`]'s body for the reason `lower_table_axes` was lifted
/// out of the AccessKit writer: six independent properties on one axis, and an
/// `allow` would raise this writer's length bound for every other property.
fn table_axes_to_json(node: &AccessNode, obj: &mut Map<String, Value>) {
    // R1523 §5.40 §5.27 — the column axis' extent pair. It reaches the wire for
    // the same reason `size_of_set` does, and more urgently: the RPC access
    // surface is the primary path an AI agent reads the tree through
    // (invariant #2), so a windowed column axis whose extent stopped at the
    // AccessKit lowering would be unobservable from the side pinion is built to
    // be observed from.
    if let Some(columns) = node.column_count {
        obj.insert("column_count".to_string(), Value::from(columns));
    }
    if let Some(col) = node.column_index {
        obj.insert("column_index".to_string(), Value::from(col));
    }
    // R1560 §5.40 §5.36 — the row axis and the two spans, for the same reason
    // and by the same argument. A cell that covers more than one slot and
    // reports only its origin puts every following cell at an apparent address
    // that is not the one it has, so the span is not decoration.
    if let Some(rows) = node.row_count {
        obj.insert("row_count".to_string(), Value::from(rows));
    }
    if let Some(row) = node.row_index {
        obj.insert("row_index".to_string(), Value::from(row));
    }
    if let Some(span) = node.row_span {
        obj.insert("row_span".to_string(), Value::from(span));
    }
    if let Some(span) = node.column_span {
        obj.insert("column_span".to_string(), Value::from(span));
    }
}

#[cfg(test)]
mod tests {
    use super::access_to_json;
    use pinion_a11y::{AccessFocus, AccessNode, AccessState, AccessValue, AriaRole, SortDirection};
    use pinion_core::scene::Rect;

    #[test]
    fn atomic_button_serializes_to_just_tag_and_role() {
        // Default-valued fields are omitted: an unstyled button is two keys.
        let nodes = vec![AccessNode::new("ok", AriaRole::Button)];
        let json = access_to_json(&nodes, None);
        assert_eq!(json["count"], 1);
        assert_eq!(json["focus"], serde_json::Value::Null);
        let node = &json["nodes"][0];
        assert_eq!(node["tag"], "ok");
        assert_eq!(node["role"], "button");
        // No state / value / bounds keys on a clean atomic node.
        assert!(node.get("state").is_none(), "no state key when no flag set");
        assert!(node.get("value").is_none());
        assert!(node.get("bounds").is_none());
    }

    #[test]
    fn slider_float_exposes_the_value_range() {
        // The R966 carry: a slider's valuenow / valuemin / valuemax was
        // RPC-invisible. It rides the `value` object now.
        let node = AccessNode::new("opacity", AriaRole::Slider)
            .with_value(AccessValue::Float {
                value: 0.5,
                min: 0.0,
                max: 1.0,
            })
            .with_state(AccessState {
                focused: true,
                ..AccessState::default()
            });
        let json = access_to_json(&[node], None);
        let value = &json["nodes"][0]["value"]["float"];
        assert_eq!(value["value"], 0.5);
        assert_eq!(value["min"], 0.0);
        assert_eq!(value["max"], 1.0);
        // The focus flag rides the state sub-object.
        assert_eq!(json["nodes"][0]["state"]["focused"], true);
    }

    #[test]
    fn full_field_set_round_trips_with_focus() {
        let node = AccessNode::new("row", AriaRole::TreeItem)
            .with_name("Layer")
            .with_bounds(Rect::new(4, 8, 120, 24))
            .with_selected(true)
            .with_expanded(true)
            .with_set_position(0, 3)
            .with_level(2)
            .with_column_count(200)
            .with_column(136)
            .with_sort(SortDirection::Ascending)
            .with_child("row#reset");
        let focus = AccessFocus {
            focus_tag: "grid".to_string(),
            active_descendant: Some("row".to_string()),
        };
        let json = access_to_json(&[node], Some(&focus));
        let n = &json["nodes"][0];
        assert_eq!(n["name"], "Layer");
        assert_eq!(n["role"], "treeitem");
        assert_eq!(
            n["bounds"],
            serde_json::json!({ "x": 4, "y": 8, "w": 120, "h": 24 })
        );
        assert_eq!(n["selected"], true);
        assert_eq!(n["expanded"], true);
        assert_eq!(n["level"], 2);
        assert_eq!(n["position_in_set"], 1);
        assert_eq!(n["size_of_set"], 3);
        // R1523 — the column axis' extent pair reaches the wire. Asserted here
        // rather than only in a grid demo because this serializer hand-writes
        // every field: a new `AccessNode` field is absent from the RPC surface
        // until someone adds a line, and this test is the only place that
        // notices.
        assert_eq!(n["column_count"], 200);
        assert_eq!(
            n["column_index"], 137,
            "one-based aria-colindex on the wire"
        );
        assert_eq!(n["sort"], "ascending");
        assert_eq!(n["children"][0], "row#reset");
        assert_eq!(json["focus"]["tag"], "grid");
        assert_eq!(json["focus"]["active_descendant"], "row");
    }
}
