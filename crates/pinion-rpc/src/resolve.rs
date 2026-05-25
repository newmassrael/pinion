//! R667 §5.34 — shared path-resolution + scene-walk + introspect-mut
//! substrate for the §5.12 RPC methods that address an External via
//! `/window[id]/<scene_segments>/external/<introspect_path>`.
//!
//! Lifts the inline pattern formerly repeated across
//! [`crate::invoke`], [`crate::intervene`], [`crate::rewind`],
//! [`crate::dry_run`] (twice — apply + rollback), [`crate::query`],
//! and four internal [`crate::simulate`] sites
//! (`query_introspect_at` / Phase-2 apply / `classify_lookup_failure`
//! / `restore_originals`). Each site previously open-coded the same
//! four-step chain:
//!
//!   1. [`crate::path::resolve`] — strip the optional `/window[id]/`
//!      prefix per §5.18.
//!   2. [`crate::path::split_at_external`] — split at the
//!      `/external/` separator into scene segments + introspect path
//!      per §5.34 R42.
//!   3. [`pinion_core::Scene::lookup_path_mut`] (or
//!      [`pinion_core::Scene::lookup_path_ref`]) — walk Container /
//!      Scroll children by tag/index per §5.45 R55.D.5.
//!   4. [`pinion_core::Scene::primary_external_mut`] (or
//!      [`pinion_core::Scene::primary_external`]) — descend the
//!      multi-widget wrap shape to the substrate's primary External.
//!   5. [`pinion_core::external::ExternalHandle::introspect_mut`] (or
//!      [`pinion_core::external::ExternalHandle::introspect`]) —
//!      reach the §5.15 item 7/8 introspect channel.
//!
//! Each call site previously also defined a parallel set of error
//! variants for the four failure modes (path / unsupported / no-
//! external / introspection-opted-out). [`ResolveExternalError`]
//! gathers them into one enum; per-site error enums implement
//! `From<ResolveExternalError>` so the lifted helpers slot in
//! behind the existing `?`-operator boilerplate.
//!
//! [[abstraction-needs-second-consumer]] / [[r47-class-incident-prevention]]
//! — N-of-N consumer rule is overshot (6+ inline patterns, 4 of them
//! inside [`crate::simulate`] alone); the helper is canonical
//! Rule-of-Three lift territory.

use pinion_core::external::ExternalIntrospect;
use pinion_core::Scene;

use crate::path::{self, PathError};

/// Failure modes shared across every §5.34 R42 external-introspect
/// resolution.
///
/// Per-site error enums implement `From<ResolveExternalError>` so
/// the lifted helpers can be `?`-chained through the existing
/// error-mapping surface (`InvokeError`, `InterveneError`,
/// `RewindError`, `DryRunError`, `QueryError`, `SimulateError`).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveExternalError {
    /// Window-prefix parsing failed (see [`PathError`]).
    Path(PathError),
    /// Scene path did not contain the `/external/` separator, so the
    /// §5.34 R42 split-and-walk shape does not apply.
    UnsupportedPath,
    /// Path resolved cleanly but the walk did not land on an
    /// `External` (either the scene-segment chain hit a `None` or the
    /// addressed scene's `primary_external` descent returned `None`).
    NoExternalAtPath,
    /// External was reached but did not opt in to §5.15 item 7/8
    /// introspection, so the `query` / `intervene` / `invoke`
    /// channels are unreachable.
    IntrospectionOptedOut,
}

impl From<PathError> for ResolveExternalError {
    fn from(err: PathError) -> Self {
        Self::Path(err)
    }
}

/// Strings-only path resolution: parse the `/window[id]/` prefix and
/// split at `/external/`.
///
/// Used by [`crate::simulate`] phase 0 where every step's path is
/// pre-validated before any scene walk could expose a borrow on the
/// mutable scene reference. Returns owned strings so the caller can
/// stash the result and re-walk the scene multiple times under
/// independent `&mut Scene` borrows.
///
/// # Errors
///
/// Returns [`ResolveExternalError::Path`] when the window prefix is
/// malformed, or [`ResolveExternalError::UnsupportedPath`] when the
/// scene path does not contain `/external/`.
pub fn resolve_external_path(
    raw_path: &str,
) -> Result<(Vec<String>, String), ResolveExternalError> {
    let resolved = path::resolve(raw_path)?;
    let _ = resolved.window;
    let (scene_segments, introspect_path) = path::split_at_external(resolved.scene_path)
        .map(|(segs, intro)| (segs, intro.to_string()))
        .ok_or(ResolveExternalError::UnsupportedPath)?;
    Ok((scene_segments, introspect_path))
}

/// Walk `scene` via `scene_segments`, descend to the primary
/// External, and reach its `&mut dyn ExternalIntrospect` surface.
///
/// `scene_segments` is the chain produced by
/// [`resolve_external_path`] (or [`crate::path::split_at_external`]
/// directly when the caller already has the borrow-free path
/// separation). The `'_` lifetime on the returned reference is tied
/// to `scene` so the caller can hold the borrow across follow-on
/// `query` / `intervene` / `invoke` calls without re-walking.
///
/// # Errors
///
/// * [`ResolveExternalError::NoExternalAtPath`] — the segment chain
///   did not reach a [`pinion_core::scene::ExternalNode`].
/// * [`ResolveExternalError::IntrospectionOptedOut`] — the External
///   was reached but did not opt in to §5.15 introspection.
pub fn introspect_mut_at<'s>(
    scene: &'s mut Scene,
    scene_segments: &[String],
) -> Result<&'s mut dyn ExternalIntrospect, ResolveExternalError> {
    let target = scene
        .lookup_path_mut(scene_segments)
        .ok_or(ResolveExternalError::NoExternalAtPath)?;
    let node = target
        .primary_external_mut()
        .ok_or(ResolveExternalError::NoExternalAtPath)?;
    node.handle
        .introspect_mut()
        .ok_or(ResolveExternalError::IntrospectionOptedOut)
}

/// Read-only sibling of [`introspect_mut_at`]. Used by
/// [`crate::query`] where the scene argument is `&Scene` and no
/// mutation occurs.
///
/// # Errors
///
/// Same failure modes as [`introspect_mut_at`], but routed through
/// [`pinion_core::Scene::lookup_path_ref`] /
/// [`pinion_core::Scene::primary_external`] /
/// [`pinion_core::external::ExternalHandle::introspect`].
pub fn introspect_at<'s>(
    scene: &'s Scene,
    scene_segments: &[String],
) -> Result<&'s dyn ExternalIntrospect, ResolveExternalError> {
    let target = scene
        .lookup_path_ref(scene_segments)
        .ok_or(ResolveExternalError::NoExternalAtPath)?;
    let node = target
        .primary_external()
        .ok_or(ResolveExternalError::NoExternalAtPath)?;
    node.handle
        .introspect()
        .ok_or(ResolveExternalError::IntrospectionOptedOut)
}

/// Composite: string parse + scene walk in one call. Used by
/// [`crate::invoke`], [`crate::intervene`], [`crate::rewind`], and
/// [`crate::dry_run`] (twice — apply and rollback under separate
/// `&mut Scene` borrows).
///
/// The returned `String` is the per-External introspect path that
/// `query` / `intervene` / `invoke` consume. Owned (not borrowed
/// from the input) so the `&'s mut dyn ExternalIntrospect` borrow
/// on `scene` can outlive `raw_path`'s lifetime.
///
/// # Errors
///
/// See [`ResolveExternalError`].
pub fn resolve_external_introspect_mut<'s>(
    scene: &'s mut Scene,
    raw_path: &str,
) -> Result<(&'s mut dyn ExternalIntrospect, String), ResolveExternalError> {
    let (scene_segments, introspect_path) = resolve_external_path(raw_path)?;
    let intro = introspect_mut_at(scene, &scene_segments)?;
    Ok((intro, introspect_path))
}

/// Read-only sibling of [`resolve_external_introspect_mut`]. Used by
/// [`crate::query`].
///
/// # Errors
///
/// See [`ResolveExternalError`].
pub fn resolve_external_introspect<'s>(
    scene: &'s Scene,
    raw_path: &str,
) -> Result<(&'s dyn ExternalIntrospect, String), ResolveExternalError> {
    let (scene_segments, introspect_path) = resolve_external_path(raw_path)?;
    let intro = introspect_at(scene, &scene_segments)?;
    Ok((intro, introspect_path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::external::{CountedExternal, IntrospectValue, StubExternal};
    use pinion_core::scene::{BoxNode, ContainerNode, ExternalNode, Rect};
    use pinion_core::Color;

    fn counted_scene(n: i64) -> Scene {
        Scene::External(ExternalNode::new(Box::new(CountedExternal::new(n))))
    }

    fn container_with_tagged_counted(tag: &'static str, count: i64) -> Scene {
        let ext = Scene::External(
            ExternalNode::new(Box::new(CountedExternal::new(count))).with_tag(tag),
        );
        let mut c = ContainerNode::new(vec![ext]);
        c.rect = Rect::new(0, 0, 100, 100);
        Scene::Container(c)
    }

    #[test]
    fn path_parses_window_prefix_and_split() {
        let (segs, intro) =
            resolve_external_path("/window[main]/counter/external/count").unwrap();
        assert_eq!(segs, vec!["counter".to_string()]);
        assert_eq!(intro, "count");
    }

    #[test]
    fn path_v0_root_external_returns_empty_segments() {
        let (segs, intro) = resolve_external_path("/external/count").unwrap();
        assert!(segs.is_empty());
        assert_eq!(intro, "count");
    }

    #[test]
    fn path_missing_external_separator_is_unsupported() {
        let err = resolve_external_path("/some/other/shape").unwrap_err();
        assert_eq!(err, ResolveExternalError::UnsupportedPath);
    }

    #[test]
    fn path_malformed_window_prefix_surfaces_as_path_error() {
        let err = resolve_external_path("/window[main/external/count").unwrap_err();
        assert!(matches!(err, ResolveExternalError::Path(PathError::MalformedPrefix)));
    }

    #[test]
    fn introspect_mut_at_root_external_reaches_query_channel() {
        let mut scene = counted_scene(7);
        let intro = introspect_mut_at(&mut scene, &[]).unwrap();
        assert_eq!(intro.query("count"), Some(IntrospectValue::Int(7)));
    }

    #[test]
    fn introspect_mut_at_tagged_descendant() {
        let mut scene = container_with_tagged_counted("counter", 11);
        let intro = introspect_mut_at(&mut scene, &["counter".to_string()]).unwrap();
        assert_eq!(intro.query("count"), Some(IntrospectValue::Int(11)));
    }

    /// `&mut dyn ExternalIntrospect` is not `Debug`, so `unwrap_err` is
    /// unavailable; this thin wrapper destructures via `match` instead.
    fn expect_err<T>(r: Result<T, ResolveExternalError>) -> ResolveExternalError {
        match r {
            Ok(_) => panic!("expected ResolveExternalError"),
            Err(e) => e,
        }
    }

    #[test]
    fn introspect_mut_at_missing_segment_is_no_external() {
        let mut scene = container_with_tagged_counted("counter", 0);
        let err = expect_err(introspect_mut_at(&mut scene, &["ghost".to_string()]));
        assert_eq!(err, ResolveExternalError::NoExternalAtPath);
    }

    #[test]
    fn introspect_mut_at_box_target_is_no_external() {
        let mut scene = Scene::Box(BoxNode::filled(Rect::default(), Color::default()));
        let err = expect_err(introspect_mut_at(&mut scene, &[]));
        assert_eq!(err, ResolveExternalError::NoExternalAtPath);
    }

    #[test]
    fn introspect_mut_at_opted_out_external_reports_introspection_opted_out() {
        let mut scene = Scene::External(ExternalNode::new(Box::new(StubExternal::new())));
        let err = expect_err(introspect_mut_at(&mut scene, &[]));
        assert_eq!(err, ResolveExternalError::IntrospectionOptedOut);
    }

    #[test]
    fn introspect_at_read_only_path_succeeds() {
        let scene = counted_scene(42);
        let intro = introspect_at(&scene, &[]).unwrap();
        assert_eq!(intro.query("count"), Some(IntrospectValue::Int(42)));
    }

    #[test]
    fn introspect_at_descendant_tag_resolves() {
        let scene = container_with_tagged_counted("counter", 99);
        let intro = introspect_at(&scene, &["counter".to_string()]).unwrap();
        assert_eq!(intro.query("count"), Some(IntrospectValue::Int(99)));
    }

    #[test]
    fn composite_mut_round_trip_query() {
        let mut scene = container_with_tagged_counted("counter", 5);
        let (intro, intro_path) =
            resolve_external_introspect_mut(&mut scene, "/counter/external/count").unwrap();
        assert_eq!(intro.query(&intro_path), Some(IntrospectValue::Int(5)));
    }

    #[test]
    fn composite_ref_round_trip_query() {
        let scene = container_with_tagged_counted("counter", 13);
        let (intro, intro_path) =
            resolve_external_introspect(&scene, "/window[main]/counter/external/count").unwrap();
        assert_eq!(intro.query(&intro_path), Some(IntrospectValue::Int(13)));
    }
}
