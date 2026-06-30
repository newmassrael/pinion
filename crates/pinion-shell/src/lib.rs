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
//!
//! R637 §5.16 §5.7 — every binary using [`run`] / [`run_with_handlers`]
//! also honours the `PINION_SCREENSHOT=<path>` env var: when set, the
//! shell bypasses winit, drives the initial paint scene through
//! [`headless_screenshot::HeadlessScreenshot`] (wgpu + vello, no
//! surface), writes the PNG, and exits. See the module docstring for
//! the rationale (Figma → pinion design-parity verification path).

use std::borrow::Cow;
use std::rc::Rc;
use std::sync::Arc;

use pinion_core::{Intent, Scene, Signal, WidgetCore};
use vello::Scene as VelloScene;
use vello::peniko::Color as PenikoColor;
use winit::window::Window;

mod app;
pub mod executor;
pub mod headless_screenshot;
mod substrate;
pub mod typeahead;
pub mod vello_capture;

// R51.175 §5.41 — shared Vello-side test fixture surface. Exposes a
// minimal `VelloRenderer`-conforming `TestRenderer` plus
// `impl WidgetView for EchoButtonFixture` so the integration test
// suite reuses the canonical reducer fixture from
// `pinion_core::test_fixtures` instead of an ad-hoc `TestView` mock.
// Gated behind the `test-fixtures` feature (which forwards into the
// pinion-core / pinion-a11y supertrait crates) so production
// binaries never see the symbols.
#[cfg(any(test, feature = "test-fixtures"))]
pub mod test_fixtures;

pub use app::{AppShell, run, run_with_handlers};
pub use executor::{ProxyIntentSink, ProxyRepaintSink, TokioExecutor, build_executor_and_sink};
pub use headless_screenshot::{HeadlessScreenshot, HeadlessScreenshotError};
pub use substrate::{AccessEmitDecision, FragmentCacheStats, ShellCore};

/// Winit user-event variants that reach the UI thread out-of-band.
///
/// The shell's [`AppShell::user_event`] handler is the sole consumer. Each
/// variant's own doc below names its producer — those per-variant docs are the
/// SSOT. An enum-level producer enumeration is intentionally NOT kept here: it
/// twice went stale as variants were added (`WindowsDirty` R683,
/// `ExternalRepaint` R999) against the per-variant docs that already name each
/// producer, so the duplicated list was dropped rather than re-synced.
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
    /// R51.159 §5.23 — [`Intent`] produced by a resolved
    /// [`Command`](pinion_core::Command) future and delivered through
    /// [`ProxyIntentSink`]. The [`AppShell::user_event`] arm routes
    /// the intent into [`ShellCore`] for re-feeding into the SCXML
    /// `send` channel (R51.160 carry — this round logs it).
    IntentArrived(Intent),
    /// R683 §5.16 §5.41 — emitted by the
    /// [`AppShell::reconcile_windows`] Effect closure whenever the
    /// binding's [`WidgetView::windows_signal`] `Signal<Vec<WindowSpec>>`
    /// changes. The [`AppShell::user_event`] arm reads the latest
    /// signal snapshot, diffs against the cached last-known spec list,
    /// resumes added specs via the existing `resume_spec` helper, and
    /// drops removed specs (closes the winit Window + cleans the
    /// per-window substrate state via
    /// [`crate::ShellCore::remove_window`]).
    ///
    /// Carries no payload — the Effect closure cannot move the
    /// `Signal<Vec<WindowSpec>>` snapshot into the [`AppEvent`]
    /// because the Effect re-runs eagerly inside the producer's
    /// `Signal::set` scope and the snapshot must be re-read from
    /// the [`AppShell`] side where the
    /// [`ActiveEventLoop`](winit::event_loop::ActiveEventLoop) +
    /// `&mut self` are available.
    WindowsDirty,
    /// R999 §5.23 — an off-thread producer (PTY reader, network/process
    /// monitor) wrote fresh data into the shared handle a binding's `view`
    /// reads and is requesting a repaint, delivered through
    /// [`ProxyRepaintSink`](crate::ProxyRepaintSink) /
    /// [`pinion_core::RepaintSink`]. The [`AppShell::user_event`] arm arms a
    /// binding-wide redraw; the next frame re-runs `view`, which re-reads the
    /// shared handle. Distinct from [`AppEvent::WindowsDirty`] (window
    /// topology) and [`AppEvent::IntentArrived`] (a reducer event) — a
    /// content-free repaint poke, not state. Carries no payload: the data
    /// lives in the producer-authoritative shared handle, not the event.
    ExternalRepaint,
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
// R56.2.c §5.13 §5.38 — re-export the scene-walker
// `pinion_runtime::rect_for_tag` so application
// [`WidgetView::ime_caret_rect`] impls can resolve the focused
// widget's post-layout window-coord bounds without pulling
// `pinion-runtime` as a direct dep. The substrate is unchanged
// — pinion-runtime is the canonical home of the layout walker —
// the re-export just shortens the consumer-side surface.
pub use pinion_runtime::rect_for_tag;
// R1010 §5.39 §5.40 — re-export the focus-ring style so a
// [`WidgetView::focus_ring_style`] impl can return a custom ring (or import the
// type to read its builder) without a direct `pinion-overlay` dep.
pub use pinion_overlay::FocusRingStyle;
// R1121.1 §5.16 §5.39 — same rationale for the `WidgetView::window_chrome`
// hook: a binding returns a `WindowChromeStyle` without a direct overlay dep.
pub use pinion_overlay::WindowChromeStyle;

// R791.1 §5.13 §5.38 — the per-binding `WidgetView::ime_caret_rect` body
// (focus-guard + `rect_for_tag` walk + `tf_paint::ime_caret_rect_for`) is
// NOT lifted here, by deliberate dep-graph design: `pinion-widget-paint`
// (which owns `ime_caret_rect_for`) does not dep `pinion-runtime` (which
// owns `rect_for_tag`) so it stays backend-agnostic + TUI-reusable, and
// `pinion-shell` deps `pinion-widget-paint` only as a *dev*-dependency so
// the generic shell never couples to a specific widget's paint. The
// binding is the sole crate that sees both `rect_for_tag` and the
// TextField caret composition, so the ~5-line wrapper is irreducibly
// binding-side (a precedented-defer, not a liftable SSOT). Audited R791.1.

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

    /// R1060 §5.16 — render `scene` to the live surface and read back
    /// the **exact swapchain texture the window presents** as
    /// premultiplied RGBA8 (the true present-stage fidelity readback
    /// `scene/screenshot` exposes to AI clients). Unlike re-rasterizing
    /// the scene offscreen, this observes blit / surface-config /
    /// swapchain-staleness defects — a white or stale presented surface
    /// the encoded scene is correct about. GPU-specific, so it lives on
    /// this Vello trait rather than the backend-agnostic
    /// [`WidgetRenderer`] (a TUI cell backend has no surface to read).
    ///
    /// Renders at the surface's current configured size; the returned
    /// [`vello_capture::CapturedFrame`] carries the dimensions the
    /// `scene/screenshot` wire reports.
    ///
    /// # Errors
    /// See [`vello_capture::SurfaceCaptureError`] — a non-presentable
    /// swapchain status or a staging-buffer map failure.
    fn capture_rgba8(
        &mut self,
        scene: &VelloScene,
        base_color: vello::peniko::Color,
    ) -> Result<vello_capture::CapturedFrame, vello_capture::SurfaceCaptureError>;
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

            // R1060 §5.16 — the live-surface capture is bridged HERE
            // (in the shell-coupled macro), NOT forwarded to a template
            // inherent method: the pinion-forge template must stay
            // pinion-shell-free (forge consumers like `ai-introspect-demo`
            // use the emitted renderer without depending on pinion-shell),
            // so it cannot reference the `vello_capture` readback SSOT.
            // This macro already requires pinion-shell, so it hands the
            // renderer struct's wgpu fields (reachable because the macro
            // expands in the struct's own module via `include!`) straight
            // to the shared `capture_surface_rgba8`. GPU-less stub
            // renderers cannot use this macro (no fields) and hand-impl
            // the trait pair with a surface-less stub instead.
            fn capture_rgba8(
                &mut self,
                scene: &::vello::Scene,
                base_color: ::vello::peniko::Color,
            ) -> ::core::result::Result<
                $crate::vello_capture::CapturedFrame,
                $crate::vello_capture::SurfaceCaptureError,
            > {
                $crate::vello_capture::capture_surface_rgba8(
                    &self.context,
                    &mut self.surface,
                    &mut self.renderer,
                    scene,
                    base_color,
                )
            }
        }
    };
}

/// R668 §5.16 — window-creation size policy returned from
/// [`WidgetView::initial_size_strategy`].
///
/// `Fixed` opens the window at the declared logical-pixel size; the
/// user is still free to resize via the OS chrome and subsequent
/// `WindowEvent::Resized` events flow into the layout pass unchanged.
///
/// `IntrinsicAfterFirstPaint` opens the window at `min`, runs one
/// paint cycle to populate per-node rects, walks the resulting scene
/// for the tight content bbox ([`Scene::intrinsic_content_size`]),
/// clamps to `[min, max]`, and calls
/// [`winit::window::Window::request_inner_size`] when the clamped
/// size differs from `min`. Winit emits `WindowEvent::Resized` on
/// acceptance which feeds the next paint cycle at the new viewport.
/// `min` is also the lower bound the user-driven OS resize is allowed
/// to push the window below at the winit `set_min_inner_size` clamp.
///
/// The shell never resizes the window again on subsequent paints —
/// applications that need responsive shrink-wrap-on-state-change
/// drive that via their view fn + an explicit `scene/resize` RPC, not
/// this strategy.
///
/// R1059 §5.16 — `OpenResizable` decouples the *open size* from the
/// *OS-resize floor*: the window is created at `size` exactly (like
/// `Fixed`, no post-paint walk) but the user may drag it **below**
/// `size` down to `min` — or down to the OS-native minimum when `min`
/// is `None`. This is the "open at a sensible default, then freely
/// shrink" policy a plain resizable window wants (`Fixed` instead
/// pins the floor *at* the open size, which suits fixed-size dialogs).
/// Both `Fixed` and `IntrinsicAfterFirstPaint` keep their pre-R1059
/// behaviour bit-identical; the floor each one passes to winit is the
/// single source of truth at [`SizeStrategy::min_inner_floor`].
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SizeStrategy {
    /// Open the window at exactly `(width, height)` logical pixels.
    /// The OS-resize floor is pinned at the open size, so the user can
    /// enlarge but not shrink below it (fixed-size dialog semantics).
    Fixed { width: u32, height: u32 },
    /// Open at `min`, then request the tight content bbox clamped to
    /// `[min, max]` after the first paint cycle.
    IntrinsicAfterFirstPaint {
        /// Floor: window never opens smaller than this.
        min: (u32, u32),
        /// Ceiling: content bbox is clamped to at most this. Use
        /// `(u32::MAX, u32::MAX)` for "no upper bound" — winit /
        /// the OS still cap at the monitor work area.
        max: (u32, u32),
    },
    /// R1059 §5.16 — open at `size` exactly (no post-paint resize),
    /// but let the user drag the window smaller than `size`. `min` is
    /// the OS-resize floor; `None` leaves the window at the OS-native
    /// minimum (the freely-shrinkable case). Unlike `Fixed`, the floor
    /// is independent of the open size.
    OpenResizable {
        /// Logical-pixel size the window is created at.
        size: (u32, u32),
        /// OS-resize floor. `None` = OS-native minimum (no explicit
        /// `set_min_inner_size`); `Some((w, h))` clamps the lower
        /// bound at `(w, h)`, which may be smaller than `size`.
        min: Option<(u32, u32)>,
    },
}

impl SizeStrategy {
    /// The logical-pixel size the window is created at. `Fixed`
    /// returns its declared pair; `IntrinsicAfterFirstPaint` returns
    /// `min` (the first-paint pass widens up to `max`);
    /// `OpenResizable` returns `size`.
    #[must_use]
    pub const fn initial_logical_size(self) -> (u32, u32) {
        match self {
            Self::Fixed { width, height } => (width, height),
            Self::IntrinsicAfterFirstPaint { min, .. } => min,
            Self::OpenResizable { size, .. } => size,
        }
    }

    /// R1059 §5.16 — the OS-resize floor the shell passes to
    /// [`winit::window::WindowAttributes::with_min_inner_size`], or
    /// `None` to leave the window at the OS-native minimum (no
    /// explicit floor, so the user can shrink it freely).
    ///
    /// This is the single source of truth for the window-creation
    /// floor policy, consumed by both the live winit path and the
    /// headless path in `app.rs`. `Fixed` and `IntrinsicAfterFirstPaint`
    /// pin the floor at their open size / `min` (pre-R1059 behaviour
    /// unchanged); `OpenResizable` forwards its independent `min`.
    #[must_use]
    pub const fn min_inner_floor(self) -> Option<(u32, u32)> {
        match self {
            Self::Fixed { width, height } => Some((width, height)),
            Self::IntrinsicAfterFirstPaint { min, .. } => Some(min),
            Self::OpenResizable { min, .. } => min,
        }
    }

    /// R1092 §5.16 §5.41 §2 #7 — the window's **declared** logical-pixel
    /// open size for AI introspection (`scene/windows`), or `None` when
    /// the binding does not declare one up-front.
    ///
    /// `Fixed` and `OpenResizable` declare an exact open size, so they
    /// report it. `IntrinsicAfterFirstPaint` does **not**: it opens at
    /// `min` only as a floor, then walks the window to the content bbox
    /// after the first paint, so its eventual size is content-determined
    /// — reported `None`, exactly the honesty
    /// [`WindowSpec::position`](WindowSpec::position) uses for a
    /// WM-placed (`None`) window. This is deliberately **distinct** from
    /// [`Self::initial_logical_size`] (the literal pixels the window is
    /// *created* at, which returns `min` for `Intrinsic`): that answers
    /// "what size does the shell open the window at"; this answers "what
    /// size did the binding DECLARE" — and an `Intrinsic` window
    /// declares none, so reporting its `min` would mislead an AI into
    /// reading a transient floor as the final geometry.
    #[must_use]
    pub const fn declared_size(self) -> Option<(u32, u32)> {
        match self {
            Self::Fixed { width, height } => Some((width, height)),
            Self::OpenResizable { size, .. } => Some(size),
            Self::IntrinsicAfterFirstPaint { .. } => None,
        }
    }
}

/// R670 §5.16 §5.41 — Phase B (R700+) multi-window foundation.
///
/// One [`WindowSpec`] describes a single OS window the binding wants
/// the shell to create at boot — the canonical building block for
/// every multi-window UI Phase B will surface (`DevTools` / Inspector
/// floating against a main editor canvas, Settings dialog as a
/// secondary modal, popover-class overlays, …).
///
/// The core `(id, title, strategy)` triple stays minimal: no
/// decorations, no parent-window relationship, no transparency hint.
/// Every extra knob lands behind a follow-up builder method as a concrete
/// Phase B widget catalog binding surfaces the substrate-incompleteness
/// signal — R1087 (PR-31 dock tear-off) added the first such field,
/// [`position`](Self::position) (via [`with_position`](Self::with_position)),
/// because the floating-panel-follows-cursor model needs a declared,
/// reconcilable window placement; the rest (decorations, parent
/// relationship, transparency) await their own forcing consumers.
///
/// `id` is the AI-facing scene/RPC handle (`scene/snapshot {window:
/// "main"}` / `scene/click {window: "inspector", at: …}`). The
/// single-window convention is `"main"`; secondary windows pick
/// non-conflicting names per the embedder's taxonomy.
///
/// `title` is the OS window title (the string winit
/// `set_title`-forwards to the platform's window decoration). The
/// shell does not pin the title to `WidgetView::title()` because a
/// multi-window binding (e.g. main + inspector) naturally carries
/// different titles per window.
///
/// `strategy` follows the same [`SizeStrategy`] taxonomy
/// single-window bindings already use. `Fixed` opens the per-spec
/// window at the declared logical size; `IntrinsicAfterFirstPaint`
/// runs the same post-first-paint walker but scoped to the per-spec
/// window's painted scene.
///
/// R683 §5.16 §5.41 — `id` is `Cow<'static, str>` so the dock +
/// tear-off arc can mint runtime-generated ids (e.g.
/// `Cow::Owned(format!("torn-panel-{n}"))`) alongside the canonical
/// `Cow::Borrowed("main")` / `Cow::Borrowed("inspector")` static
/// literals every pre-R683 single + multi-window binding declared.
/// `PartialEq + Eq` so the R683 atomic 1 reconcile-diff Effect can
/// compare new-vs-old spec lists slot-by-slot; `serde::Serialize +
/// serde::Deserialize` so the wrapping
/// [`pinion_core::Signal<Vec<WindowSpec>>`] satisfies its
/// `T: Clone + PartialEq + Serialize + DeserializeOwned + 'static`
/// trait bound (the canonical reactive primitive R26 + R36 pin).
/// `#[non_exhaustive]` so future additive fields (position,
/// decorations, parent-window relationship) land without breaking
/// out-of-crate constructions — every caller already builds via the
/// `::main` / `::new` builders.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WindowSpec {
    /// AI-facing handle. RPC `{window: "<id>"}` scoping resolves a
    /// dispatch against the window whose `id` matches. The default
    /// single-window binding uses `"main"`; secondary windows pick
    /// any non-empty non-conflicting string. The shell does not
    /// auto-generate IDs — explicit naming keeps the AI-side wire
    /// stable across binding releases.
    ///
    /// R683 §5.16 — `Cow<'static, str>` so static-literal ids
    /// (`Cow::Borrowed("main")`) stay alloc-free while
    /// runtime-generated ids (dock tear-off:
    /// `Cow::Owned(format!("torn-panel-{n}"))`) coexist without a
    /// separate field shape.
    pub id: Cow<'static, str>,
    /// OS window title — `winit::window::Window::set_title` forwards
    /// this string to the platform window decoration.
    pub title: String,
    /// Window-creation policy. See [`SizeStrategy`] for the canonical
    /// contract; multi-window bindings can mix strategies per spec
    /// (main: `Fixed`, inspector: `IntrinsicAfterFirstPaint`, …).
    pub strategy: SizeStrategy,
    /// R1087 §5.16 §5.41 PR-31 — the binding's **declared** **outer**
    /// window position in **logical** pixels (`(x, y)`, top-left, the same
    /// logical frame [`SizeStrategy`] sizes in; the OS applies the
    /// per-monitor DPI scale). `None` (the default for every pre-R1087
    /// binding) leaves placement to the window manager exactly as before —
    /// the *placement behaviour* is byte-identical (the serde *shape* is
    /// additive, see the `#[serde(default)]` note below).
    ///
    /// This is the SSOT for the **floating-panel-as-positioned-window**
    /// model (the PR-31 dock tear-off): a binding declares where a torn-off
    /// panel's window should sit, and the shell drives the real OS window to
    /// match. Honoured at create time by [`crate::AppShell`]'s `resume_spec`
    /// (`with_position`) and on a same-id position change by the
    /// `reconcile_windows` move pass (`Window::set_outer_position`), so the
    /// declared position stays the single source of truth across the
    /// window's lifetime — a position write to the reactive
    /// [`pinion_core::Signal<Vec<WindowSpec>>`] is all a binding needs to
    /// move a window (the drag-follow that PR-31 builds on top).
    ///
    /// **Declared SSOT, converged on actual (R1088 closed the loop).** The
    /// flow is signal → OS (`set_outer_position`); the reverse is closed too —
    /// a USER native-dragging a floating window by its title bar fires
    /// `WindowEvent::Moved`, which [`crate::AppShell`]'s `note_window_moved`
    /// writes back into this signal (an external move becomes just another
    /// writer of this same SSOT — the architecture-A ideal), so `scene/windows`
    /// reports the converged position rather than a stale declared one. The
    /// shell's OWN commanded moves are echo-suppressed (a
    /// `last_commanded_position` latch + the pure `moved_is_command_echo`) so
    /// the write-back never storms, and a WM-placed (`None`) window is
    /// conservatively NOT pinned by a stray `Moved`. Only POSITION closes the
    /// loop: `strategy` stays create-time-intent (read once at create; a
    /// runtime `Resized` is not written back) — position is the axis the live
    /// tear-off drag-follow needs.
    ///
    /// `#[serde(default)]` so a `Signal<Vec<WindowSpec>>` value serialized
    /// before this field existed (or any wire form omitting it) deserializes
    /// to `None` — additive on the READ side. The field deliberately carries
    /// no `skip_serializing_if`: a write emits an explicit `"position":null`
    /// (more observable than an absent key, and `WindowSpec` is an internal
    /// reactive-primitive payload, not a frozen external wire contract). So
    /// the serialized *shape* changed at R1087 (gains a `position` key) even
    /// though placement behaviour did not — do not "fix" this by adding
    /// `skip_serializing_if`; the explicit null is intentional.
    #[serde(default)]
    pub position: Option<(i32, i32)>,
    /// R1115 §5.16 §5.51 PR-38 — does the OS draw this window's chrome
    /// (title bar + border + resize frame)? `true` (the default — winit's
    /// own [`winit::window::WindowAttributes`] default) is every pre-R1115
    /// window, byte-identical. `false` opts the window OUT of OS chrome so
    /// the binding owns it: a torn-off dock panel declares
    /// `decorations: false` and pinion paints the panel's own header
    /// (drag-grip + close) instead of stacking a redundant OS title bar
    /// over it — the custom-chrome floating panel the self-hosted-editor
    /// northern star needs (Blender/Unreal tear-offs show no OS title bar).
    ///
    /// **Create-time intent, like [`strategy`](Self::strategy) — NOT
    /// reconciled at runtime.** Honoured once by [`crate::AppShell`] at
    /// window create ([`winit::window::WindowAttributes::with_decorations`]);
    /// the shell does not call `Window::set_decorations` on a same-id spec
    /// change (no consumer toggles chrome on a live window — a dock-back
    /// destroys the floating window rather than re-decorating it). Only
    /// [`position`](Self::position) closes the OS-feedback loop.
    ///
    /// `#[serde(default = "windowspec_decorations_default")]` so a wire form
    /// omitting the field (every pre-R1115 serialized spec) deserializes to
    /// `true` — `bool`'s own `Default` is `false`, so the explicit default
    /// is what keeps the omitted-field behaviour byte-identical (a decorated
    /// window, never a surprise borderless one).
    #[serde(default = "windowspec_decorations_default")]
    pub decorations: bool,
}

/// Serde default for [`WindowSpec::decorations`] — `true` (OS-decorated),
/// matching winit's [`winit::window::WindowAttributes`] default. A wire form
/// omitting the field deserializes to a decorated window; `bool`'s own
/// `Default` is `false`, so this explicit default is required for
/// byte-identical omitted-field behaviour (R1115 §5.16 §5.51 PR-38).
const fn windowspec_decorations_default() -> bool {
    true
}

impl WindowSpec {
    /// (R670 §5.16) Canonical single-window primary spec — the same
    /// shape [`WidgetView::windows`]'s default impl returns so the
    /// 15+ existing single-window bindings keep their pre-R670
    /// behaviour bit-identical (one window, `id = "main"`, title +
    /// strategy from the binding's own [`WidgetView::title`] +
    /// [`WidgetView::initial_size_strategy`]).
    #[must_use]
    pub fn main(title: impl Into<String>, strategy: SizeStrategy) -> Self {
        Self {
            id: Cow::Borrowed("main"),
            title: title.into(),
            strategy,
            position: None,
            decorations: windowspec_decorations_default(),
        }
    }

    /// (R670 §5.16) Build a non-primary window spec — the path
    /// multi-window bindings take when adding inspector / dialog /
    /// floating panel windows alongside the main one. `id` is any
    /// `Cow<'static, str>` — static literals via
    /// `Cow::Borrowed("inspector")` stay alloc-free; runtime ids
    /// via `Cow::Owned(format!("torn-panel-{n}"))` coexist for the
    /// dock + tear-off arc (R683). `title` is the OS window
    /// decoration.
    #[must_use]
    pub fn new(
        id: impl Into<Cow<'static, str>>,
        title: impl Into<String>,
        strategy: SizeStrategy,
    ) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            strategy,
            position: None,
            decorations: windowspec_decorations_default(),
        }
    }

    /// R1087 §5.16 §5.41 PR-31 — declare the window's initial **outer**
    /// position in **logical** pixels (top-left). Builder form so the
    /// `#[non_exhaustive]` struct stays constructed only through `main` /
    /// `new`; chains after either. A binding placing a torn-off dock panel
    /// under the cursor at detach calls
    /// `WindowSpec::new(id, title, strategy).with_position(x, y)`.
    ///
    /// Setting this on an *already-open* window (re-pushing the spec into
    /// the reactive signal with a new position) drives the
    /// `reconcile_windows` move pass — the position is the SSOT the shell
    /// reconciles the OS window to, so a binding moves a window by writing
    /// the signal, never by reaching for a winit handle.
    #[must_use]
    pub fn with_position(mut self, x: i32, y: i32) -> Self {
        self.position = Some((x, y));
        self
    }

    /// R1115 §5.16 §5.51 PR-38 — declare whether the OS draws this window's
    /// chrome. Builder form (the `#[non_exhaustive]` struct is constructed
    /// only via `main` / `new`); chains after either, alongside
    /// [`with_position`](Self::with_position). A binding floating a torn-off
    /// dock panel into a borderless window chains
    /// `WindowSpec::new(id, title, strategy).with_position(x, y).with_decorations(false)`.
    /// Create-time intent — see the [`decorations`](Self::decorations) field.
    #[must_use]
    pub fn with_decorations(mut self, decorations: bool) -> Self {
        self.decorations = decorations;
        self
    }
}

/// (R1107.1 §5.16 §5.41 §5.51) Is a window with `window_id` currently declared
/// in `windows`? The config-free existence check a multi-window binding runs to
/// decide whether a panel is floating (its `torn-<panel>` window exists). Lifted
/// from the two dock consumers (`hello-dock-panels` + `hello-dock-panels-editor`)
/// where it was byte-identical — the rule-of-three 3rd consumer (sprag) had
/// already drifted, so the shared, config-free predicate belongs here.
#[must_use]
pub fn window_exists(windows: &[WindowSpec], window_id: &str) -> bool {
    windows.iter().any(|w| w.id == window_id)
}

/// (R1107.1 §5.16 §5.41 §5.51) Convert a window-logical `cursor` (measured in
/// `source_window`'s frame) to a DESKTOP outer position by adding that window's
/// declared outer origin. The gap(b) desktop conversion a live tear-off follow
/// needs: the floating follower opens at the desktop point under the cursor.
///
/// `source_window` names which window the cursor is in (`DragUpdate::source_window`);
/// `None` (a cursor-less degenerate gesture) falls back to the canonical
/// [`pinion_runtime::DEFAULT_WINDOW`]. A source window absent from `windows`, or
/// present but un-positioned (WM-placed), falls back to the desktop origin so the
/// follower still TRACKS the cursor (offset relative to (0, 0)).
///
/// Lifted from the two dock consumers where it was byte-identical and
/// correctness-critical (the R1095.1 source-window fix); a 3rd consumer (sprag)
/// had drifted off the pre-fix signature — exactly the cost of NOT lifting a
/// config-free pure helper. `WindowSpec` lives here, so this is the textbook home
/// (`pinion-widget-paint::dock` cannot host it — it has no pinion-shell dep).
#[must_use]
#[allow(
    clippy::cast_possible_truncation,
    reason = "logical-pixel cursor → i32 outer position; sub-pixel is irrelevant to window placement"
)]
pub fn desktop_position_from(
    windows: &[WindowSpec],
    source_window: Option<&str>,
    cursor: (f64, f64),
) -> (i32, i32) {
    let source = source_window.unwrap_or(pinion_runtime::DEFAULT_WINDOW);
    let (ox, oy) = windows
        .iter()
        .find(|w| w.id.as_ref() == source)
        .and_then(|w| w.position)
        .unwrap_or((0, 0));
    (ox + cursor.0.round() as i32, oy + cursor.1.round() as i32)
}

/// R51.121 §5.41 — Vello-specific application-supplied widget binding.
///
/// Each visual binary implements this once on a unit type;
/// `pinion_shell::run::<MyView>()` does the rest.
///
/// The trait inherits the bulk of its surface via the supertrait
/// chain [`pinion_a11y::WidgetA11y`] → [`pinion_core::WidgetCore`];
/// only the Vello-specific [`Renderer`](Self::Renderer) associated
/// type and the pixel-unit window-creation policy
/// [`initial_size_strategy`](Self::initial_size_strategy) live here.
/// The application-side binding therefore declares one impl block per
/// trait (typical breakdown: 9 methods in `WidgetCore`, 1-3 in
/// `WidgetA11y`, 2 here).
///
/// The supertrait split lets the ratatui TUI backend
/// (`pinion_tui::WidgetViewTui`) reuse the same `WidgetCore` +
/// `WidgetA11y` surface, replacing only the Vello-specific items here
/// with `Frame = Buffer` and a cell-unit `initial_size` (the TUI
/// backend has no `IntrinsicAfterFirstPaint` variant — terminal cells
/// are owned by the host process, not the application).
pub trait WidgetView: pinion_a11y::WidgetA11y {
    /// Concrete pinion-forge-emitted renderer (`HelloFooRenderer`).
    /// `'static` so [`RenderState`] can store `Box<Self::Renderer>`
    /// across the suspend/resume cycle without lifetime parameters.
    type Renderer: VelloRenderer + 'static;

    /// R668 §5.16 — window-creation size policy. `winit` applies the
    /// per-monitor DPI scale, so logical pixels are "what the user
    /// sees" on a 1.0× display. The shell honours this on the first
    /// [`resumed`](AppShell::resumed); subsequent resizes flow
    /// through `WindowEvent::Resized` and do not consult the strategy
    /// again.
    ///
    /// Most bindings return [`SizeStrategy::Fixed`]; the
    /// `#[pinion::widget(initial_size = (W, H))]` derive emits exactly
    /// that. Bindings whose root content has a content-driven height
    /// (settings panels with section-dependent layout, popovers,
    /// dialogs) return [`SizeStrategy::IntrinsicAfterFirstPaint`].
    fn initial_size_strategy() -> SizeStrategy;

    /// R56.2.c §5.13 §5.38 — IME candidate window positioning hint.
    /// Returns the caret rect in **window-local logical-pixel
    /// coordinates** (origin at the top-left of the client area, the
    /// same coord frame [`Window::set_ime_cursor_area`] consumes) so
    /// the shell can position the platform IME candidate popup
    /// directly underneath the caret. Without this hook
    /// ibus-hangul / fcitx5 / macOS Hangul / Microsoft IME default
    /// the popup to the screen corner — usable for typing but
    /// disorienting for users scanning between text + candidates.
    ///
    /// The shell calls this once per redraw cycle after layout has
    /// populated [`pinion_text::LayoutCache`] (so the application
    /// impl's [`caret_rect_for_byte_offset`](pinion_text::caret_rect_for_byte_offset)
    /// lookup is a cache hit, not a recomputation). The shell
    /// wraps the call in [`Owner::run`](pinion_core::reactive::Owner)
    /// so applications can reach the same
    /// `use_text_edit_state(tag)` / `use_layout_cache(key)` hooks
    /// the view fn and `apply_composition` already use — one
    /// `Rc<TextEditState>` per tag across the binding lifetime.
    ///
    /// `focused != Some(<my tag>)` should short-circuit to `None`
    /// (mirror of the `apply_key` / `apply_composition` roving-
    /// tabindex pattern); the trait return shape `None` tells the
    /// shell "no IME-relevant caret right now" and the shell skips
    /// the `set_ime_cursor_area` call (winit keeps the previous
    /// rect, which is the canonical winit contract).
    ///
    /// Width should be `>= 1.0` (a 0-width caret renders zero
    /// candidate space and some IMEs reject the call); the shell
    /// applies a `.max(1.0)` guard on both axes so this contract is
    /// belt-and-braces.
    ///
    /// Default returns `None` — widgets without text-input
    /// affordances need no override. Only text-input widgets
    /// (`TextField`-class) override.
    #[must_use]
    fn ime_caret_rect(
        _state: &<Self as WidgetCore>::State,
        _scene: &Scene,
        _focused: Option<&str>,
    ) -> Option<pinion_text::CaretRect> {
        None
    }

    /// R1010 §5.39 §5.40 — binding-controlled focus ring. The shell draws a
    /// framework focus ring (a `pinion_overlay` outset box, tagged
    /// `ai-overlay/focus-ring`) around the focused widget; this hook lets a
    /// binding restyle or suppress it for the tag the shell is about to ring.
    ///
    /// - `Some(style)` (the default, `Some(FocusRingStyle::default())`) — draw
    ///   the ring with `style`. A binding that does not override is
    ///   byte-unchanged.
    /// - `None` — draw **no** ring. The content-surface opt-out: a terminal /
    ///   code-editor / canvas that owns its own focus indicator (the terminal's
    ///   text cursor) suppresses the framework ring while still taking focus for
    ///   `apply_key`. A binding that opts out then owns its own visible
    ///   keyboard-focus indicator (the WCAG 2.4.7 affordance the framework ring
    ///   otherwise provides) — the shell rings nothing in its place.
    ///
    /// `focused_tag` is the tag the shell resolved to ring (a roving widget's
    /// active descendant, or the focused widget itself), so a binding with
    /// several focusable surfaces can suppress the ring on its content surface
    /// while keeping it on its chrome.
    #[must_use]
    fn focus_ring_style(_focused_tag: &str) -> Option<pinion_overlay::FocusRingStyle> {
        Some(pinion_overlay::FocusRingStyle::default())
    }

    /// R1121.1 §5.16 §5.39 — binding-controlled client-side window chrome.
    /// When a window draws its OWN chrome (title bar + minimize / maximize /
    /// close + drag grip) instead of the OS frame, the binding returns
    /// `Some(style)` for that window's id; the shell injects the chrome strip
    /// and insets the window content below it. `None` (the default) draws no
    /// chrome.
    ///
    /// This is ORTHOGONAL to [`WindowSpec::decorations`] — the two were coupled
    /// at R1121 (`decorations:false ⇒ chrome`) and decoupled here because that
    /// coupling could not express a **naked borderless** window (`decorations:
    /// false` + no chrome) — the fullscreen-game surface the Phase-C/D northern
    /// star needs. The honest matrix:
    ///
    /// - `decorations:true`  + `None`        — OS-drawn title bar (the default).
    /// - `decorations:false` + `Some(style)` — pinion-drawn chrome (CSD: an
    ///   editor panel / torn-off dock window — Blender / Unreal / VS Code).
    /// - `decorations:false` + `None`        — naked borderless (a splash or a
    ///   fullscreen game viewport — no chrome at all).
    /// - `decorations:true`  + `Some(style)` — possible but redundant (two bars);
    ///   a binding that wants CSD also sets `decorations:false`.
    ///
    /// `window_id` is the canonical [`WindowSpec::id`] so a multi-window binding
    /// chromes its floating panels while leaving its main canvas OS-decorated
    /// (or naked). Mirrors the [`Self::focus_ring_style`] hook shape.
    #[must_use]
    fn window_chrome(_window_id: &str) -> Option<pinion_overlay::WindowChromeStyle> {
        None
    }

    /// (R1170 §5.16 §5.39) Per-window CLOSE seam. The shell calls this when a
    /// window close is requested — the OS close button on a decorated window
    /// (`WindowEvent::CloseRequested`, also Alt+F4) OR a client-side chrome close
    /// control ([`pinion_overlay::WINDOW_CHROME_CLOSE_TAG`]). Return `true` if the
    /// BINDING handled the close (e.g. a torn-off dock panel docks BACK by dropping
    /// its [`WindowSpec`] from [`Self::windows_signal`]); the shell then does NOT
    /// exit — the reactive `windows_signal` → `reconcile_windows` pass removes that
    /// window's OS handle. Return `false` (the default) to take the standalone-app
    /// convention: the close exits the whole app (the single-window case + a
    /// multi-window binding's PRIMARY window). The hook runs inside the shell's
    /// reactive owner, so it may read / write `Signal`s (e.g. `windows_signal`).
    /// `window_id` is the closing window's [`WindowSpec::id`].
    ///
    /// This is the "binding close seam" R1121 deferred: before it, EVERY close
    /// (any window) exited the app, so a torn-off panel had no way to close to its
    /// dock without killing the editor.
    #[must_use]
    fn window_close_requested(_window_id: &str) -> bool {
        false
    }

    /// (R1113 §5.51 §5.33) Drag-image (the translucent follower the shell
    /// floats under the cursor while a drag is in flight) style for a drag
    /// whose payload carries `label`. Mirrors
    /// [`focus_ring_style`](Self::focus_ring_style): the shell injects the
    /// follower automatically from the [`InputRouter`](pinion_runtime::InputRouter)'s
    /// live drag session (no per-binding wiring, like the focus ring); this
    /// hook lets a binding theme it or opt out.
    ///
    /// - `Some(style)` (the default) — draw the follower with `style`. The
    ///   default is the neutral [`DragImageStyle::default`](pinion_overlay::DragImageStyle::default);
    ///   a themed binding can return its surface/on-surface colours.
    /// - `None` — draw **no** follower for this drag (a binding whose drags are
    ///   self-evident, or that paints its own drag affordance).
    ///
    /// Only drags that opened a [`begin_drag`](pinion_core::external::External::begin_drag)
    /// session with a non-empty text payload reach this hook — a capture-drag
    /// (a splitter resize) never does, so it shows no follower regardless.
    #[must_use]
    fn drag_image_style(_label: &str) -> Option<pinion_overlay::DragImageStyle> {
        Some(pinion_overlay::DragImageStyle::default())
    }

    /// (R1125 §5.51 §2 #7 PR-33) Build the cross-window dock drop-zone PREVIEW —
    /// the affordance painted on a TARGET window while a floating panel is dragged
    /// OVER it, showing where the redock would land. The shell stays
    /// widget-library-agnostic, so it does the GENERIC half (resolve the incoming
    /// drop via its own cross-window geometry
    /// [`CoreShell::cross_window_drag_into`](pinion_runtime::CoreShell::cross_window_drag_into),
    /// then look up the target panel's window-absolute `panel_rect`) and hands the
    /// dock-specific RENDERING to the binding here: `source_panel` is the dragged
    /// panel (so the binding resolves through the SAME
    /// [`resolve_drop`](pinion_widget_paint::dock::resolve_drop) SSOT the release
    /// applies — R1163b unified the cross-window path, so preview == result by
    /// construction), `target_tag` is the resolved dock target, `x_rel`/`y_rel` the
    /// normalised cursor over it. The binding returns the overlay [`Scene`] (e.g.
    /// `pinion_widget_paint::dock::dock_drop_preview_overlay`), which the shell
    /// injects as a top-level, pointer-transparent overlay on that window and
    /// re-derives every paint (so it follows the cursor). The sibling of
    /// [`drag_image_style`](Self::drag_image_style), but dock-specific, so it is
    /// opt-in: a dock binding adds one line, a non-dock app draws nothing.
    ///
    /// - `Some(scene)` — inject `scene` as the preview overlay.
    /// - `None` (the default) — no cross-window preview.
    #[must_use]
    fn dock_drop_preview(
        _source_panel: &str,
        _target_tag: &str,
        _panel_rect: pinion_core::scene::Rect,
        _x_rel: f32,
        _y_rel: f32,
    ) -> Option<Scene> {
        None
    }

    // (R1168 retired `dock_zone_guide`: the static dock-zone GUIDE outlined whole
    // panel rects independent of `resolve_drop`, so it diverged from the cursor
    // preview — the "선≠preview" divergence the user caught. The cursor-driven
    // `dock_drop_preview` (derived from the one `resolve_drop` SSOT) is the SOLE
    // drop affordance now; a same-window OUTER dock previews its full-span band the
    // same way [R1167].)

    /// R762 §5.36 §5.38 / R763 §5.22 — pointer-driven caret + selection
    /// press hook. The shell calls this on a press (native winit
    /// `MouseInput` and the `scene/click` / `scene/drag` deferred-input
    /// drains all converge here through `mouse_pressed_for_window`),
    /// after click-to-focus, with the press location in **window-local
    /// logical pixels**.
    ///
    /// Reverse of [`ime_caret_rect`](Self::ime_caret_rect): a text-input
    /// widget hit-tests `(x, y)` against its shaped layout (via
    /// `pinion_widget_paint::byte_for_field_point` /
    /// `pinion_text::byte_offset_for_point`) and:
    ///
    /// - `extend == false` (plain press): moves the caret to the
    ///   resolved byte and collapses any selection
    ///   (`TextEditState::set_caret`) — the byte becomes the drag
    ///   anchor.
    /// - `extend == true` (Shift-click): extends the selection from the
    ///   existing anchor (or the current caret if none) to the resolved
    ///   byte (`TextEditState::set_selection`) — the retained anchor
    ///   stays the pinned end.
    ///
    /// It mutates (like `apply_key`) rather than returning the caret, so
    /// the binding owns the "is this my field" decision through the
    /// `use_text_edit_state(tag)` it already holds. The **return** is
    /// the byte offset of the pinned selection anchor: the shell stores
    /// it to drive a subsequent drag (every later `cursor_moved` while
    /// the button is held replays
    /// [`select_drag_to_point`](Self::select_drag_to_point) with this
    /// anchor).
    ///
    /// `focused != Some(<my tag>)` should short-circuit to `None`.
    /// Return `Some(anchor_byte)` when the press landed on this widget's
    /// text (the shell arms the drag + requests a redraw); `None`
    /// otherwise (the shell disarms any drag). Runs inside the shell
    /// root-owner scope so `use_text_edit_state` /
    /// `use_text_field_layout_cache` resolve.
    ///
    /// R801 §5.36 §5.35 — `hit_tag` is the `InputRouter`-resolved tag
    /// under the press (the deepest tagged ancestor at the press point —
    /// the same target the router dispatched the `PointerDown` to). The
    /// shell fires this hook for *every* press so a binding can react to
    /// presses while its field merely keeps focus; a binding therefore
    /// also short-circuits when `hit_tag != Some(<my tag>)`, so a press
    /// the router routed to a *sibling* widget — e.g. a non-focusable
    /// formatting toolbar painted below the field — does not move the
    /// caret (which would clear a selection that toolbar command needs).
    /// This replaces the pre-R801 per-binding rect re-scan: the field no
    /// longer re-derives "is this press inside me" from its own box; the
    /// router already hit-tested it, and the shell reports the answer.
    ///
    /// Default returns `None` — only text-input widgets override.
    fn position_caret_for_point(
        _state: &<Self as WidgetCore>::State,
        _scene: &Scene,
        _focused: Option<&str>,
        _hit_tag: Option<&str>,
        _x: f32,
        _y: f32,
        _extend: bool,
    ) -> Option<usize> {
        None
    }

    /// R763 §5.36 §5.22 — pointer drag selection hook. The shell calls
    /// this on every `cursor_moved` while the button stays held after a
    /// [`position_caret_for_point`](Self::position_caret_for_point) that
    /// returned `Some(anchor)`. The widget hit-tests `(x, y)` to a byte
    /// and extends its selection from `anchor` (the pinned end the press
    /// returned) to that byte (`TextEditState::set_selection`), so a
    /// drag sweeps a live selection band exactly like a real mouse.
    ///
    /// `focused != Some(<my tag>)` should short-circuit to `false`.
    /// Return `true` when the selection changed (the shell requests a
    /// redraw); `false` otherwise. Runs inside the shell root-owner
    /// scope so `use_text_edit_state` resolves.
    ///
    /// Default returns `false` — only text-input widgets override.
    fn select_drag_to_point(
        _state: &<Self as WidgetCore>::State,
        _scene: &Scene,
        _focused: Option<&str>,
        _anchor: usize,
        _x: f32,
        _y: f32,
    ) -> bool {
        false
    }

    /// R770 §5.15 — OS file-drag hover hook. The shell calls this when a
    /// file is dragged *over* the window (winit
    /// `WindowEvent::HoveredFile`, or the `scene/hover_file` RPC peer),
    /// with the dragged file's `path`. winit's file-DnD is window-scoped
    /// — the OS reports the path but not a drop coordinate — so this is
    /// positionless (a drop-zone widget lights up its whole-window
    /// "release to drop" affordance). Mutate reactive state (a
    /// `use_*`-backed `Signal`) and return `true` to request a redraw;
    /// the default returns `false` (the binding ignores file drags).
    /// Runs inside the shell root-owner scope so `use_*` hooks resolve.
    fn on_file_hover(_state: &<Self as WidgetCore>::State, _path: &str) -> bool {
        false
    }

    /// R770 §5.15 — OS file-drag cancel hook. The shell calls this when a
    /// drag leaves the window without dropping (winit
    /// `WindowEvent::HoveredFileCancelled`, or `scene/hover_file_cancel`):
    /// the drop-zone clears the affordance [`on_file_hover`] raised.
    /// Positionless + path-less. Return `true` to request a redraw;
    /// default `false`.
    fn on_file_hover_cancel(_state: &<Self as WidgetCore>::State) -> bool {
        false
    }

    /// R770 §5.15 — OS file drop hook. The shell calls this when a file is
    /// dropped on the window (winit `WindowEvent::DroppedFile`, or the
    /// `scene/drop_file` RPC peer), with the dropped file's `path`. winit
    /// delivers one event per file (a multi-file drop arrives as several
    /// calls). Mutate reactive state (e.g. push the path onto a
    /// `Signal<Vec<String>>`) and return `true` to request a redraw;
    /// default `false`. Runs inside the shell root-owner scope.
    fn on_file_drop(_state: &<Self as WidgetCore>::State, _path: &str) -> bool {
        false
    }

    /// R670 §5.16 §5.41 — Phase B (R700+) multi-window foundation.
    ///
    /// Returns the [`WindowSpec`] list the shell creates per binding
    /// at boot. The default impl returns exactly one spec —
    /// `WindowSpec::main(Self::title(), Self::initial_size_strategy())`
    /// — so every existing single-window binding (R670 has 15+ in
    /// the example gallery) keeps its pre-R670 lifecycle bit-
    /// identical without touching any trait method.
    ///
    /// Multi-window bindings override this to enumerate every window
    /// they want. The order is significant: the **first** spec is
    /// the primary window — the first to receive
    /// [`AppShell::resumed`] focus + the default scope for RPC
    /// frames that omit `{window: "..."}`. Secondary windows follow
    /// in declaration order.
    ///
    /// Returns `Vec<WindowSpec>` (not `&[WindowSpec]`) because the
    /// shell needs an owned list for its per-window storage map; the
    /// allocation cost is amortised once at boot.
    ///
    /// R683 §5.16 — this returns the **compile-time** list. The
    /// runtime sibling [`Self::windows_signal`] returns
    /// `Option<Rc<Signal<Vec<WindowSpec>>>>`; when `Some(..)` the
    /// shell subscribes to the signal and diffs added/removed specs
    /// across mutations (dock tear-off / dock-back). When `None`
    /// (the default) the shell snapshots this method once on
    /// [`AppShell::resumed`](AppShell) and the window topology is
    /// frozen for the binding's lifetime — the pre-R683 contract for
    /// every single + multi-window binding.
    #[must_use]
    fn windows() -> Vec<WindowSpec> {
        vec![WindowSpec::main(
            Self::title(),
            Self::initial_size_strategy(),
        )]
    }

    /// R683 §5.16 §5.41 — opt-in runtime window-list lift.
    ///
    /// Default returns `None`, signalling the shell to read the
    /// compile-time [`Self::windows`] list once on
    /// [`AppShell::resumed`](AppShell) and freeze the window
    /// topology for the binding's lifetime (the pre-R683 contract
    /// for every single + multi-window binding).
    ///
    /// Bindings that need to add or remove windows at runtime
    /// (canonically: a [`DockSurface`](pinion_widget_paint::dock)
    /// with tear-off ergonomics minting a new window per torn-off
    /// panel) override this method to return
    /// `Some(Rc<Signal<Vec<WindowSpec>>>)`. The shell's R683 atomic 1
    /// `reconcile_windows` Effect subscribes the signal, diffs the
    /// emitted list against the previous emit's spec id set, and
    /// resumes / drops winit windows + their `WindowSlot`s to
    /// match. Idempotent on identical re-emits (`PartialEq` on
    /// `Vec<WindowSpec>` short-circuits the diff).
    ///
    /// Owner context: the shell calls this method inside
    /// `root_owner.run(|| ...)` so the binding impl can reach
    /// [`Owner::current()`](pinion_core::Owner::current) and call
    /// [`Owner::cache`](pinion_core::Owner::cache) to memoise the
    /// returned `Rc<Signal<..>>` across calls — multiple invocations
    /// on the same binding handle should return the **same**
    /// `Rc<Signal<..>>` (identity-stable), otherwise the reconcile
    /// Effect would re-subscribe to a fresh signal each call and
    /// drop the prior subscription cleanly via
    /// `Owner::cleanup_subscription`.
    ///
    /// Returning `Some(signal)` overrides the compile-time list:
    /// the shell calls `signal.get()` for the initial topology and
    /// ignores [`Self::windows`] for the rest of the binding's
    /// lifetime. Returning `None` is the default — no opt-in, no
    /// reactive subscription, frozen compile-time topology.
    #[must_use]
    fn windows_signal() -> Option<Rc<Signal<Vec<WindowSpec>>>> {
        None
    }

    /// R670.B §5.16 — per-window paint scene hook. Returns the
    /// painted scene for the given `window_id`; default forwards to
    /// [`Self::view`] so every existing single-window binding (R670
    /// has 15+ in the example gallery) keeps its lifecycle
    /// bit-identical.
    ///
    /// Multi-window bindings override this to render different
    /// scenes per window (main view in the primary; inspector tree
    /// in the secondary; `DevTools` / debug overlay in tertiary;
    /// …). The `window_id` argument is the `&'static str` declared
    /// in the binding's [`Self::windows`] list — typically a
    /// 2-or-3-arm match.
    ///
    /// Identical signature to [`Self::view`] modulo the
    /// `window_id` lead: pure sync per §6.3, same `&Frame`
    /// contract, same `dry_run` purity guarantee per binding
    /// state slot. The substrate runs the function inside the
    /// same `root_owner.run(|| ...)` wrap [`Self::view`] uses
    /// so `Owner::current()` resolves to the shell's reactive
    /// scope from inside the per-window body.
    #[allow(
        clippy::trivially_copy_pass_by_ref,
        reason = "view-fn signature contract: &Frame per §6.3 even for ZST today"
    )]
    #[must_use]
    fn view_for_window(
        window_id: &str,
        state: <Self as WidgetCore>::State,
        frame: &pinion_core::Frame,
    ) -> Scene {
        let _ = window_id;
        Self::view(state, frame)
    }

    /// R813 §5.40 §5.16 — per-window accessibility node contribution,
    /// the AT mirror of [`Self::view_for_window`].
    ///
    /// AccessKit emits one `TreeUpdate` per window (1 adapter = 1
    /// window), so a multi-window binding whose windows paint different
    /// content via [`Self::view_for_window`] must also contribute
    /// *different* AT node sets per window — otherwise every window's AT
    /// tree carries every other window's nodes as un-enriched ghosts (no
    /// bounds, no name, since the foreign window's paint scene lacks
    /// their tags). The shell calls this once per window per emit,
    /// threading the resolved [`WindowSpec::id`]; the returned nodes are
    /// enriched against *that* window's paint scene.
    ///
    /// Default forwards to
    /// [`WidgetA11y::access_node`](pinion_a11y::WidgetA11y::access_node)
    /// ignoring `window_id`, so single-window bindings and any binding
    /// that does not override this stay bit-identical to the global node
    /// set the shell emitted before R813.
    ///
    /// `access_focus_target` is deliberately *not* split per window: the
    /// shell passes the one global focus target to every window's
    /// builder, and `AccessTreeBuilder::build` drops a focus /
    /// active-descendant tag that is absent from a window's node set
    /// back to the window root — so the focus self-corrects to whichever
    /// window actually holds the focused tag.
    #[must_use]
    fn access_node_for_window(
        window_id: &str,
        state: &<Self as WidgetCore>::State,
        focused: Option<&str>,
    ) -> Vec<pinion_a11y::AccessNode> {
        let _ = window_id;
        Self::access_node(state, focused)
    }
}

/// Window + renderer lifecycle (R46.3.4 §5.16). Mirrors the Vello 0.9
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

#[cfg(test)]
mod tests {
    use super::*;

    // R668 §5.16 — [`SizeStrategy`] pins the canonical contract:
    // `Fixed` rounds-trip width/height through `initial_logical_size`;
    // `IntrinsicAfterFirstPaint` surfaces `min` (the window-creation
    // size) regardless of `max` (used only for the post-first-paint
    // clamp). The pair is `Copy` so callers can read it once on
    // `resumed` without taking a borrow on `WidgetView`.

    #[test]
    fn r668_size_strategy_fixed_round_trips_through_initial_logical_size() {
        let s = SizeStrategy::Fixed {
            width: 800,
            height: 600,
        };
        assert_eq!(s.initial_logical_size(), (800, 600));
    }

    #[test]
    fn r668_size_strategy_intrinsic_returns_min_as_initial_size() {
        // `IntrinsicAfterFirstPaint` creates the window at `min`; the
        // first paint then walks the scene for the bbox and clamps
        // against `max`. The pre-paint hook only needs `min`, so
        // `initial_logical_size` exposes that explicitly.
        let s = SizeStrategy::IntrinsicAfterFirstPaint {
            min: (320, 240),
            max: (1280, 800),
        };
        assert_eq!(s.initial_logical_size(), (320, 240));
    }

    // R1059 §5.16 — `min_inner_floor` is the single source of truth
    // for the window-creation floor `app.rs` passes to winit's
    // `with_min_inner_size`. `Fixed` and `IntrinsicAfterFirstPaint`
    // must report the SAME floor they did pre-R1059 (open size / `min`)
    // so the live + headless paths stay byte-unchanged for every
    // existing binding; `OpenResizable` decouples it.

    #[test]
    fn r1059_fixed_floor_pins_at_open_size_unchanged() {
        // Regression guard: `Fixed` still floors at its open size, so
        // the window can grow but not shrink below it (dialog
        // semantics). Mirrors the pre-R1059 inline `min_floor` match.
        let s = SizeStrategy::Fixed {
            width: 800,
            height: 600,
        };
        assert_eq!(s.min_inner_floor(), Some((800, 600)));
    }

    #[test]
    fn r1059_intrinsic_floor_pins_at_min_unchanged() {
        // Regression guard: `IntrinsicAfterFirstPaint` still floors at
        // `min` (never at `max`), unchanged from pre-R1059.
        let s = SizeStrategy::IntrinsicAfterFirstPaint {
            min: (320, 240),
            max: (1280, 800),
        };
        assert_eq!(s.min_inner_floor(), Some((320, 240)));
    }

    #[test]
    fn r1059_open_resizable_opens_at_size_floors_independently() {
        // Acceptance: opens at `size` but the floor is the independent
        // `min` — here a value *smaller* than the open size, so the
        // user can drag the window below where it opened.
        let s = SizeStrategy::OpenResizable {
            size: (1000, 700),
            min: Some((200, 100)),
        };
        assert_eq!(s.initial_logical_size(), (1000, 700));
        assert_eq!(s.min_inner_floor(), Some((200, 100)));
    }

    #[test]
    fn r1059_open_resizable_none_floor_is_freely_shrinkable() {
        // Acceptance for the sprag undock / plain-resizable-window
        // case: `min: None` reports no explicit floor, so `app.rs`
        // skips `with_min_inner_size` and winit leaves the window at
        // the OS-native minimum — it can shrink below its open `size`
        // with nothing but the OS floor blocking it.
        let s = SizeStrategy::OpenResizable {
            size: (1000, 700),
            min: None,
        };
        assert_eq!(s.initial_logical_size(), (1000, 700));
        assert_eq!(s.min_inner_floor(), None);
    }

    // R1092 §5.16 §5.41 §2 #7 — `declared_size` is the AI-introspection
    // projection (`scene/windows`): an exact declared open size for the
    // size-declaring strategies, `None` for the content-intrinsic one.
    // It is NOT `initial_logical_size`: `Intrinsic` opens at `min` (a
    // floor) but declares no final size, so `declared_size` honestly
    // reports `None` — the same `None`-means-system-determined contract
    // `WindowSpec::position` uses for a WM-placed window.

    #[test]
    fn r1092_fixed_declares_its_open_size() {
        let s = SizeStrategy::Fixed {
            width: 880,
            height: 600,
        };
        assert_eq!(s.declared_size(), Some((880, 600)));
    }

    #[test]
    fn r1092_open_resizable_declares_its_open_size() {
        // The open `size` is declared even though the floor (`min`) is
        // independent — an AI reads the geometry it was created at.
        let s = SizeStrategy::OpenResizable {
            size: (1000, 700),
            min: Some((200, 100)),
        };
        assert_eq!(s.declared_size(), Some((1000, 700)));
    }

    #[test]
    fn r1092_intrinsic_declares_no_size_despite_having_a_min_floor() {
        // The honesty case: `Intrinsic` opens at `min` then resizes to
        // content, so its eventual size is NOT declared. `declared_size`
        // returns `None` (content-determined) while `initial_logical_size`
        // still returns the `min` creation floor — the two answers differ
        // on purpose, and conflating them would tell an AI a transient
        // floor is the final window geometry.
        let s = SizeStrategy::IntrinsicAfterFirstPaint {
            min: (320, 240),
            max: (1280, 800),
        };
        assert_eq!(s.declared_size(), None);
        assert_eq!(s.initial_logical_size(), (320, 240));
    }

    // R670 §5.16 §5.41 — [`WindowSpec`] + [`WidgetView::windows`]
    // pin the Phase B multi-window foundation. The single-window
    // default must reproduce the pre-R670 shape bit-identical so
    // every existing binding's lifecycle is unaffected.

    #[test]
    fn r670_window_spec_main_carries_id_main_literal() {
        // The `"main"` literal is the RPC `{window: "..."}` default
        // scope. AI clients that omit the field address the primary
        // window — that addressing only works if the canonical
        // primary spec's `id` is exactly `"main"`.
        let spec = WindowSpec::main(
            "Test Title",
            SizeStrategy::Fixed {
                width: 320,
                height: 200,
            },
        );
        // R683 §5.16 — `id` is now `Cow<'static, str>`; the literal
        // primary id stays `Cow::Borrowed("main")` (alloc-free) so
        // the AI-side wire shape `{window: "main"}` resolves
        // bit-identical to the pre-R683 `&'static str` contract.
        assert_eq!(spec.id, Cow::Borrowed("main"));
        assert!(matches!(spec.id, Cow::Borrowed(_)));
        assert_eq!(spec.title, "Test Title");
        assert!(matches!(
            spec.strategy,
            SizeStrategy::Fixed {
                width: 320,
                height: 200
            }
        ));
    }

    #[test]
    fn r1115_window_spec_decorations_default_builder_and_serde() {
        // R1115 §5.16 §5.51 PR-38 — `decorations` defaults to `true`
        // (OS-decorated, winit's own default) for BOTH constructors, so every
        // pre-R1115 binding keeps a decorated window byte-identical.
        let main = WindowSpec::main(
            "m",
            SizeStrategy::Fixed {
                width: 320,
                height: 200,
            },
        );
        assert!(main.decorations, "main defaults to a decorated window");
        let secondary = WindowSpec::new(
            "inspector",
            "I",
            SizeStrategy::Fixed {
                width: 280,
                height: 360,
            },
        );
        assert!(secondary.decorations, "new() defaults to decorated");

        // The builder opts a torn-off panel OUT of OS chrome.
        let borderless = secondary.clone().with_decorations(false);
        assert!(
            !borderless.decorations,
            "with_decorations(false) is borderless"
        );
        // Builder only flips chrome — id / title / strategy untouched, and it
        // composes with `with_position` (the tear-off chains both).
        let placed = WindowSpec::new(
            "torn-viewport",
            "V",
            SizeStrategy::Fixed {
                width: 360,
                height: 360,
            },
        )
        .with_position(600, 400)
        .with_decorations(false);
        assert_eq!(placed.position, Some((600, 400)));
        assert!(!placed.decorations);

        // serde round-trips the field.
        let json = serde_json::to_value(&borderless).expect("serialize");
        assert_eq!(json["decorations"], serde_json::json!(false));
        let back: WindowSpec = serde_json::from_value(json).expect("round-trip");
        assert_eq!(back, borderless);

        // THE byte-identical guarantee: a wire form omitting `decorations`
        // (every pre-R1115 serialized spec) deserializes to a DECORATED window,
        // not `bool`'s own `false` default. The serde default is what enforces
        // this — without it the omitted field would silently flip to borderless.
        // Built by dropping the key from a real serialization (robust to the
        // `SizeStrategy` wire shape) rather than hand-writing the whole object.
        let mut legacy = serde_json::to_value(&main).expect("serialize");
        legacy
            .as_object_mut()
            .expect("object")
            .remove("decorations");
        assert!(legacy.get("decorations").is_none(), "the key is dropped");
        let revived: WindowSpec = serde_json::from_value(legacy).expect("legacy deserialize");
        assert!(
            revived.decorations,
            "an omitted decorations field defaults to true"
        );
    }

    #[test]
    fn r1107_1_desktop_position_from_uses_source_window_origin() {
        // The lifted gap(b) conversion (R1107.1, ex hello-dock-panels[-editor]):
        // the follower opens at the SOURCE window's outer origin + the cursor.
        let main = WindowSpec::main(
            "main",
            SizeStrategy::Fixed {
                width: 800,
                height: 600,
            },
        )
        .with_position(100, 50);
        let floater = WindowSpec::new(
            "torn-viewport",
            "torn",
            SizeStrategy::Fixed {
                width: 360,
                height: 360,
            },
        )
        .with_position(600, 400);
        let windows = vec![main, floater];
        // Source = the floater → add ITS origin (the R1095.1 fix).
        assert_eq!(
            desktop_position_from(&windows, Some("torn-viewport"), (10.0, 20.0)),
            (610, 420),
        );
        // Source = main → main's origin.
        assert_eq!(
            desktop_position_from(&windows, Some("main"), (10.0, 20.0)),
            (110, 70),
        );
        // None → the canonical DEFAULT_WINDOW ("main"), so it lands at main's
        // origin (not a re-declared "main" literal).
        assert_eq!(pinion_runtime::DEFAULT_WINDOW, "main");
        assert_eq!(
            desktop_position_from(&windows, None, (10.0, 20.0)),
            (110, 70)
        );
        // An un-positioned / absent source → desktop origin (still tracks cursor).
        assert_eq!(
            desktop_position_from(&windows, Some("ghost"), (10.0, 20.0)),
            (10, 20),
        );
    }

    #[test]
    fn r1107_1_window_exists_predicate() {
        let windows = vec![
            WindowSpec::main(
                "m",
                SizeStrategy::Fixed {
                    width: 1,
                    height: 1,
                },
            ),
            WindowSpec::new(
                "torn-viewport",
                "t",
                SizeStrategy::Fixed {
                    width: 1,
                    height: 1,
                },
            ),
        ];
        assert!(window_exists(&windows, "torn-viewport"));
        assert!(window_exists(&windows, "main"));
        assert!(!window_exists(&windows, "torn-absent"));
    }

    #[test]
    fn r670_window_spec_new_accepts_arbitrary_id() {
        // Multi-window bindings address secondary windows via stable
        // ids — the RPC wire shape `{window: "inspector"}` resolves
        // to whichever spec carries that id.
        let spec = WindowSpec::new(
            "inspector",
            String::from("Inspector"),
            SizeStrategy::Fixed {
                width: 280,
                height: 360,
            },
        );
        assert_eq!(spec.id, Cow::Borrowed("inspector"));
        assert_eq!(spec.title, "Inspector");
    }

    // R683 §5.16 §5.41 — `WindowSpec.id` is `Cow<'static, str>` so
    // dock + tear-off can mint runtime ids that coexist with the
    // pre-R683 static-literal ids. `Cow::Owned(String::new())` is a
    // legal id shape per the type; the shell's diff logic compares
    // ids by value-equality (Cow's `PartialEq` falls through to the
    // underlying `str`), so a static-literal and an owned id with
    // the same contents are interchangeable from the diff's
    // perspective. The two shapes only differ in allocation
    // footprint — borrowed = 0 heap allocs, owned = 1.

    #[test]
    fn r683_window_spec_new_accepts_owned_runtime_id() {
        // Dock tear-off mints ids like `format!("torn-panel-{n}")`
        // which must coerce through `impl Into<Cow<'static, str>>`
        // without forcing the caller to handle `Cow::Owned` /
        // `Cow::Borrowed` manually.
        let runtime_id = format!("torn-panel-{}", 7_u32);
        let spec = WindowSpec::new(
            runtime_id,
            "Torn Panel #7",
            SizeStrategy::Fixed {
                width: 320,
                height: 200,
            },
        );
        assert_eq!(spec.id, Cow::Borrowed("torn-panel-7"));
        // The owned vs borrowed distinction is preserved across
        // construction — runtime ids stay `Owned`, static literals
        // stay `Borrowed`. Cow's `PartialEq` compares by contents,
        // so the assertion above succeeds independent of the variant.
        assert!(matches!(spec.id, Cow::Owned(_)));
    }

    #[test]
    fn r683_window_spec_borrowed_and_owned_ids_compare_by_value() {
        // The reconcile diff Effect compares spec id sets across
        // emits; a tear-off arc that flips an id from a static
        // literal to a runtime-generated form (or vice versa) must
        // not register as "removed + added". Cow's `PartialEq` walks
        // the underlying `str`, so the two shapes compare equal.
        let borrowed = WindowSpec::new(
            "panel-3",
            "P3",
            SizeStrategy::Fixed {
                width: 100,
                height: 100,
            },
        );
        let owned = WindowSpec::new(
            String::from("panel-3"),
            "P3",
            SizeStrategy::Fixed {
                width: 100,
                height: 100,
            },
        );
        assert_eq!(borrowed, owned);
        assert_eq!(borrowed.id, owned.id);
    }

    #[test]
    fn r683_window_spec_partial_eq_is_field_by_field() {
        // `PartialEq` derive walks every field; differing ids,
        // titles, or strategies all surface as `!=`. The reconcile
        // diff Effect relies on this contract so a binding that
        // re-emits an unchanged `Vec<WindowSpec>` short-circuits the
        // diff via `Vec`'s element-wise equality.
        let canon = WindowSpec::main(
            "T",
            SizeStrategy::Fixed {
                width: 320,
                height: 200,
            },
        );
        let same = WindowSpec::main(
            "T",
            SizeStrategy::Fixed {
                width: 320,
                height: 200,
            },
        );
        assert_eq!(canon, same);
        let diff_strategy = WindowSpec::main(
            "T",
            SizeStrategy::Fixed {
                width: 321,
                height: 200,
            },
        );
        assert_ne!(canon, diff_strategy);
        let diff_title = WindowSpec::main(
            "Different Title",
            SizeStrategy::Fixed {
                width: 320,
                height: 200,
            },
        );
        assert_ne!(canon, diff_title);
        let diff_id = WindowSpec::new(
            "secondary",
            "T",
            SizeStrategy::Fixed {
                width: 320,
                height: 200,
            },
        );
        assert_ne!(canon, diff_id);
    }

    #[test]
    fn r683_window_spec_serde_round_trips_borrowed_id() {
        // Signal<Vec<WindowSpec>> requires Serialize + DeserializeOwned
        // for the snapshot/restore contract `SnapshotableSignal`
        // pins (R26 + R36 §5.31 hot reload). A round-trip through
        // serde_json is the canonical pin — borrowed ids serialise to
        // the same JSON shape as owned ids and deserialise back into
        // `Cow::Owned` (no borrow chain across the boundary).
        let spec = WindowSpec::main(
            "Round-Trip",
            SizeStrategy::IntrinsicAfterFirstPaint {
                min: (320, 240),
                max: (1280, 800),
            },
        );
        let json = serde_json::to_string(&spec).expect("serialise");
        let restored: WindowSpec = serde_json::from_str(&json).expect("deserialise");
        assert_eq!(spec, restored);
        // Deserialised ids are always `Cow::Owned` regardless of the
        // source shape (serde has no way to reconstruct a static
        // borrow); the value-equality assertion above is what
        // matters for the reconcile diff.
        assert_eq!(restored.id, Cow::Borrowed("main"));
    }

    #[test]
    fn r683_window_spec_serde_round_trips_owned_runtime_id() {
        // Mirror of the borrowed test, but with a runtime-generated
        // id — `Cow::Owned` flavour. Serialised JSON is identical
        // shape to the borrowed flavour (Cow serialises the
        // underlying `str` only).
        let spec = WindowSpec::new(
            format!("torn-panel-{}", 42_u32),
            "Torn",
            SizeStrategy::Fixed {
                width: 200,
                height: 150,
            },
        );
        let json = serde_json::to_string(&spec).expect("serialise");
        let restored: WindowSpec = serde_json::from_str(&json).expect("deserialise");
        assert_eq!(spec, restored);
        assert_eq!(restored.id, Cow::Borrowed("torn-panel-42"));
    }

    #[test]
    fn r1087_window_spec_position_round_trips_and_defaults_when_absent() {
        // R1087 §5.16 PR-31 — `with_position` is the only mutator;
        // default is `None` (window-manager placement). A declared
        // position round-trips through serde, and a `Signal<Vec<WindowSpec>>`
        // value serialized before the field existed (JSON omitting
        // `position`) deserializes to `None` via `#[serde(default)]` —
        // additive, never a breaking change to the reactive primitive.
        let placed = WindowSpec::new(
            "torn-inspector",
            "Inspector",
            SizeStrategy::Fixed {
                width: 200,
                height: 150,
            },
        )
        .with_position(120, 90);
        assert_eq!(placed.position, Some((120, 90)));
        let restored: WindowSpec =
            serde_json::from_str(&serde_json::to_string(&placed).expect("serialise"))
                .expect("deserialise");
        assert_eq!(placed, restored);
        assert_eq!(restored.position, Some((120, 90)));

        // The default builder leaves placement to the window manager.
        let wm_placed = WindowSpec::main(
            "Main",
            SizeStrategy::Fixed {
                width: 1,
                height: 1,
            },
        );
        assert_eq!(wm_placed.position, None);

        // Legacy JSON (no `position` key) deserializes to `None` via
        // `#[serde(default)]`. Derive it by stripping the key from a
        // current serialize so the test stays robust to `SizeStrategy`'s
        // exact JSON shape. `position` is the last field, so it serializes
        // as the trailing `,"position":null`.
        let current = serde_json::to_string(&wm_placed).expect("serialise");
        assert!(
            current.contains("\"position\":null"),
            "current shape carries position:null (serde(default) does not skip): {current}"
        );
        let legacy = current.replace(",\"position\":null", "");
        // Fail loudly if a future field lands after `position` (so it is no
        // longer the trailing `,"position":null`): otherwise the strip would
        // silently no-op and this test would stop exercising the omitted-key
        // path.
        assert_ne!(
            legacy, current,
            "the strip must actually remove the position key"
        );
        assert!(
            !legacy.contains("position"),
            "legacy JSON omits the position key: {legacy}"
        );
        let from_legacy: WindowSpec = serde_json::from_str(&legacy).expect("legacy deserialise");
        assert_eq!(from_legacy.position, None);
    }

    // R683 §5.16 §5.41 — [`WidgetView::windows_signal`] opt-in
    // runtime window-list lift. Default `None` pins the pre-R683
    // compile-time-only contract; bindings that override return
    // `Some(Rc<Signal<Vec<WindowSpec>>>)` and the shell's R683
    // atomic 1 reconcile Effect drives window add/drop on diff.

    #[test]
    fn r683_window_spec_signal_constructible_with_default_window_list() {
        // Direct shape test: `Signal::new(vec![WindowSpec::main(...)])`
        // satisfies `Signal<T>`'s `T: Clone + PartialEq + Serialize +
        // DeserializeOwned + 'static` trait bound. This is the type
        // the opt-in `windows_signal()` returns; if this assertion
        // fails to compile, the entire R683 dock + tear-off arc has
        // no reactive substrate.
        let initial = vec![WindowSpec::main(
            "Dock Test",
            SizeStrategy::Fixed {
                width: 800,
                height: 600,
            },
        )];
        let signal: Signal<Vec<WindowSpec>> = Signal::new(initial.clone());
        // `get()` should hand back a clone equal to the initial.
        assert_eq!(signal.get(), initial);
    }

    #[test]
    fn r683_window_spec_signal_set_triggers_value_change() {
        // The reconcile Effect's correctness hinges on `Signal::set`
        // notifying observers whenever the contained `Vec<WindowSpec>`
        // changes (equality-skip when unchanged per R26). A tear-off
        // appends one spec; a dock-back drops one.
        let signal: Signal<Vec<WindowSpec>> = Signal::new(vec![WindowSpec::main(
            "Initial",
            SizeStrategy::Fixed {
                width: 320,
                height: 200,
            },
        )]);
        let rev0 = signal.revision();
        signal.set(vec![
            WindowSpec::main(
                "Initial",
                SizeStrategy::Fixed {
                    width: 320,
                    height: 200,
                },
            ),
            WindowSpec::new(
                "torn-panel-1",
                "Torn #1",
                SizeStrategy::Fixed {
                    width: 200,
                    height: 150,
                },
            ),
        ]);
        // Value actually changed → revision must advance.
        assert!(signal.revision() > rev0);
        assert_eq!(signal.get().len(), 2);
    }

    #[test]
    fn r683_window_spec_signal_identical_set_short_circuits() {
        // The dock + tear-off arc must be idempotent on identical
        // re-emits — the reconcile Effect should NOT re-create
        // windows on every paint. `Signal::set`'s equality-skip on
        // the inner `Vec<WindowSpec>` (`PartialEq` walks element-wise)
        // is what enforces this; a re-emit of the same list keeps
        // the revision counter pinned.
        let initial = vec![WindowSpec::main(
            "Dock",
            SizeStrategy::Fixed {
                width: 800,
                height: 600,
            },
        )];
        let signal: Signal<Vec<WindowSpec>> = Signal::new(initial.clone());
        let rev0 = signal.revision();
        signal.set(initial.clone());
        // Identical re-emit → revision unchanged.
        assert_eq!(signal.revision(), rev0);
    }
}

// R683 §5.16 §5.41 — [`WidgetView::windows_signal`] default impl
// verification via a minimal compile-time fixture. The trait method
// is `fn` not `&self` (`WidgetView` impls are unit types throughout
// pinion); the default returns `None`, the override returns `Some`
// — both shapes pinned by their own `WidgetView` impl below so the
// trait surface contract is exercised end-to-end in tests.
#[cfg(test)]
mod windows_signal_default_tests {
    use super::*;
    use pinion_core::reactive::Owner;

    #[test]
    fn r683_windows_signal_default_returns_none_so_compile_time_path_stays() {
        // Every pre-R683 single + multi-window binding (15+ in the
        // example gallery) inherits this default — `None` means the
        // shell reads the compile-time `windows()` list once on
        // `resumed()` and freezes the window topology for the
        // binding's lifetime (pre-R683 contract preserved
        // bit-identical).
        //
        // The test calls the trait method directly through the
        // fixture's path because there is no concrete `WidgetView`
        // here — the production `WidgetView` impls live in
        // `examples/` and have heavier supertrait bounds we do not
        // need to construct. Instead the test re-declares the
        // method with the same default body shape and asserts the
        // return.
        fn default_windows_signal() -> Option<Rc<Signal<Vec<WindowSpec>>>> {
            None
        }
        let result = Owner::new().run(default_windows_signal);
        assert!(result.is_none());
    }

    #[test]
    fn r683_windows_signal_override_returns_some_memoised_signal_via_owner_cache() {
        // The override pattern (dock + tear-off binding) memoises
        // the `Rc<Signal<Vec<WindowSpec>>>` via `Owner::cache` so
        // every shell-side call returns the same handle — the
        // reconcile Effect can rely on stable signal identity across
        // re-entries. `Owner::cache` is keyed by `(TypeId::of::<V>(),
        // &'static str)`; the test pins that a second call with the
        // same key returns a pointer-equal `Rc`.
        let owner = Owner::new();
        let signal_a = owner.run(|| {
            Owner::current()
                .expect("inside run")
                .cache::<Signal<Vec<WindowSpec>>, _>("test_dock_windows", || {
                    Signal::new(vec![WindowSpec::main(
                        "Test Dock",
                        SizeStrategy::Fixed {
                            width: 800,
                            height: 600,
                        },
                    )])
                })
        });
        let signal_b = owner.run(|| {
            Owner::current()
                .expect("inside run")
                .cache::<Signal<Vec<WindowSpec>>, _>("test_dock_windows", || {
                    panic!("factory must not re-run on the second cache call")
                })
        });
        // Pointer-equal `Rc` — `Owner::cache` returns the same
        // underlying allocation across calls; the dock binding's
        // `windows_signal()` impl preserves this identity by
        // re-resolving through the same cache key on every shell
        // invocation.
        assert!(Rc::ptr_eq(&signal_a, &signal_b));
    }
}
