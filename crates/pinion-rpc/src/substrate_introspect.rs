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
//!
//! ## R608+ write-side reuse
//!
//! The R608-R612 setter cascade (`set_theme_palettes` /
//! `set_scroll_offset` / `set_text` / `set_selection` / `set_caret`)
//! reuses [`lookup`] for the mutation pair — the closure receives a
//! `&S` reference and writes through interior mutability
//! ([`Signal`](pinion_core::reactive::Signal) `set`), then projects
//! the post-mutation state into the outcome. The helper's signature
//! does not need to change: Rust's borrow-checker permits the
//! interior-mutability write, and the `Owner::cache_get_by_str`
//! lookup + `NotBound { tag }` gate apply identically to read and
//! write paths.
//!
//! Four write-side consumers ratify the pattern as a textbook
//! canonical use of the helper. A dedicated `mutate_substrate`
//! alias was considered but deferred per
//! [[abstraction-needs-second-consumer]]: the signature would be
//! byte-identical and the cosmetic separation would not improve
//! call-site clarity — the action verb already lives in the closure
//! body (`state.scroll_to`, `state.set_text`, etc.).

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
/// The helper itself is side-effect-free in two specific senses:
/// [`Owner::cache_get_by_str`] never creates a slot on miss, and
/// no new reactive subscription is registered (`Owner::current` is
/// not activated by this helper, so a `Signal::get` inside
/// `project` reads the value without auto-subscribing the calling
/// scope).
///
/// `project` itself MAY write through interior mutability — the
/// R608+ write-side cascade calls `state.scroll_to(...)` /
/// `state.set_text(...)` / etc. inside the closure body, which
/// mutates the underlying [`Signal`](pinion_core::reactive::Signal)s
/// and schedules subscriber re-runs. The closure's mutations are
/// orthogonal to the helper's own no-side-effect contract — the
/// helper just borrows `&S` and hands it off; what `project` does
/// with that borrow is the caller's choice. See the module-level
/// "R608+ write-side reuse" section for the textbook canonical
/// pattern.
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
    let state: Rc<S> =
        owner
            .cache_get_by_str::<S>(tag)
            .ok_or_else(|| SubstrateIntrospectError::NotBound {
                tag: tag.to_owned(),
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
        let err = lookup::<ProbeState, _, _>(Some(&owner), "phantom", |_, _| ()).unwrap_err();
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
        let out: (String, u32) =
            lookup::<ProbeState, _, _>(Some(&owner), "widget", |tag, s| (tag.to_owned(), s.0))
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
            introspect_error_to_data(&SubstrateIntrospectError::NotBound { tag: "any".into() }),
            "NotBound",
        );
    }
}
