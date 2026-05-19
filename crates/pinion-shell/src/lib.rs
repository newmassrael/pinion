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

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, Write};
use std::sync::Arc;
use std::thread;

use accesskit::NodeId;
use pinion_core::external::{External, IntrospectValue};
use pinion_core::scene::BoxNode;
use pinion_core::{Frame, Scene, SceneRevision};
use pinion_rpc::{
    build_layout_node, dispatch, DispatchContext, LayoutNode, PreviewLedger,
};
use pinion_a11y::{
    tag_to_node_id, translate_action, AccessAction, AccessFocus, AccessNode,
    AccessTreeBuilder, PinionAccessAction, ROOT_NODE_ID,
};
use pinion_runtime::{
    compute_layout, paint_adapter, rect_for_tag, walk_scene_and_drain, FocusManager, InputRouter,
    IntentQueue, PointerId,
};
use pinion_text::LayoutCache;
use vello::peniko::Color as PenikoColor;
use vello::Scene as VelloScene;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, Touch, TouchPhase, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Window, WindowId};

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

/// R51.76 §5.40 — framework-side dispatch substrate, decoupled from
/// winit / wgpu / `accesskit_winit`.
///
/// [`ShellCore`] owns every piece of state that `AppShell`'s dispatch
/// path mutates: the authoritative state scene, the cached state
/// projection, the §5.20 intent queue, the §5.34 preview ledger +
/// revision token, the §5.35 input router, the §5.39 focus manager
/// and cached winit modifier state, the §5.36 text layout cache, the
/// §5.12 last-paint snapshot, and the §5.40 incremental AT-emit
/// caches (tag map / node diff / focus diff / initial-emit flag).
///
/// The split is the textbook substrate/surface separation: the
/// shell-coupled bits (winit `Window`, wgpu surface, Vello renderer,
/// `accesskit_winit::Adapter`, `EventLoopProxy`) live in [`AppShell`];
/// everything else lives here so the dispatch surface is reachable
/// from headless tests without standing up a winit `EventLoop` or a
/// real wgpu device.
///
/// `request_redraw` no longer touches a `Window` directly — it sets
/// [`ShellCore::redraw_requested`] so [`AppShell`] can drain the flag
/// once per event-loop iteration and forward to `Window::request_redraw`
/// when a `Window` exists, while headless tests just observe the flag.
pub struct ShellCore<V: WidgetView> {
    /// Authoritative state scene — owns the live widget External via
    /// `Box<dyn External>`. Both winit input (via the `InputRouter`)
    /// and RPC dispatch (via the `DispatchContext`) reach the SCXML
    /// statechart through this single scene.
    pub(crate) scene: Scene,
    /// Cached projection of the introspect state, kept in sync by
    /// `refresh_state` after every input. Drives change-detection
    /// for the redraw request + the view fn's input.
    pub(crate) cached_state: V::State,
    /// §5.20 intent harvest buffer. Refilled by `drain_intents` after
    /// every winit / RPC event; consumed by stderr logging. The
    /// `scene/intents` RPC method drains the same source independently
    /// since the underlying `External::pending_intents` is the single
    /// queue.
    pub(crate) intent_queue: IntentQueue,
    /// §5.34 preview lifecycle ledger — passed into every
    /// `pinion_rpc::dispatch` call alongside the scene. Lifecycle RPC
    /// methods read or mutate it through interior mutability;
    /// non-lifecycle methods ignore it.
    pub(crate) previews: PreviewLedger,
    /// §5.34 R40.4 OCC revision token. `dispatch` auto-bumps on
    /// mutating RPC methods; [`ShellCore::forward`] explicitly bumps
    /// after the winit-side `invoke` since that path bypasses the
    /// dispatcher entirely.
    pub(crate) revision: SceneRevision,
    /// R48 §5.35 framework-side input dispatch primitive. Owns the
    /// retained paint scene + cursor state + `hover_target` and
    /// routes pointer events to the matching `ExternalNode` in
    /// `self.scene` (the one tagged `V::tag()`).
    pub(crate) router: InputRouter,
    /// R51.53 §5.39 framework-side focus state owner. Tab/Shift+Tab
    /// traverses [`FocusManager::tab_order`] (seeded from
    /// `V::focusable_tags()` at boot); click on a tagged widget
    /// aliases [`FocusManager::focus_set`]; click on background
    /// aliases [`FocusManager::focus_clear`]. The shell consults the
    /// manager on every key dispatch so `apply_key` runs only when
    /// the widget's own tag is focused (eliminating the broadcast
    /// aliasing the pre-R51.53 design carried).
    pub(crate) focus: FocusManager,
    /// R51.53 §5.39 — winit [`ModifiersState`] cache. Refreshed by
    /// `WindowEvent::ModifiersChanged`; consulted on every
    /// `KeyboardInput` for Shift detection (Shift+Tab = `focus_prev`).
    /// winit emits `KeyEvent` without modifier state, so the shell
    /// has to track it out-of-band.
    pub(crate) modifiers: ModifiersState,
    /// R47.3 §5.36 — owned [`LayoutCache`] (LRU 256). `paint_adapter`'s
    /// Text arm consults this cache for every `Scene::Text` it walks
    /// so the view fn's static labels shape once on first paint and
    /// hit the cache on every subsequent frame. The cache also owns
    /// parley's `FontContext` / `LayoutContext` so the shell never
    /// holds parley state directly.
    pub(crate) text_cache: LayoutCache,
    /// R47.7.5 §5.12 — most recent winit-rendered frame's paint scene
    /// projected into a [`LayoutNode`] tree. Refreshed at the end of
    /// every paint pass; `dispatch_rpc` hands it to
    /// `DispatchContext::with_last_paint_layout` so AI clients reach
    /// the winit-actual frame via `scene/layout {viewport: null}`.
    /// `None` until the first frame has rendered.
    pub(crate) last_paint_layout: Option<LayoutNode>,
    /// R51.67 §5.40 — `NodeId` → widget tag map from the most recent
    /// `TreeUpdate`. Refreshed at the end of every `render` (when an
    /// adapter is attached). Consumed by `handle_action_request` so
    /// AT-side actions arriving via `AppEvent::AccessKit` resolve
    /// back to the widget tag without recomputing the tree.
    pub(crate) last_access_tag_map: HashMap<NodeId, String>,
    /// R51.72 §5.40 — previous frame's `AccessNode` set (keyed by
    /// `tag`). The next frame diffs against this to compute the
    /// dirty subset passed to `AccessTreeBuilder::dirty_tags`.
    /// AccessKit's incremental-update guidance: "an update should
    /// only include nodes that are new or changed".
    pub(crate) last_access_nodes: HashMap<String, AccessNode>,
    /// R51.72 §5.40 — `true` until the first `TreeUpdate` has been
    /// emitted (carrying the `Tree` metadata + every node). After
    /// that, subsequent emits set `initial(false)` and pass only
    /// the dirty subset.
    pub(crate) access_emit_initial: bool,
    /// R51.75 §5.40 — previous frame's `AccessFocus`. Compared
    /// alongside the dirty-node diff: when neither nodes nor focus
    /// changed, `update_if_active` is skipped entirely so a
    /// steady-state animation frame costs no AT-side traffic.
    pub(crate) last_access_focus: Option<pinion_a11y::AccessFocus>,
    /// R51.76 §5.40 — flag set whenever a method on
    /// [`ShellCore`] decides the next frame should repaint. Drained
    /// by [`AppShell`] after each event-loop iteration and forwarded
    /// to `Window::request_redraw` when a winit `Window` is attached;
    /// remains observable for headless tests that never spin up a
    /// `Window`. The flag-based design replaces the pre-R51.76
    /// direct `window.request_redraw()` call buried in every
    /// dispatch method, which made the substrate untestable without
    /// a real event loop.
    pub(crate) redraw_requested: bool,
}

/// R51.77 §5.40 — pure decision returned by
/// [`ShellCore::plan_access_emit`].
///
/// Carries only the emit verdict + diff metadata: the should-emit
/// flag (`should_emit`), the initial-frame flag (`initial` — forces a
/// full tree metadata emit on the first frame), and the dirty-tag
/// set. Nodes / focus stay with the caller — the decision struct
/// borrows them while planning and lets the render path consume them
/// once for `Adapter::update_if_active` (no clone for the closure).
///
/// R51.77 split: pre-R51.77 `AccessEmitPlan` bundled the decision
/// AND the consumed nodes / focus AND mutated the `ShellCore` cache
/// inside a single `compute_access_emit` call (silent surprise —
/// pure-looking name but mutating). The textbook canonical shape
/// separates pure planning from the cache-update commit step. See
/// [`ShellCore::commit_access_emit`].
#[derive(Debug)]
pub struct AccessEmitDecision {
    /// `true` when the caller should invoke
    /// `Adapter::update_if_active`. `false` when the tree is
    /// byte-identical to the previous frame's emit (no dirty nodes,
    /// no focus change, not initial).
    pub should_emit: bool,
    /// `true` for the first emit (carries `Tree` metadata + every
    /// node). Subsequent emits set `false` and pass only the dirty
    /// subset via `AccessTreeBuilder::dirty_tags`.
    pub initial: bool,
    /// Set of tags whose `AccessNode` body (name / value / state /
    /// bounds / children) changed since the previous emit. Empty
    /// when only focus changed. On `initial` the set contains every
    /// node's tag (the AT has no prior state).
    pub dirty: HashSet<String>,
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
    pub(crate) core: ShellCore<V>,
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

impl<V: WidgetView> ShellCore<V> {
    /// R51.76 §5.40 — construct the dispatch substrate with a
    /// freshly-built state scene and the initial cached state read
    /// through the §5.15 introspect channel.
    ///
    /// Identical bootstrapping to the pre-R51.76 `AppShell::new` minus
    /// the winit / wgpu / AccessKit surface (which lives on
    /// [`AppShell`] and is constructed lazily on `resumed`). Headless
    /// tests build only this struct.
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
        // R51.53 §5.39 — seed FocusManager with the binding's
        // `focusable_tags()` enumeration. The default impl returns
        // `vec![V::tag()]` (single tab stop), which is the right
        // shape for every single-widget example; composite widgets
        // (`RadioGroup`, multi-widget views) override to enumerate
        // sub-tags or sibling widget tags.
        let mut focus = FocusManager::new();
        let tags: Vec<String> = V::focusable_tags()
            .into_iter()
            .map(str::to_owned)
            .collect();
        focus.update_focusable_tags(tags);
        Self {
            scene,
            cached_state,
            intent_queue: IntentQueue::new(),
            previews: PreviewLedger::default(),
            revision: SceneRevision::default(),
            router: InputRouter::new(),
            focus,
            modifiers: ModifiersState::empty(),
            text_cache: LayoutCache::new(),
            last_paint_layout: None,
            last_access_tag_map: HashMap::new(),
            last_access_nodes: HashMap::new(),
            access_emit_initial: true,
            last_access_focus: None,
            redraw_requested: false,
        }
    }

    /// R51.76 §5.40 — borrow the focus manager. Tests inspect the
    /// focused tag through this accessor; production code accesses
    /// the field directly via `pub(crate)`.
    #[must_use]
    pub fn focus(&self) -> &FocusManager {
        &self.focus
    }
}

/// `ShellCore::new()` is the canonical constructor; the
/// `Default` impl exists so the substrate composes with any
/// future builder that defaults a member field via
/// [`Default::default`] (R51.76 — workspace lints set
/// `clippy::pedantic = "deny"`, which promotes
/// `clippy::new_without_default` to a hard build error; this
/// impl is mandatory to satisfy the lint without weakening
/// the baseline).
impl<V: WidgetView> Default for ShellCore<V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<V: WidgetView> ShellCore<V> {

    /// R51.76 §5.40 — borrow the cached state projection. Tests
    /// observe widget state transitions through this accessor.
    #[must_use]
    pub fn cached_state(&self) -> &V::State {
        &self.cached_state
    }

    /// R51.76 §5.40 — current §5.34 R40.4 OCC revision counter
    /// (loaded with `Acquire` ordering — see
    /// [`SceneRevision::current`]). Mutating winit / AT-side
    /// dispatches bump it; tests assert the before/after delta when
    /// verifying that a dispatch path actually committed.
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.revision.current()
    }

    /// R51.76 §5.40 — borrow the live state scene. Tests reach the
    /// widget External through `Scene::External(node) => node.handle`
    /// when verifying introspect side effects.
    #[must_use]
    pub fn scene(&self) -> &Scene {
        &self.scene
    }

    /// R51.76 §5.40 — drain the redraw flag set by `request_redraw`.
    ///
    /// Returns `true` once for each call to `request_redraw` between
    /// drains. [`AppShell`] calls this at the end of every event-loop
    /// iteration and forwards to `Window::request_redraw` on `true`;
    /// headless tests call it directly to verify that a dispatch
    /// triggered a repaint request without standing up a `Window`.
    pub fn take_redraw_request(&mut self) -> bool {
        let r = self.redraw_requested;
        self.redraw_requested = false;
        r
    }

    /// R51.76 §5.40 — `true` when a redraw has been requested since
    /// the last drain. Tests prefer [`take_redraw_request`](Self::take_redraw_request)
    /// when they want to consume the signal; this accessor is for
    /// debug logging and peek-only assertions.
    #[must_use]
    pub fn redraw_requested(&self) -> bool {
        self.redraw_requested
    }

    /// R51.76 §5.40 — note that a repaint is required.
    ///
    /// The flag is drained by [`AppShell`] once per event-loop
    /// iteration, so multiple `request_redraw` calls within one
    /// dispatch collapse to a single `Window::request_redraw` call
    /// (the textbook winit idiom: redraws are coalesced).
    pub fn request_redraw(&mut self) {
        self.redraw_requested = true;
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
                self.click_to_focus(pid);
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
    pub fn forward(&mut self, event: V::Event) {
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
    pub fn apply_key(&mut self, key: &str) {
        if V::apply_key(&mut self.scene, self.focus.focused(), key) {
            self.revision.bump();
            self.refresh_state();
            self.drain_intents();
        }
    }

    /// R51.78 §5.39 — Tab / Shift+Tab dispatch decoupled from winit.
    ///
    /// [`AppShell::handle_key_press`] (winit-side) maps
    /// `Key::Named(NamedKey::Tab) + modifiers.shift_key()` into a
    /// boolean `shift` flag and forwards here. The substrate then
    /// invokes [`FocusManager::focus_next`] / [`FocusManager::focus_prev`]
    /// against the seeded `focusable_tags` order and requests a
    /// redraw when the focused tag actually changed (avoiding
    /// no-op repaints when Tab cycles back to a one-tag list).
    ///
    /// Returns the underlying `FocusManager` change flag for
    /// callers / tests that want to assert on the cycle behaviour.
    pub fn handle_focus_traverse(&mut self, shift: bool) -> bool {
        let changed = if shift {
            self.focus.focus_prev()
        } else {
            self.focus.focus_next()
        };
        if changed {
            self.request_redraw();
        }
        changed
    }

    /// R51.78 §5.37 — `Key::Character` dispatch decoupled from winit.
    ///
    /// First consults [`WidgetView::keybinding`]; on `Some(event)`
    /// routes through [`Self::forward`] (typed event channel). On
    /// `None` falls through to [`Self::apply_key`] (raw key-string
    /// dispatch). Matches the pre-R51.78 inline behaviour in
    /// `AppShell::handle_key_press` byte-for-byte.
    pub fn handle_character_key(&mut self, c: &str) {
        if let Some(ev) = V::keybinding(c) {
            self.forward(ev);
        } else {
            self.apply_key(c);
        }
    }

    /// R51.78 §5.37 — `Key::Named` dispatch decoupled from winit.
    ///
    /// `AppShell::handle_key_press` (winit-side) maps the winit
    /// `NamedKey` enum to the W3C `KeyboardEvent.key` string via
    /// [`named_key_str`] and forwards the resulting `&'static str`
    /// here. The substrate routes through [`Self::apply_key`]; widgets
    /// match on the W3C string in their `apply_key` impls.
    ///
    /// `Escape` and `Tab` never reach this method — they are
    /// shell-reserved in `AppShell::handle_key_press` (`Escape` quits
    /// the window via `event_loop.exit`; `Tab` routes through
    /// [`Self::handle_focus_traverse`]).
    pub fn handle_named_key(&mut self, key_str: &str) {
        self.apply_key(key_str);
    }

    /// R51.80 §5.35 — winit `CursorMoved` dispatch decoupled from
    /// winit at the [`ShellCore`] surface. Forwards through the
    /// [`InputRouter`] (which routes to the matching
    /// `Scene::External` via tag hit-test), then refreshes cached
    /// state + drains intents.
    pub fn cursor_moved(&mut self, pid: PointerId, x: f64, y: f64) {
        self.router.cursor_moved(pid, x, y, &mut self.scene);
        self.refresh_state();
        self.drain_intents();
    }

    /// R51.80 §5.35 — winit `CursorLeft` dispatch decoupled from
    /// winit at the [`ShellCore`] surface.
    pub fn cursor_left(&mut self, pid: PointerId) {
        self.router.cursor_left(pid, &mut self.scene);
        self.refresh_state();
        self.drain_intents();
    }

    /// R51.80 §5.35 — winit `MouseInput { Pressed, Left }` dispatch.
    /// Combines `InputRouter::pointer_down` with the §5.39
    /// click-to-focus rule (the same path
    /// `TouchPhase::Started` runs after a synthetic cursor move).
    pub fn mouse_pressed(&mut self, pid: PointerId) {
        self.router.pointer_down(pid, &mut self.scene);
        self.click_to_focus(pid);
        self.refresh_state();
        self.drain_intents();
    }

    /// R51.80 §5.35 — winit `MouseInput { Released, Left }` dispatch.
    pub fn mouse_released(&mut self, pid: PointerId) {
        self.router.pointer_up(pid, &mut self.scene);
        self.refresh_state();
        self.drain_intents();
    }

    /// R51.80 §5.35 — winit `WindowEvent::Touch` dispatch. Delegates
    /// to the multi-pointer [`Self::handle_touch`] (R51.45 §5.35)
    /// then refreshes cached state + drains intents.
    pub fn touch_event(&mut self, touch: Touch) {
        self.handle_touch(touch);
        self.refresh_state();
        self.drain_intents();
    }

    /// R51.80 §5.39 — winit `WindowEvent::ModifiersChanged` cache.
    /// `KeyEvent` carries no modifier state in winit; the substrate
    /// remembers the most-recent `ModifiersChanged` so the
    /// [`AppShell::handle_key_press`] Tab arm can branch on Shift.
    pub fn set_modifiers(&mut self, modifiers: ModifiersState) {
        self.modifiers = modifiers;
    }

    /// R51.80 §5.39 / R51.59 — winit `WindowEvent::Focused(true)`
    /// dispatch. ARIA Focus Order asks the framework to reinstate the
    /// previously-focused widget when the window regains focus (the
    /// [`FocusManager`] owns the snapshot). Sets `redraw_requested` when
    /// `restore` reports a change so the focus ring repaints.
    pub fn window_focused(&mut self) {
        if self.focus.restore() {
            self.request_redraw();
        }
    }

    /// R51.80 §5.39 / R51.59 — winit `WindowEvent::Focused(false)`
    /// dispatch. Saves the currently-focused widget tag so a future
    /// [`Self::window_focused`] can restore it.
    pub fn window_blurred(&mut self) {
        self.focus.save();
    }

    /// R51.80 §5.16 §5.36 — compute one frame's paint scene from the
    /// cached state.
    ///
    /// Encapsulates `Frame::new` + `V::view(state, &frame)` +
    /// `compute_layout(&mut scene, &mut text_cache, w, h)` so
    /// [`AppShell::render`] does not have to reach into
    /// `self.core.cached_state` and `self.core.text_cache` directly.
    /// Pure with respect to substrate state (only `text_cache`
    /// mutates internally, by design — the LRU records each freshly
    /// shaped text run for the next frame's cache hit).
    pub fn compute_paint_scene(&mut self, w: u32, h: u32) -> Scene {
        let frame = Frame::new();
        let mut paint_scene = V::view(self.cached_state, &frame);
        compute_layout(&mut paint_scene, &mut self.text_cache, w, h);
        paint_scene
    }

    /// R51.80 §5.40 — build the inputs to
    /// [`Self::plan_access_emit`] from a freshly-computed paint
    /// scene.
    ///
    /// Runs the pipeline `V::access_node` → `enrich_names_from_scene`
    /// → `rect_for_tag` → `V::access_focus_target` in one place so
    /// [`AppShell::render`] does not have to reach into
    /// `self.core.cached_state` / `self.core.focus` four times in a
    /// row. The pure paint scene + the substrate's read-only state
    /// (focus + `cached_state`) are the only inputs; nothing on
    /// `ShellCore` mutates.
    #[must_use]
    pub fn collect_access_emit_inputs(
        &self,
        paint_scene: &Scene,
    ) -> (Vec<AccessNode>, Option<pinion_a11y::AccessFocus>) {
        let focused = self.focus.focused().map(str::to_owned);
        let mut nodes =
            V::access_node(&self.cached_state, focused.as_deref());
        pinion_a11y::enrich_names_from_scene(&mut nodes, paint_scene);
        for node in &mut nodes {
            if let Some(rect) = rect_for_tag(paint_scene, &node.tag) {
                node.bounds = Some(rect);
            }
        }
        let at_focus = V::access_focus_target(
            &self.cached_state,
            focused.as_deref(),
        );
        (nodes, at_focus)
    }

    /// R51.80 §5.12 §5.35 — post-render bookkeeping.
    ///
    /// Snapshots the just-rendered paint scene into the §5.12
    /// `last_paint_layout` so an AI client's
    /// `scene/layout {viewport: null}` reaches the actual frame; hands
    /// the same scene to the [`InputRouter`] so the next pointer
    /// event hit-tests against current geometry; refreshes cached
    /// state and drains pending intents (winit input bypasses the
    /// dispatcher, so the substrate has to close the loop here).
    pub fn finalize_frame(&mut self, paint_scene: Scene) {
        self.last_paint_layout = Some(build_layout_node(&paint_scene, "/0"));
        self.router.update_paint_scene(paint_scene, &mut self.scene);
        self.refresh_state();
        self.drain_intents();
    }

    /// R51.53 §5.39 — click → focus auto-set / background → clear.
    /// Called after every `pointer_down` (mouse Left press or touch
    /// `TouchPhase::Started`). Mirrors the W3C HTML convention:
    /// pressing on a tagged focusable widget focuses it; pressing
    /// on background blurs the focused widget. Non-focusable tagged
    /// widgets (decoration regions that respond to hover but aren't
    /// `focusable_tags()` members) leave focus unchanged — the
    /// [`FocusManager::focus_set`] guard rejects unknown tags so
    /// the no-op falls out naturally.
    fn click_to_focus(&mut self, pid: PointerId) {
        if let Some(target) = self.router.hover_target(pid).map(str::to_owned) {
            if !self.focus.focus_set(&target) {
                // Tagged but non-focusable (decoration) — leave focus
                // unchanged. The W3C HTML convention says only
                // focusable elements receive focus on mousedown.
            }
        } else {
            self.focus.focus_clear();
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
    /// Dispatch one JSON-RPC frame against the LIVE state scene.
    /// `scene/invoke /external/send PointerEnter` (and friends) drive
    /// the SCXML the same way a winit click would.
    ///
    /// R47.7.2 §5.12 — `scene/layout` requests reach the framework
    /// via `DispatchContext::with_paint_producer`: the closure captures
    /// `cached_state` (`Copy`) and `text_cache` (`&mut`), runs the
    /// view fn and `compute_layout` for the hypothetical viewport,
    /// and returns the freshly-measured paint scene.
    ///
    /// R51.76 §5.40 — `resize_request` is supplied by the caller so
    /// the substrate stays winit-free. [`AppShell`] constructs the
    /// production closure (calls `Window::request_inner_size` +
    /// `Window::request_redraw`); headless tests pass a no-op.
    ///
    /// Returns the optional JSON-RPC 2.0 response frame; the caller
    /// owns the IO surface (production writes to stdout; tests
    /// inspect the string).
    ///
    /// Signature note — `&mut dyn FnMut(u32, u32)` (not generic
    /// `F: FnMut`) for two reasons: (a) the downstream
    /// [`DispatchContext::with_resize_request`] takes the same
    /// `&mut (dyn FnMut + 'a)` shape, so the substrate forwards the
    /// reference straight through without re-wrapping; (b) avoids
    /// per-callsite monomorphisation of the entire dispatch body
    /// (production callsite vs test no-op closure would otherwise
    /// duplicate ~1 KiB of code).
    pub fn dispatch_rpc(
        &mut self,
        request: &str,
        resize_request: &mut dyn FnMut(u32, u32),
    ) -> Option<String> {
        // R51.73 §5.40 — sample focus before dispatch so we can
        // detect `focus/set` (or any other focus-mutating method)
        // and trigger a redraw to refresh the focus ring.
        let focus_before = self.focus.focused().map(str::to_owned);
        let resp = {
            // Disjoint-field split mutable borrows so the producer
            // closure can capture `cached_state` + `text_cache` while
            // the dispatcher still gets `scene` + `previews` + `revision`.
            let scene_ptr = &mut self.scene;
            let previews = &self.previews;
            let revision = &self.revision;
            let focus_ptr = &mut self.focus;
            let cached_state = self.cached_state;
            let text_cache_ptr = &mut self.text_cache;
            let last_paint = self.last_paint_layout.as_ref();
            let mut produce = |w: u32, h: u32| -> Scene {
                let frame = Frame::new();
                let mut paint = V::view(cached_state, &frame);
                compute_layout(&mut paint, text_cache_ptr, w, h);
                paint
            };
            // R47.7.5 §5.12 — surface the most recent winit-rendered
            // frame to the dispatcher so `scene/layout {viewport: null}`
            // returns the actual frame snapshot. Builder pattern keeps
            // the `Option` wiring branchless at the AI-client level.
            let mut ctx = DispatchContext::new(scene_ptr, previews, revision)
                .with_paint_producer(&mut produce)
                .with_resize_request(resize_request)
                .with_focus_manager(focus_ptr);
            if let Some(snapshot) = last_paint {
                ctx = ctx.with_last_paint_layout(snapshot);
            }
            dispatch(&mut ctx, request)
        };
        self.refresh_state();
        self.drain_intents();
        // R51.73 §5.40 — `focus/set` from the AI client must trigger
        // a redraw so the focus ring repaints on the new target. The
        // before/after comparison catches every focus-mutating
        // method without enumerating method names.
        if self.focus.focused().map(str::to_owned) != focus_before {
            self.request_redraw();
        }
        resp
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

    /// R51.77 §5.40 — pure planning step for the §5.40 AccessKit
    /// emit. Borrows the freshly-computed nodes + focus, consults
    /// the substrate's incremental caches, and returns the emit
    /// verdict + dirty-tag diff. **Does not mutate any
    /// `ShellCore` state** — the caller invokes
    /// [`Self::commit_access_emit`] after the `Adapter::update_if_active`
    /// closure has consumed the nodes, completing the cache update
    /// in a separate step.
    ///
    /// Two-step rationale (R51.77 split): pre-R51.77
    /// `compute_access_emit` bundled the decision AND the cache
    /// update into one `&mut self` call named like a pure function.
    /// Reading the name without reading the body suggested
    /// idempotence; two back-to-back calls actually yielded different
    /// answers (the second saw the first's cache update). The
    /// `plan_access_emit` / `commit_access_emit` pair makes the
    /// state-machine step explicit:
    ///
    /// 1. `plan_access_emit(&nodes, focus.as_ref())` — pure decision.
    /// 2. If `decision.should_emit`, feed `nodes` + `focus` into the
    ///    closure passed to `Adapter::update_if_active`.
    /// 3. `commit_access_emit(&nodes, focus.as_ref())` — advances
    ///    the cache so the next plan sees the post-emit baseline.
    ///
    /// Tests exercise the pure planner via two back-to-back
    /// `plan_access_emit` calls separated by a `commit_access_emit`
    /// without any AccessKit adapter on hand (R51.75 no-change
    /// verification path).
    #[must_use]
    pub fn plan_access_emit(
        &self,
        nodes: &[AccessNode],
        focus: Option<&pinion_a11y::AccessFocus>,
    ) -> AccessEmitDecision {
        // R51.72 §5.40 — diff against the previous frame's node
        // cache. The initial frame emits every tag (the AT has no
        // prior state); subsequent frames emit only tags whose
        // `AccessNode` body (name / value / state / bounds / children)
        // actually changed.
        let initial = self.access_emit_initial;
        let dirty: HashSet<String> = if initial {
            nodes.iter().map(|n| n.tag.clone()).collect()
        } else {
            nodes
                .iter()
                .filter(|n| self.last_access_nodes.get(&n.tag) != Some(*n))
                .map(|n| n.tag.clone())
                .collect()
        };
        // R51.75 §5.40 — no-change frame skip. Emit only when the
        // initial-frame flag is set, the dirty set is non-empty, or
        // the focus declaration shifted. Otherwise the TreeUpdate
        // would be a pure no-op (root re-emit + identical focus).
        let focus_changed = focus != self.last_access_focus.as_ref();
        let should_emit = initial || !dirty.is_empty() || focus_changed;
        AccessEmitDecision {
            should_emit,
            initial,
            dirty,
        }
    }

    /// R51.77 §5.40 — commit step paired with
    /// [`Self::plan_access_emit`]. Advances the substrate's
    /// incremental caches to the just-emitted baseline so the next
    /// planning call diffs against this frame.
    ///
    /// Always run after the `Adapter::update_if_active` closure has
    /// consumed (or borrowed) the nodes — even when
    /// `decision.should_emit` is `false`, calling `commit_access_emit`
    /// is safe (it idempotently rewrites the cache to the same
    /// values). The textbook canonical idiom is "plan, optionally
    /// emit, always commit".
    ///
    /// R51.79 §5.40 — signature takes `nodes: Vec<AccessNode>`
    /// by-value so the Vec moves straight into `last_access_nodes`
    /// without a per-node clone. Pre-R51.79 took `&[AccessNode]` and
    /// did `nodes.iter().cloned()` internally, doubling the per-frame
    /// allocation budget (one clone for the emit closure, one clone
    /// for the cache). The new shape pairs with
    /// [`AccessTreeBuilder::add`] taking `&AccessNode` — the emit
    /// closure borrows from `nodes`, then `commit_access_emit`
    /// consumes by-value: one clone per node, in the builder only.
    ///
    /// Update set: `last_access_tag_map` (`NodeId` → tag for AT-side
    /// action routing), `last_access_nodes` (per-tag snapshot for the
    /// next dirty diff — moved in by-value), `last_access_focus`
    /// (for the next focus-change detection), `access_emit_initial`
    /// (set to `false` after the first commit so the next plan emits
    /// incrementally).
    pub fn commit_access_emit(
        &mut self,
        nodes: Vec<AccessNode>,
        focus: Option<&pinion_a11y::AccessFocus>,
    ) {
        // R51.67 §5.40 — refresh the NodeId → tag map. Borrow before
        // the by-value move below.
        self.last_access_tag_map = build_tag_map(&nodes);
        // R51.79 §5.40 — move the Vec straight into the per-tag
        // HashMap. `tag.clone()` lifts only the key (a String) out;
        // each `AccessNode` itself moves without an extra clone.
        self.last_access_nodes = nodes
            .into_iter()
            .map(|n| (n.tag.clone(), n))
            .collect();
        // Refresh the focus snapshot for the next frame's
        // focus-change check.
        self.last_access_focus = focus.cloned();
        self.access_emit_initial = false;
    }

    /// R51.67 §5.40 — translate an AccessKit `ActionRequest` into a
    /// pinion-native widget intent and dispatch it through the same
    /// focus / `apply_key` substrate the winit keyboard path uses.
    /// Returns silently when the request targets the synthetic root
    /// window or an unknown `NodeId` (stale tree, AT race).
    pub fn handle_action_request(&mut self, req: &accesskit::ActionRequest) {
        let Some(action) = translate_action(req, &self.last_access_tag_map) else {
            return;
        };
        self.dispatch_access_action(&action);
    }

    /// R51.67 §5.40 — pinion-native dispatch for one AT-driven
    /// widget action.
    ///
    /// Mapping (atomic widgets):
    /// - `Focus`          → [`FocusManager::focus_set`] + redraw
    /// - `Click` / `Default` → focus + `apply_key("Enter")`
    /// - `Increment`      → focus + `apply_key("ArrowRight")`
    /// - `Decrement`      → focus + `apply_key("ArrowLeft")`
    /// - `Other`          → silent drop
    ///
    /// R51.70 §5.40 — composite child tags (containing `#`) focus
    /// the parent and route the action through
    /// [`WidgetView::access_child_invoke`] before falling back to
    /// the atomic chain. The composite parses the sub-tag (the
    /// segment after `#`) and dispatches through its own wire-format
    /// invocation path; the shell stays composite-agnostic.
    pub fn dispatch_access_action(&mut self, action: &PinionAccessAction) {
        let (parent_tag, sub_tag) = match action.tag.split_once('#') {
            Some((p, s)) => (p, Some(s)),
            None => (action.tag.as_str(), None),
        };
        match action.kind {
            AccessAction::Focus => {
                self.focus.focus_set(parent_tag);
                self.request_redraw();
            }
            AccessAction::Click | AccessAction::Default => {
                self.focus.focus_set(parent_tag);
                if let Some(sub) = sub_tag {
                    // R51.70 §5.40 — composite child dispatch hook.
                    // The composite invokes its wire format and
                    // returns `true`; we commit the same revision /
                    // refresh / drain bookkeeping `apply_a11y_key`
                    // performs so AT-driven activation matches the
                    // keyboard path 1:1.
                    if V::access_child_invoke(&mut self.scene, sub, action.kind) {
                        self.revision.bump();
                        self.refresh_state();
                        self.drain_intents();
                        self.request_redraw();
                        return;
                    }
                    // Composite declined (unrecognised sub-tag /
                    // unsupported action) — fall through so the AT
                    // still sees activation feedback via the parent.
                }
                self.apply_a11y_key(parent_tag, "Enter");
            }
            AccessAction::Increment => self.apply_a11y_key(parent_tag, "ArrowRight"),
            AccessAction::Decrement => self.apply_a11y_key(parent_tag, "ArrowLeft"),
            AccessAction::Other => {}
        }
    }

    /// R51.67 §5.40 — focus + `apply_key` shared by `Click`,
    /// `Increment`, and `Decrement` arms. Mirrors the winit
    /// keyboard-path bookkeeping ([`Self::apply_key`]): bump the
    /// §5.34 OCC revision, re-read cached state, drain pending
    /// intents on handled, request a redraw regardless so the
    /// AT-side activation surfaces visually.
    fn apply_a11y_key(&mut self, tag: &str, key: &str) {
        self.focus.focus_set(tag);
        if V::apply_key(&mut self.scene, Some(tag), key) {
            self.revision.bump();
            self.refresh_state();
            self.drain_intents();
        }
        self.request_redraw();
    }

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
            &mut self.core.text_cache,
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
                        builder = builder.initial(false);
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
                self.core.handle_focus_traverse(self.core.modifiers.shift_key());
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
                    V::fmt_state_log(&self.core.cached_state),
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

/// R51.67 §5.40 — build the `NodeId` → widget tag map for a
/// freshly-collected list of `AccessNode`s.
///
/// Includes the synthetic root entry (`ROOT_NODE_ID` → `""`) so
/// `pinion_a11y::translate_action` can treat a root-targeted action
/// request as a sentinel and drop it without crossing into widget
/// dispatch.
fn build_tag_map(nodes: &[AccessNode]) -> HashMap<NodeId, String> {
    let mut map = HashMap::with_capacity(nodes.len() + 1);
    map.insert(ROOT_NODE_ID, String::new());
    for node in nodes {
        map.insert(tag_to_node_id(&node.tag), node.tag.clone());
    }
    map
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
