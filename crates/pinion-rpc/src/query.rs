//! `scene/query` RPC method dispatch (§5.12 hybrid query, R16 slice 9,
//! R42 nested External addressing).
//!
//! Wires three pieces together:
//!
//!   1. **§5.18 path resolution** — `/[window[id]/]<scene_path>` with
//!      single-window short-circuit (see [`crate::path`]).
//!   2. **§5.2 scene tree walk** — R42: the `/external/` literal acts
//!      as the separator between scene-walk segments and the introspect
//!      path. `/external/<intro>` keeps the v0 root-External shape;
//!      `/<seg>/.../external/<intro>` walks Container/Box descendants
//!      to find an `ExternalNode` before descending.
//!   3. **§5.15 item 8 introspect dispatch** — when the resolved
//!      target is an `ExternalNode`, the call descends through
//!      [`External::introspect`](pinion_core::external::External::introspect)
//!      and consults the [`ExternalIntrospect`] surface.
//!
//! Scene-path syntax accepted (R42): `/[<scene_segments>/]external/<introspect_path>`,
//! optionally preceded by `/window[id]/`. Other shapes return
//! [`QueryError::UnsupportedPath`].
//!
//! Transport (JSON-RPC 2.0 framing per §5.7) is a separate slice — this
//! module exposes the typed dispatcher only.

use pinion_core::external::{ExternalIntrospect, IntrospectValue};
use pinion_core::Scene;
use serde_json::{json, Value};

use crate::path::PathError;
use crate::resolve::{resolve_external_introspect, ResolveExternalError};

/// R825 §5.12 — reserved introspect path that returns an external's
/// **declared schema** (every queryable path + its type tag) instead of a
/// value. The discovery primitive under the whole introspection surface: a
/// plain `scene/query` reads one *known* path, and `scene/snapshot` shows
/// the current value of each *scalar* path — but neither reveals the
/// **contract** (the parametric paths like `id_at` / `level_at`, which
/// `query("id_at")` without a `.<pos>` index resolves to `None`, so they
/// never appear in a snapshot). Querying `/<tag>/external/$schema` returns
/// the full `IntrospectSchema` as JSON, so an AI client discovers what it
/// can ask for without hard-coded knowledge ([[ai-first-rpc-introspection-obligation]]).
/// The `$` prefix cannot collide with a real introspect path (no widget
/// declares one).
pub const SCHEMA_PATH: &str = "$schema";

/// Render an external's declared schema as a JSON array of
/// `{"path", "type"}` objects, in the schema's declared field order.
fn schema_value(intro: &dyn ExternalIntrospect) -> IntrospectValue {
    let fields: Vec<Value> = intro
        .schema()
        .fields
        .iter()
        .map(|(path, ty)| json!({ "path": path, "type": ty }))
        .collect();
    IntrospectValue::Json(Value::Array(fields))
}

/// Reasons the typed [`query`] dispatcher can fail.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryError {
    /// Window-prefix parsing failed (see [`PathError`]).
    Path(PathError),
    /// Scene path does not match a v0-supported shape.
    UnsupportedPath,
    /// Path expected an `External` at the scene root, found a different
    /// primitive.
    NoExternalAtPath,
    /// The `External` did not opt in to §5.15 item 8 introspection.
    IntrospectionOptedOut,
    /// `External` opted in, but the introspect path is not in its schema.
    UnknownIntrospectPath,
}

impl From<PathError> for QueryError {
    fn from(err: PathError) -> Self {
        QueryError::Path(err)
    }
}

impl From<ResolveExternalError> for QueryError {
    fn from(err: ResolveExternalError) -> Self {
        match err {
            ResolveExternalError::Path(e) => QueryError::Path(e),
            ResolveExternalError::UnsupportedPath => QueryError::UnsupportedPath,
            ResolveExternalError::NoExternalAtPath => QueryError::NoExternalAtPath,
            ResolveExternalError::IntrospectionOptedOut => QueryError::IntrospectionOptedOut,
        }
    }
}

/// Resolve `raw_path` against `scene` and return the queried value.
///
/// See module docs for the v0 path syntax. The `scene` reference is
/// borrowed for the lifetime of the call; no scene mutation occurs.
///
/// # Errors
///
/// Returns [`QueryError`] when the path is malformed, the scene root
/// does not match the path shape, or the underlying `External` rejects
/// the introspect path.
pub fn query(scene: &Scene, raw_path: &str) -> Result<IntrospectValue, QueryError> {
    // R667 §5.34 — `/window[id]/<segs>/external/<intro>` parse +
    // Container/Scroll walk + multi-widget primary descent + §5.15
    // introspect lookup lifted into [`resolve_external_introspect`].
    let (intro, introspect_path) = resolve_external_introspect(scene, raw_path)?;
    // R825 — the reserved `$schema` path returns the declared schema rather
    // than a value (the discovery primitive). Intercepted after resolution
    // so an opted-out external still reports `IntrospectionOptedOut`.
    if introspect_path == SCHEMA_PATH {
        return Ok(schema_value(intro));
    }
    intro
        .query(&introspect_path)
        .ok_or(QueryError::UnknownIntrospectPath)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::external::{CountedExternal, StubExternal};
    use pinion_core::scene::{BoxNode, ExternalNode, Rect};
    use pinion_core::Color;

    fn counted_scene(n: i64) -> Scene {
        Scene::External(ExternalNode::new(Box::new(CountedExternal::new(n))))
    }

    #[test]
    fn introspect_count_via_short_circuit_path() {
        let scene = counted_scene(7);
        assert_eq!(
            query(&scene, "/external/count").unwrap(),
            IntrospectValue::Int(7),
        );
    }

    #[test]
    fn introspect_count_via_explicit_window_prefix() {
        let scene = counted_scene(11);
        assert_eq!(
            query(&scene, "/window[main]/external/count").unwrap(),
            IntrospectValue::Int(11),
        );
    }

    #[test]
    fn stub_at_root_reports_introspection_opted_out() {
        let scene = Scene::External(ExternalNode::new(Box::new(StubExternal::new())));
        assert_eq!(
            query(&scene, "/external/count").unwrap_err(),
            QueryError::IntrospectionOptedOut,
        );
    }

    #[test]
    fn box_at_root_reports_no_external() {
        let scene = Scene::Box(BoxNode::filled(Rect::default(), Color::default()));
        assert_eq!(
            query(&scene, "/external/count").unwrap_err(),
            QueryError::NoExternalAtPath,
        );
    }

    #[test]
    fn unsupported_scene_path_rejected() {
        let scene = counted_scene(0);
        assert_eq!(
            query(&scene, "/some/other/shape").unwrap_err(),
            QueryError::UnsupportedPath,
        );
    }

    #[test]
    fn unknown_introspect_path_propagates() {
        let scene = counted_scene(0);
        assert_eq!(
            query(&scene, "/external/ghost").unwrap_err(),
            QueryError::UnknownIntrospectPath,
        );
    }

    #[test]
    fn malformed_window_prefix_surfaces_as_path_error() {
        let scene = counted_scene(0);
        assert_eq!(
            query(&scene, "/window[main/external/count").unwrap_err(),
            QueryError::Path(PathError::MalformedPrefix),
        );
    }

    // ---- §5.34 R42: nested External addressing ----

    fn container_with_nested_counted(tag: &'static str, count: i64) -> Scene {
        use pinion_core::scene::{ContainerNode, Rect};
        let ext = Scene::External(
            ExternalNode::new(Box::new(CountedExternal::new(count))).with_tag(tag),
        );
        let mut c = ContainerNode::new(vec![ext]);
        c.rect = Rect::new(0, 0, 100, 100);
        Scene::Container(c)
    }

    #[test]
    fn query_nested_external_by_tag() {
        // R42: scene root is a Container holding a tagged ExternalNode.
        // /counter/external/count walks "counter" → finds External →
        // introspect "count". Path walker extension prevents R40.8's
        // state/paint scene workaround.
        let scene = container_with_nested_counted("counter", 42);
        assert_eq!(
            query(&scene, "/counter/external/count").unwrap(),
            IntrospectValue::Int(42),
        );
    }

    #[test]
    fn query_nested_external_with_window_prefix() {
        let scene = container_with_nested_counted("counter", 7);
        assert_eq!(
            query(&scene, "/window[main]/counter/external/count").unwrap(),
            IntrospectValue::Int(7),
        );
    }

    #[test]
    fn query_nested_external_by_index() {
        // Untagged ExternalNode addressable via positional index.
        use pinion_core::scene::{ContainerNode, ExternalNode as ExtNode, Rect};
        let ext = Scene::External(ExtNode::new(Box::new(CountedExternal::new(5))));
        let mut c = ContainerNode::new(vec![ext]);
        c.rect = Rect::new(0, 0, 100, 100);
        let scene = Scene::Container(c);
        assert_eq!(
            query(&scene, "/0/external/count").unwrap(),
            IntrospectValue::Int(5),
        );
    }

    #[test]
    fn query_nested_unknown_segment_is_no_external_at_path() {
        let scene = container_with_nested_counted("counter", 0);
        assert_eq!(
            query(&scene, "/ghost/external/count").unwrap_err(),
            QueryError::NoExternalAtPath,
        );
    }

    // ─────────────────────────────────────────────────────────────────
    // R55.D.8 §5.45 §5.7 — RPC tag-addressable path against the
    // multi-External state-scene wrap that `CoreShell::new` composes
    // when `WidgetCore::create_extra_externals` is non-empty. The
    // R55.D.5 carry-forward bullet "scene/<tag>/external/<action>
    // RPC path syntax for addressing extra Externals by name" was
    // already covered by the §5.34 R42 nested-External walker
    // (`Scene::lookup_path_ref` matches children by tag); these
    // tests pin the contract on the canonical R55.D.5 wrap shape
    // (`Scene::Container([External, External])`) so a future
    // regression surfaces against that shape specifically.
    // ─────────────────────────────────────────────────────────────────

    fn multi_external_wrap(primary_tag: &'static str, extra_tag: &'static str) -> Scene {
        // Mirror the substrate composition `CoreShell::new` produces:
        // `Container([External(primary), External(extra)])`. Both
        // children carry `CountedExternal` so we can query and tell
        // them apart by initial count.
        use pinion_core::scene::{ContainerNode, ExternalNode as ExtNode, Rect};
        let primary =
            Scene::External(ExtNode::new(Box::new(CountedExternal::new(11))).with_tag(primary_tag));
        let extra =
            Scene::External(ExtNode::new(Box::new(CountedExternal::new(22))).with_tag(extra_tag));
        let mut c = ContainerNode::new(vec![primary, extra]);
        c.rect = Rect::new(0, 0, 100, 100);
        Scene::Container(c)
    }

    #[test]
    fn r55_d8_external_root_resolves_primary_on_wrap() {
        // R55.D.8 — `external/<action>` against the wrap descends to
        // the first External (substrate's primary by convention).
        let scene = multi_external_wrap("primary", "extra");
        assert_eq!(
            query(&scene, "/external/count").unwrap(),
            IntrospectValue::Int(11),
            "bare external/ resolves to primary (first in DFS pre-order)",
        );
    }

    #[test]
    fn r55_d8_tag_path_resolves_extra_external() {
        // R55.D.8 — `<extra_tag>/external/<action>` walks the wrap
        // Container by the named child's tag and reaches the matching
        // External. The trailing `primary_external` descent on
        // `Scene::External` returns self, so the tagged sibling is
        // queryable by symbolic name (the AI-introspection contract
        // §5.7 needs to surface sibling state).
        let scene = multi_external_wrap("primary", "extra");
        assert_eq!(
            query(&scene, "/extra/external/count").unwrap(),
            IntrospectValue::Int(22),
            "extra/external/ resolves to the tagged sibling",
        );
    }

    #[test]
    fn r55_d8_tag_path_resolves_primary_by_name() {
        // R55.D.8 — addressing the primary by its tag also works:
        // `primary/external/<action>` reaches the same node the bare
        // `external/<action>` short-circuit does, but via the typed
        // path. AI clients can use the symbolic form uniformly.
        let scene = multi_external_wrap("primary", "extra");
        assert_eq!(
            query(&scene, "/primary/external/count").unwrap(),
            IntrospectValue::Int(11),
            "primary/external/ resolves to the primary via tag walk",
        );
    }

    #[test]
    fn query_nested_non_external_target_is_no_external_at_path() {
        // Walk lands on a Box (not External) → reject.
        use pinion_core::scene::{BoxNode, ContainerNode, Rect};
        let child = Scene::Box(
            BoxNode::filled(Rect::default(), Color::default()).with_tag("info"),
        );
        let mut c = ContainerNode::new(vec![child]);
        c.rect = Rect::new(0, 0, 100, 100);
        let scene = Scene::Container(c);
        assert_eq!(
            query(&scene, "/info/external/count").unwrap_err(),
            QueryError::NoExternalAtPath,
        );
    }

    #[test]
    fn schema_path_returns_declared_fields_as_json() {
        // `$schema` returns the contract (paths + types), not a value.
        let scene = counted_scene(3);
        assert_eq!(
            query(&scene, "/external/$schema").unwrap(),
            IntrospectValue::Json(serde_json::json!([{ "path": "count", "type": "int" }])),
        );
    }

    #[test]
    fn schema_path_resolves_through_the_window_prefix() {
        let scene = counted_scene(0);
        assert_eq!(
            query(&scene, "/window[main]/external/$schema").unwrap(),
            IntrospectValue::Json(serde_json::json!([{ "path": "count", "type": "int" }])),
        );
    }

    #[test]
    fn schema_path_on_opted_out_external_reports_opted_out() {
        // An external with no introspect surface has no schema to report —
        // resolution fails before the `$schema` interception.
        let scene = Scene::External(ExternalNode::new(Box::new(StubExternal::new())));
        assert_eq!(
            query(&scene, "/external/$schema").unwrap_err(),
            QueryError::IntrospectionOptedOut,
        );
    }
}
