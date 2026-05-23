//! R622 §5.7 §5.22 — crate-internal test fixtures shared across
//! the per-axis test modules.
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
//! The 3-line body repeated 4 times across modules. Per
//! [[three-site-internal-duplication-substrate-lift]] the rule of
//! three was crossed at the 3rd module; R622 cleared the deferred
//! lift as part of the 7-debt cascade per the user directive.
//!
//! ## Design choice — trait + generic helper, not 4 named fns
//!
//! The shared `bind_state` is generic over [`CacheBindable`]:
//! `bind_state::<S>(&owner, tag)` resolves the `use_*` hook through
//! the trait impl, owner-runs it, and returns the `Rc<S>`. Per-type
//! `use_*` hooks live with their owning module (`pinion-core::theme::use_theme`,
//! `pinion-core::widgets::scroll::use_scroll_state`, …) — the trait
//! impls below are the test-only adapters that route them through
//! one shared `owner.run` wrapper.
//!
//! ## Why a trait, not 4 thin wrappers
//!
//! 4 named wrappers (`bind_scroll`, `bind_text`, `bind_theme`,
//! `bind_caret`) would still duplicate the `owner.run(|| use_X(tag))`
//! body — the lift would be cosmetic. The trait collapses the
//! `owner.run` glue into one place; each test site calls
//! `bind_state::<S>(...)` and the compiler picks the right hook via
//! the impl. Adding a 5th axis (R612 was already 4 — a future write
//! surface might add a 5th) lands as one trait impl, not a new
//! named function.

use pinion_core::reactive::Owner;
use pinion_core::theme::{use_theme, ThemeProvider};
use pinion_core::widgets::caret_blink::{use_caret_blink, CaretBlink};
use pinion_core::widgets::scroll::{use_scroll_state, ScrollState};
use pinion_core::widgets::text_edit::{use_text_edit_state, TextEditState};
use std::rc::Rc;

/// Test-only trait that routes a `'static`-keyed cache binding
/// through the appropriate `use_*` hook. Implemented for every
/// substrate-introspection axis the RPC layer wires.
///
/// `pinion-core`-side hooks (`use_theme`, `use_scroll_state`, …)
/// require an active [`Owner`] scope at call time; the wrapper
/// [`bind_state`] establishes the scope so each test site sees a
/// uniform `(owner, tag) -> Rc<S>` API.
pub(crate) trait CacheBindable: Sized + 'static {
    fn use_in_scope(tag: &'static str) -> Rc<Self>;
}

impl CacheBindable for ScrollState {
    fn use_in_scope(tag: &'static str) -> Rc<Self> {
        use_scroll_state(tag)
    }
}

impl CacheBindable for TextEditState {
    fn use_in_scope(tag: &'static str) -> Rc<Self> {
        use_text_edit_state(tag)
    }
}

impl CacheBindable for ThemeProvider {
    fn use_in_scope(tag: &'static str) -> Rc<Self> {
        use_theme(tag)
    }
}

impl CacheBindable for CaretBlink {
    fn use_in_scope(tag: &'static str) -> Rc<Self> {
        use_caret_blink(tag)
    }
}

/// Bind a substrate-introspection state slot under `tag` on `owner`.
/// Wraps [`CacheBindable::use_in_scope`] in [`Owner::run`] so each
/// per-axis test site collapses to a single line.
///
/// Generic over `S: CacheBindable`. Call as
/// `bind_state::<ScrollState>(&owner, "list")` (or
/// `bind_state::<_>(&owner, "list")` when the return type is
/// inferred from the binding's later use).
pub(crate) fn bind_state<S: CacheBindable>(
    owner: &Owner,
    tag: &'static str,
) -> Rc<S> {
    owner.run(|| S::use_in_scope(tag))
}
