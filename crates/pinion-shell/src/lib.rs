//! R51.30 `§5.16` + `§5.20` + `§5.35` paint-side `AppShell` — the
//! framework response to the [[substrate-incompleteness-signal]]
//! surfaced by R51.29 (hello-button + hello-toggle had ~400 LOC of
//! identical App boilerplate). Every Vello-backed visual binary now
//! reduces to:
//!
//! ```rust,ignore
//! pinion_shell::vello_renderer_impl!(HelloFooRenderer, HelloFooRendererError);
//!
//! struct FooView;
//! impl pinion_shell::WidgetView for FooView { /* widget-specific bits */ }
//!
//! fn main() { pinion_shell::run::<FooView>(); }
//! ```
//!
//! The shell owns the [`AppShell`] struct (scene, cached state,
//! [`IntentQueue`](pinion_runtime::IntentQueue), [`PreviewLedger`],
//! [`SceneRevision`](pinion_core::SceneRevision),
//! [`InputRouter`](pinion_runtime::InputRouter), [`LayoutCache`],
//! reusable [`vello::Scene`] buffer, last-paint-layout snapshot), the
//! [`RenderState`] suspend/resume ADT (R46.3.4), the JSON-RPC stdin
//! reader thread, and the [`winit::application::ApplicationHandler`]
//! impl that wires pointer events through the input router and routes
//! `scene/layout` / `scene/resize` through [`DispatchContext`] —
//! every step then flows through the `§6.3` view-fn → paint → vello
//! submission loop. The application supplies only the widget-specific
//! diff via [`WidgetView`] (state shape, event enum, view fn,
//! introspect parser, optional keybindings, window title / size).
//!
//! Substrate-incompleteness-signal lesson encoded:
//!
//! * **substrate land 직후 application 첫 진입 시 boilerplate 의
//!   incompleteness signal** → R51.29 was that signal; this round is
//!   the immediate refactor response.
//! * **둘째 client 진행 금지 (substrate 미완성 시)** — going forward,
//!   any new visual binary must be tried against `pinion_shell::run::<V>()`
//!   first; if 5+ LOC of `App` boilerplate creeps back in, the next
//!   round refactors the shell instead of adding the binary.
//!
//! The shell is intentionally Vello-only. TUI / headless / future
//! `pinion-render-*` alternatives go through a different shell or
//! none at all — there is no `Renderer` trait abstracting over both
//! Vello and (say) a terminal backend, because the lifecycle / event
//! model / surface ownership differ enough that a unified abstraction
//! would leak. The §6.3 view-fn purity invariant is the cross-shell
//! contract; everything else is shell-local.

use std::sync::Arc;

use vello::peniko::Color as PenikoColor;
use vello::Scene as VelloScene;
use winit::window::Window;

mod app;
mod substrate;
pub mod typeahead;

pub use app::{run, AppShell};
pub use substrate::{AccessEmitDecision, ShellCore};

/// Winit user-event variants that reach the UI thread out-of-band.
///
/// The shell's [`AppShell::user_event`] handler is the sole consumer;
/// producers are the stdin reader thread ([`AppEvent::RpcRequest`])
/// and the `accesskit_winit` adapter ([`AppEvent::AccessKit`], R51.62
/// §5.40 wiring).
///
/// `Clone` is intentionally absent: `accesskit_winit::Event` is not
/// `Clone`, and the shell never duplicates a user event in-flight.
#[derive(Debug)]
pub enum AppEvent {
    /// One JSON-RPC 2.0 frame read from stdin, awaiting dispatch.
    RpcRequest(String),
    /// R51.62 §5.40 — AT-side accessibility event delivered by
    /// `accesskit_winit`. Carries `InitialTreeRequested` (AT first
    /// connected — shell answers with a redraw, which calls
    /// `Adapter::update_if_active` to emit the current tree),
    /// `ActionRequested` (AT-side `Click` / `Focus` / `Increment` /
    /// `Decrement`, routed to the widget's intent surface — R51.67
    /// dispatch wiring), or `AccessibilityDeactivated` (AT
    /// disconnected — adapter remains in place for the next attach).
    AccessKit(accesskit_winit::Event),
}

impl From<accesskit_winit::Event> for AppEvent {
    fn from(event: accesskit_winit::Event) -> Self {
        Self::AccessKit(event)
    }
}

/// R51.109.1 §5.41 — Vello-specific frame render context.
///
/// Carries the application-level paint hint (window background base
/// color) that the Vello pipeline needs at frame submit. `Copy` so
/// the shell can pass it by value per frame without lifetime
/// gymnastics; `Default::default()` returns opaque black which is
/// also the fallback `paint_adapter::root_background` returns when
/// no Container fill is set — the shell overrides via
/// `paint_adapter::root_background(&paint_scene)` on every frame
/// when a real fill is present.
///
/// This is the Vello specialization of [`WidgetRenderer::Context`];
/// the parallel `TuiContext` (R51.109.2 in `pinion-tui`) carries the
/// TUI palette / capability hints. Single-typed `Context` per
/// backend keeps the substrate dispatch backend-agnostic while
/// preserving the zero-virtual-dispatch guarantee.
#[derive(Debug, Clone, Copy)]
pub struct VelloContext {
    /// Window background color sampled from
    /// `paint_adapter::root_background(&paint_scene)`. Vello clears
    /// the surface to this color on `render` submit so the visible
    /// frame matches the paint scene's root Container fill.
    pub base_color: PenikoColor,
}

impl Default for VelloContext {
    fn default() -> Self {
        // `PenikoColor::BLACK` matches `paint_adapter::root_background`'s
        // fallback for scenes without a root Container fill — keeps
        // the substrate's `WidgetRenderer::Context: Copy` constraint
        // trivially constructible without forcing a non-trivial
        // sentinel through every caller.
        Self {
            base_color: PenikoColor::BLACK,
        }
    }
}

// R51.109.2 §5.41 — `WidgetRenderer` trait moved to `pinion-core` so
// the TUI backend crate (`pinion-tui`) can implement it without
// transitively pulling Vello / wgpu through pinion-shell. Re-export
// preserves every existing `pinion_shell::WidgetRenderer` callsite
// (app.rs render dispatch, `vello_renderer_impl!` macro path).
pub use pinion_core::WidgetRenderer;

/// R51.109.1 §5.41 — Vello specialization of [`WidgetRenderer`].
///
/// Locks `Frame = vello::Scene` and `Context = VelloContext` so every
/// Vello binding ships the same dispatch surface, plus the
/// Vello-specific async wgpu surface constructor. The wrapper trait
/// the pinion-forge codegen output (`HelloFooRenderer` +
/// `HelloFooRendererError`) bridges into via the
/// [`vello_renderer_impl!`] macro; the shell is generic over `R:
/// VelloRenderer` so each binary keeps the zero-virtual-dispatch
/// Vello pipeline §5.16 R45 guarantees — there is no `dyn
/// VelloRenderer` anywhere in the hot path.
///
/// The shape mirrors the codegen template (R46.3.3) — async `new`,
/// sync `render` + `resize` (the latter two inherited from
/// `WidgetRenderer`), `Sized` so the shell can store `Box<Self>` in
/// [`RenderState::Active`] without pulling object-safety constraints
/// in. `Error: Display` so the shell can `eprintln!` any failure
/// without forcing the application into the error type.
pub trait VelloRenderer:
    WidgetRenderer<Frame = VelloScene, Context = VelloContext> + Sized
{
    /// Initialize the Vello renderer against a wgpu surface target.
    /// Async because wgpu adapter + device acquisition is async; the
    /// shell wraps the future in `pollster::block_on` at the §6.3
    /// boundary (app boot, not a render closure).
    ///
    /// # Errors
    /// Implementation-defined per the codegen template — surface
    /// creation, adapter request, or Vello renderer init failure.
    fn new<W>(
        target: W,
        width: u32,
        height: u32,
    ) -> impl core::future::Future<Output = Result<Self, Self::Error>>
    where
        W: Into<vello::wgpu::SurfaceTarget<'static>>;
}

/// Bridge a pinion-forge-emitted renderer struct into the
/// [`WidgetRenderer`] + [`VelloRenderer`] trait pair. The codegen
/// template emits inherent methods (`async fn new<W>(...)`,
/// `fn render(...)`, `fn resize(...)`) matching the substrate's
/// signature byte-for-byte; this macro generates two thin trait-impls
/// (one for the backend-agnostic `WidgetRenderer`, one for the
/// Vello-specialised `VelloRenderer`) that forward each method call
/// to the inherent one. Keeps the codegen template free of any
/// pinion-shell coupling (consumers without the shell can still use
/// the renderer).
///
/// # Example
///
/// ```rust,ignore
/// include!(concat!(env!("OUT_DIR"), "/app.rs"));
/// pinion_shell::vello_renderer_impl!(HelloButtonRenderer, HelloButtonRendererError);
/// ```
#[macro_export]
macro_rules! vello_renderer_impl {
    ($name:ident, $err:ident) => {
        impl $crate::WidgetRenderer for $name {
            type Error = $err;
            type Frame = ::vello::Scene;
            type Context = $crate::VelloContext;

            fn render(
                &mut self,
                frame: &::vello::Scene,
                ctx: $crate::VelloContext,
            ) -> ::core::result::Result<(), $err> {
                <$name>::render(self, frame, ctx.base_color)
            }

            fn resize(&mut self, width: u32, height: u32) {
                <$name>::resize(self, width, height);
            }
        }

        impl $crate::VelloRenderer for $name {
            async fn new<W>(
                target: W,
                width: u32,
                height: u32,
            ) -> ::core::result::Result<Self, $err>
            where
                W: ::core::convert::Into<::vello::wgpu::SurfaceTarget<'static>>,
            {
                <$name>::new(target, width, height).await
            }
        }
    };
}

/// R51.121 §5.41 — Vello-specific application-supplied widget binding.
///
/// Each visual binary implements this once on a unit type;
/// `pinion_shell::run::<MyView>()` does the rest.
///
/// The trait inherits the bulk of its surface via the supertrait
/// chain [`pinion_a11y::WidgetA11y`] → [`pinion_core::WidgetCore`];
/// only the Vello-specific [`Renderer`](Self::Renderer) associated
/// type and the pixel-unit [`initial_size`](Self::initial_size) live
/// here. The application-side binding therefore declares one impl
/// block per trait (typical breakdown: 9 methods in `WidgetCore`, 1-3
/// in `WidgetA11y`, 2 here).
///
/// The supertrait split lets the ratatui TUI backend
/// (`pinion_tui::WidgetViewTui`) reuse the same `WidgetCore` +
/// `WidgetA11y` surface, replacing only the Vello-specific items
/// here with `Frame = Buffer` and cell-unit `initial_size`.
pub trait WidgetView: pinion_a11y::WidgetA11y {
    /// Concrete pinion-forge-emitted renderer (`HelloFooRenderer`).
    /// `'static` so [`RenderState`] can store `Box<Self::Renderer>`
    /// across the suspend/resume cycle without lifetime parameters.
    type Renderer: VelloRenderer + 'static;

    /// Default window dimensions in logical pixels. `winit` applies
    /// the per-monitor DPI scale, so this is "what the user sees" on
    /// a 1.0× display. The shell honours this exactly on first
    /// [`resumed`](AppShell::resumed); subsequent resizes go through
    /// `WindowEvent::Resized`.
    fn initial_size() -> (u32, u32);
}

/// Window + renderer lifecycle (R46.3.4 §5.16). Mirrors the Vello 0.6
/// canonical `RenderState` enum (Linebender examples / Xilem) so the
/// shell survives the Android / Wayland suspend → resume cycle where
/// the wgpu surface backing must be dropped and re-created. Desktop
/// targets fire `resumed` once at boot and never `suspended`; mobile
/// targets fire it on every focus change. `Suspended(Some(window))`
/// caches the winit `Window` across the drop-and-recreate cycle.
pub enum RenderState<R: VelloRenderer> {
    /// GPU resources live; ready to paint frames.
    Active {
        window: Arc<Window>,
        /// Boxed because the renderer struct is ~1.5 KiB (wgpu / vello
        /// state) while `Suspended` is two words; without the indirection
        /// the whole enum would pay the larger size
        /// (clippy `large_enum_variant`, R47.1.1 §5.36 MSRV 1.88 fix).
        renderer: Box<R>,
    },
    /// GPU released; window may be cached for the next resume.
    Suspended(Option<Arc<Window>>),
}
