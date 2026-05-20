//! R51.129 §5.40 — atomic-default `WidgetA11y` impl for the shared
//! [`pinion_core::test_fixtures::ButtonFixture`].
//!
//! Lives in a dedicated module (not inside [`crate::widget_a11y`])
//! so the trait definition file stays focused on the trait itself.
//! Gated on the `test-fixtures` feature: production binaries never
//! see this impl.
//!
//! The impl body is intentionally empty — all three `WidgetA11y`
//! methods carry default bodies that are correct for atomic widgets,
//! and `ButtonFixture` is the canonical atomic Button binding.
//!
//! ## Why this module exists in `pinion-a11y` (not `pinion-core`)
//!
//! Orphan rule: `WidgetA11y` is defined here, `ButtonFixture` lives in
//! `pinion-core`. A downstream test crate cannot `impl WidgetA11y for
//! ButtonFixture` directly without violating coherence. The textbook
//! resolution is to keep the impl in whichever crate owns the trait —
//! `pinion-a11y` in this case — behind a feature gate that forwards
//! `pinion-core/test-fixtures` so the [`ButtonFixture`] symbol is in
//! scope at the same time the impl compiles.
//!
//! [`ButtonFixture`]: pinion_core::test_fixtures::ButtonFixture

use pinion_core::test_fixtures::ButtonFixture;

use crate::widget_a11y::WidgetA11y;

impl WidgetA11y for ButtonFixture {}
