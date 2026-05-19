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

use pinion_a11y::{AccessAction, AccessFocus, AccessNode};
use pinion_core::external::External;
use pinion_core::{Frame, Scene};
use vello::peniko::Color as PenikoColor;
use vello::Scene as VelloScene;
use winit::window::Window;

mod app;
mod substrate;

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
