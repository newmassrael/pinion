//! `scene/intervene` RPC method dispatch — R56.1.f.3 §5.22 substrate
//! gap (Round 17 8 typed methods → 9 with this slice). Symmetric peer
//! of [`crate::invoke`] for the read-write state-mutation side door
//! defined by [`ExternalIntrospect::intervene`] (§5.15 item 7).
//!
//! Wires the same three pieces as [`crate::query`] / [`crate::invoke`]:
//!
//!   1. **§5.18 path resolution** — `/[window[id]/]<scene_path>`
//!      with single-window short-circuit.
//!   2. **§5.2 scene tree walk** — v0 only the root is considered;
//!      `Scene::primary_external_mut` descends through the R55.D.5
//!      multi-External container shape.
//!   3. **§5.15 item 7 intervene dispatch** — when the path resolves
//!      to a `Scene::External`, the call descends through
//!      [`External::introspect_mut`] and consults the
//!      [`ExternalIntrospect::intervene`] write channel.
//!
//! v0 scene-path syntax accepted today: `/external/<state_path>`
//! (optionally preceded by `/window[id]/`). Other shapes return
//! [`InterveneError::UnsupportedPath`].
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

    let state_path = resolved
        .scene_path
        .strip_prefix("/external/")
        .ok_or(InterveneError::UnsupportedPath)?;

    // (R55.D.5 §5.45) Descend to the substrate's primary External so
    // both the single-widget shape (`Scene::External`) and the
    // multi-widget shape (`Scene::Container([primary, ...extras])`)
    // route `external/<state>` here without per-binding
    // disambiguation. See `Scene::primary_external_mut` for the DFS
    // convention.
    let node = scene
        .primary_external_mut()
        .ok_or(InterveneError::NoExternalAtPath)?;

    let intro: &mut dyn ExternalIntrospect = node
        .handle
        .introspect_mut()
        .ok_or(InterveneError::IntrospectionOptedOut)?;

    intro.intervene(state_path, value).map_err(Into::into)
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
}
