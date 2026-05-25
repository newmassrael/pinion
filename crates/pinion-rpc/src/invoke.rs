//! `scene/invoke` RPC method dispatch — R17 bidirectional RPC spec
//! round, eighth typed handler extending the §5.12 set from 7 to 8.
//!
//! Wires the same three pieces as [`crate::query`] / [`crate::rewind`]:
//!
//!   1. **§5.18 path resolution** — `/[window[id]/]<scene_path>`
//!      with single-window short-circuit.
//!   2. **§5.2 scene tree walk** — R666 §5.34 v1 multi-External
//!      addressing: `path::split_at_external` returns scene segments +
//!      introspect path; [`Scene::lookup_path_mut`] walks Container /
//!      Scroll by tag/index until it reaches the addressed
//!      [`ExternalNode`]; [`Scene::primary_external_mut`] then descends
//!      the R55.D.5 multi-widget container shape to the substrate's
//!      first External.
//!   3. **§5.15 item 8 invoke dispatch** — descend through
//!      [`External::introspect_mut`] and consult the
//!      [`ExternalIntrospect::invoke`] action channel.
//!
//! v1 scene-path syntax accepted:
//!
//!   * `/external/<action>` — primary External at root (v0 retained).
//!   * `/<tag>/external/<action>` — DFS lookup by `ExternalNode` tag.
//!     Composite tags (`todo_toggle#1`) pass through verbatim, so
//!     R55.D.5 `create_extra_externals` siblings are addressable
//!     without per-binding RPC plumbing.
//!   * `/<seg>/.../<tag>/external/<action>` — nested Container walk
//!     followed by tagged child match (mirror of [`crate::query`]).
//!
//! Other shapes return [`InvokeError::UnsupportedPath`].
//!
//! Transport (JSON-RPC 2.0 framing per §5.7) is handled by
//! [`crate::dispatch`]; this module exposes the typed dispatcher
//! only.

use pinion_core::external::{IntrospectValue, InvokeError as TraitInvokeError};
use pinion_core::Scene;

use crate::path::PathError;
use crate::resolve::{resolve_external_introspect_mut, ResolveExternalError};

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

impl From<ResolveExternalError> for InvokeError {
    fn from(err: ResolveExternalError) -> Self {
        match err {
            ResolveExternalError::Path(e) => InvokeError::Path(e),
            ResolveExternalError::UnsupportedPath => InvokeError::UnsupportedPath,
            ResolveExternalError::NoExternalAtPath => InvokeError::NoExternalAtPath,
            ResolveExternalError::IntrospectionOptedOut => InvokeError::IntrospectionOptedOut,
        }
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
    // R667 §5.34 — `/window[id]/<segs>/external/<action>` parse +
    // Container/Scroll walk + multi-widget primary descent + §5.15
    // introspect-mut lookup lifted into [`resolve_external_introspect_mut`].
    // `action_path` is owned (String) so the `&mut dyn ExternalIntrospect`
    // borrow on `scene` can outlive `raw_path`'s lifetime.
    let (intro, action_path) = resolve_external_introspect_mut(scene, raw_path)?;
    intro.invoke(&action_path, args).map_err(Into::into)
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

    // ---- R666 §5.34 v1: multi-External addressing ----

    fn container_with_nested_counted(tag: &'static str, count: i64) -> Scene {
        use pinion_core::scene::{ContainerNode, Rect};
        let ext = Scene::External(
            ExternalNode::new(Box::new(CountedExternal::new(count))).with_tag(tag),
        );
        let mut c = ContainerNode::new(vec![ext]);
        c.rect = Rect::new(0, 0, 100, 100);
        Scene::Container(c)
    }

    fn container_with_two_counted_siblings(
        primary_tag: &'static str,
        primary_count: i64,
        extra_tag: &'static str,
        extra_count: i64,
    ) -> Scene {
        use pinion_core::scene::{ContainerNode, Rect};
        let primary = Scene::External(
            ExternalNode::new(Box::new(CountedExternal::new(primary_count))).with_tag(primary_tag),
        );
        let extra = Scene::External(
            ExternalNode::new(Box::new(CountedExternal::new(extra_count))).with_tag(extra_tag),
        );
        let mut c = ContainerNode::new(vec![primary, extra]);
        c.rect = Rect::new(0, 0, 100, 100);
        Scene::Container(c)
    }

    #[test]
    fn r666_invoke_nested_external_by_tag() {
        // /counter/external/increment walks Container.children by tag,
        // finds the External with that tag, calls increment on it.
        let mut scene = container_with_nested_counted("counter", 0);
        let result = invoke(
            &mut scene,
            "/counter/external/increment",
            IntrospectValue::Int(5),
        )
        .unwrap();
        assert_eq!(result, IntrospectValue::Int(5));
    }

    #[test]
    fn r666_invoke_extra_external_by_composite_tag() {
        // R55.D.5 multi-External shape: the extra sibling carries a
        // composite tag (`todo_toggle#1` convention). v1 path syntax
        // addresses it directly without disturbing the primary.
        let mut scene = container_with_two_counted_siblings(
            "todo_list", 100, "todo_toggle#1", 0,
        );
        let result = invoke(
            &mut scene,
            "/todo_toggle#1/external/increment",
            IntrospectValue::Int(3),
        )
        .unwrap();
        assert_eq!(result, IntrospectValue::Int(3));
        // Primary untouched — v0 fallback still resolves to it.
        let primary_via_v0 = invoke(
            &mut scene,
            "/external/increment",
            IntrospectValue::Int(0),
        )
        .unwrap();
        assert_eq!(primary_via_v0, IntrospectValue::Int(100));
    }

    #[test]
    fn r666_invoke_extra_external_with_window_prefix() {
        let mut scene = container_with_two_counted_siblings(
            "primary", 0, "todo_delete#7", 0,
        );
        let result = invoke(
            &mut scene,
            "/window[main]/todo_delete#7/external/increment",
            IntrospectValue::Int(11),
        )
        .unwrap();
        assert_eq!(result, IntrospectValue::Int(11));
    }

    #[test]
    fn r666_invoke_unknown_segment_is_no_external() {
        let mut scene = container_with_nested_counted("counter", 0);
        let err = invoke(
            &mut scene,
            "/ghost/external/increment",
            IntrospectValue::Int(0),
        )
        .unwrap_err();
        assert_eq!(err, InvokeError::NoExternalAtPath);
    }

    #[test]
    fn r666_invoke_non_external_target_is_no_external() {
        use pinion_core::scene::{BoxNode as BNode, ContainerNode, Rect};
        let child = Scene::Box(
            BNode::filled(Rect::default(), Color::default()).with_tag("info"),
        );
        let mut c = ContainerNode::new(vec![child]);
        c.rect = Rect::new(0, 0, 100, 100);
        let mut scene = Scene::Container(c);
        let err = invoke(
            &mut scene,
            "/info/external/increment",
            IntrospectValue::Int(0),
        )
        .unwrap_err();
        assert_eq!(err, InvokeError::NoExternalAtPath);
    }
}
