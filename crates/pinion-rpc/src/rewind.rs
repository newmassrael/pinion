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

use pinion_core::external::{InterveneError, IntrospectValue};
use pinion_core::Scene;

use crate::path::{self, PathError};

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
    let resolved = path::resolve(raw_path)?;
    let _ = resolved.window;

    // R42: split at the `/external/` separator. Empty scene segments
    // = root-External (v0 shape); non-empty = walk Container/Box
    // chain through `lookup_path_mut` to reach the nested
    // ExternalNode before intervening. introspect_path owned because
    // the &mut borrow of scene below would otherwise outlive the
    // resolved.scene_path borrow.
    let (scene_segments, introspect_path) = path::split_at_external(resolved.scene_path)
        .map(|(segs, intro)| (segs, intro.to_string()))
        .ok_or(RewindError::UnsupportedPath)?;

    let target = scene
        .lookup_path_mut(&scene_segments)
        .ok_or(RewindError::NoExternalAtPath)?;

    let Scene::External(node) = target else {
        return Err(RewindError::NoExternalAtPath);
    };

    let intro = node
        .handle
        .introspect_mut()
        .ok_or(RewindError::IntrospectionOptedOut)?;

    intro
        .intervene(&introspect_path, value)
        .map_err(RewindError::Intervene)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::external::{CountedExternal, StubExternal};
    use pinion_core::scene::{BoxNode, ExternalNode, Rect};
    use pinion_core::Color;

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
        let ext = Scene::External(
            ExternalNode::new(Box::new(CountedExternal::new(count))).with_tag(tag),
        );
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
        rewind(&mut scene, "/counter/external/count", IntrospectValue::Int(99)).unwrap();
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
        let err = rewind(
            &mut scene,
            "/ghost/external/count",
            IntrospectValue::Int(0),
        )
        .unwrap_err();
        assert_eq!(err, RewindError::NoExternalAtPath);
    }

    #[test]
    fn rewind_nested_non_external_target_is_no_external() {
        use pinion_core::scene::{BoxNode as BNode, ContainerNode, Rect};
        let child = Scene::Box(
            BNode::filled(Rect::default(), Color::default()).with_tag("info"),
        );
        let mut c = ContainerNode::new(vec![child]);
        c.rect = Rect::new(0, 0, 100, 100);
        let mut scene = Scene::Container(c);
        let err = rewind(
            &mut scene,
            "/info/external/count",
            IntrospectValue::Int(0),
        )
        .unwrap_err();
        assert_eq!(err, RewindError::NoExternalAtPath);
    }
}
