//! R51.36 §5.16 + §5.38 — compile-only smoke test for the pinion-shell
//! substrate. Closes the R51.30 carry "//! example is `rust,ignore`":
//! the docstring stays a visual snippet (the
//! `async`-+-`Into<SurfaceTarget>` bounds make hidden-line doctest
//! stubs unwieldy), and this test provides the equivalent guarantee
//! that the [`VelloRenderer`] + [`WidgetView`] +
//! [`vello_renderer_impl`] + [`run`] surface type-checks end-to-end
//! against a minimal fixture independent of the five
//! `examples/hello-*` binaries.
//!
//! Strategy:
//!
//! * A `SmokeRenderer` struct + `SmokeRendererError` empty enum
//!   implement the inherent `new` / `render` / `resize` shape the
//!   pinion-forge codegen template emits, then route through
//!   [`vello_renderer_impl!`] to satisfy the [`VelloRenderer`] trait.
//! * A `SmokeExternal` opts in to [`ExternalIntrospect`] just enough
//!   to give [`WidgetView::read_state`] a state slot to query.
//! * A `SmokeView` implements [`WidgetView`] with `State = ()` —
//!   smallest possible cached projection.
//! * The `#[test]` does *not* call [`run`] (wgpu surface acquisition
//!   needs a real winit `EventLoop` running) — it captures the
//!   function pointer via `let _: fn() = run::<SmokeView>` so the
//!   trait constraints are exercised by the type checker. Any future
//!   regression that breaks the trait surface (e.g. a renamed
//!   associated type, a tightened bound, an `unsafe` slip) shows up
//!   as a compile error here at `cargo test --workspace` time.

// Stub-method shape is dictated by the codegen template / trait
// contract, not by what clippy can infer from the empty body alone.
// Scoping the allow to this fixture (rather than the trait itself)
// keeps the workspace baseline strict.
#![allow(clippy::unused_self, clippy::unnecessary_wraps)]

use core::fmt;

use pinion_a11y::WidgetA11y;
use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner, SchemaField, ThreadOwnership,
};
use pinion_core::scene::{BoxNode, ContainerNode, Rect};
use pinion_core::style::{BoxStyle, Color};
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::{VelloContext, VelloRenderer, WidgetRenderer, WidgetView, run};

/// Mirror of the pinion-forge codegen output: a renderer struct that
/// stores nothing (no actual wgpu surface — the smoke test never
/// calls `new`). The inherent methods match the template signature
/// byte-for-byte so `vello_renderer_impl!` succeeds.
struct SmokeRenderer;

/// Error variant mirroring the codegen template's
/// `HelloFooRendererError` enum. `Display` impl satisfies
/// [`VelloRenderer::Error`]'s `Display` bound; the smoke test never
/// constructs an instance, so the empty enum is exhaustively
/// pattern-matched in `Display`.
#[derive(Debug)]
enum SmokeRendererError {}

impl fmt::Display for SmokeRendererError {
    fn fmt(&self, _f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {}
    }
}

impl std::error::Error for SmokeRendererError {}

impl SmokeRenderer {
    /// Async constructor matching the codegen template's signature.
    /// Returns immediately with an empty `SmokeRenderer`; the smoke
    /// test never awaits this future.
    #[allow(clippy::unused_async)]
    async fn new<W>(_target: W, _width: u32, _height: u32) -> Result<Self, SmokeRendererError>
    where
        W: Into<vello::wgpu::SurfaceTarget<'static>>,
    {
        Ok(Self)
    }

    fn render(
        &mut self,
        _scene: &vello::Scene,
        _base_color: vello::peniko::Color,
    ) -> Result<(), SmokeRendererError> {
        Ok(())
    }

    fn resize(&mut self, _width: u32, _height: u32) {}

    /// R1060 §5.16 — stub capture mirroring the codegen template's
    /// `capture_rgba8` signature so `vello_renderer_impl!` type-checks.
    /// The smoke renderer owns no surface, so it reports the typed
    /// `SurfaceUnavailable` error rather than a fabricated frame.
    fn capture_rgba8(
        &mut self,
        _scene: &vello::Scene,
        _base_color: vello::peniko::Color,
    ) -> Result<
        pinion_shell::vello_capture::CapturedFrame,
        pinion_shell::vello_capture::SurfaceCaptureError,
    > {
        Err(
            pinion_shell::vello_capture::SurfaceCaptureError::SurfaceUnavailable(
                "SmokeRenderer: no live surface",
            ),
        )
    }
}

// R1060 §5.16 — GPU-less stub: hand-bridge the trait pair (the
// `vello_renderer_impl!` macro hands wgpu fields to the surface-capture
// SSOT, which this surfaceless smoke renderer lacks). Forwarding every
// method to the inherent stubs still type-checks the full
// `VelloRenderer` trait surface — the smoke test's whole purpose.
impl WidgetRenderer for SmokeRenderer {
    type Error = SmokeRendererError;
    type Frame = vello::Scene;
    type Context = VelloContext;

    fn render(
        &mut self,
        frame: &vello::Scene,
        ctx: VelloContext,
    ) -> Result<(), SmokeRendererError> {
        SmokeRenderer::render(self, frame, ctx.base_color)
    }

    fn resize(&mut self, width: u32, height: u32) {
        SmokeRenderer::resize(self, width, height);
    }
}

impl VelloRenderer for SmokeRenderer {
    /// R1361.1 — a surface-less stub never waits on a swapchain, so it
    /// blocks for 0µs. Stated per-impl rather than inherited from a trait
    /// default: the default made "this backend cannot block" the silent
    /// assumption for EVERY implementor, including ones that can.
    fn last_acquire_us(&self) -> u64 {
        0
    }

    async fn new<W>(target: W, width: u32, height: u32) -> Result<Self, SmokeRendererError>
    where
        W: Into<vello::wgpu::SurfaceTarget<'static>>,
    {
        SmokeRenderer::new(target, width, height).await
    }

    fn capture_rgba8(
        &mut self,
        scene: &vello::Scene,
        base_color: vello::peniko::Color,
    ) -> Result<
        pinion_shell::vello_capture::CapturedFrame,
        pinion_shell::vello_capture::SurfaceCaptureError,
    > {
        SmokeRenderer::capture_rgba8(self, scene, base_color)
    }
}

/// Minimal [`External`] opting in to the introspect channel so
/// [`WidgetView::read_state`] has a query target. `value` is a
/// boolean stand-in for the cached state projection — the smoke
/// test does not exercise any state transitions.
#[derive(Debug, Default)]
struct SmokeExternal {
    value: bool,
}

impl External for SmokeExternal {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui, Backend::Rpc], BackendFallback::Skip)
    }
    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }
    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }
    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        Some(self)
    }
    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        Some(self)
    }
}

impl ExternalIntrospect for SmokeExternal {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(const { &[SchemaField::new("value", "bool")] })
    }
    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "value" => Some(IntrospectValue::Bool(self.value)),
            _ => None,
        }
    }
    fn intervene(&mut self, _path: &str, _value: IntrospectValue) -> Result<(), InterveneError> {
        Err(InterveneError::UnknownPath)
    }
    fn invoke(
        &mut self,
        _path: &str,
        _args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        Ok(IntrospectValue::Null)
    }
}

/// Minimal [`WidgetView`]: `State = ()` (no cached projection), event
/// payload is the unit type. The view fn paints a single solid-fill
/// rect tagged so the shell's
/// [`pinion_runtime::InputRouter`](pinion_runtime::InputRouter)
/// hit-tests against a non-empty surface.
struct SmokeView;

impl WidgetCore for SmokeView {
    type State = ();
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(SmokeExternal::default())
    }

    fn tag() -> &'static str {
        "smoke"
    }

    fn read_state(_scene: &Scene) -> Self::State {}

    fn view(_state: Self::State, _frame: &Frame) -> Scene {
        Scene::Container(
            ContainerNode::new(vec![Scene::Box(BoxNode::filled(
                Rect::default(),
                Color::rgb(0x00, 0x00, 0x00),
            ))])
            .with_tag("smoke")
            .with_style(BoxStyle::filled(Color::rgb(0x00, 0x00, 0x00))),
        )
    }

    fn event_name(_event: Self::Event) -> &'static str {
        "__smoke__"
    }

    fn title() -> &'static str {
        "smoke"
    }
}

impl WidgetA11y for SmokeView {}

impl WidgetView for SmokeView {
    type Renderer = SmokeRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: 8,
            height: 8,
        }
    }
}

/// Compile-only smoke: capture the function pointer of
/// `run::<SmokeView>` so every trait bound in [`WidgetView`] +
/// [`VelloRenderer`] is exercised by the type checker. Never invokes
/// `run` itself (would need a winit `EventLoop` + real wgpu surface).
/// Future regressions that break the trait surface fail at compile
/// time on `cargo test --workspace`.
#[test]
fn shell_substrate_type_checks_with_minimal_fixture() {
    let _: fn() = run::<SmokeView>;
}
