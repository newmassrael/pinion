//! `scene/invoke` RPC method dispatch — R17 bidirectional RPC spec
//! round, eighth typed handler extending the §5.12 set from 7 to 8.
//!
//! Wires the same three pieces as [`crate::query`]:
//!
//!   1. **§5.18 path resolution** — `/[window[id]/]<scene_path>`
//!      with single-window short-circuit.
//!   2. **§5.2 scene tree walk** — v0 only the root is considered.
//!   3. **§5.15 item 8 invoke dispatch** — when the path resolves to
//!      a `Scene::External` root, the call descends through
//!      [`External::introspect_mut`] and consults the
//!      [`ExternalIntrospect::invoke`] action channel.
//!
//! v0 scene-path syntax accepted today: `/external/<action_path>`
//! (optionally preceded by `/window[id]/`). Other shapes return
//! [`InvokeError::UnsupportedPath`].
//!
//! Transport (JSON-RPC 2.0 framing per §5.7) is handled by
//! [`crate::dispatch`]; this module exposes the typed dispatcher
//! only.

use pinion_core::external::{ExternalIntrospect, IntrospectValue, InvokeError as TraitInvokeError};
use pinion_core::Scene;

use crate::path::{self, PathError};

/// Reasons the typed [`invoke`] dispatcher can fail.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvokeError {
    /// Window-prefix parsing failed (see [`PathError`]).
    Path(PathError),
    /// Scene path does not match a v0-supported shape.
    UnsupportedPath,
    /// Path expected an `External` at the scene root, found a different
    /// primitive.
    NoExternalAtPath,
    /// The `External` did not opt in to §5.15 item 8 introspection
    /// (so the invoke channel is unreachable).
    IntrospectionOptedOut,
    /// The action path is not declared in the `External`'s schema.
    UnknownInvokePath,
    /// The args variant does not match the action's declared type.
    InvokeTypeMismatch,
    /// The action declined to fire (preconditions unmet, etc.).
    InvokeRejected,
}

impl From<PathError> for InvokeError {
    fn from(err: PathError) -> Self {
        InvokeError::Path(err)
    }
}

impl From<TraitInvokeError> for InvokeError {
    fn from(err: TraitInvokeError) -> Self {
        // `TraitInvokeError` is `#[non_exhaustive]`; future trait
        // variants collapse into `InvokeRejected` via the wildcard
        // until a follow-up slice maps them explicitly.
        match err {
            TraitInvokeError::UnknownPath => InvokeError::UnknownInvokePath,
            TraitInvokeError::TypeMismatch => InvokeError::InvokeTypeMismatch,
            _ => InvokeError::InvokeRejected,
        }
    }
}

/// Resolve `raw_path` against `scene` and invoke the action there
/// with `args`, returning the action's typed result value.
///
/// See module docs for the v0 path syntax. The `scene` reference is
/// borrowed mutably for the lifetime of the call so the underlying
/// `External` can advance its state.
///
/// # Errors
///
/// Returns [`InvokeError`] when the path is malformed, the scene root
/// does not match the path shape, or the underlying `External`
/// rejects the action.
pub fn invoke(
    scene: &mut Scene,
    raw_path: &str,
    args: IntrospectValue,
) -> Result<IntrospectValue, InvokeError> {
    let resolved = path::resolve(raw_path)?;
    let _ = resolved.window;

    let action_path = resolved
        .scene_path
        .strip_prefix("/external/")
        .ok_or(InvokeError::UnsupportedPath)?;

    let Scene::External(node) = scene else {
        return Err(InvokeError::NoExternalAtPath);
    };

    let intro: &mut dyn ExternalIntrospect = node
        .handle
        .introspect_mut()
        .ok_or(InvokeError::IntrospectionOptedOut)?;

    intro.invoke(action_path, args).map_err(Into::into)
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
    fn counted_increment_returns_new_total() {
        let mut scene = counted_scene(5);
        let result = invoke(
            &mut scene,
            "/external/increment",
            IntrospectValue::Int(3),
        )
        .unwrap();
        assert_eq!(result, IntrospectValue::Int(8));
    }

    #[test]
    fn counted_increment_via_window_prefix() {
        let mut scene = counted_scene(0);
        let result = invoke(
            &mut scene,
            "/window[main]/external/increment",
            IntrospectValue::Int(7),
        )
        .unwrap();
        assert_eq!(result, IntrospectValue::Int(7));
    }

    #[test]
    fn stub_at_root_reports_introspection_opted_out() {
        let mut scene = Scene::External(ExternalNode::new(Box::new(StubExternal::new())));
        let err = invoke(
            &mut scene,
            "/external/anything",
            IntrospectValue::Null,
        )
        .unwrap_err();
        assert_eq!(err, InvokeError::IntrospectionOptedOut);
    }

    #[test]
    fn box_at_root_reports_no_external() {
        let mut scene = Scene::Box(BoxNode::filled(Rect::default(), Color::default()));
        let err = invoke(
            &mut scene,
            "/external/x",
            IntrospectValue::Null,
        )
        .unwrap_err();
        assert_eq!(err, InvokeError::NoExternalAtPath);
    }

    #[test]
    fn unsupported_scene_path_rejected() {
        let mut scene = counted_scene(0);
        let err = invoke(
            &mut scene,
            "/some/other/shape",
            IntrospectValue::Int(0),
        )
        .unwrap_err();
        assert_eq!(err, InvokeError::UnsupportedPath);
    }

    #[test]
    fn unknown_action_path_propagates() {
        let mut scene = counted_scene(0);
        let err = invoke(
            &mut scene,
            "/external/ghost",
            IntrospectValue::Null,
        )
        .unwrap_err();
        assert_eq!(err, InvokeError::UnknownInvokePath);
    }

    #[test]
    fn type_mismatch_propagates() {
        let mut scene = counted_scene(0);
        let err = invoke(
            &mut scene,
            "/external/increment",
            IntrospectValue::Text("nope".to_string()),
        )
        .unwrap_err();
        assert_eq!(err, InvokeError::InvokeTypeMismatch);
    }

    #[test]
    fn malformed_window_prefix_surfaces_as_path_error() {
        let mut scene = counted_scene(0);
        let err = invoke(
            &mut scene,
            "/window[main/external/increment",
            IntrospectValue::Int(1),
        )
        .unwrap_err();
        assert_eq!(err, InvokeError::Path(PathError::MalformedPrefix));
    }
}
