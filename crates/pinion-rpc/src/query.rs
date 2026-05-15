//! `scene/query` RPC method dispatch (§5.12 hybrid query, R16 slice 9).
//!
//! Wires three pieces together:
//!
//!   1. **§5.18 path resolution** — `/[window[id]/]<scene_path>` with
//!      single-window short-circuit (see [`crate::path`]).
//!   2. **§5.2 scene tree walk** — currently only the *root* is
//!      considered; full addressing of nested scene nodes lands when
//!      §5.3 DSL settles a path syntax for the introspectable variants.
//!   3. **§5.15 item 8 introspect dispatch** — when the path resolves to
//!      a `Scene::External` root, the call descends through
//!      [`External::introspect`](pinion_core::external::External::introspect)
//!      and consults the [`ExternalIntrospect`] surface.
//!
//! v0 scene-path syntax accepted today: `/external/<introspect_path>`
//! (optionally preceded by `/window[id]/`). Other shapes return
//! [`QueryError::UnsupportedPath`]; that error variant is the carry-forward
//! marker for §5.3 DSL adding richer addressing.
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

    let introspect_path = resolved
        .scene_path
        .strip_prefix("/external/")
        .ok_or(QueryError::UnsupportedPath)?;

    let Scene::External(node) = scene else {
        return Err(QueryError::NoExternalAtPath);
    };

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
    use pinion_core::scene::{BoxNode, ExternalNode};

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
        let scene = Scene::Box(BoxNode::new());
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
}
