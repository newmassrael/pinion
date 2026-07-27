//! R51.178 §5.41 §5.23 R27 — TUI-side test fixtures.
//!
//! Pairs with `pinion_a11y::test_fixtures` (which carries the blank
//! `WidgetA11y` impls for `pinion_core::test_fixtures::
//! {ButtonFixture, EchoButtonFixture}`) and
//! `pinion_shell::test_fixtures` (which carries the Vello-side
//! `WidgetView` impls). The three together let both backends drive
//! the same fixtures through their own dispatch / paint paths, so
//! the test suites assert identical R27 behaviour without
//! reimplementing the carrier each time.
//!
//! Gated behind the `test-fixtures` feature so production binaries
//! never compile this module; the substrate test module picks it up
//! through the self-`dev-dependencies` path in `Cargo.toml`.
//!
//! ## Why the impl lives here, not in `pinion-core`
//!
//! Orphan rule: [`WidgetViewTui`] is defined in this crate (it's the
//! TUI-specific supertrait), the fixtures live in `pinion-core`. A
//! downstream test crate cannot `impl WidgetViewTui for ButtonFixture`
//! directly without violating coherence. The textbook resolution
//! mirrors the `pinion-shell::test_fixtures` precedent (R51.175) and
//! the `pinion-a11y::test_fixtures` precedent (R51.129, R51.168):
//! keep the impl in whichever crate owns the trait — `pinion-tui`
//! for the TUI side — behind a feature gate that forwards
//! `pinion-core/test-fixtures` and `pinion-a11y/test-fixtures` so the
//! type symbols and the supertrait impls are both in scope at the
//! same time this impl compiles.

use pinion_core::test_fixtures::{
    ButtonFixture, ContextMenuFixture, EchoButtonFixture, ModalTailFixture, ScrollbarMultiFixture,
};
use ratatui::backend::TestBackend;

use crate::widget::WidgetViewTui;

/// R51.178 §5.41 — TUI-side `WidgetViewTui` impl for the canonical
/// atomic Button binding. Pairs with the [`WidgetA11y`] blank impl
/// in `pinion-a11y::test_fixtures` and the `WidgetCore` body in
/// `pinion-core::test_fixtures`. The substrate test module drove
/// the same impl inline pre-R51.178; this module is the lift
/// position so both R51.168 / R51.169 wiring sub-modules and the
/// upstream `mod tests` block reuse one canonical declaration.
///
/// `TestBackend` is the ratatui-native off-screen backend; the
/// dispatch tests never paint to a real terminal, so the choice is
/// observationally inert.
///
/// [`WidgetA11y`]: pinion_a11y::WidgetA11y
impl WidgetViewTui for ButtonFixture {
    type Renderer = crate::TuiRenderer<TestBackend>;
}

/// R51.178 §5.41 §5.23 R27 — TUI-side `WidgetViewTui` impl for the
/// shared reducer fixture. Pairs with the [`crate::widget::WidgetViewTui`]
/// blank impl pattern above. Drives the R51.168 incoming /
/// R51.169 drain reducer wiring tests on the TUI side, matching the
/// `pinion_shell::test_fixtures` Vello-side impl byte-for-byte
/// behaviourally.
impl WidgetViewTui for EchoButtonFixture {
    type Renderer = crate::TuiRenderer<TestBackend>;
}

/// R884 §5.41 §5.45 — TUI-side `WidgetViewTui` impl for the
/// multi-External composition fixture, so the TUI `dispatch_intent`
/// producer pins the Container-root send invariant
/// (`CoreShell::send_to_primary`) through its own wiring path.
impl WidgetViewTui for ScrollbarMultiFixture {
    type Renderer = crate::TuiRenderer<TestBackend>;
}

/// R887 §5.49 §5.53 — TUI-side `WidgetViewTui` impl for the
/// secondary-click fixture, so both TUI producers of the right-click
/// arc (the `DeferredInput::SecondaryClick` drain and the crossterm
/// `Down(Right)` arm, both through `ShellCoreTui::secondary_click`)
/// pin the same popup-opens-at-press-point observable the Vello
/// sibling pins.
impl WidgetViewTui for ContextMenuFixture {
    type Renderer = crate::TuiRenderer<TestBackend>;
}

/// R1456 R1462 §5.41 §5.39 — TUI-side `WidgetViewTui` impl for the
/// dispatch-tail modal-focus fixture, so the terminal backend drives the
/// modal drain through its OWN `handle_tail` wiring rather than trusting
/// the Vello side's result. §2 #6: the two backends share the
/// `modal_scope_request` / `focus_request` mailboxes, so an untested
/// mirror is the class of defect where GUI and TUI end up with different
/// focus — and different modal stacks — from identical input.
impl WidgetViewTui for ModalTailFixture {
    type Renderer = crate::TuiRenderer<TestBackend>;
}
