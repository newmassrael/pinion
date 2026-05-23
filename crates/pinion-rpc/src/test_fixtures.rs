//! R622 §5.7 §5.22 — crate-internal test fixtures shared across
//! the per-axis test modules.
//!
//! R633 §5.7 §5.22 — the substrate trait + impls now live in
//! [`pinion_core::test_fixtures`] under the `test-fixtures` feature
//! gate (forward dep direction); this module retains the
//! crate-local `bind_state` thin wrapper + `CacheBindable` alias so
//! the per-axis sites continue to compile against the pre-R633 name
//! while the abstraction itself rides the upstream trait.
//!
//! Pre-R622 every `pinion-rpc` axis test module (`scroll_state`,
//! `text_state`, `caret_state`, `theme`) carried a near-identical
//! `bind_X` helper:
//!
//! ```text
//! fn bind_state(owner: &Owner, tag: &'static str) -> Rc<S> {
//!     owner.run(|| use_X(tag))
//! }
//! ```
//!
//! Per [[three-site-internal-duplication-substrate-lift]] the rule
//! of three was crossed at the 3rd module; R622 cleared the
//! deferred lift; R633 flips the dep direction so `pinion-core`
//! owns the trait and `pinion-rpc` consumes it via dev-deps.
//!
//! ## Re-export shape
//!
//! All four pre-R633 widget impls (`ScrollState` / `TextEditState`
//! / `ThemeProvider` / `CaretBlink`) now live in
//! [`pinion_core::test_fixtures`] under the same `test-fixtures`
//! feature; this module re-exports the helper `bind_cache_slot`
//! under its pre-R633 name (`bind_state`) so the per-axis test
//! sites (`scroll_state.rs::tests::bind_state` /
//! `text_state.rs::tests::bind_state` / …) continue to compile.
//! The `BindableCacheSlot` trait is reachable through its
//! upstream path; downstream call sites only use the helper.

pub(crate) use pinion_core::test_fixtures::bind_cache_slot as bind_state;
