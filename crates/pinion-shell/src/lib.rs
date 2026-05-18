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

use std::io::{BufRead, Write};
use std::sync::Arc;
use std::thread;

use pinion_core::external::{External, IntrospectValue};
use pinion_core::scene::BoxNode;
use pinion_core::{Frame, Scene, SceneRevision};
use pinion_rpc::{
    build_layout_node, dispatch, DispatchContext, LayoutNode, PreviewLedger,
};
use pinion_runtime::{
    compute_layout, paint_adapter, walk_scene_and_drain, InputRouter, IntentQueue, PointerId,
};
use pinion_text::LayoutCache;
use vello::peniko::Color as PenikoColor;
use vello::Scene as VelloScene;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, Touch, TouchPhase, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

/// Winit user-event variant carrying one stdin-fed JSON-RPC 2.0
/// frame across to the UI thread. The shell's [`AppShell::user_event`]
/// handler is the sole consumer; the stdin reader thread is the sole
/// producer.
#[derive(Debug, Clone)]
pub enum AppEvent {
    /// One JSON-RPC 2.0 frame read from stdin, awaiting dispatch.
    RpcRequest(String),
}

/// `Send`-able renderer wrapper trait that the pinion-forge codegen
/// output (`HelloFooRenderer` + `HelloFooRendererError`) bridges into
/// via the [`vello_renderer_impl!`] macro. The shell is generic over
/// `R: VelloRenderer` so each binary keeps the zero-virtual-dispatch
/// Vello pipeline §5.16 R45 guarantees — there is no `dyn VelloRenderer`
/// anywhere in the hot path.
///
/// The shape mirrors the codegen template (R46.3.3) exactly — async
/// `new`, sync `render` + `resize`, `Sized` so the shell can store
/// `Box<Self>` in [`RenderState::Active`] without pulling object-safety
/// constraints in. `Error: Display` so the shell can `eprintln!` any
/// failure without forcing the application into the error type.
pub trait VelloRenderer: Sized {
    /// Concrete error type emitted by the renderer constructor and
    /// `render`. The codegen template emits `HelloFooRendererError`
    /// which derives `Debug` + impls `Display` + `Error`; meets the
    /// `Display` bound directly.
    type Error: core::fmt::Display;

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

    /// Submit one Vello scene frame against the configured surface.
    ///
    /// # Errors
    /// Implementation-defined — frame submission failure or
    /// swapchain acquisition failure.
    fn render(
        &mut self,
        scene: &VelloScene,
        base_color: PenikoColor,
    ) -> Result<(), Self::Error>;

    /// Resize the wgpu surface to match a new window dimension.
    fn resize(&mut self, width: u32, height: u32);
}

/// Bridge a pinion-forge-emitted renderer struct into the
/// [`VelloRenderer`] trait. The codegen template emits inherent
/// methods (`async fn new<W>(...)`, `fn render(...)`, `fn resize(...)`)
/// matching the trait signature byte-for-byte; this macro generates
/// a thin trait-impl that forwards each method call to the inherent
/// one. Keeps the codegen template free of any pinion-shell coupling
/// (consumers without the shell can still use the renderer).
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
        impl $crate::VelloRenderer for $name {
            type Error = $err;

            async fn new<W>(
                target: W,
                width: u32,
                height: u32,
            ) -> ::core::result::Result<Self, Self::Error>
            where
                W: ::core::convert::Into<::vello::wgpu::SurfaceTarget<'static>>,
            {
                <$name>::new(target, width, height).await
            }

            fn render(
                &mut self,
                scene: &::vello::Scene,
                base_color: ::vello::peniko::Color,
            ) -> ::core::result::Result<(), Self::Error> {
                <$name>::render(self, scene, base_color)
            }

            fn resize(&mut self, width: u32, height: u32) {
                <$name>::resize(self, width, height);
            }
        }
    };
}

/// Application-supplied widget binding. Each visual binary implements
/// this once on a unit type; `pinion_shell::run::<MyView>()` does the
/// rest.
///
/// Only the widget-specific bits live here: the cached state shape,
/// the typed event enum, the [`VelloRenderer`] concrete type, the
/// view fn, the introspect parser, the [`Scene::External`] factory,
/// the input-router tag, optional keybindings, optional log format,
/// the window title, and the initial window size.
pub trait WidgetView: 'static {
    /// Cached projection of the live state scene. `Copy` so the shell
    /// can clone it into the §5.12 `paint_producer` closure without
    /// lifetime gymnastics; `Debug` + `PartialEq` for the transition
    /// log + change-detection redraw request.
    type State: Copy + core::fmt::Debug + PartialEq;

    /// Typed widget event enum — usually the SCXML-emitted
    /// `<Widget>Event` (e.g. `ButtonEvent`, `ToggleEvent`). Threaded
    /// through [`WidgetView::event_name`] before reaching the §5.15
    /// `invoke("send", Text(<name>))` channel so the application
    /// keeps typed event payloads without giving up the symbolic
    /// RPC contract.
    type Event: Copy;

    /// Concrete pinion-forge-emitted renderer (`HelloFooRenderer`).
    /// `'static` so [`RenderState`] can store `Box<Self::Renderer>`
    /// across the suspend/resume cycle without lifetime parameters.
    type Renderer: VelloRenderer + 'static;

    /// Build a fresh state scene root. Called once at [`AppShell::new`]
    /// — should return `Scene::External(ExternalNode::new(<my widget>)
    /// .with_tag(Self::tag()))` so the [`InputRouter`] hit-test on
    /// the paint-side tag routes to this node.
    fn create_external() -> Box<dyn External>;

    /// Stable identifier matching the paint-side `Container::tag` the
    /// view fn attaches to the interactive surface (track, button
    /// container, etc.). The [`InputRouter`] forwards pointer events
    /// to any [`Scene::External`] in the state scene whose tag equals
    /// this hit-test target.
    fn tag() -> &'static str;

    /// Extract the cached projection from the live state scene via
    /// the §5.15 introspect channel — the same path an RPC
    /// `scene/query /external/<slot>` request uses, so the
    /// `cached_state` and the AI client always see the same value.
    fn read_state(scene: &Scene) -> Self::State;

    /// Build the paint scene for the current cached state. Pure sync
    /// per `§6.3` (`dry_run` invariant): same `(state, frame)` always
    /// yields the same `Scene`. The shell calls [`compute_layout`] on
    /// the result before handing it to `paint_adapter::to_vello`, so
    /// the view fn need not (and should not) resolve pixel rects.
    fn view(state: Self::State, frame: &Frame) -> Scene;

    /// Convert a typed widget event into the symbolic name the §5.15
    /// `invoke("send", IntrospectValue::Text(<name>))` channel expects.
    /// SCXML-internal variants that never come from winit should
    /// route through a wildcard with a sentinel name the parser
    /// rejects (mirrors `ButtonEvent::__internal__` precedent).
    fn event_name(event: Self::Event) -> &'static str;

    /// Window title displayed by the OS / winit's `set_title`. Static
    /// because winit doesn't take ownership of a `String` at window
    /// creation.
    fn title() -> &'static str;

    /// Default window dimensions in logical pixels. `winit` applies
    /// the per-monitor DPI scale, so this is "what the user sees" on
    /// a 1.0× display. The shell honours this exactly on first
    /// [`resumed`](AppShell::resumed); subsequent resizes go through
    /// `WindowEvent::Resized`.
    fn initial_size() -> (u32, u32);

    /// Optional keyboard event mapping. The shell consults this on
    /// every `Key::Character(<c>)` press; `None` means "no
    /// keybinding for this char" and the shell ignores the press.
    /// `Escape` is always handled by the shell (window quit) and is
    /// not threaded through this hook.
    ///
    /// Default returns `None` for every key — widgets without
    /// keyboard affordances need no override.
    #[must_use]
    fn keybinding(_key: &str) -> Option<Self::Event> {
        None
    }

    /// R51.37 §5.35 — escape hatch for keyboard affordances that the
    /// enum-typed [`keybinding`](Self::keybinding) channel cannot
    /// express. The shell consults this AFTER `keybinding` returns
    /// `None` for [`Key::Character`] presses, and as the *only* hook
    /// for non-character named keys (`ArrowLeft`, `ArrowRight`,
    /// `ArrowUp`, `ArrowDown`, `Home`, `End`, `PageUp`, `PageDown`,
    /// `Tab`, `Enter`, `Space`). `Escape` remains shell-reserved
    /// (window quit) and is not threaded through this hook.
    ///
    /// Implementations receive the authoritative state scene `&mut`
    /// and may walk it to the matching [`Scene::External`] to call
    /// [`ExternalIntrospect::intervene`](pinion_core::external::ExternalIntrospect::intervene)
    /// — the same side door the RPC `scene/intervene` route uses.
    /// This closes the W3C/ARIA Slider keyboard-accessibility gap
    /// where arrow / Home / End / Page* must mutate the slider value
    /// but no [`SliderEvent`](pinion_core::widgets::slider::SliderEvent)
    /// variant carries the new float (event payloads are unit-only).
    ///
    /// Returns `true` if the key was handled (the shell bumps the
    /// §5.34 revision, re-reads state, drains intents, and repaints
    /// on visible change). Returns `false` to defer to whatever
    /// fallback the shell adds next (none today; same swallow
    /// semantics as an unmatched `keybinding`).
    ///
    /// Default returns `false` for every key — widgets without
    /// keyboard affordances beyond `keybinding` need no override.
    /// The five `examples/hello-*` paint-side amortization binaries
    /// (button / toggle / checkbox / radio) all rely on the default;
    /// `hello-slider` overrides to wire arrow + page + home/end to
    /// `intervene("value", Float(...))`.
    #[must_use]
    fn apply_key(_scene: &mut Scene, _key: &str) -> bool {
        false
    }

    /// Format the cached state for stderr logging on the transition
    /// path (`from -> to`) and the final-state line. Default falls
    /// back to `Debug`; widgets with composite state can format a
    /// human-readable view (e.g. `Toggle::fmt_state_log` may render
    /// `"Idle / Off"`).
    fn fmt_state_log(state: &Self::State) -> String {
        format!("{state:?}")
    }
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

/// The framework-side shell. Generic over a widget binding
/// [`WidgetView`]; concrete examples instantiate via `run::<V>()`.
pub struct AppShell<V: WidgetView> {
    /// Authoritative state scene — owns the live widget External via
    /// `Box<dyn External>`. Both winit input (via the `InputRouter`)
    /// and RPC dispatch (via the `DispatchContext`) reach the SCXML
    /// statechart through this single scene.
    scene: Scene,
    /// Cached projection of the introspect state, kept in sync by
    /// `refresh_state` after every input. Drives change-detection
    /// for the redraw request + the view fn's input.
    cached_state: V::State,
    /// §5.20 intent harvest buffer. Refilled by `drain_intents` after
    /// every winit / RPC event; consumed by stderr logging. The
    /// `scene/intents` RPC method drains the same source independently
    /// since the underlying `External::pending_intents` is the single
    /// queue.
    intent_queue: IntentQueue,
    /// §5.34 preview lifecycle ledger — passed into every
    /// `pinion_rpc::dispatch` call alongside the scene. Lifecycle RPC
    /// methods read or mutate it through interior mutability;
    /// non-lifecycle methods ignore it.
    previews: PreviewLedger,
    /// §5.34 R40.4 OCC revision token. `dispatch` auto-bumps on
    /// mutating RPC methods; [`AppShell::forward`] explicitly bumps
    /// after the winit-side `invoke` since that path bypasses the
    /// dispatcher entirely.
    revision: SceneRevision,
    /// R48 §5.35 framework-side input dispatch primitive. Owns the
    /// retained paint scene + cursor state + `hover_target` and
    /// routes pointer events to the matching `ExternalNode` in
    /// `self.scene` (the one tagged `V::tag()`).
    router: InputRouter,
    /// R46.5 §5.16 suspend / resume lifecycle (R46.3.4 pattern).
    render: RenderState<V::Renderer>,
    /// Reusable Vello scene buffer — reset at the start of each frame
    /// rather than reallocated. Vello's Scene API expects this pattern
    /// (Linebender Vello examples / Xilem).
    vello_scene: VelloScene,
    /// R47.3 §5.36 — owned [`LayoutCache`] (LRU 256). `paint_adapter`'s
    /// Text arm consults this cache for every `Scene::Text` it walks
    /// so the view fn's static labels shape once on first paint and
    /// hit the cache on every subsequent frame. The cache also owns
    /// parley's `FontContext` / `LayoutContext` so the shell never
    /// holds parley state directly.
    text_cache: LayoutCache,
    /// R47.7.5 §5.12 — most recent winit-rendered frame's paint scene
    /// projected into a [`LayoutNode`] tree. Refreshed at the end of
    /// every paint pass; `dispatch_rpc` hands it to
    /// `DispatchContext::with_last_paint_layout` so AI clients reach
    /// the winit-actual frame via `scene/layout {viewport: null}`.
    /// `None` until the first frame has rendered.
    last_paint_layout: Option<LayoutNode>,
}

impl<V: WidgetView> AppShell<V> {
    /// Construct the shell with a freshly-built state scene and the
    /// initial cached state read through the §5.15 introspect channel.
    /// The renderer + window stay [`RenderState::Suspended`] until the
    /// first `resumed` event lands.
    #[must_use]
    pub fn new() -> Self {
        use pinion_core::scene::ExternalNode;
        // R22 §5.20: the scene-side `ExternalNode.tag` supplies the
        // widget identifier used as the intent-tag prefix. The widget
        // External itself emits the kind (e.g. "click", "toggle"); the
        // runtime walk composes `<tag>.<kind>` on drain.
        let scene = Scene::External(
            ExternalNode::new(V::create_external()).with_tag(V::tag()),
        );
        // Initial cached state via the same introspect channel
        // everything else uses — single source of truth.
        let cached_state = V::read_state(&scene);
        eprintln!(
            "shell: initial state = {}",
            V::fmt_state_log(&cached_state),
        );
        Self {
            scene,
            cached_state,
            intent_queue: IntentQueue::new(),
            previews: PreviewLedger::default(),
            revision: SceneRevision::default(),
            router: InputRouter::new(),
            render: RenderState::Suspended(None),
            vello_scene: VelloScene::new(),
            text_cache: LayoutCache::new(),
            last_paint_layout: None,
        }
    }

    /// R51.45 §5.35 — winit [`Touch`] dispatch. Each finger mints a
    /// distinct [`PointerId::touch(finger_id)`] so two simultaneous
    /// touches drive two widgets without aliasing the capture lock.
    ///
    /// * [`TouchPhase::Started`] runs a synthetic
    ///   [`InputRouter::cursor_moved`] first so the hover target
    ///   resolves under the press point before the
    ///   [`InputRouter::pointer_down`] lands — mirrors the mouse
    ///   case where `CursorMoved` always precedes `MouseInput`.
    /// * [`TouchPhase::Moved`] forwards the new position.
    /// * [`TouchPhase::Ended`] runs `pointer_up` then `cursor_left`
    ///   so the post-release hover refresh fires and the finger's
    ///   cursor state is dropped (a future touch with the same
    ///   finger id is a new gesture per winit's `WindowEvent::Touch`
    ///   contract).
    /// * [`TouchPhase::Cancelled`] follows the same `Ended` path
    ///   with the carry that the dispatched `PointerUp` may emit a
    ///   commit-class intent the gesture did not actually
    ///   authorise — a future `PointerCancel` event variant or
    ///   `InputRouter::cancel_pointer` lands as a separate round.
    fn handle_touch(&mut self, touch: Touch) {
        let pid = PointerId::touch(touch.id);
        match touch.phase {
            TouchPhase::Started => {
                self.router.cursor_moved(
                    pid,
                    touch.location.x,
                    touch.location.y,
                    &mut self.scene,
                );
                self.router.pointer_down(pid, &mut self.scene);
            }
            TouchPhase::Moved => {
                self.router.cursor_moved(
                    pid,
                    touch.location.x,
                    touch.location.y,
                    &mut self.scene,
                );
            }
            TouchPhase::Ended | TouchPhase::Cancelled => {
                self.router.pointer_up(pid, &mut self.scene);
                self.router.cursor_left(pid, &mut self.scene);
            }
        }
    }

    /// Translate a typed widget event into the symbolic
    /// `invoke("send", Text(<name>))` call — the same channel the RPC
    /// `scene/invoke` route uses. Failures from the statechart
    /// (`InvokeError::Rejected` etc.) are swallowed: the SCXML decides
    /// whether a given transition fires.
    fn forward(&mut self, event: V::Event) {
        let name = V::event_name(event);
        if let Scene::External(node) = &mut self.scene {
            if let Some(intro) = node.handle.introspect_mut() {
                let _ = intro.invoke("send", IntrospectValue::Text(name.to_string()));
            }
        }
        // §5.34 R40.4: winit-side input bypasses the RPC dispatcher,
        // so bump the OCC revision token directly. Spurious bumps for
        // SCXML-rejected events are acceptable per the
        // conservative-bump policy.
        self.revision.bump();
        self.refresh_state();
        self.drain_intents();
    }

    /// R51.37 §5.35 — route a key string through
    /// [`WidgetView::apply_key`] and, on handled (`true`), run the
    /// same post-input bookkeeping as [`Self::forward`]: bump the
    /// §5.34 revision, re-read cached state (paint on visible
    /// change), drain pending intents. Unhandled keys are swallowed
    /// quietly (same shape as an unmatched [`WidgetView::keybinding`]).
    fn apply_key(&mut self, key: &str) {
        if V::apply_key(&mut self.scene, key) {
            self.revision.bump();
            self.refresh_state();
            self.drain_intents();
        }
    }

    /// Dispatch one JSON-RPC frame against the LIVE state scene.
    /// `scene/invoke /external/send PointerEnter` (and friends) drive
    /// the SCXML the same way a winit click would.
    ///
    /// R47.7.2 §5.12 — `scene/layout` requests reach the framework
    /// via `DispatchContext::with_paint_producer`: the closure captures
    /// `cached_state` (`Copy`) and `text_cache` (`&mut`), runs the
    /// view fn and `compute_layout` for the hypothetical viewport,
    /// and returns the freshly-measured paint scene. The dispatch
    /// block scope releases the split borrows before
    /// `self.refresh_state()` runs.
    fn dispatch_rpc(&mut self, request: &str) {
        let resp = {
            // Disjoint-field split mutable borrows so the producer
            // closure can capture `cached_state` + `text_cache` while
            // the dispatcher still gets `scene` + `previews` + `revision`.
            let scene_ptr = &mut self.scene;
            let previews = &self.previews;
            let revision = &self.revision;
            let cached_state = self.cached_state;
            let text_cache_ptr = &mut self.text_cache;
            let render_ref = &self.render;
            let last_paint = self.last_paint_layout.as_ref();
            let mut produce = |w: u32, h: u32| -> Scene {
                let frame = Frame::new();
                let mut paint = V::view(cached_state, &frame);
                compute_layout(&mut paint, text_cache_ptr, w, h);
                paint
            };
            // R47.7.4.2 — `scene/resize` reaches winit through this
            // closure: `request_inner_size` queues a size change that
            // winit emits as a `Resized` event on the next loop pass,
            // and the explicit `request_redraw` shortens the gap to
            // the new paint scene observation.
            let mut resize_req = |w: u32, h: u32| {
                if let RenderState::Active { window, .. } = render_ref {
                    let _ = window.request_inner_size(LogicalSize::new(w, h));
                    window.request_redraw();
                }
            };
            // R47.7.5 §5.12 — surface the most recent winit-rendered
            // frame to the dispatcher so `scene/layout {viewport: null}`
            // returns the actual frame snapshot. Builder pattern keeps
            // the `Option` wiring branchless at the AI-client level.
            let mut ctx = DispatchContext::new(scene_ptr, previews, revision)
                .with_paint_producer(&mut produce)
                .with_resize_request(&mut resize_req);
            if let Some(snapshot) = last_paint {
                ctx = ctx.with_last_paint_layout(snapshot);
            }
            dispatch(&mut ctx, request)
        };
        if let Some(resp) = resp {
            let mut out = std::io::stdout().lock();
            if writeln!(out, "{resp}").is_err() {
                // stdout closed (downstream consumer gone) — silently
                // skip; do not abort the GUI loop on a broken pipe.
            }
        }
        self.refresh_state();
        self.drain_intents();
    }

    /// §5.20 live dogfood: walk the scene, drain any pending intents
    /// into the local queue, log each one to stderr. The
    /// `scene/intents` RPC method races with this drain — whichever
    /// caller harvests first wins (poll-form, single-consumer v0).
    fn drain_intents(&mut self) {
        walk_scene_and_drain(&mut self.scene, &mut self.intent_queue);
        for intent in self.intent_queue.drain() {
            eprintln!(
                "shell: intent {} payload={:?}",
                intent.tag_str(),
                intent.payload,
            );
        }
    }

    /// Re-read the cached state from the live scene; log and repaint
    /// if it changed since the previous refresh.
    fn refresh_state(&mut self) {
        let now = V::read_state(&self.scene);
        if now != self.cached_state {
            eprintln!(
                "shell: state {} -> {}",
                V::fmt_state_log(&self.cached_state),
                V::fmt_state_log(&now),
            );
            self.cached_state = now;
            self.request_redraw();
        }
    }

    fn request_redraw(&self) {
        if let RenderState::Active { window, .. } = &self.render {
            window.request_redraw();
        }
    }

    /// Build the paint scene for the current cached state, run layout,
    /// hand it to the framework-side `paint_adapter` walker, and submit
    /// the resulting `vello::Scene` to the renderer. No-op while
    /// suspended (R46.3.4 lifecycle).
    fn render(&mut self) {
        let RenderState::Active { window, renderer } = &mut self.render else {
            return;
        };
        let size = window.inner_size();
        let Some(w) = core::num::NonZeroU32::new(size.width) else { return };
        let Some(h) = core::num::NonZeroU32::new(size.height) else { return };
        let frame = Frame::new();
        let mut paint_scene = V::view(self.cached_state, &frame);
        compute_layout(&mut paint_scene, &mut self.text_cache, w.get(), h.get());
        self.vello_scene.reset();
        let base = paint_adapter::root_background(&paint_scene);
        paint_adapter::to_vello(
            &paint_scene,
            &|_b: &BoxNode| None,
            &mut self.text_cache,
            &mut self.vello_scene,
        );
        if let Err(e) = renderer.render(&self.vello_scene, base) {
            eprintln!("shell: vello render: {e}");
        }
        // R47.7.5 §5.12 — snapshot the freshly-measured paint scene
        // into a `LayoutNode` tree so `scene/layout {viewport: null}`
        // can return the *actual* winit-rendered frame on the next
        // dispatch. Must run before `router.update_paint_scene` moves
        // `paint_scene` out of scope.
        self.last_paint_layout = Some(build_layout_node(&paint_scene, "/0"));
        // R48 §5.35: hand the post-layout paint scene to the framework
        // router. The router retains it for subsequent hit-tests and
        // re-resolves hover_target now (window resize may have moved
        // the interactive rect under a stationary cursor).
        self.router.update_paint_scene(paint_scene, &mut self.scene);
        self.refresh_state();
        self.drain_intents();
    }
}

impl<V: WidgetView> Default for AppShell<V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<V: WidgetView> ApplicationHandler<AppEvent> for AppShell<V> {
    /// R46.3.4 — winit may fire `resumed` more than once on platforms
    /// that suspend (Android, Wayland-compositor focus changes). The
    /// Vello canonical pattern caches the previous `Window` across
    /// the drop-and-recreate cycle so the OS-side handle survives,
    /// while the GPU renderer is freshly constructed each time.
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if matches!(self.render, RenderState::Active { .. }) {
            return;
        }
        let cached = match core::mem::replace(&mut self.render, RenderState::Suspended(None)) {
            RenderState::Suspended(cached) => cached,
            RenderState::Active { .. } => unreachable!("matched as non-Active above"),
        };
        let (init_w, init_h) = V::initial_size();
        let window = if let Some(w) = cached {
            w
        } else {
            let attrs = Window::default_attributes()
                .with_title(V::title())
                .with_inner_size(LogicalSize::new(f64::from(init_w), f64::from(init_h)));
            match event_loop.create_window(attrs) {
                Ok(w) => Arc::new(w),
                Err(e) => {
                    eprintln!("shell: window create failed: {e}");
                    event_loop.exit();
                    return;
                }
            }
        };
        let size = window.inner_size();
        let renderer = pollster::block_on(<V::Renderer as VelloRenderer>::new(
            Arc::clone(&window),
            size.width.max(1),
            size.height.max(1),
        ));
        let renderer = match renderer {
            Ok(r) => r,
            Err(e) => {
                eprintln!("shell: renderer init: {e}");
                // Keep the window cached so a subsequent `resumed` can
                // retry — only the renderer creation failed.
                self.render = RenderState::Suspended(Some(window));
                event_loop.exit();
                return;
            }
        };
        self.render = RenderState::Active {
            window,
            renderer: Box::new(renderer),
        };
        // R47.7.5 — winit does not auto-emit `RedrawRequested` on
        // `resumed` (platform-dependent). Explicitly request the
        // first redraw so `last_paint_layout` populates before the
        // first AI client `scene/layout {viewport: null}` lands.
        self.request_redraw();
        eprintln!(
            "shell: {} resumed (initial size {}x{}); keys handled by V::keybinding + Esc=quit; pipe JSON-RPC 2.0 frames on stdin",
            V::title(),
            init_w,
            init_h,
        );
    }

    /// R46.3.4 — release the GPU-side renderer on suspend so the OS
    /// can reclaim the wgpu surface. The winit window itself is
    /// cached for the next `resumed` so its handle / OS state survives.
    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        if let RenderState::Active { window, .. } =
            core::mem::replace(&mut self.render, RenderState::Suspended(None))
        {
            self.render = RenderState::Suspended(Some(window));
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                eprintln!(
                    "shell: final state = {}",
                    V::fmt_state_log(&self.cached_state),
                );
                event_loop.exit();
            }
            // R48 §5.35: all pointer routing flows through the framework
            // InputRouter. The handler bodies just forward the winit
            // event into the router; the router does the hit-test, emits
            // PointerEnter/Leave/Down/Up to the matching ExternalNode,
            // and the shell only refreshes its cached state + drains
            // intents afterwards. CursorEntered is a no-op (winit
            // guarantees a CursorMoved follows, which resolves the
            // real cursor position).
            WindowEvent::CursorMoved { position, .. } => {
                // R51.38 §5.35 — winit mouse events are single-source
                // on every desktop platform pinion supports; the
                // shell threads `PointerId::MOUSE` unconditionally.
                // Touch / pen wiring will mint distinct ids via
                // `PointerId::touch` when the `WindowEvent::Touch`
                // handler lands as a follow-up.
                self.router.cursor_moved(
                    PointerId::MOUSE,
                    position.x,
                    position.y,
                    &mut self.scene,
                );
                self.refresh_state();
                self.drain_intents();
            }
            WindowEvent::CursorLeft { .. } => {
                self.router.cursor_left(PointerId::MOUSE, &mut self.scene);
                self.refresh_state();
                self.drain_intents();
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                self.router.pointer_down(PointerId::MOUSE, &mut self.scene);
                self.refresh_state();
                self.drain_intents();
            }
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => {
                self.router.pointer_up(PointerId::MOUSE, &mut self.scene);
                self.refresh_state();
                self.drain_intents();
            }
            // R51.45 §5.35 — winit `WindowEvent::Touch` closes the
            // R51.38 multi-pointer first-design substrate arc.
            // Dispatch is in [`AppShell::handle_touch`] below.
            WindowEvent::Touch(touch) => {
                self.handle_touch(touch);
                self.refresh_state();
                self.drain_intents();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == ElementState::Pressed {
                    match event.logical_key.as_ref() {
                        Key::Named(NamedKey::Escape) => event_loop.exit(),
                        Key::Character(c) => {
                            if let Some(ev) = V::keybinding(c) {
                                self.forward(ev);
                            } else {
                                self.apply_key(c);
                            }
                        }
                        Key::Named(named) => {
                            if let Some(key_str) = named_key_str(named) {
                                self.apply_key(key_str);
                            }
                        }
                        _ => {}
                    }
                }
            }
            WindowEvent::Resized(size) => {
                if let RenderState::Active { renderer, .. } = &mut self.render {
                    renderer.resize(size.width.max(1), size.height.max(1));
                }
            }
            WindowEvent::RedrawRequested => self.render(),
            _ => {}
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: AppEvent) {
        match event {
            AppEvent::RpcRequest(json) => self.dispatch_rpc(&json),
        }
    }
}

/// R51.37 §5.35 — bridge from winit's [`NamedKey`] enum to the
/// W3C-aligned `KeyboardEvent.key` strings the
/// [`WidgetView::apply_key`] contract speaks. Only the keys with
/// established cross-platform widget meanings are surfaced;
/// `NamedKey::Escape` is filtered upstream (shell-reserved quit),
/// and unmapped variants return `None` so the shell stays silent on
/// keys no widget cares about. The ASCII / W3C names match the
/// strings Material / `SwiftUI` / Qt / W3C ARIA Slider authoring
/// patterns specify, so a widget implementation can match against
/// the same identifiers a browser-side application would consume.
fn named_key_str(named: NamedKey) -> Option<&'static str> {
    match named {
        NamedKey::ArrowLeft => Some("ArrowLeft"),
        NamedKey::ArrowRight => Some("ArrowRight"),
        NamedKey::ArrowUp => Some("ArrowUp"),
        NamedKey::ArrowDown => Some("ArrowDown"),
        NamedKey::Home => Some("Home"),
        NamedKey::End => Some("End"),
        NamedKey::PageUp => Some("PageUp"),
        NamedKey::PageDown => Some("PageDown"),
        NamedKey::Tab => Some("Tab"),
        NamedKey::Enter => Some("Enter"),
        NamedKey::Space => Some("Space"),
        _ => None,
    }
}

/// Background thread: read JSON-RPC 2.0 lines from stdin and forward
/// each as an `AppEvent::RpcRequest` user event. Blank lines are
/// skipped; EOF or any read error terminates the thread quietly (the
/// GUI loop keeps running). The proxy `send_event` fails only after
/// the event loop has shut down, in which case we also exit the
/// thread.
fn spawn_stdin_rpc_reader(proxy: EventLoopProxy<AppEvent>) {
    thread::spawn(move || {
        let stdin = std::io::stdin();
        let handle = stdin.lock();
        for line in handle.lines() {
            let Ok(text) = line else {
                break;
            };
            if text.trim().is_empty() {
                continue;
            }
            if proxy.send_event(AppEvent::RpcRequest(text)).is_err() {
                break;
            }
        }
    });
}

/// Run the visual binary end-to-end: build the winit event loop with
/// the [`AppEvent`] user-event slot, spawn the stdin RPC reader, run
/// the [`AppShell<V>`] until quit. The single line every shell
/// consumer needs in `fn main()`.
///
/// # Panics
/// Panics if `winit::event_loop::EventLoop::with_user_event().build()`
/// fails — that constructor only errors on platforms that cannot
/// supply a user-event loop (none of the desktop / mobile targets
/// pinion supports), so this is treated as an unrecoverable setup
/// fault rather than a propagated error.
pub fn run<V: WidgetView>() {
    let event_loop = EventLoop::<AppEvent>::with_user_event()
        .build()
        .expect("winit EventLoop::with_user_event failed");
    event_loop.set_control_flow(ControlFlow::Wait);
    spawn_stdin_rpc_reader(event_loop.create_proxy());
    let mut app = AppShell::<V>::new();
    if let Err(e) = event_loop.run_app(&mut app) {
        eprintln!("shell: event loop error: {e}");
    }
}
