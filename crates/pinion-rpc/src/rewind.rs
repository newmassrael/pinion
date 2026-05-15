//! `scene/rewind` RPC method dispatch (§5.12 method 5 of 7, R16 slice 12).
//!
//! Mutating counterpart to [`crate::query`]: takes the same
//! `/[window[id]/]external/<introspect_path>` shape and writes `value`
//! to the addressed slot via
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

    let introspect_path = resolved
        .scene_path
        .strip_prefix("/external/")
        .ok_or(RewindError::UnsupportedPath)?
        .to_string();

    let Scene::External(node) = scene else {
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
    use pinion_core::scene::{BoxNode, ExternalNode};

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
        let mut scene = Scene::Box(BoxNode::new());
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
}
