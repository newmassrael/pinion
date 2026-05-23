//! R607 §5.7 §5.22 — shared substrate-introspection scaffold for RPC
//! methods that project an [`Owner::cache`]-bound reactive primitive
//! into a JSON-RPC outcome.
//!
//! Three R602/R603/R604 modules (`scroll_state`, `text_state`,
//! `caret_state`) shared a near-identical skeleton:
//!
//! 1. Reject when no [`runtime_owner`](crate::DispatchContext) is
//!    attached.
//! 2. Resolve the per-widget tag via
//!    [`Owner::cache_get_by_str`] (R605 substrate primitive).
//! 3. On miss, surface a typed `NotBound { tag }` error carrying
//!    the failing tag for AI-agent retry.
//! 4. On hit, project the substrate state into the outcome shape.
//!
//! Pre-R607 each module carried its own copy of the gate + error
//! enum. Per [[three-site-internal-duplication-substrate-lift]] the
//! third internal-form variant cements the abstraction: this module
//! lifts the common scaffold and the three call sites collapse to
//! a one-line delegate each.
//!
//! Per-module type aliases ([`crate::ScrollStateError`],
//! [`crate::TextStateError`], [`crate::CaretStateError`]) point at
//! the unified [`SubstrateIntrospectError`] so the documentation,
//! error-data wire identifiers, and call-site naming stay distinct
//! at the type-system level while sharing one implementation.
//! When any one module needs to diverge a variant (e.g.
//! `text_state` growing an `InvalidUtf8Boundary` arm) the alias
//! gets replaced with a dedicated enum and the lifted scaffold
//! continues to serve the others.

use pinion_core::reactive::Owner;
use std::rc::Rc;

/// Typed errors every substrate-introspection RPC method shares.
/// Each variant maps onto a JSON-RPC `-32602 Invalid params` with
/// the variant name in `error.data` so AI clients can
/// pattern-match without parsing prose. See
/// [`introspect_error_to_data`] for the canonical
/// `data`-string mapping.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubstrateIntrospectError {
    /// The embedder did not register a
    /// [`runtime_owner`](crate::DispatchContext) on the dispatch
    /// context. Without the substrate's root [`Owner`] there is no
    /// reactive scope to consult.
    RuntimeOwnerUnavailable,
    /// `params.tag` was absent from the JSON-RPC request. Surface
    /// at the dispatch layer where the JSON shape is parsed —
    /// [`lookup`] itself never produces this variant (it takes
    /// `&str` directly).
    TagRequired,
    /// The runtime owner is bound but no `S` has been cached under
    /// `tag` yet. Carries the failing tag for AI-agent retry with
    /// the correct value.
    NotBound { tag: String },
}

/// Canonical `error.data` string for each
/// [`SubstrateIntrospectError`] variant. Used by the dispatcher
/// when mapping the typed error into the JSON-RPC wire shape so
/// the wire identifier stays in one place.
#[must_use]
pub fn introspect_error_to_data(err: &SubstrateIntrospectError) -> &'static str {
    match err {
        SubstrateIntrospectError::RuntimeOwnerUnavailable => "RuntimeOwnerUnavailable",
        SubstrateIntrospectError::TagRequired => "TagRequired",
        SubstrateIntrospectError::NotBound { .. } => "NotBound",
    }
}

/// Resolve `tag` against `runtime_owner`'s
/// [`Owner::cache`](pinion_core::reactive::Owner::cache) under the
/// `S` typed slot and project the resulting state through
/// `project`. Composes the four-step skeleton documented at the
/// module level into a single call.
///
/// `tag` may be any `&str` lifetime (R605 substrate lift removed
/// the `&'static` requirement). `project` receives both the
/// resolved tag (so the outcome can echo it back) and a borrowed
/// `&S` snapshot.
///
/// # Errors
///
/// - [`SubstrateIntrospectError::RuntimeOwnerUnavailable`]
///   when `runtime_owner` is [`None`].
/// - [`SubstrateIntrospectError::NotBound`] with the failing
///   `tag` when the typed cache slot does not exist.
///
/// # Side effects
///
/// None. [`Owner::cache_get_by_str`] never creates a slot on miss;
/// `project` only sees a borrowed reference and cannot register a
/// new reactive subscription (no `Owner::current` is activated by
/// this helper).
pub fn lookup<S, V, F>(
    runtime_owner: Option<&Owner>,
    tag: &str,
    project: F,
) -> Result<V, SubstrateIntrospectError>
where
    S: 'static,
    F: FnOnce(&str, &S) -> V,
{
    let owner = runtime_owner.ok_or(SubstrateIntrospectError::RuntimeOwnerUnavailable)?;
    let state: Rc<S> = owner.cache_get_by_str::<S>(tag).ok_or_else(|| {
        SubstrateIntrospectError::NotBound {
            tag: tag.to_owned(),
        }
    })?;
    Ok(project(tag, &state))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::reactive::Owner;

    #[derive(Debug, PartialEq)]
    struct ProbeState(u32);

    #[test]
    fn r607_lookup_returns_runtime_owner_unavailable_when_none() {
        let err = lookup::<ProbeState, _, _>(None, "x", |_, _| ()).unwrap_err();
        assert_eq!(err, SubstrateIntrospectError::RuntimeOwnerUnavailable);
    }

    #[test]
    fn r607_lookup_returns_not_bound_with_tag_when_missing() {
        let owner = Owner::new();
        let err =
            lookup::<ProbeState, _, _>(Some(&owner), "phantom", |_, _| ()).unwrap_err();
        assert_eq!(
            err,
            SubstrateIntrospectError::NotBound {
                tag: "phantom".into(),
            },
        );
    }

    #[test]
    fn r607_lookup_projects_cached_state_with_tag_echoed() {
        let owner = Owner::new();
        owner.cache::<ProbeState, _>("widget", || ProbeState(42));
        let out: (String, u32) = lookup::<ProbeState, _, _>(
            Some(&owner),
            "widget",
            |tag, s| (tag.to_owned(), s.0),
        )
        .unwrap();
        assert_eq!(out, ("widget".into(), 42));
    }

    #[test]
    fn r607_lookup_failed_miss_does_not_insert_slot() {
        let owner = Owner::new();
        let _ = lookup::<ProbeState, _, _>(Some(&owner), "ghost", |_, _| ()).unwrap_err();
        assert!(!owner.cache_contains::<ProbeState>("ghost"));
    }

    #[test]
    fn r607_introspect_error_data_round_trips_with_each_variant() {
        assert_eq!(
            introspect_error_to_data(&SubstrateIntrospectError::RuntimeOwnerUnavailable),
            "RuntimeOwnerUnavailable",
        );
        assert_eq!(
            introspect_error_to_data(&SubstrateIntrospectError::TagRequired),
            "TagRequired",
        );
        assert_eq!(
            introspect_error_to_data(&SubstrateIntrospectError::NotBound {
                tag: "any".into()
            }),
            "NotBound",
        );
    }
}
