//! R667 §5.34 — shared path-resolution + scene-walk + introspect-mut
//! substrate for the §5.12 RPC methods that address an External via
//! `/window[id]/<scene_segments>/external/<introspect_path>`.
//!
//! Lifts the inline pattern formerly repeated across
//! [`crate::invoke`](fn@crate::invoke),
//! [`crate::intervene`](fn@crate::intervene), [`crate::rewind`](fn@crate::rewind),
//! [`crate::dry_run`](fn@crate::dry_run) (twice — apply + rollback), [`crate::query`](fn@crate::query),
//! and four internal [`crate::simulate`](fn@crate::simulate) sites
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
//!   5. [`External::introspect_mut`](pinion_core::external::External::introspect_mut) (or
//!      [`External::introspect`](pinion_core::external::External::introspect)) —
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
//! inside [`crate::simulate`](fn@crate::simulate) alone); the helper is canonical
//! Rule-of-Three lift territory.

use pinion_core::Scene;
use pinion_core::external::ExternalIntrospect;
use pinion_core::scene::ExternalNode;

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
/// Used by [`crate::simulate`](fn@crate::simulate) phase 0 where every step's path is
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

/// R1483 §5.34 §5.45 §2 #2 — does this segment chain name the scene
/// root itself?
///
/// A node's tag becomes a path segment because its **parent** lists it
/// among its children ([`Scene::lookup_path_ref`] matches children only,
/// which round-trips with the `HitPath::segments` its own producer emits:
/// there, the root's path is the empty chain). A root has no parent, so
/// its tag was never a segment — consistently, but not harmlessly.
///
/// `CoreShell::compose_root` gives a binding's primary External the
/// `WidgetCore::tag()` and then puts it at the **root** when the binding
/// has no extra externals, or inside a `Container` when it has some.
/// Measured, on the same tag naming the same logical surface:
///
/// ```text
/// extras = 0  ->  /model/external/count = NoExternalAtPath
/// extras = 1  ->  /model/external/count = Ok(1)
/// ```
///
/// So whether a binding's primary answered to its own name depended on a
/// composition detail no client can see — and since R688 made the external
/// set a reactive projection of state, `reconcile_externals` re-composes at
/// runtime, so a working address could stop working when an unrelated extra
/// surface appeared or went away. That is the §2 #2 stable-address contract
/// failing, not a walker bug.
///
/// The alias is deliberately a **fallback**: a descendant that matches is
/// still preferred, so every path that resolves today keeps its present
/// meaning and this can only turn a former `NoExternalAtPath` into an
/// answer. It is one segment, because the root is one node.
fn names_the_root(scene: &Scene, segments: &[String]) -> bool {
    matches!(segments, [only] if scene.tag() == Some(only.as_str()))
}

/// R1483 §5.34 — the addressing walk every §5.12 external path uses.
///
/// [`Scene::lookup_path_ref`] plus the root-name alias described on
/// `names_the_root`. It exists as one function because five sites resolve
/// an external path (this module's two, plus the immediate-mode branches of
/// [`crate::query`](fn@crate::query), [`crate::invoke`](fn@crate::invoke)
/// and [`crate::intervene`](fn@crate::intervene)); a rule applied at four of
/// five is how the read and write channels come to disagree about what exists.
#[must_use]
pub fn lookup_addressed<'s>(scene: &'s Scene, segments: &[String]) -> Option<&'s Scene> {
    scene
        .lookup_path_ref(segments)
        .or_else(|| names_the_root(scene, segments).then_some(scene))
}

/// Mutable sibling of [`lookup_addressed`].
///
/// The descendant lookup is probed through the shared reference first so the
/// mutable borrow is taken once — which also keeps the precedence identical
/// to the read path by construction rather than by two matching edits.
#[must_use]
pub fn lookup_addressed_mut<'s>(
    scene: &'s mut Scene,
    segments: &[String],
) -> Option<&'s mut Scene> {
    if scene.lookup_path_ref(segments).is_none() {
        return names_the_root(scene, segments).then_some(scene);
    }
    scene.lookup_path_mut(segments)
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
    let target = lookup_addressed_mut(scene, scene_segments)
        .ok_or(ResolveExternalError::NoExternalAtPath)?;
    let node = target
        .primary_external_mut()
        .ok_or(ResolveExternalError::NoExternalAtPath)?;
    node.handle
        .introspect_mut()
        .ok_or(ResolveExternalError::IntrospectionOptedOut)
}

/// Read-only sibling of [`introspect_mut_at`]. Used by
/// [`crate::query`](fn@crate::query) where the scene argument is `&Scene` and no
/// mutation occurs.
///
/// # Errors
///
/// Same failure modes as [`introspect_mut_at`], but routed through
/// [`pinion_core::Scene::lookup_path_ref`] /
/// [`pinion_core::Scene::primary_external`] /
/// [`External::introspect`](pinion_core::external::External::introspect).
pub fn introspect_at<'s>(
    scene: &'s Scene,
    scene_segments: &[String],
) -> Result<&'s dyn ExternalIntrospect, ResolveExternalError> {
    external_node_at(scene, scene_segments)
        .ok_or(ResolveExternalError::NoExternalAtPath)?
        .handle
        .introspect()
        .ok_or(ResolveExternalError::IntrospectionOptedOut)
}

/// R1487 §5.12 §2 #7 — *is* there a retained `External` at this address?
///
/// The existence half of [`introspect_at`], which is written in terms of it
/// so the two cannot answer differently. That matters because two channels
/// ask this question about the same address and used to answer it with two
/// different walks: `scene/query` resolved the surface, while the R1481
/// write fallback never looked and reported `NoExternalAtPath` — "there is
/// no external here" — for addresses the read had just resolved. One walk,
/// one answer ([[wire-form-read-write-symmetry]]).
///
/// Reached-but-opted-out still counts as reached: whether the node exposes
/// an introspect surface is the *next* question, and answering this one
/// with "nothing is there" would state something false about the scene
/// (§2 #7).
#[must_use]
pub fn external_node_at<'s>(
    scene: &'s Scene,
    scene_segments: &[String],
) -> Option<&'s ExternalNode> {
    lookup_addressed(scene, scene_segments)?.primary_external()
}

/// Composite: string parse + scene walk in one call. Used by
/// [`crate::invoke`](fn@crate::invoke),
/// [`crate::intervene`](fn@crate::intervene), [`crate::rewind`](fn@crate::rewind), and
/// [`crate::dry_run`](fn@crate::dry_run) (twice — apply and rollback under separate
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
/// [`crate::query`](fn@crate::query).
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
    use pinion_core::Color;
    use pinion_core::external::{CountedExternal, IntrospectValue, StubExternal};
    use pinion_core::scene::{BoxNode, ContainerNode, ExternalNode, Rect};

    fn counted_scene(n: i64) -> Scene {
        Scene::External(ExternalNode::new(Box::new(CountedExternal::new(n))))
    }

    fn container_with_tagged_counted(tag: &'static str, count: i64) -> Scene {
        let ext =
            Scene::External(ExternalNode::new(Box::new(CountedExternal::new(count))).with_tag(tag));
        let mut c = ContainerNode::new(vec![ext]);
        c.rect = Rect::new(0, 0, 100, 100);
        Scene::Container(c)
    }

    #[test]
    fn path_parses_window_prefix_and_split() {
        let (segs, intro) = resolve_external_path("/window[main]/counter/external/count").unwrap();
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
        assert!(matches!(
            err,
            ResolveExternalError::Path(PathError::MalformedPrefix)
        ));
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

    // ---- R1483 §5.34 §2 #2 — one name for one surface, both compositions ----

    /// The two shapes `CoreShell::compose_root` produces for the SAME
    /// binding: the primary alone when it has no extra externals, wrapped in
    /// a `Container` when it has some. The primary carries `WidgetCore::tag()`
    /// either way, which is the whole point — one tag, one logical surface.
    fn bare_root_primary(tag: &'static str, count: i64) -> Scene {
        Scene::External(ExternalNode::new(Box::new(CountedExternal::new(count))).with_tag(tag))
    }

    fn wrapped_primary(tag: &'static str, count: i64) -> Scene {
        let primary =
            Scene::External(ExternalNode::new(Box::new(CountedExternal::new(count))).with_tag(tag));
        let extra =
            Scene::External(ExternalNode::new(Box::new(CountedExternal::new(999))).with_tag("aux"));
        let mut c = ContainerNode::new(vec![primary, extra]);
        c.rect = Rect::new(0, 0, 100, 100);
        Scene::Container(c)
    }

    #[test]
    fn r1483_one_tag_reaches_the_primary_in_both_compositions() {
        // The defect in one assertion. Measured before the fix:
        //   extras = 0  ->  NoExternalAtPath
        //   extras = 1  ->  Ok(1)
        // …for the same tag naming the same surface, decided by a
        // composition detail no client can observe.
        for (what, scene) in [
            (
                "no extras (primary is the root)",
                bare_root_primary("model", 1),
            ),
            (
                "one extra (primary is a child)",
                wrapped_primary("model", 1),
            ),
        ] {
            let intro = introspect_at(&scene, &["model".to_string()])
                .unwrap_or_else(|e| panic!("{what}: {e:?}"));
            assert_eq!(
                intro.query("count"),
                Some(IntrospectValue::Int(1)),
                "{what}: the tag must reach the primary",
            );
        }
    }

    #[test]
    fn r1483_the_bare_shorthand_still_reaches_the_primary_in_both() {
        // The address that already worked in both shapes must keep working —
        // the alias is added beside it, not instead of it.
        for scene in [bare_root_primary("model", 4), wrapped_primary("model", 4)] {
            let intro = introspect_at(&scene, &[]).expect("bare /external resolves");
            assert_eq!(intro.query("count"), Some(IntrospectValue::Int(4)));
        }
    }

    /// A root tagged `dup` whose SECOND child is also tagged `dup`, with a
    /// different external ahead of it. The two orderings then reach different
    /// externals — `primary_external` descends a container to its FIRST
    /// external, so a root-first walk answers `1` and a descendant-first walk
    /// answers `7`. Without that gap the fixture cannot tell them apart: the
    /// first version of this test held one child and passed under BOTH
    /// orderings, because both descended to the same node.
    fn root_shadowing_a_descendant() -> Scene {
        let decoy =
            Scene::External(ExternalNode::new(Box::new(CountedExternal::new(1))).with_tag("decoy"));
        let named =
            Scene::External(ExternalNode::new(Box::new(CountedExternal::new(7))).with_tag("dup"));
        let mut c = ContainerNode::new(vec![decoy, named]);
        c.rect = Rect::new(0, 0, 100, 100);
        Scene::Container(c.with_tag("dup".to_owned()))
    }

    #[test]
    fn r1483_a_descendant_still_wins_over_the_root_name() {
        // Precedence guard: the alias is a FALLBACK, so a path that resolves
        // today keeps its present meaning. Paint-scene roots really are
        // tagged, so this collision is reachable, not hypothetical.
        let scene = root_shadowing_a_descendant();
        let intro = introspect_at(&scene, &["dup".to_string()]).expect("resolves");
        assert_eq!(
            intro.query("count"),
            Some(IntrospectValue::Int(7)),
            "the child named `dup`, not the root's first external",
        );
    }

    #[test]
    fn r1483_the_alias_names_one_node_not_a_prefix() {
        // The root is one node, so it consumes one segment. A longer chain
        // beginning with the root's name must not resolve through the alias —
        // that would make the root's tag a silent path prefix.
        let scene = bare_root_primary("model", 1);
        let err = expect_err(introspect_at(
            &scene,
            &["model".to_string(), "deeper".to_string()],
        ));
        assert_eq!(err, ResolveExternalError::NoExternalAtPath);
    }

    #[test]
    fn r1483_an_untagged_root_is_not_reachable_by_any_name() {
        // The alias keys on the root's OWN tag; an untagged root still has
        // only the empty path, so a wrong name is still an honest refusal.
        let scene = counted_scene(1);
        let err = expect_err(introspect_at(&scene, &["model".to_string()]));
        assert_eq!(err, ResolveExternalError::NoExternalAtPath);
    }

    #[test]
    fn r1483_the_write_channel_resolves_the_same_addresses_as_the_read() {
        // R1481's lesson as a test: a rule applied to the read and not the
        // write is how the two channels come to disagree about what exists.
        // Both shapes, both channels, one loop.
        for (what, mut scene) in [
            ("no extras", bare_root_primary("model", 1)),
            ("one extra", wrapped_primary("model", 1)),
        ] {
            let segs = ["model".to_string()];
            let readable = introspect_at(&scene, &segs).is_ok();
            let writable = introspect_mut_at(&mut scene, &segs).is_ok();
            assert!(readable, "{what}: the read reaches it");
            assert_eq!(
                readable, writable,
                "{what}: the write channel must resolve what the read resolved",
            );
        }
    }

    #[test]
    fn r1483_lookup_addressed_and_its_mut_sibling_agree() {
        // The two entry points are separate functions, so their agreement is
        // asserted rather than assumed. It compares WHICH node each reached
        // (by the count its external answers), not merely that each found
        // one — the shadowing case resolves under either precedence, so a
        // presence-only comparison could not see a divergence there.
        fn reached(scene: &Scene, name: &str) -> Option<IntrospectValue> {
            introspect_at(scene, &[name.to_string()])
                .ok()
                .and_then(|i| i.query("count"))
        }
        fn reached_mut(scene: &mut Scene, name: &str) -> Option<IntrospectValue> {
            introspect_mut_at(scene, &[name.to_string()])
                .ok()
                .and_then(|i| i.query("count"))
        }
        let cases = [
            ("bare root", bare_root_primary("model", 1)),
            ("wrapped", wrapped_primary("model", 1)),
            ("shadowing", root_shadowing_a_descendant()),
            ("untagged root", counted_scene(1)),
        ];
        for (what, mut scene) in cases {
            for name in ["model", "dup", "decoy", "ghost"] {
                assert_eq!(
                    reached(&scene, name),
                    reached_mut(&mut scene, name),
                    "{what} / {name}: the channels reached different nodes",
                );
            }
        }
    }
}
