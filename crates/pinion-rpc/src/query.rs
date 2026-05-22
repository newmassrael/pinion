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

use crate::path::{self, PathError};

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
    let resolved = path::resolve(raw_path)?;
    // §5.17 §5.18: `resolved.window` is the routed window. Multi-window
    // scene addressing lands in a later slice; today the scene argument
    // *is* the resolved window's root, so we ignore the WindowId here.
    let _ = resolved.window;

    // R42: split at the `/external/` separator. Empty scene segments
    // = root-External (v0 shape); non-empty = walk Container/Box
    // chain to find a nested ExternalNode before introspecting.
    let (scene_segments, introspect_path) =
        path::split_at_external(resolved.scene_path).ok_or(QueryError::UnsupportedPath)?;

    let target = scene
        .lookup_path_ref(&scene_segments)
        .ok_or(QueryError::NoExternalAtPath)?;

    // (R55.D.5 §5.45) `target` is the substrate's state-scene root —
    // either `Scene::External(primary)` (single-widget shape) or
    // `Scene::Container([primary, ...extras])` when the binding
    // overrides `create_extra_externals`. `primary_external` descends
    // to the first `ExternalNode` in DFS pre-order so an
    // `external/<action>` path against either shape resolves to the
    // primary widget without per-binding disambiguation.
    let node = target
        .primary_external()
        .ok_or(QueryError::NoExternalAtPath)?;

    let intro: &dyn ExternalIntrospect = node
        .handle
        .introspect()
        .ok_or(QueryError::IntrospectionOptedOut)?;

    intro
        .query(introspect_path)
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
}
