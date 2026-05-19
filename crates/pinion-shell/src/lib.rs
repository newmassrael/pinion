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

use pinion_a11y::{AccessAction, AccessFocus, AccessNode, AccessTreeBuilder};
use pinion_core::external::External;
use pinion_core::scene::BoxNode;
use pinion_core::{Frame, Scene};
use pinion_runtime::{paint_adapter, PointerId};
use vello::peniko::Color as PenikoColor;
use vello::Scene as VelloScene;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

mod substrate;

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

    /// R51.53 §5.39 — focusable tag enumeration in Tab order.
    /// Returned tags must match either `Self::tag()` (the top-level
    /// widget) or a sub-tag the view fn paints inside the widget
    /// (composite widgets like `RadioGroup` register the group's
    /// `tag()` as a single tab stop and roving-tabindex among its
    /// children internally).
    ///
    /// Default returns a single-entry list containing `Self::tag()`,
    /// which is the right shape for every Tier-1 single-widget
    /// example. Composite widget bindings or multi-widget views
    /// override to enumerate all focusable children.
    #[must_use]
    fn focusable_tags() -> Vec<&'static str> {
        vec![Self::tag()]
    }

    /// R51.37 §5.35 / R51.53 §5.39 — escape hatch for keyboard
    /// affordances that the enum-typed
    /// [`keybinding`](Self::keybinding) channel cannot express. The
    /// shell consults this AFTER `keybinding` returns `None` for
    /// [`Key::Character`] presses, and as the *only* hook for
    /// non-character named keys (`ArrowLeft`, `ArrowRight`,
    /// `ArrowUp`, `ArrowDown`, `Home`, `End`, `PageUp`, `PageDown`,
    /// `Enter`, `Space`). `Escape` and `Tab` / `Shift+Tab` are
    /// shell-reserved — `Escape` quits the window, `Tab` advances
    /// the [`FocusManager`] (§5.39), neither reaches this hook.
    ///
    /// R51.53 added the `focused` argument carrying the
    /// [`FocusManager::focused`] tag at dispatch time. Widgets that
    /// match against `focused` route keys only when their own tag
    /// is focused; the previous broadcast model (every keypress
    /// fired every widget's `apply_key`) caused aliasing with
    /// multiple focusable widgets on screen.
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
    #[must_use]
    fn apply_key(_scene: &mut Scene, _focused: Option<&str>, _key: &str) -> bool {
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

    /// R51.62 §5.40 — accessibility semantic tree contribution.
    ///
    /// Return one [`AccessNode`] per AT-visible tag the widget paints
    /// (atomic widgets emit a single node; composite widgets like
    /// `RadioGroup` emit the group node plus one per child radio).
    /// Bounds are filled in by the shell after layout — widgets need
    /// not (and should not) resolve pixel rects here. The `focused`
    /// argument carries [`FocusManager::focused`] at emit time so
    /// each [`AccessNode::state`] can set its `focused` flag without
    /// the widget tracking focus state independently.
    ///
    /// Default returns an empty vector — widgets that opt out are
    /// AT-invisible (a deliberate intent declaration; see WAI-ARIA
    /// Authoring Practices on `role="presentation"`). Tier-1
    /// catalogue widgets override starting R51.63
    /// (`Button → AriaRole::Button`, `Toggle → AriaRole::Switch`,
    /// `Checkbox → AriaRole::CheckBox`, `Radio → AriaRole::RadioButton`,
    /// `Slider → AriaRole::Slider`, `RadioGroup → AriaRole::RadioGroup`).
    #[must_use]
    fn access_node(_state: &Self::State, _focused: Option<&str>) -> Vec<AccessNode> {
        Vec::new()
    }

    /// R51.66 §5.40 — composite focus model for AccessKit.
    ///
    /// AccessKit's `TreeUpdate::focus` points at a single `NodeId`,
    /// and ARIA Authoring Practices' roving-tabindex pattern adds
    /// a second piece: the focused composite parent's
    /// `aria-activedescendant` (lowered as
    /// `accesskit::Node::set_active_descendant`) names the currently
    /// addressed child within that parent.
    ///
    /// R51.71 §5.40 — return type switched from `Option<String>`
    /// (a single tag, which conflated focus with active descendant
    /// by addressing the child directly in `TreeUpdate::focus`) to
    /// [`AccessFocus`] (typed parent + optional child carrier).
    /// Now:
    ///
    /// * Atomic widgets (Button, Switch, Slider) return
    ///   `Some(AccessFocus::atomic(tag))` — own `NodeId`, no
    ///   descendant.
    /// * Composite widgets (`RadioGroup`) return
    ///   `Some(AccessFocus::composite(parent_tag, child_tag))` —
    ///   parent `NodeId` becomes `TreeUpdate::focus`, parent
    ///   `accesskit::Node` is annotated via
    ///   `set_active_descendant(child_id)`.
    ///
    /// Default wraps `focused` as [`AccessFocus::atomic`].
    /// Returning `None` leaves AccessKit's focus on the synthetic
    /// window root.
    #[must_use]
    fn access_focus_target(
        _state: &Self::State,
        focused: Option<&str>,
    ) -> Option<AccessFocus> {
        focused.map(AccessFocus::atomic)
    }

    /// R51.70 §5.40 — composite-side dispatch for an AT-driven action
    /// targeting a sub-child by the segment after `#` in the widget
    /// tag.
    ///
    /// `AccessKit`'s `ActionRequest` delivers `Click` / `Default` /
    /// `Focus` / `Increment` / `Decrement` against a `NodeId`; the
    /// shell recovers the widget tag and, for composite widgets,
    /// splits it at `#` (e.g. `"main_group#1"` → `("main_group",
    /// "1")`). The shell focuses the parent tag uniformly and then
    /// calls this hook so composite widgets can wire the activation
    /// through their existing wire-format invocation path
    /// (`invoke("send", Text("<i>:<EventName>"))` for the
    /// `RadioGroup`, similarly for future `ListBox` / `MenuButton`
    /// / `TreeView` composites).
    ///
    /// Returns `true` if the action was handled — the shell then
    /// bumps the §5.34 revision, refreshes cached state, and drains
    /// pending intents (mirrors `apply_a11y_key`). Returns `false`
    /// to let the shell fall through to the atomic-widget chain
    /// (`focus_set` + `apply_key("Enter")`).
    ///
    /// Default returns `false` — atomic widgets receive no
    /// composite-child requests because their `access_node` impls
    /// never expose `#`-suffixed tags.
    ///
    /// WAI-ARIA / WCAG 4.1.2 (Name, Role, **Value**) coverage:
    /// without this hook, an AT issuing `Click` on a composite
    /// child cannot programmatically set the underlying value —
    /// the §5.40 substrate prior to R51.70 only logged a carry. The
    /// hook closes the write-path gap for composites.
    fn access_child_invoke(
        _scene: &mut Scene,
        _sub_tag: &str,
        _action: AccessAction,
    ) -> bool {
        false
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
///
/// R51.76 §5.40 — every piece of testable dispatch state lives in
/// [`ShellCore`]; this struct only owns the winit / wgpu / AccessKit
/// surface so headless tests can target [`ShellCore`] directly.
pub struct AppShell<V: WidgetView> {
    /// R51.76 §5.40 — extracted dispatch substrate (scene, cached
    /// state, focus, intents, previews, revision, router, modifiers,
    /// text cache, last paint snapshot, AT caches, redraw flag).
    ///
    /// R51.83 §5.40 — private. All surface-side access happens
    /// through the substrate's typed methods + accessors so the
    /// boundary stays one-way.
    core: ShellCore<V>,
    /// R46.5 §5.16 suspend / resume lifecycle (R46.3.4 pattern).
    render: RenderState<V::Renderer>,
    /// Reusable Vello scene buffer — reset at the start of each frame
    /// rather than reallocated. Vello's Scene API expects this pattern
    /// (Linebender Vello examples / Xilem).
    vello_scene: VelloScene,
    /// R51.62 §5.40 — winit [`EventLoopProxy`] cached so the shell
    /// can construct the per-window `accesskit_winit::Adapter` on
    /// `resumed` (the constructor requires both the active event
    /// loop and a proxy that produces `Adapter`-routed user events).
    proxy: EventLoopProxy<AppEvent>,
    /// R51.62 §5.40 — per-window `accesskit_winit::Adapter`. `None`
    /// while the window is `Suspended`; populated by `resumed` once
    /// the winit `Window` exists. The adapter relays winit events
    /// (`Moved`, `Resized`, `Focused`) into AccessKit's internal
    /// state and delivers AT-side requests (`InitialTreeRequested`,
    /// `ActionRequested`, `AccessibilityDeactivated`) through
    /// [`AppEvent::AccessKit`].
    accesskit: Option<accesskit_winit::Adapter>,
}


impl<V: WidgetView> AppShell<V> {
    /// R51.76 §5.40 — construct the shell with a freshly-built state
    /// scene and the initial cached state read through the §5.15
    /// introspect channel. Delegates the dispatch substrate to
    /// [`ShellCore::new`]; this constructor only adds the winit /
    /// wgpu / AccessKit surface (`render`, `vello_scene`, `proxy`,
    /// `accesskit`) which lives on `AppShell` itself.
    ///
    /// `proxy` is the same `EventLoopProxy<AppEvent>` the
    /// [`spawn_stdin_rpc_reader`] background thread holds; the shell
    /// retains a clone so it can hand a fresh copy to the
    /// `accesskit_winit::Adapter` on `resumed` (R51.62 §5.40).
    #[must_use]
    pub fn new(proxy: EventLoopProxy<AppEvent>) -> Self {
        Self {
            core: ShellCore::new(),
            render: RenderState::Suspended(None),
            vello_scene: VelloScene::new(),
            proxy,
            accesskit: None,
        }
    }

    /// R51.76 §5.40 — drain the [`ShellCore::redraw_requested`] flag
    /// and forward to the live winit `Window` (if attached).
    ///
    /// Called at the end of every `window_event` / `user_event`
    /// `ApplicationHandler` arm so all redraw requests collapse into a
    /// single `Window::request_redraw` call (the winit idiom: winit
    /// itself coalesces back-to-back redraw requests, but the
    /// AppShell-level coalesce avoids the round-trip when no window
    /// is attached yet).
    fn drain_redraw_to_winit(&mut self) {
        if self.core.take_redraw_request()
            && let RenderState::Active { window, .. } = &self.render
        {
            window.request_redraw();
        }
    }

    /// R51.76 §5.40 — thin wrapper around [`ShellCore::dispatch_rpc`]
    /// that builds the production `resize_request` closure from the
    /// live winit `Window` and writes the JSON-RPC response to
    /// stdout. Headless tests call `ShellCore::dispatch_rpc` directly
    /// with a no-op closure.
    fn dispatch_rpc(&mut self, request: &str) {
        let render_ref = &self.render;
        let mut resize_req = |w: u32, h: u32| {
            // R47.7.4.2 — `scene/resize` reaches winit through this
            // closure: `request_inner_size` queues a size change that
            // winit emits as a `Resized` event on the next loop pass,
            // and the explicit `request_redraw` shortens the gap to
            // the new paint scene observation.
            if let RenderState::Active { window, .. } = render_ref {
                let _ = window.request_inner_size(LogicalSize::new(w, h));
                window.request_redraw();
            }
        };
        let resp = self.core.dispatch_rpc(request, &mut resize_req);
        if let Some(resp) = resp {
            let mut out = std::io::stdout().lock();
            if writeln!(out, "{resp}").is_err() {
                // stdout closed (downstream consumer gone) — silently
                // skip; do not abort the GUI loop on a broken pipe.
            }
        }
    }

    /// Build the paint scene for the current cached state, run layout,
    /// hand it to the framework-side `paint_adapter` walker, and submit
    /// the resulting `vello::Scene` to the renderer. No-op while
    /// suspended (R46.3.4 lifecycle).
    ///
    /// R51.76 §5.40 — the AccessKit emit decision is delegated to
    /// [`ShellCore::compute_access_emit`] so the same diff logic is
    /// exercised by headless tests; the AppShell-side responsibility
    /// is just to feed the plan to `Adapter::update_if_active`.
    fn render(&mut self) {
        let RenderState::Active { window, renderer } = &mut self.render else {
            return;
        };
        let size = window.inner_size();
        let Some(w) = core::num::NonZeroU32::new(size.width) else { return };
        let Some(h) = core::num::NonZeroU32::new(size.height) else { return };
        // R51.80 §5.16 §5.36 — ShellCore owns the paint scene
        // pipeline; AppShell only handles the vello/wgpu submit.
        let paint_scene = self.core.compute_paint_scene(w.get(), h.get());
        self.vello_scene.reset();
        let base = paint_adapter::root_background(&paint_scene);
        paint_adapter::to_vello(
            &paint_scene,
            &|_b: &BoxNode| None,
            self.core.text_cache_mut(),
            &mut self.vello_scene,
        );
        // R51.58 §5.39 — paint the ARIA focus ring on top of the
        // widget visual. Runs after `to_vello` so the ring overlays
        // its target; runs before `renderer.render` so it lands in
        // the same frame submit. No-op when nothing is focused.
        paint_adapter::paint_focus_ring(
            &paint_scene,
            self.core.focus().focused(),
            &mut self.vello_scene,
        );
        if let Err(e) = renderer.render(&self.vello_scene, base) {
            eprintln!("shell: vello render: {e}");
        }
        // R51.62 / R51.80 §5.40 — AccessKit emit. The substrate
        // assembles the nodes + focus inputs; the surface plans +
        // optionally emits + commits. No accesskit-side work when
        // no AT client is attached (`update_if_active` is a no-op).
        if self.accesskit.is_some() {
            let (nodes, at_focus) =
                self.core.collect_access_emit_inputs(&paint_scene);
            let window_bounds =
                pinion_core::scene::Rect::new(0, 0, size.width, size.height);
            let decision =
                self.core.plan_access_emit(&nodes, at_focus.as_ref());
            if decision.should_emit
                && let Some(adapter) = self.accesskit.as_mut()
            {
                let initial = decision.initial;
                let dirty = decision.dirty;
                let at_focus_ref = at_focus.as_ref();
                let nodes_ref = &nodes;
                adapter.update_if_active(|| {
                    let mut builder = AccessTreeBuilder::new();
                    if !initial {
                        builder.initial(false);
                    }
                    for node in nodes_ref {
                        builder.add(node);
                    }
                    if !initial {
                        builder.dirty_tags(dirty);
                    }
                    if let Some(f) = at_focus_ref {
                        builder.focused(Some(&f.focus_tag));
                        if let Some(child) = &f.active_descendant {
                            // R51.71 §5.40 — roving-tabindex active
                            // descendant.
                            builder.active_descendant(&f.focus_tag, child);
                        }
                    } else {
                        builder.focused(None);
                    }
                    builder.build(Some(window_bounds))
                });
            }
            // R51.77 / R51.79 §5.40 — commit step. By-value Vec move
            // into the cache; nodes consumed here. Idempotent on
            // !should_emit so the next frame's plan diffs against
            // the post-emit baseline.
            self.core.commit_access_emit(nodes, at_focus.as_ref());
        }
        // R51.80 §5.12 §5.35 — snapshot the rendered scene + hand it
        // to the input router + refresh state + drain intents in one
        // method.
        self.core.finalize_frame(paint_scene);
    }

    /// R51.78 §5.39 — pressed-key routing surface. Translates the
    /// winit-specific [`Key`] enum into the winit-free
    /// [`ShellCore::handle_focus_traverse`] /
    /// [`ShellCore::handle_character_key`] /
    /// [`ShellCore::handle_named_key`] triple, keeping `Escape`
    /// shell-side because it terminates the event loop.
    ///
    /// Pre-R51.78 this helper did the full dispatch (focus traversal,
    /// `V::keybinding` lookup, `V::apply_key`) inline and was untestable
    /// without a winit `ActiveEventLoop`. R51.78 pushes the substrate
    /// logic into [`ShellCore`] and leaves only the winit↔substrate
    /// adapter shape here.
    fn handle_key_press(
        &mut self,
        event_loop: &ActiveEventLoop,
        logical_key: &Key,
    ) {
        match logical_key.as_ref() {
            Key::Named(NamedKey::Escape) => event_loop.exit(),
            Key::Named(NamedKey::Tab) => {
                self.core.handle_focus_traverse(self.core.modifiers_shift_key());
            }
            Key::Character(c) => self.core.handle_character_key(c),
            Key::Named(named) => {
                if let Some(key_str) = named_key_str(named) {
                    self.core.handle_named_key(key_str);
                }
            }
            _ => {}
        }
    }

    /// R51.62 §5.40 — relay one winit `WindowEvent` to the
    /// `accesskit_winit::Adapter` (if attached).
    ///
    /// Every winit event must reach the adapter before the shell
    /// processes it so the AT-side mirror of window bounds / focus
    /// state stays consistent. The adapter handles `Moved` /
    /// `Resized` / `Focused` internally (`set_root_window_bounds` +
    /// `update_window_focus_state`) and ignores everything else, so
    /// the relay is unconditional.
    fn forward_to_accesskit(&mut self, event: &WindowEvent) {
        if let (Some(adapter), RenderState::Active { window, .. }) =
            (self.accesskit.as_mut(), &self.render)
        {
            adapter.process_event(window, event);
        }
    }

    /// R51.62 §5.40 — dispatch one AT-side event reported by
    /// `accesskit_winit`.
    ///
    /// * `InitialTreeRequested` (AT attaches): trigger a redraw — the
    ///   next `render` will see `Adapter::update_if_active` as active
    ///   and emit the full tree. No state mutation here.
    /// * `ActionRequested(req)`: routed through
    ///   [`ShellCore::handle_action_request`] which lifts the
    ///   AccessKit action into the same dispatch path the winit
    ///   keyboard arm uses.
    /// * `AccessibilityDeactivated`: log only — the adapter stays in
    ///   place so a subsequent AT reconnect can reuse it without
    ///   recreating the per-window state.
    fn handle_accesskit_event(&mut self, event: accesskit_winit::Event) {
        use accesskit_winit::WindowEvent as AccessEvent;
        match event.window_event {
            AccessEvent::InitialTreeRequested => {
                self.core.request_redraw();
            }
            AccessEvent::ActionRequested(req) => {
                self.core.handle_action_request(&req);
            }
            AccessEvent::AccessibilityDeactivated => {
                eprintln!("shell: accesskit deactivated");
            }
        }
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
        // R51.62 §5.40 — construct the per-window accesskit_winit
        // Adapter once renderer init succeeds. Skipped if an adapter
        // already exists (cached-window resume path). The proxy is
        // cloned because Adapter consumes one internally for each of
        // its three handler hooks (activation / action / deactivation).
        if self.accesskit.is_none() {
            let adapter = accesskit_winit::Adapter::with_event_loop_proxy(
                event_loop,
                &window,
                self.proxy.clone(),
            );
            self.accesskit = Some(adapter);
        }
        self.render = RenderState::Active {
            window,
            renderer: Box::new(renderer),
        };
        // R47.7.5 — winit does not auto-emit `RedrawRequested` on
        // `resumed` (platform-dependent). Explicitly request the
        // first redraw so `last_paint_layout` populates before the
        // first AI client `scene/layout {viewport: null}` lands.
        self.core.request_redraw();
        self.drain_redraw_to_winit();
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
        self.forward_to_accesskit(&event);
        match event {
            WindowEvent::CloseRequested => {
                eprintln!(
                    "shell: final state = {}",
                    V::fmt_state_log(self.core.cached_state()),
                );
                event_loop.exit();
            }
            // R48 / R51.80 §5.35: all pointer routing flows through
            // the framework `InputRouter` via [`ShellCore`] wrapper
            // methods. The handler arms only translate winit events
            // into the substrate's pinion-native shape.
            WindowEvent::CursorMoved { position, .. } => {
                // R51.38 §5.35 — winit mouse events are single-source
                // on every desktop platform pinion supports; the
                // shell threads `PointerId::MOUSE` unconditionally.
                self.core
                    .cursor_moved(PointerId::MOUSE, position.x, position.y);
            }
            WindowEvent::CursorLeft { .. } => {
                self.core.cursor_left(PointerId::MOUSE);
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                self.core.mouse_pressed(PointerId::MOUSE);
            }
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => {
                self.core.mouse_released(PointerId::MOUSE);
            }
            // R51.45 §5.35 — winit `WindowEvent::Touch` closes the
            // R51.38 multi-pointer first-design substrate arc.
            WindowEvent::Touch(touch) => {
                self.core.touch_event(touch);
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                // R51.53 §5.39 — winit emits `KeyEvent` without
                // modifier state, so cache the most-recent value
                // out-of-band for Shift+Tab detection.
                self.core.set_modifiers(modifiers.state());
            }
            WindowEvent::Focused(focused) => {
                // R51.59 §5.39 — Window blur / refocus. ARIA Focus
                // Order asks the framework to reinstate the focused
                // widget when the user returns to the window.
                if focused {
                    self.core.window_focused();
                } else {
                    self.core.window_blurred();
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == ElementState::Pressed {
                    self.handle_key_press(event_loop, &event.logical_key);
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
        self.drain_redraw_to_winit();
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: AppEvent) {
        match event {
            AppEvent::RpcRequest(json) => self.dispatch_rpc(&json),
            AppEvent::AccessKit(ak) => self.handle_accesskit_event(ak),
        }
        self.drain_redraw_to_winit();
    }
}


/// R51.37 §5.35 — bridge from winit's [`NamedKey`] enum to the
/// W3C-aligned `KeyboardEvent.key` strings the
/// [`WidgetView::apply_key`] contract speaks. Only the keys with
/// established cross-platform widget meanings are surfaced;
/// `NamedKey::Escape` is filtered upstream (shell-reserved quit),
/// `NamedKey::Tab` is filtered upstream (R51.53 §5.39 `FocusManager`
/// swallow), and unmapped variants return `None` so the shell stays
/// silent on keys no widget cares about. The ASCII / W3C names match
/// the strings Material / `SwiftUI` / Qt / W3C ARIA Slider authoring
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
    let mut app = AppShell::<V>::new(event_loop.create_proxy());
    if let Err(e) = event_loop.run_app(&mut app) {
        eprintln!("shell: event loop error: {e}");
    }
}
