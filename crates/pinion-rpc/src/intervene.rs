//! `scene/intervene` RPC method dispatch — R56.1.f.3 §5.22 substrate
//! gap (Round 17 8 typed methods → 9 with this slice). Symmetric peer
//! of [`crate::invoke`] for the read-write state-mutation side door
//! defined by [`ExternalIntrospect::intervene`] (§5.15 item 7).
//!
//! Wires the same three pieces as [`crate::query`] / [`crate::invoke`]:
//!
//!   1. **§5.18 path resolution** — `/[window[id]/]<scene_path>`
//!      with single-window short-circuit.
//!   2. **§5.2 scene tree walk** — R666 §5.34 v1 multi-External
//!      addressing via `path::split_at_external` +
//!      [`Scene::lookup_path_mut`] + [`Scene::primary_external_mut`].
//!   3. **§5.15 item 7 intervene dispatch** — descend through
//!      [`External::introspect_mut`] and consult the
//!      [`ExternalIntrospect::intervene`] write channel.
//!
//! v1 scene-path syntax accepted (mirror of [`crate::invoke`]):
//!
//!   * `/external/<state>` — primary External at root (v0 retained).
//!   * `/<tag>/external/<state>` — DFS lookup by `ExternalNode` tag,
//!     composite tags (`todo_toggle#1`) pass through verbatim.
//!   * `/<seg>/.../<tag>/external/<state>` — nested Container walk.
//!
//! Other shapes return [`InterveneError::UnsupportedPath`].
//!
//! Where [`crate::invoke`] is the "call an action and get a value
//! back" surface (W3C `RPC.call`-mirror), [`intervene`] is the
//! "write a value to a state path" surface (W3C `RPC.set`-mirror).
//! `TextField.caret = 4` flows through this path; `TextField.send
//! Focus` flows through [`crate::invoke`]. R56.1.f.3 motivated
//! landing it: the §5.22 selection sidecar exposes both a query path
//! (`/external/selection` → Json) and an intervene path (Json or
//! Null) on `TextFieldExternal`, but the RPC wiring side was missing
//! a wire-form entry point for the mutation half.
//!
//! Transport (JSON-RPC 2.0 framing per §5.7) is handled by
//! [`crate::dispatch`]; this module exposes the typed dispatcher only.

use pinion_core::external::{
    ExternalIntrospect, InterveneError as TraitInterveneError, IntrospectValue,
};
use pinion_core::Scene;

use crate::path::{self, PathError};

/// Reasons the typed [`intervene`] dispatcher can fail.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InterveneError {
    /// Window-prefix parsing failed (see [`PathError`]).
    Path(PathError),
    /// Scene path does not match a v0-supported shape.
    UnsupportedPath,
    /// Path expected an `External` at the scene root, found a different
    /// primitive.
    NoExternalAtPath,
    /// The `External` did not opt in to §5.15 item 7 introspection
    /// (so the intervene channel is unreachable).
    IntrospectionOptedOut,
    /// The state path is not declared in the `External`'s schema, or
    /// is declared but cannot be written (read-only slots return the
    /// trait's `ReadOnly` variant which surfaces here).
    UnknownIntervenePath,
    /// The value variant does not match the slot's declared type.
    InterveneTypeMismatch,
    /// The slot is read-only on this `External`. Distinct from
    /// `UnknownIntervenePath` so a client can tell "schema says this
    /// exists but you cannot write it" from "schema does not declare
    /// this slot".
    ReadOnly,
    /// The value is the right variant but its content is outside the
    /// slot's accepted range (e.g. negative caret position).
    OutOfRange,
}

impl From<PathError> for InterveneError {
    fn from(err: PathError) -> Self {
        InterveneError::Path(err)
    }
}

impl From<TraitInterveneError> for InterveneError {
    fn from(err: TraitInterveneError) -> Self {
        // `TraitInterveneError` is `#[non_exhaustive]`; future variants
        // collapse into `InterveneTypeMismatch` via the wildcard until
        // a follow-up slice maps them explicitly. The existing four
        // variants get distinct codes so the RPC client can branch
        // (`UnknownPath` ↔ "schema does not declare", `ReadOnly` ↔
        // "schema declares but immutable", `OutOfRange` ↔ "value
        // outside accepted bounds").
        match err {
            TraitInterveneError::UnknownPath => InterveneError::UnknownIntervenePath,
            TraitInterveneError::ReadOnly => InterveneError::ReadOnly,
            TraitInterveneError::OutOfRange => InterveneError::OutOfRange,
            // Wildcard absorbs `TypeMismatch` plus any future variant
            // a follow-up slice adds to the non_exhaustive enum.
            _ => InterveneError::InterveneTypeMismatch,
        }
    }
}

/// Resolve `raw_path` against `scene` and write `value` to the slot
/// there.
///
/// See module docs for the v0 path syntax. The `scene` reference is
/// borrowed mutably for the lifetime of the call so the underlying
/// `External` can update its state.
///
/// # Errors
///
/// Returns [`InterveneError`] when the path is malformed, the scene
/// root does not match the path shape, or the underlying `External`
/// rejects the write (unknown path, type mismatch, read-only slot,
/// or out-of-range value).
pub fn intervene(
    scene: &mut Scene,
    raw_path: &str,
    value: IntrospectValue,
) -> Result<(), InterveneError> {
    let resolved = path::resolve(raw_path)?;
    let _ = resolved.window;

    // R666 §5.34 — split at the `/external/` separator (mirror of
    // [`crate::rewind`] / [`crate::invoke`]). Empty scene segments =
    // root-External (v0 shape); non-empty = walk Container/Scroll by
    // tag/index. `state_path` is owned so the &mut borrow on `scene`
    // can outlive `resolved.scene_path`'s lifetime.
    let (scene_segments, state_path) = path::split_at_external(resolved.scene_path)
        .map(|(segs, intro)| (segs, intro.to_string()))
        .ok_or(InterveneError::UnsupportedPath)?;

    let target = scene
        .lookup_path_mut(&scene_segments)
        .ok_or(InterveneError::NoExternalAtPath)?;

    // (R55.D.5 §5.45) Descend to the addressed scene's primary
    // External — both single-widget and R55.D.5 multi-widget shapes
    // collapse to a single ExternalNode through this DFS pre-order
    // walk. When the path tagged an extra sibling explicitly
    // (`/todo_delete#5/external/...`), `lookup_path_mut` already
    // landed on `Scene::External(extra)` and `primary_external_mut`
    // returns `Some(self)`.
    let node = target
        .primary_external_mut()
        .ok_or(InterveneError::NoExternalAtPath)?;

    let intro: &mut dyn ExternalIntrospect = node
        .handle
        .introspect_mut()
        .ok_or(InterveneError::IntrospectionOptedOut)?;

    intro.intervene(&state_path, value).map_err(Into::into)
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
    fn counted_set_writes_total() {
        // `CountedExternal::intervene` accepts `total = Int` and
        // overwrites the running total — the path here is the same
        // `"total"` slot `query` reports.
        let mut scene = counted_scene(5);
        intervene(&mut scene, "/external/count", IntrospectValue::Int(42)).unwrap();
        // Round-trip — invoke `total` query through the External
        // directly to confirm the write landed.
        if let Scene::External(node) = &scene {
            let intro = node.handle.introspect().expect("counted introspects");
            assert_eq!(intro.query("count"), Some(IntrospectValue::Int(42)));
        } else {
            panic!("expected External at root");
        }
    }

    #[test]
    fn counted_set_via_window_prefix() {
        let mut scene = counted_scene(0);
        intervene(
            &mut scene,
            "/window[main]/external/count",
            IntrospectValue::Int(7),
        )
        .unwrap();
        if let Scene::External(node) = &scene {
            let intro = node.handle.introspect().expect("counted introspects");
            assert_eq!(intro.query("count"), Some(IntrospectValue::Int(7)));
        }
    }

    #[test]
    fn stub_at_root_reports_introspection_opted_out() {
        let mut scene = Scene::External(ExternalNode::new(Box::new(StubExternal::new())));
        let err = intervene(&mut scene, "/external/anything", IntrospectValue::Null).unwrap_err();
        assert_eq!(err, InterveneError::IntrospectionOptedOut);
    }

    #[test]
    fn box_at_root_reports_no_external() {
        let mut scene = Scene::Box(BoxNode::filled(Rect::default(), Color::default()));
        let err = intervene(&mut scene, "/external/anything", IntrospectValue::Null).unwrap_err();
        assert_eq!(err, InterveneError::NoExternalAtPath);
    }

    #[test]
    fn unsupported_path_shape_rejects() {
        let mut scene = counted_scene(0);
        let err =
            intervene(&mut scene, "/state/count", IntrospectValue::Int(0)).unwrap_err();
        assert_eq!(err, InterveneError::UnsupportedPath);
    }

    #[test]
    fn unknown_state_path_lifted_from_trait_error() {
        let mut scene = counted_scene(0);
        let err = intervene(
            &mut scene,
            "/external/unknown_slot",
            IntrospectValue::Int(0),
        )
        .unwrap_err();
        assert_eq!(err, InterveneError::UnknownIntervenePath);
    }

    // ---- R666 §5.34 v1: multi-External addressing ----

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

    fn read_count(scene: &Scene, tag: &str) -> i64 {
        let node = scene
            .find_external_with_tag(tag)
            .unwrap_or_else(|| panic!("no External tagged {tag}"));
        let intro = node.handle.introspect().expect("counted introspects");
        match intro.query("count").expect("count slot") {
            IntrospectValue::Int(n) => n,
            other => panic!("expected Int, got {other:?}"),
        }
    }

    #[test]
    fn r666_intervene_extra_external_by_composite_tag() {
        // Write only to the composite-tagged extra sibling; primary
        // untouched.
        let mut scene = container_with_two_counted_siblings(
            "todo_list", 100, "todo_toggle#1", 0,
        );
        intervene(
            &mut scene,
            "/todo_toggle#1/external/count",
            IntrospectValue::Int(42),
        )
        .unwrap();
        assert_eq!(read_count(&scene, "todo_toggle#1"), 42);
        assert_eq!(read_count(&scene, "todo_list"), 100);
    }

    #[test]
    fn r666_intervene_extra_external_with_window_prefix() {
        let mut scene = container_with_two_counted_siblings(
            "primary", 0, "todo_delete#7", 0,
        );
        intervene(
            &mut scene,
            "/window[main]/todo_delete#7/external/count",
            IntrospectValue::Int(11),
        )
        .unwrap();
        assert_eq!(read_count(&scene, "todo_delete#7"), 11);
    }

    #[test]
    fn r666_intervene_unknown_segment_is_no_external() {
        let mut scene = container_with_two_counted_siblings("a", 0, "b", 0);
        let err = intervene(
            &mut scene,
            "/ghost/external/count",
            IntrospectValue::Int(0),
        )
        .unwrap_err();
        assert_eq!(err, InterveneError::NoExternalAtPath);
    }
}
