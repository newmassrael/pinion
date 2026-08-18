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

use pinion_core::test_fixtures::{
    ContextMenuFixture, EchoButtonFixture, ModalTailFixture, NoPrimaryFixture, RawSinkFocusFixture,
    RepeatingButtonFixture, ScrollbarMultiFixture,
};

use crate::{VelloContext, VelloRenderer, WidgetRenderer, WidgetView};

/// R51.175 §5.41 — minimal `VelloRenderer`-conforming renderer for
/// fixture tests. Mirrors the pinion-forge codegen template: an
/// inherent `async new` / `render` / `resize` / `capture_rgba8` set
/// bridged into the trait pair (by hand since R1060 — a GPU-less stub
/// cannot use the field-access `vello_renderer_impl!` macro) to satisfy
/// the `WidgetView::Renderer` bound. The shell's dispatch path never
/// touches the renderer, so the bodies are inert. Construction never
/// fails — the error variant is an uninhabited enum so the `?` operator
/// type-checks without forcing the caller to enumerate cases.
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
    pub async fn new<W>(_target: W, _width: u32, _height: u32) -> Result<Self, TestRendererError>
    where
        W: Into<vello::wgpu::SurfaceTarget<'static>> + Clone + 'static,
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

    /// R1060 §5.16 — stub capture: this fixture owns no live wgpu
    /// surface (it never enters the `run` loop), so the present-stage
    /// readback has nothing to read. Reports `SurfaceUnavailable` so a
    /// dispatch test that reached the capture path sees a typed,
    /// honest error rather than a fabricated frame.
    ///
    /// # Errors
    ///
    /// Always [`crate::vello_capture::SurfaceCaptureError::SurfaceUnavailable`]
    /// — the fixture has no live surface to read back.
    #[allow(clippy::unused_self)]
    pub fn capture_rgba8(
        &mut self,
        _scene: &vello::Scene,
        _base: vello::peniko::Color,
    ) -> Result<crate::vello_capture::CapturedFrame, crate::vello_capture::SurfaceCaptureError>
    {
        Err(
            crate::vello_capture::SurfaceCaptureError::SurfaceUnavailable(
                "TestRenderer: no live surface",
            ),
        )
    }
}

// R1060 §5.16 — GPU-less stubs cannot use `vello_renderer_impl!` (it
// hands the renderer's wgpu fields to the surface-capture SSOT, and
// this fixture has none), so the trait pair is bridged by hand. Every
// method still forwards to the inherent stub above, so the fixture
// exercises the full `VelloRenderer` trait surface (a trait-shape
// regression breaks this impl exactly as it would the macro).
impl WidgetRenderer for TestRenderer {
    type Error = TestRendererError;
    type Frame = vello::Scene;
    type Context = VelloContext;

    fn render(&mut self, frame: &vello::Scene, ctx: VelloContext) -> Result<(), TestRendererError> {
        TestRenderer::render(self, frame, ctx.base_color)
    }

    fn resize(&mut self, width: u32, height: u32) {
        TestRenderer::resize(self, width, height);
    }
}

impl VelloRenderer for TestRenderer {
    /// R1361.1 — a surface-less stub never waits on a swapchain, so it
    /// blocks for 0µs. Stated per-impl rather than inherited from a trait
    /// default: the default made "this backend cannot block" the silent
    /// assumption for EVERY implementor, including ones that can.
    fn last_acquire_us(&self) -> u64 {
        0
    }

    /// R1537 — `Unsupported`, which is the literal truth: this fixture holds
    /// no device, so no waiting will ever produce a timing. `Pending`
    /// would promise one that never arrives, and a `Some(0)` would let a
    /// test assert a GPU measurement no GPU produced.
    fn gpu_clock(&mut self) -> pinion_gpu::GpuFrameClock {
        pinion_gpu::GpuFrameClock::Unsupported
    }

    /// R1537 — a fixture with no device takes no measurements, so it discards
    /// none. `0` here is a measured zero, not a stand-in for absence.
    fn gpu_dropped_samples(&self) -> u64 {
        0
    }

    /// R1709 — the default is the literal truth here rather than a
    /// convenience: this fixture holds no swapchain, so no frame of its has
    /// ever failed to reach a screen and no recovery has ever been owed.
    /// Stated per-impl for the reason the two above are — a trait default
    /// would make "this backend always presents" the silent assumption for
    /// every implementor, including the one that cannot.
    fn surface_health(&self) -> pinion_gpu::SurfaceHealth {
        pinion_gpu::SurfaceHealth::default()
    }

    async fn new<W>(target: W, width: u32, height: u32) -> Result<Self, TestRendererError>
    where
        W: Into<vello::wgpu::SurfaceTarget<'static>> + Clone + 'static,
    {
        TestRenderer::new(target, width, height).await
    }

    fn capture_rgba8(
        &mut self,
        scene: &vello::Scene,
        base_color: vello::peniko::Color,
    ) -> Result<crate::vello_capture::CapturedFrame, crate::vello_capture::SurfaceCaptureError>
    {
        TestRenderer::capture_rgba8(self, scene, base_color)
    }
}

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
        crate::SizeStrategy::Fixed {
            width: 8,
            height: 8,
        }
    }
}

/// R884 §5.41 §5.45 — Vello-side `WidgetView` impl for the
/// multi-External composition fixture, so the shell's
/// `dispatch_intent` producer pins the Container-root send invariant
/// (`CoreShell::send_to_primary`) through its own wiring path.
impl WidgetView for ScrollbarMultiFixture {
    type Renderer = TestRenderer;

    fn initial_size_strategy() -> crate::SizeStrategy {
        crate::SizeStrategy::Fixed {
            width: 8,
            height: 8,
        }
    }
}

/// R887 §5.49 §5.53 — Vello-side `WidgetView` impl for the
/// secondary-click fixture, so the shell's
/// `DeferredInput::SecondaryClick` drain producer pins the
/// right-click arc (`secondary_click_for_window` →
/// `apply_secondary_click`) through its own wiring path.
impl WidgetView for ContextMenuFixture {
    type Renderer = TestRenderer;

    fn initial_size_strategy() -> crate::SizeStrategy {
        crate::SizeStrategy::Fixed {
            width: 8,
            height: 8,
        }
    }
}

/// R1456 R1462 §5.39 — Vello-side `WidgetView` impl for the dispatch-tail
/// modal-focus fixture, so the shell's `handle_tail` drain is tested
/// against the same binding the TUI mirror uses.
impl WidgetView for ModalTailFixture {
    type Renderer = TestRenderer;

    fn initial_size_strategy() -> crate::SizeStrategy {
        crate::SizeStrategy::Fixed {
            width: 8,
            height: 8,
        }
    }
}

/// R1715 §5.39 §5.35 §5.15 — Vello-side `WidgetView` impl for the raw-edge
/// focus fixture, so `ShellCore::pointer_button_for_window` — the one seam
/// both the native `MouseInput` path and the `scene/pointer_button` RPC drain
/// cross — is driven with a raw sink in the scene. The surface is sized to the
/// fixture's two 40x40 focus stops so both are hit-testable.
impl WidgetView for RawSinkFocusFixture {
    type Renderer = TestRenderer;

    fn initial_size_strategy() -> crate::SizeStrategy {
        crate::SizeStrategy::Fixed {
            width: 80,
            height: 40,
        }
    }
}

/// R1715.1 (R1306 PR-51) §5.16 — Vello-side `WidgetView` impl for the
/// no-primary topology fixture, so the shell's OWN suite paints a binding
/// whose `tag()` is `unreachable!`. R1714 reached one from the paint path and
/// only CI found out, because the topology's dedicated example never paints.
impl WidgetView for NoPrimaryFixture {
    type Renderer = TestRenderer;

    fn initial_size_strategy() -> crate::SizeStrategy {
        crate::SizeStrategy::Fixed {
            width: 80,
            height: 40,
        }
    }
}

/// R1549.3 §5.41 §5.35 §2 #6 — Vello-side `WidgetView` impl for the
/// repeating-button fixture, the mirror of the `pinion-tui` one.
///
/// R1549.2 added the fixture for the TUI and stopped there, which left
/// the GUI's own LIVE paint-clock arm with no fixture that could reach
/// it — measured: deleting that arm outright kept every test AND the
/// 58-assertion demo green, because the demo pauses the window
/// (`set_fps 0`) for determinism and so exercises only the injected
/// `scene/tick` arm. The path a real mouse takes was the one nothing
/// covered.
impl WidgetView for RepeatingButtonFixture {
    type Renderer = TestRenderer;

    fn initial_size_strategy() -> crate::SizeStrategy {
        crate::SizeStrategy::Fixed {
            width: 200,
            height: 200,
        }
    }
}
