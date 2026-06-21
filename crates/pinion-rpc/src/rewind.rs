//! `scene/rewind` RPC method dispatch (§5.12 method 5 of 7, R16 slice 12,
//! R42 nested External addressing).
//!
//! Mutating counterpart to [`crate::query`]: takes the same
//! `/[window[id]/][<scene_segments>/]external/<introspect_path>` shape
//! (R42 nested) and writes `value` to the addressed slot via
//! [`ExternalIntrospect::intervene`](pinion_core::external::ExternalIntrospect::intervene).
//!
//! Combined with `scene/query`, this is the symbolic snapshot/restore
//! primitive that anchors §5.8 `dry_run` once an engine-level hook is
//! wired in; for now it is the standalone state-write half of the RPC
//! surface.

use pinion_core::Scene;
use pinion_core::external::{InterveneError, IntrospectValue};

use crate::path::PathError;
use crate::resolve::{ResolveExternalError, resolve_external_introspect_mut};

/// Reasons [`rewind`] can fail.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RewindError {
    /// Window-prefix parsing failed.
    Path(PathError),
    /// Scene path does not match a v0-supported shape.
    UnsupportedPath,
    /// Expected an `External` at the scene root, found a different
    /// primitive.
    NoExternalAtPath,
    /// External did not opt in to §5.15 item 8 introspection.
    IntrospectionOptedOut,
    /// External rejected the intervene call (unknown path, type
    /// mismatch, or read-only slot).
    Intervene(InterveneError),
}

impl From<PathError> for RewindError {
    fn from(err: PathError) -> Self {
        RewindError::Path(err)
    }
}

impl From<ResolveExternalError> for RewindError {
    fn from(err: ResolveExternalError) -> Self {
        match err {
            ResolveExternalError::Path(e) => RewindError::Path(e),
            ResolveExternalError::UnsupportedPath => RewindError::UnsupportedPath,
            ResolveExternalError::NoExternalAtPath => RewindError::NoExternalAtPath,
            ResolveExternalError::IntrospectionOptedOut => RewindError::IntrospectionOptedOut,
        }
    }
}

/// Write `value` to the slot addressed by `raw_path`.
///
/// # Errors
///
/// Returns [`RewindError`] when the path is malformed, the scene root
/// is not an `External`, introspection is opted out, or the underlying
/// `intervene` call rejects the write.
pub fn rewind(
    scene: &mut Scene,
    raw_path: &str,
    value: IntrospectValue,
) -> Result<(), RewindError> {
    // R667 §5.34 — `/window[id]/<segs>/external/<intro>` parse +
    // Container/Scroll walk + multi-widget primary descent + §5.15
    // introspect-mut lookup lifted into [`resolve_external_introspect_mut`].
    let (intro, introspect_path) = resolve_external_introspect_mut(scene, raw_path)?;
    intro
        .intervene(&introspect_path, value)
        .map_err(RewindError::Intervene)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::Color;
    use pinion_core::external::{CountedExternal, StubExternal};
    use pinion_core::scene::{BoxNode, ExternalNode, Rect};

    use crate::query::query;

    fn counted_scene(n: i64) -> Scene {
        Scene::External(ExternalNode::new(Box::new(CountedExternal::new(n))))
    }

    #[test]
    fn rewind_then_query_round_trips() {
        let mut scene = counted_scene(0);
        rewind(&mut scene, "/external/count", IntrospectValue::Int(99)).unwrap();
        assert_eq!(
            query(&scene, "/external/count").unwrap(),
            IntrospectValue::Int(99),
        );
    }

    #[test]
    fn window_prefix_short_circuits() {
        let mut scene = counted_scene(0);
        rewind(
            &mut scene,
            "/window[main]/external/count",
            IntrospectValue::Int(7),
        )
        .unwrap();
        assert_eq!(
            query(&scene, "/external/count").unwrap(),
            IntrospectValue::Int(7),
        );
    }

    #[test]
    fn stub_external_opts_out() {
        let mut scene = Scene::External(ExternalNode::new(Box::new(StubExternal::new())));
        let err = rewind(&mut scene, "/external/anything", IntrospectValue::Int(0)).unwrap_err();
        assert_eq!(err, RewindError::IntrospectionOptedOut);
    }

    #[test]
    fn box_at_root_rejected() {
        let mut scene = Scene::Box(BoxNode::filled(Rect::default(), Color::default()));
        let err = rewind(&mut scene, "/external/count", IntrospectValue::Int(0)).unwrap_err();
        assert_eq!(err, RewindError::NoExternalAtPath);
    }

    #[test]
    fn type_mismatch_propagates() {
        let mut scene = counted_scene(0);
        let err = rewind(&mut scene, "/external/count", IntrospectValue::Bool(true)).unwrap_err();
        assert_eq!(err, RewindError::Intervene(InterveneError::TypeMismatch));
    }

    #[test]
    fn unknown_introspect_path_propagates() {
        let mut scene = counted_scene(0);
        let err = rewind(&mut scene, "/external/ghost", IntrospectValue::Int(1)).unwrap_err();
        assert_eq!(err, RewindError::Intervene(InterveneError::UnknownPath));
    }

    #[test]
    fn unsupported_scene_path_rejected() {
        let mut scene = counted_scene(0);
        let err = rewind(&mut scene, "/some/other/shape", IntrospectValue::Int(0)).unwrap_err();
        assert_eq!(err, RewindError::UnsupportedPath);
    }

    #[test]
    fn malformed_window_prefix_surfaces_as_path_error() {
        let mut scene = counted_scene(0);
        let err = rewind(
            &mut scene,
            "/window[main/external/count",
            IntrospectValue::Int(0),
        )
        .unwrap_err();
        assert_eq!(err, RewindError::Path(PathError::MalformedPrefix));
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
    fn rewind_nested_external_by_tag_then_query_round_trips() {
        // R42 path walker debt repayment: scene root is Container
        // holding a tagged ExternalNode. /counter/external/count
        // walks the tree, finds External, intervenes. Query confirms.
        let mut scene = container_with_nested_counted("counter", 0);
        rewind(
            &mut scene,
            "/counter/external/count",
            IntrospectValue::Int(99),
        )
        .unwrap();
        assert_eq!(
            query(&scene, "/counter/external/count").unwrap(),
            IntrospectValue::Int(99),
        );
    }

    #[test]
    fn rewind_nested_external_with_window_prefix() {
        let mut scene = container_with_nested_counted("counter", 0);
        rewind(
            &mut scene,
            "/window[main]/counter/external/count",
            IntrospectValue::Int(11),
        )
        .unwrap();
        assert_eq!(
            query(&scene, "/counter/external/count").unwrap(),
            IntrospectValue::Int(11),
        );
    }

    #[test]
    fn rewind_nested_unknown_segment_is_no_external() {
        let mut scene = container_with_nested_counted("counter", 0);
        let err = rewind(&mut scene, "/ghost/external/count", IntrospectValue::Int(0)).unwrap_err();
        assert_eq!(err, RewindError::NoExternalAtPath);
    }

    #[test]
    fn rewind_nested_non_external_target_is_no_external() {
        use pinion_core::scene::{BoxNode as BNode, ContainerNode, Rect};
        let child = Scene::Box(BNode::filled(Rect::default(), Color::default()).with_tag("info"));
        let mut c = ContainerNode::new(vec![child]);
        c.rect = Rect::new(0, 0, 100, 100);
        let mut scene = Scene::Container(c);
        let err = rewind(&mut scene, "/info/external/count", IntrospectValue::Int(0)).unwrap_err();
        assert_eq!(err, RewindError::NoExternalAtPath);
    }
}
