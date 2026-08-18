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

use pinion_core::test_fixtures::{
    ButtonFixture, ContextMenuFixture, EchoButtonFixture, ModalTailFixture, NoPrimaryFixture,
    RawSinkFocusFixture, RepeatingButtonFixture, ScrollbarMultiFixture, ShadowingFixture,
};

use crate::widget_a11y::WidgetA11y;

impl WidgetA11y for ButtonFixture {}

/// R1569 §5.39 — atomic-default impl for the accelerator-shadow fixture; the
/// orphan-rule placement rationale above applies identically.
impl WidgetA11y for ShadowingFixture {}

/// R884 §5.40 §5.45 — atomic-default `WidgetA11y` impl for the
/// multi-External composition fixture [`ScrollbarMultiFixture`].
/// Same default-empty shape as [`ButtonFixture`]; the orphan-rule
/// placement rationale applies identically (trait lives here, the
/// fixture lives in `pinion-core`).
impl WidgetA11y for ScrollbarMultiFixture {}

/// R51.168 §5.40 §5.23 — atomic-default `WidgetA11y` impl for the
/// reducer test fixture [`EchoButtonFixture`]. Same default-empty
/// shape as [`ButtonFixture`]; the orphan-rule placement rationale
/// applies identically (trait lives here, fixture lives in
/// `pinion-core`).
impl WidgetA11y for EchoButtonFixture {}

/// R887 §5.40 §5.53 — atomic-default `WidgetA11y` impl for the
/// secondary-click fixture [`ContextMenuFixture`]. Same default-empty
/// shape as [`ButtonFixture`]; the orphan-rule placement rationale
/// applies identically (trait lives here, fixture lives in
/// `pinion-core`).
impl WidgetA11y for ContextMenuFixture {}

/// R1456 R1462 §5.40 §5.39 — atomic-default `WidgetA11y` impl for the
/// dispatch-tail modal-focus fixture [`ModalTailFixture`]. Same
/// default-empty shape as [`ButtonFixture`]; the orphan-rule placement
/// rationale applies identically (trait lives here, fixture lives in
/// `pinion-core`).
impl WidgetA11y for ModalTailFixture {}

/// R1715 §5.40 §5.39 — atomic-default `WidgetA11y` impl for the raw-edge
/// focus fixture [`RawSinkFocusFixture`]. Same default-empty shape as
/// [`ButtonFixture`]; the orphan-rule placement rationale applies identically
/// (trait lives here, fixture lives in `pinion-core`).
impl WidgetA11y for RawSinkFocusFixture {}

/// R1715.1 §5.40 §5.16 — atomic-default `WidgetA11y` impl for the no-primary
/// topology fixture [`NoPrimaryFixture`].
impl WidgetA11y for NoPrimaryFixture {}

/// R1549.2 §2 #6 — the repeating-button fixture's blank impl, so a TUI
/// test can drive a held press through the same trait stack.
impl WidgetA11y for RepeatingButtonFixture {}
