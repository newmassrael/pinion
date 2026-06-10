//! R51.175 §5.41 §5.23 R27 — Vello-side test fixtures.
//!
//! Pairs with `pinion_a11y::test_fixtures` (which carries the blank
//! `WidgetA11y` impl for `pinion_core::test_fixtures::EchoButtonFixture`)
//! and `pinion_tui::test_fixtures::EchoButtonFixture` (which carries
//! the TUI-side `WidgetViewTui` impl). The three together let both
//! backends drive the same reducer fixture through their own
//! `dispatch_intent` / `handle_tail` wiring paths, so the shell-side
//! and TUI-side test suites assert identical R27 behaviour without
//! reimplementing the carrier each time.
//!
//! Gated behind the `test-fixtures` feature so production binaries
//! never compile this module; the integration test suite picks it up
//! through the self-`dev-dependencies` path in `Cargo.toml`.
//!
//! ## Why the impl lives here, not in `pinion-core`
//!
//! Orphan rule: `WidgetView` is defined in this crate (it's the
//! Vello-specific supertrait), `EchoButtonFixture` lives in
//! `pinion-core`. A downstream test crate cannot
//! `impl WidgetView for EchoButtonFixture` directly without
//! violating coherence. The textbook resolution mirrors the
//! `pinion-a11y::test_fixtures` precedent (R51.129, R51.168): keep
//! the impl in whichever crate owns the trait — `pinion-shell` for
//! the Vello side — behind a feature gate that forwards
//! `pinion-core/test-fixtures` and `pinion-a11y/test-fixtures` so
//! the type symbol and the supertrait impl are both in scope at the
//! same time this impl compiles.

use core::fmt;

use pinion_core::test_fixtures::{EchoButtonFixture, ScrollbarMultiFixture};

use crate::{vello_renderer_impl, WidgetView};

/// R51.175 §5.41 — minimal `VelloRenderer`-conforming renderer for
/// fixture tests. Mirrors the pinion-forge codegen template: an
/// inherent `async new` / `render` / `resize` triple wrapped by
/// `vello_renderer_impl!` to satisfy the `WidgetView::Renderer`
/// bound. The shell's dispatch path never touches the renderer, so
/// the bodies are inert. Construction never fails — the error
/// variant is an uninhabited enum so the `?` operator type-checks
/// without forcing the caller to enumerate cases.
pub struct TestRenderer;

/// Uninhabited error type for [`TestRenderer`] — no runtime case
/// can construct it, so every error branch in the integration test
/// suite is statically unreachable.
#[derive(Debug)]
pub enum TestRendererError {}

impl fmt::Display for TestRendererError {
    fn fmt(&self, _f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {}
    }
}

impl std::error::Error for TestRendererError {}

impl TestRenderer {
    /// Mirrors the `vello_renderer_impl!`-bridged signature; never
    /// actually awaited by the dispatch tests (the shell's `run`
    /// loop is not entered), but must compile against the
    /// `VelloRenderer::new` bound.
    ///
    /// # Errors
    ///
    /// Never errors — the [`TestRendererError`] enum is uninhabited.
    /// The `Result` shape exists only to match the
    /// [`crate::VelloRenderer::new`] contract.
    #[allow(clippy::unused_async)]
    pub async fn new<W>(
        _target: W,
        _width: u32,
        _height: u32,
    ) -> Result<Self, TestRendererError>
    where
        W: Into<vello::wgpu::SurfaceTarget<'static>>,
    {
        Ok(Self)
    }

    /// Inert render — dispatch tests never paint.
    ///
    /// # Errors
    ///
    /// Never errors — see [`Self::new`].
    #[allow(clippy::unused_self)]
    pub fn render(
        &mut self,
        _scene: &vello::Scene,
        _base: vello::peniko::Color,
    ) -> Result<(), TestRendererError> {
        Ok(())
    }

    /// Inert resize — dispatch tests never call this.
    #[allow(clippy::unused_self)]
    pub fn resize(&mut self, _w: u32, _h: u32) {}
}

vello_renderer_impl!(TestRenderer, TestRendererError);

/// R51.175 §5.41 §5.23 R27 — Vello-side `WidgetView` impl for the
/// shared reducer fixture. Pairs with
/// `pinion_tui::test_fixtures::EchoButtonFixture` (TUI side) so both
/// backends drive the same reducer through their own dispatch path.
///
/// The `8 × 8` logical size is the smallest the substrate accepts
/// without rejecting the initial window dimensions; the wiring tests
/// never paint, so the exact value is observationally inert.
impl WidgetView for EchoButtonFixture {
    type Renderer = TestRenderer;

    fn initial_size_strategy() -> crate::SizeStrategy {
        crate::SizeStrategy::Fixed { width: 8, height: 8 }
    }
}

/// R884 §5.41 §5.45 — Vello-side `WidgetView` impl for the
/// multi-External composition fixture, so the shell's
/// `dispatch_intent` producer pins the Container-root send invariant
/// (`CoreShell::send_to_primary`) through its own wiring path.
impl WidgetView for ScrollbarMultiFixture {
    type Renderer = TestRenderer;

    fn initial_size_strategy() -> crate::SizeStrategy {
        crate::SizeStrategy::Fixed { width: 8, height: 8 }
    }
}
