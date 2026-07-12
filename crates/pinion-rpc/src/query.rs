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

use pinion_core::Scene;
use pinion_core::external::{ExternalIntrospect, IntrospectValue};
use serde_json::{Value, json};

use crate::path::PathError;
use crate::resolve::{ResolveExternalError, introspect_at, resolve_external_path};

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

/// R828 §5.12 — the schema-or-value decision SSOT shared by the
/// retained-`External` and immediate-mode-driver query branches. The
/// reserved `$schema` path (R825) returns the declared schema (discovery);
/// any other path reads the value, or fails [`QueryError::UnknownIntrospectPath`].
/// One site so the two branches cannot diverge on discovery support — a
/// new reserved path is handled for both at once.
fn introspect_or_schema(
    intro: &dyn ExternalIntrospect,
    introspect_path: &str,
) -> Result<IntrospectValue, QueryError> {
    if introspect_path == SCHEMA_PATH {
        Ok(schema_value(intro))
    } else {
        intro
            .query(introspect_path)
            .ok_or(QueryError::UnknownIntrospectPath)
    }
}

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
    // R667 §5.34 — `/window[id]/<segs>/external/<intro>` parse into the
    // borrow-free (scene_segments, introspect_path) pair.
    let (scene_segments, introspect_path) = resolve_external_path(raw_path)?;
    // R828 §2 #4 §5.12 — immediate-mode driver introspect branch. An
    // [`Scene::ImmediateModeNode`] holds its driver behind
    // `Rc<RefCell<dyn ImmediateMode>>` (interior mutability), so unlike a
    // `Box<dyn External>` it is only *transiently* borrowable — the
    // borrow cannot outlive this call. That lifetime seam is exactly why
    // R681 deferred the introspect consumer (atomic 4). It dissolves on
    // the read path: `query` returns an *owned* [`IntrospectValue`], so
    // we borrow the driver, read the value WITHIN scope, and drop the
    // borrow before returning — no `&dyn ExternalIntrospect` escapes the
    // `RefCell`. Mirrors the External branch's §5.15 introspect + R825
    // `$schema` semantics. (`intervene` / `invoke` return a borrow that
    // *does* need to outlive resolution, so immediate-mode mutation stays
    // deferred until a consumer needs it — driver state is tick-driven,
    // read-only like the R823 tree introspect.)
    if let Some(Scene::ImmediateModeNode(node)) = scene.lookup_path_ref(&scene_segments) {
        let driver = node.handle.borrow();
        let intro = driver
            .introspect()
            .ok_or(QueryError::IntrospectionOptedOut)?;
        return introspect_or_schema(intro, &introspect_path);
    }
    // §5.15 retained-`External` branch — `lookup_path_ref` +
    // `primary_external` + §5.15 introspect, lifted into
    // [`introspect_at`] (R667). `$schema` (R825) is intercepted after
    // resolution so an opted-out external still reports
    // `IntrospectionOptedOut` rather than a schema.
    let intro = introspect_at(scene, &scene_segments)?;
    introspect_or_schema(intro, &introspect_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::Color;
    use pinion_core::external::{CountedExternal, StubExternal};
    use pinion_core::scene::{BoxNode, ExternalNode, Rect};

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
        let ext =
            Scene::External(ExternalNode::new(Box::new(CountedExternal::new(count))).with_tag(tag));
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

    // ─────────────────────────────────────────────────────────────────
    // R1303 PR-51 §5.45 §5.7 §2 #2 §2 #7 — no-primary binding over the RPC
    // wire. `CoreShell` composes a no-primary binding's state scene as
    // `Container([extra, extra, ...])` marked no-primary-head (R1307). The
    // RPC path layer is V-agnostic (scene-as-data, §2 #7); the marker lets
    // it self-describe the absence. These pin the two contracts for that
    // shape: (a) every extra is reachable by its explicit tag — the stable
    // §2 #2 AI address a no-primary binding relies on; (b) the bare
    // `/external` shorthand (which names "the primary") REJECTS with
    // `NoExternalAtPath` because the marked container has no primary head —
    // self-describing, not a silent misroute to an arbitrary extra.
    // ─────────────────────────────────────────────────────────────────

    fn extras_only_wrap() -> Scene {
        // No primary: `Container([External(pane_a), External(pane_b)])`
        // marked no-primary-head — the exact shape
        // `compose_root(None, [a, b])` produces. Distinct counts tell the
        // two extras apart.
        use pinion_core::scene::{ContainerNode, ExternalNode as ExtNode, Rect};
        let a =
            Scene::External(ExtNode::new(Box::new(CountedExternal::new(101))).with_tag("pane_a"));
        let b =
            Scene::External(ExtNode::new(Box::new(CountedExternal::new(202))).with_tag("pane_b"));
        let mut c = ContainerNode::new(vec![a, b]).without_primary_head();
        c.rect = Rect::new(0, 0, 100, 100);
        Scene::Container(c)
    }

    #[test]
    fn r1303_no_primary_extras_reachable_by_tag() {
        // §2 #2 — a no-primary binding's every surface is addressable by
        // its explicit tag over the wire (the stable AI address), with no
        // primary present.
        let scene = extras_only_wrap();
        assert_eq!(
            query(&scene, "/pane_a/external/count").unwrap(),
            IntrospectValue::Int(101),
        );
        assert_eq!(
            query(&scene, "/pane_b/external/count").unwrap(),
            IntrospectValue::Int(202),
            "the second extra is reachable by tag with no primary present",
        );
    }

    #[test]
    fn r1303_no_primary_bare_external_rejects() {
        // (R1307) The bare `/external` shorthand names "the primary"; the
        // marked no-primary-head container has none, so `primary_external`
        // returns `None` and the query REJECTS with `NoExternalAtPath` —
        // self-describing (§2 #7) rather than silently resolving pane_a.
        let scene = extras_only_wrap();
        assert_eq!(
            query(&scene, "/external/count").unwrap_err(),
            QueryError::NoExternalAtPath,
            "bare /external rejects for a no-primary-head container",
        );
    }

    #[test]
    fn query_nested_non_external_target_is_no_external_at_path() {
        // Walk lands on a Box (not External) → reject.
        use pinion_core::scene::{BoxNode, ContainerNode, Rect};
        let child = Scene::Box(BoxNode::filled(Rect::default(), Color::default()).with_tag("info"));
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

    // ---- R828 §2 #4 §5.12 — immediate-mode driver introspect ----

    use pinion_core::external::{InterveneError, IntrospectSchema};
    use pinion_core::scene::{ContainerNode, ImmediateMode, ImmediateModeNode};

    /// Read-only immediate-mode driver exposing one `pos` float — the
    /// query peer of `CountedExternal`.
    #[derive(Debug)]
    struct PosDriver {
        pos: f64,
    }

    impl ImmediateMode for PosDriver {
        fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
            Some(self)
        }
    }

    impl ExternalIntrospect for PosDriver {
        fn schema(&self) -> IntrospectSchema {
            IntrospectSchema::new(&[("pos", "float")])
        }
        fn query(&self, path: &str) -> Option<IntrospectValue> {
            (path == "pos").then_some(IntrospectValue::Float(self.pos))
        }
        fn intervene(&mut self, _: &str, _: IntrospectValue) -> Result<(), InterveneError> {
            Err(InterveneError::ReadOnly)
        }
    }

    /// Immediate-mode driver that does NOT opt in to introspection
    /// (default `introspect()` returns `None`).
    #[derive(Debug)]
    struct OpaqueDriver;
    impl ImmediateMode for OpaqueDriver {}

    fn immediate_root(pos: f64) -> Scene {
        Scene::ImmediateModeNode(ImmediateModeNode::from_driver(
            PosDriver { pos },
            Rect::default(),
        ))
    }

    #[test]
    fn immediate_driver_query_at_root() {
        let scene = immediate_root(0.5);
        assert_eq!(
            query(&scene, "/external/pos").unwrap(),
            IntrospectValue::Float(0.5),
        );
    }

    #[test]
    fn immediate_driver_query_via_tagged_descendant() {
        let node = Scene::ImmediateModeNode(
            ImmediateModeNode::from_driver(PosDriver { pos: 0.25 }, Rect::default())
                .with_tag("ball"),
        );
        let scene = Scene::Container(ContainerNode::new(vec![node]));
        assert_eq!(
            query(&scene, "/ball/external/pos").unwrap(),
            IntrospectValue::Float(0.25),
        );
    }

    #[test]
    fn immediate_driver_schema_discovery() {
        let scene = immediate_root(0.0);
        assert_eq!(
            query(&scene, "/external/$schema").unwrap(),
            IntrospectValue::Json(serde_json::json!([{ "path": "pos", "type": "float" }])),
        );
    }

    #[test]
    fn immediate_driver_unknown_path_propagates() {
        let scene = immediate_root(0.0);
        assert_eq!(
            query(&scene, "/external/ghost").unwrap_err(),
            QueryError::UnknownIntrospectPath,
        );
    }

    #[test]
    fn immediate_driver_opted_out_reports_opted_out() {
        let scene = Scene::ImmediateModeNode(ImmediateModeNode::from_driver(
            OpaqueDriver,
            Rect::default(),
        ));
        assert_eq!(
            query(&scene, "/external/pos").unwrap_err(),
            QueryError::IntrospectionOptedOut,
        );
    }
}
