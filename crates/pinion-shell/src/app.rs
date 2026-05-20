//! R51.92.1 §5.40 — surface (winit / wgpu / `accesskit_winit`) module
//! split from `lib.rs`.
//!
//! Houses the framework-side application surface ([`AppShell`]) and
//! its `winit::application::ApplicationHandler` implementation, plus
//! the two surface-only helpers (`named_key_str` for the winit↔W3C
//! `KeyboardEvent.key` bridge and `spawn_stdin_rpc_reader` for the
//! background JSON-RPC reader thread) and the public [`run`]
//! entry-point every visual binary's `fn main()` collapses to.
//!
//! R51.92 (substrate.rs) extracted [`crate::ShellCore`] so the
//! dispatch substrate is module-private (14 fields + the
//! [`crate::AccessEmitDecision`] body genuinely private to
//! `substrate.rs`). R51.92.1 completes the textbook 3-module split:
//! every `AppShell` field — including the `core: ShellCore<V>`
//! borrow — is private to `app.rs`, so the surface boundary is now
//! enforced at the module level as well. Any future addition to
//! `lib.rs` cannot accidentally reach across the boundary to touch
//! either the substrate's fields or `AppShell`'s winit surface
//! state.
//!
//! See `claim-accuracy-self-audit` and `substrate-incompleteness-signal`.

use std::io::{BufRead, Write};
use std::sync::Arc;
use std::thread;

use pinion_a11y::AccessTreeBuilder;
use pinion_core::scene::BoxNode;
use pinion_runtime::{paint_adapter, CommandExecutor, HandlerRegistry, PointerId};
use vello::Scene as VelloScene;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

use crate::executor::build_executor_and_sink;
use crate::substrate::ShellCore;
use crate::{AppEvent, RenderState, VelloContext, VelloRenderer, WidgetRenderer, WidgetView};

/// The framework-side shell. Generic over a widget binding
/// [`WidgetView`]; concrete examples instantiate via `run::<V>()`.
///
/// R51.76 §5.40 — every piece of testable dispatch state lives in
/// [`ShellCore`]; this struct only owns the winit / wgpu / AccessKit
/// surface so headless tests can target [`ShellCore`] directly.
///
/// R51.92.1 §5.40 — moved from `lib.rs` to its own module so every
/// field below (including the `core: ShellCore<V>` substrate borrow
/// added in R51.76) is genuinely private to `app.rs`. The lib.rs
/// entry point reaches the shell only through [`run`] / the
/// re-exported `AppShell::new` constructor; private state is now
/// module-bound rather than file-bound.
pub struct AppShell<V: WidgetView> {
    /// R51.76 §5.40 — extracted dispatch substrate (scene, cached
    /// state, focus, intents, previews, revision, router, modifiers,
    /// text cache, last paint snapshot, AT caches, redraw flag).
    ///
    /// R51.83 §5.40 — private. All surface-side access happens
    /// through the substrate's typed methods + accessors so the
    /// boundary stays one-way.
    ///
    /// R51.92.1 §5.40 — module-private. Even `lib.rs` (the entry
    /// crate root) cannot touch this field directly.
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
        // R51.109.1 §5.41 — call through the backend-agnostic
        // `WidgetRenderer` trait. `VelloContext::base_color` carries
        // the window background sampled from
        // `paint_adapter::root_background`; the renderer's macro impl
        // forwards to the inherent `<R>::render(frame, base_color)`.
        // `renderer.render` auto-derefs through `Box<R>` because the
        // `WidgetRenderer` trait is in scope.
        if let Err(e) = renderer.render(&self.vello_scene, VelloContext { base_color: base }) {
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
            // R51.108 §5.41 — convert at the winit boundary so the
            // substrate sees only the abstract `pinion_runtime::Touch`.
            WindowEvent::Touch(touch) => {
                self.core.touch_event(winit_touch_to_pinion(touch));
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                // R51.53 §5.39 — winit emits `KeyEvent` without
                // modifier state, so cache the most-recent value
                // out-of-band for Shift+Tab detection.
                // R51.108 §5.41 — convert at the winit boundary.
                self.core.set_modifiers(winit_modifiers_to_pinion(modifiers.state()));
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
            // R51.159 §5.23 — re-feed an Intent produced by a resolved
            // Command future back into the SCXML `send` channel via
            // `ShellCore::dispatch_intent`. The closing step of the
            // §5.23 R27 dispatch loop.
            AppEvent::IntentArrived(intent) => self.core.dispatch_intent(&intent),
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
///
/// R51.92.1 §5.40 — module-local helper (sole caller is
/// [`AppShell::handle_key_press`] above).
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
///
/// R51.92.1 §5.40 — module-local helper (sole caller is [`run`]
/// below).
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

/// R51.108 §5.41 — convert a `winit::event::Touch` to the
/// substrate-local [`pinion_runtime::Touch`] at the winit boundary so
/// `ShellCore` stays winit-free for the §2 #6 GUI/TUI dual invariant.
/// `winit::event::Touch::id` already matches the abstract `id: u64`;
/// `location: PhysicalPosition<f64>` decomposes to `(x, y)`; the
/// four-variant `TouchPhase` enum maps 1:1.
fn winit_touch_to_pinion(touch: winit::event::Touch) -> pinion_runtime::Touch {
    pinion_runtime::Touch {
        id: touch.id,
        x: touch.location.x,
        y: touch.location.y,
        phase: match touch.phase {
            winit::event::TouchPhase::Started => pinion_runtime::TouchPhase::Started,
            winit::event::TouchPhase::Moved => pinion_runtime::TouchPhase::Moved,
            winit::event::TouchPhase::Ended => pinion_runtime::TouchPhase::Ended,
            winit::event::TouchPhase::Cancelled => pinion_runtime::TouchPhase::Cancelled,
        },
    }
}

/// R51.108 §5.41 — convert a `winit::keyboard::ModifiersState` to the
/// substrate-local [`pinion_runtime::Modifiers`] at the winit boundary.
/// The four W3C DOM Level 3 modifier bits map 1:1 (winit's `super_key`
/// is the Meta / Cmd / Win key in the abstract vocabulary).
fn winit_modifiers_to_pinion(
    modifiers: winit::keyboard::ModifiersState,
) -> pinion_runtime::Modifiers {
    pinion_runtime::Modifiers {
        shift: modifiers.shift_key(),
        ctrl: modifiers.control_key(),
        alt: modifiers.alt_key(),
        meta: modifiers.super_key(),
    }
}

/// Run the visual binary end-to-end: build the winit event loop with
/// the [`AppEvent`] user-event slot, spawn the stdin RPC reader, run
/// the [`AppShell<V>`] until quit. The single line every shell
/// consumer needs in `fn main()`.
///
/// R51.159 §5.23 — no [`CommandExecutor`] is installed by this entry
/// point; pending [`pinion_core::Command`] queues stay parked on the
/// owner side and never fire. Use [`run_with_handlers`] to register
/// async [`Handler`](pinion_runtime::Handler)s and bind a tokio
/// runtime + intent-arrival event channel.
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

/// R51.159 §5.23 — variant of [`run`] that installs a
/// [`CommandExecutor`](pinion_runtime::CommandExecutor) at boot so
/// pending [`pinion_core::Command`]s queued by reducer fallout or
/// SCXML / Update steps reach their registered
/// [`Handler`](pinion_runtime::Handler)s asynchronously.
///
/// Composes:
///
/// - A tokio multi-thread [`TokioExecutor`](crate::TokioExecutor) (1
///   worker thread, `enable_all`) backing
///   [`Executor::spawn`](pinion_runtime::Executor).
/// - A [`ProxyIntentSink`](crate::ProxyIntentSink) wrapping the
///   winit [`EventLoopProxy`] so resolved [`pinion_core::Intent`]s
///   arrive on the UI thread through
///   [`AppEvent::IntentArrived`] for re-feed.
/// - The supplied `registry` of [`Handler`](pinion_runtime::Handler)
///   impls keyed by [`pinion_core::Command::kind_str`].
///
/// # Panics
/// Panics if the winit event loop cannot be built (same condition as
/// [`run`]) or if the tokio runtime cannot spin up its worker
/// thread (the OS-level thread-spawn failure that
/// [`TokioExecutor::new`](crate::TokioExecutor) wraps).
pub fn run_with_handlers<V: WidgetView>(registry: HandlerRegistry) {
    let event_loop = EventLoop::<AppEvent>::with_user_event()
        .build()
        .expect("winit EventLoop::with_user_event failed");
    event_loop.set_control_flow(ControlFlow::Wait);
    spawn_stdin_rpc_reader(event_loop.create_proxy());

    // R51.159 §5.23 — assemble the CommandExecutor and inject it
    // before the event loop starts so the first dispatch tail can
    // already drain pending commands.
    let (executor, sink) = build_executor_and_sink(event_loop.create_proxy())
        .expect("tokio runtime build failed");
    let cmd_exec = Arc::new(CommandExecutor::new(registry, executor, sink));

    let mut app = AppShell::<V>::new(event_loop.create_proxy());
    let _prior = app.core.set_command_executor(cmd_exec);

    if let Err(e) = event_loop.run_app(&mut app) {
        eprintln!("shell: event loop error: {e}");
    }
}
