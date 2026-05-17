//! `hello-button` — §4 first dogfood + live §2 bidirectional RPC.
//!
//! R46.5 §5.16: Vello render path. Replaces the R21 softbuffer / cosmic-text
//! paint pipeline; the GPU scene is built via the `paint_adapter` framework
//! primitive (R46.3.1) and submitted through a pinion-forge-emitted
//! `HelloButtonRenderer` (app.pinion.xml). R47.3 §5.36 — text labels
//! render through `paint_adapter`'s Text arm (parley-shaped glyph runs +
//! `vello::Scene::draw_glyphs`); the `LayoutCache` owned by `App` keeps
//! the shaping cost amortized across frames.
//!
//! Architecture (R17 bidirectional RPC spec round, live dogfood):
//!
//!   * The app owns the **state scene**:
//!     `Scene::External(Box<ButtonExternal>)`. The live R12 `Button`
//!     SCXML statechart is reachable via the §5.15 introspect
//!     surface — there is no other copy of the state.
//!   * **Input flows through a single channel**: both winit pointer
//!     events and JSON-RPC frames hit `ExternalIntrospect::invoke
//!     ("send", Text(<event name>))`. winit translates `WindowEvent`
//!     to a `ButtonEvent` variant name; `pinion_rpc::dispatch` routes
//!     `scene/invoke` to the same method. The §2 invariant #2 ("RPC
//!     headless as AI primary path") is *literal* — the AI uses the
//!     same channel a human would.
//!   * **Output flows through the §5.20 intent channel** (R18 live
//!     dogfood): after every winit event or RPC dispatch, the app
//!     walks the scene through `pinion_runtime::walk_scene_and_drain`,
//!     pulls any pending `Intent` (e.g. `button.click` on `Pressed` →
//!     `Hover` via `PointerUp`), and logs them to stderr. The same
//!     intents stay reachable through the `scene/intents` RPC method
//!     so AI agents see the same emission stream a human observer
//!     would.
//!   * The **paint scene** is separate: `view(state, &Frame) -> Scene`
//!     (§6.3 pure sync) builds a `Scene::Container` (bg + centered
//!     button + text label) from the current `ButtonState` each frame.
//!     `paint_adapter::to_vello` walks that tree into a `vello::Scene`.
//!     Model/view split — state scene is authoritative, paint scene
//!     is a derived view.
//!   * A background thread reads JSON-RPC 2.0 lines from `stdin`
//!     and forwards each as a winit `UserEvent`; the main thread
//!     handles it on the UI thread, then refreshes the cached state
//!     and requests a redraw if the RPC mutated anything.

use std::io::{BufRead, Write};
use std::sync::Arc;
use std::thread;

use pinion_core::external::IntrospectValue;
use pinion_core::scene::{BoxNode, ContainerNode, ExternalNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::widgets::button::{ButtonEvent, ButtonExternal, ButtonState};
use pinion_core::{Color, Frame, Scene};
use pinion_core::SceneRevision;
use pinion_rpc::{dispatch, DispatchContext, PreviewLedger};
use pinion_runtime::{compute_layout, paint_adapter, walk_scene_and_drain, InputRouter, IntentQueue};
use pinion_text::LayoutCache;
use vello::Scene as VelloScene;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::{Window, WindowId};

// pinion-forge codegen output. Defines `pub struct HelloButtonRenderer`
// + `pub enum HelloButtonRendererError` + async `new<W: Into<wgpu::SurfaceTarget<'static>>>`
// + sync `render(&vello::Scene, peniko::Color)` + sync `resize(u32, u32)`.
// R46.3.3 emit template uses fully-qualified `::vello::*` paths so the
// include is bare (no `mod` wrap).
include!(concat!(env!("OUT_DIR"), "/app.rs"));

/// Winit user-event variants — today only the stdin-fed RPC line.
#[derive(Debug, Clone)]
enum AppEvent {
    /// One JSON-RPC 2.0 frame read from stdin, awaiting dispatch.
    RpcRequest(String),
}

const WIN_W: u32 = 320;
const WIN_H: u32 = 200;
// R46.5: opaque RGB triples replace the pre-R46.5 `0x00RRGGBB` literals.
// `Color::from_argb(0x00...)` decodes to alpha = 0 = fully transparent
// on the Vello path (see paint_adapter::to_peniko_alpha_zero test).
const BG_FILL: Color = Color::rgb(0x20, 0x30, 0x40); // dark navy
const BTN_W: u32 = 160;
const BTN_H: u32 = 80;

/// view-fn (§6.3): pure sync mapping `ButtonState` → `Scene`. The
/// `&Frame` slot is the §6.3 ZST hedge — zero-cost today, readied
/// for `dt`/`frame_index` without a `SemVer` major. Purity here is
/// the §2 `dry_run` invariant: same `(state, frame)` always yields
/// the same `Scene`.
///
/// R24 §5.21 migration: hardcoded `BTN_RECT` is gone. The root
/// container does flex centering; the button container has a fixed
/// `Size::px(160, 80)`. `compute_layout` (called from `render`)
/// resolves every node's pixel rect each frame.
///
/// R46.5 §5.16 — `paint_adapter::to_vello` walks the resulting tree.
/// R47.3 §5.36 — `Scene::Text` is now live: each label is parley-shaped
/// through `App::text_cache` (LRU 256 entries) and emitted as Vello
/// glyph runs. Static labels ("Click me!" / "Disabled") shape once on
/// first paint and hit the cache on every subsequent frame.
//
// `&Frame` is intentional per the §6.3 signature contract even
// though `Frame` is presently a ZST: once real per-frame fields
// land, passing by value would force a `SemVer` major on every
// view-fn. Allow the lint at the view-fn boundary.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: ButtonState, _frame: &Frame) -> Scene {
    let btn_fill: Color = match state {
        ButtonState::Idle => Color::rgb(0xff, 0xff, 0xff),     // white
        ButtonState::Hover => Color::rgb(0xd0, 0xd0, 0xd0),    // light grey
        ButtonState::Pressed => Color::rgb(0x50, 0x50, 0x50),  // dark grey
        ButtonState::Disabled => Color::rgb(0xb0, 0x20, 0x20), // muted red
    };
    let label = match state {
        ButtonState::Disabled => "Disabled",
        _ => "Click me!",
    };
    let label_text = Scene::Text(TextNode::styled(
        label,
        Rect::default(),
        TextStyle::new()
            .with_size_px(18)
            .with_fg(Color::rgb(0, 0, 0)),
    ));
    let button = Scene::Container(
        ContainerNode::new(vec![label_text])
            // R48 §5.35: framework dispatch identifier. The InputRouter
            // hit-tests the paint scene for this tag and routes pointer
            // events to the state scene's ExternalNode("main_btn") —
            // application code never sees the cursor coordinates.
            .with_tag("main_btn")
            .with_style(BoxStyle::filled(btn_fill))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(BTN_W, BTN_H)),
            ),
    );
    Scene::Container(
        ContainerNode::new(vec![button])
            .with_style(BoxStyle::filled(BG_FILL))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center),
            ),
    )
}

/// Read the current `ButtonState` from the live state scene by
/// going through the §5.15 introspect `state` slot — the same path
/// an RPC `scene/query /external/state` request uses. Returns
/// `Idle` defensively if the scene shape is unexpected (should not
/// happen with the current `App::new` setup).
fn read_state(scene: &Scene) -> ButtonState {
    if let Scene::External(node) = scene {
        if let Some(intro) = node.handle.introspect() {
            if let Some(IntrospectValue::Text(name)) = intro.query("state") {
                return parse_button_state(&name);
            }
        }
    }
    ButtonState::Idle
}

fn parse_button_state(name: &str) -> ButtonState {
    match name {
        "Hover" => ButtonState::Hover,
        "Pressed" => ButtonState::Pressed,
        "Disabled" => ButtonState::Disabled,
        // "Idle" + anything unexpected — defensive default.
        _ => ButtonState::Idle,
    }
}

/// Mirror of the `parse_button_event` table in `pinion-core` — the
/// winit handler side. Converts a typed `ButtonEvent` to the string
/// name the §5.15 `invoke("send", ...)` channel expects. The SCXML
/// emit may carry internal variants (`Null`, `ButtonActivate`,
/// future additions) that winit never produces; route those through
/// the wildcard with a sentinel name that the parser rejects.
fn button_event_name(event: ButtonEvent) -> &'static str {
    match event {
        ButtonEvent::PointerEnter => "PointerEnter",
        ButtonEvent::PointerLeave => "PointerLeave",
        ButtonEvent::PointerDown => "PointerDown",
        ButtonEvent::PointerUp => "PointerUp",
        ButtonEvent::Disable => "Disable",
        ButtonEvent::Enable => "Enable",
        _ => "__internal__",
    }
}

/// Window + renderer lifecycle (R46.3.4 §5.16). Mirrors the Vello 0.6
/// canonical `RenderState` enum (Linebender examples / Xilem) so the
/// app survives the Android / Wayland suspend → resume cycle where the
/// wgpu surface backing must be dropped and re-created. Desktop targets
/// fire `resumed` once at boot and never `suspended`; mobile targets
/// fire it on every focus change. `Suspended(Some(window))` caches the
/// winit `Window` across the drop-and-recreate cycle.
enum RenderState {
    Active {
        window: Arc<Window>,
        /// Boxed because `HelloButtonRenderer` is ~1.5 KiB (wgpu /
        /// vello state) while `Suspended` is two words; without the
        /// indirection the whole enum would pay the larger size
        /// (clippy `large_enum_variant`, R47.1.1 §5.36 MSRV 1.88 fix).
        renderer: Box<HelloButtonRenderer>,
    },
    Suspended(Option<Arc<Window>>),
}

struct App {
    /// Authoritative state scene — owns the live `ButtonExternal`
    /// via `Box<dyn External>`. Both winit input and RPC dispatch
    /// reach the SCXML statechart through this single scene.
    scene: Scene,
    /// Cached projection of the inner `ButtonState`, kept in sync
    /// by `refresh_state` after every input. Drives change-detection
    /// for the redraw request + the paint scene's fill mapping.
    cached_state: ButtonState,
    /// §5.20 intent harvest buffer. Refilled by `drain_intents` after
    /// every winit / RPC event; consumed by stderr logging plus any
    /// future on-frame app hook. The `scene/intents` RPC method
    /// drains the same source independently, since the underlying
    /// `External::pending_intents` is the single queue.
    intent_queue: IntentQueue,
    /// §5.34 preview lifecycle ledger — passed into every
    /// `pinion_rpc::dispatch` call alongside the scene. The lifecycle
    /// RPC methods (`scene/cancel_preview` today; more in R40.3+)
    /// read or mutate it through interior mutability; non-lifecycle
    /// methods ignore it.
    previews: PreviewLedger,
    /// §5.34 R40.4 OCC revision token. `dispatch` auto-bumps on
    /// mutating RPC methods; [`forward`](App::forward) explicitly
    /// bumps after the winit-side `invoke` since that path bypasses
    /// the dispatcher entirely.
    revision: SceneRevision,
    /// R48 §5.35 framework-side input dispatch primitive. Owns the
    /// retained paint scene + cursor state + `hover_target` and routes
    /// pointer events to the matching `ExternalNode` in `self.scene`.
    router: InputRouter,
    /// R46.5 §5.16 — suspend / resume lifecycle (R46.3.4 pattern).
    /// Replaces the pre-R46.5 softbuffer `window` + `surface` +
    /// `context` triple. Keeping all three in sync as separate
    /// `Option`s was error-prone and missed the mobile forward-compat
    /// requirement; the typed ADT collapses to a single match arm.
    state: RenderState,
    /// Reusable Vello scene buffer — reset (`scene.reset()`) at the
    /// start of each frame rather than reallocated. Vello's Scene API
    /// expects this pattern (Linebender Vello examples / Xilem).
    vello_scene: VelloScene,
    /// R47.3 §5.36 — owned [`LayoutCache`] (LRU 256). `paint_adapter`'s
    /// Text arm consults this cache for every `Scene::Text` it walks,
    /// so the button's static labels shape once on first paint and hit
    /// the cache on every subsequent frame. The cache also owns the
    /// parley `FontContext` / `LayoutContext` so the App never holds
    /// parley state directly.
    text_cache: LayoutCache,
}

impl App {
    fn new() -> Self {
        // R22 §5.20: the scene-side `ExternalNode.tag` supplies the
        // widget identifier used as the intent-tag prefix. The
        // ButtonExternal itself only emits the "click" kind; the
        // runtime walk composes `main_btn.click` on drain.
        let scene = Scene::External(
            ExternalNode::new(Box::new(ButtonExternal::new())).with_tag("main_btn"),
        );
        // Initial state is whatever the freshly-constructed Button is
        // at — read it via the same introspect channel everything else
        // uses, so there is exactly one source of truth.
        let cached_state = read_state(&scene);
        eprintln!("button: initial state = {cached_state:?}");
        Self {
            scene,
            cached_state,
            intent_queue: IntentQueue::new(),
            previews: PreviewLedger::default(),
            revision: SceneRevision::default(),
            router: InputRouter::new(),
            state: RenderState::Suspended(None),
            vello_scene: VelloScene::new(),
            text_cache: LayoutCache::new(),
        }
    }

    /// Translate a typed `ButtonEvent` (from a winit handler) into
    /// the symbolic `invoke("send", Text(<name>))` call — the same
    /// channel the RPC `scene/invoke` route uses. Failures from the
    /// statechart (`InvokeError::Rejected` etc.) are swallowed: the
    /// SCXML decides whether a given transition fires.
    fn forward(&mut self, event: ButtonEvent) {
        let name = button_event_name(event);
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

    /// Dispatch one JSON-RPC frame against the LIVE state scene.
    /// `scene/invoke /external/send PointerEnter` (and friends) now
    /// drive the SCXML the same way a winit click would.
    fn dispatch_rpc(&mut self, request: &str) {
        let mut ctx = DispatchContext::new(&mut self.scene, &self.previews, &self.revision);
        if let Some(resp) = dispatch(&mut ctx, request) {
            let mut out = std::io::stdout().lock();
            if writeln!(out, "{resp}").is_err() {
                // stdout closed (downstream consumer gone) — silently
                // skip; do not abort the GUI loop on a broken pipe.
            }
        }
        // The RPC frame may have mutated state — re-read, log the
        // delta, and trigger a redraw if the visual changed.
        self.refresh_state();
        self.drain_intents();
    }

    /// §5.20 live dogfood: walk the scene, drain any pending intents
    /// into the local queue, log each one to stderr. The `scene/intents`
    /// RPC method races with this drain — whichever caller harvests
    /// first wins (poll-form, single-consumer v0).
    fn drain_intents(&mut self) {
        walk_scene_and_drain(&mut self.scene, &mut self.intent_queue);
        for intent in self.intent_queue.drain() {
            eprintln!(
                "intent: {} payload={:?}",
                intent.tag_str(),
                intent.payload,
            );
        }
    }

    /// Re-read the cached `ButtonState` from the live scene; log
    /// and repaint if it changed since the previous refresh.
    fn refresh_state(&mut self) {
        let now = read_state(&self.scene);
        if now != self.cached_state {
            eprintln!("button: {:?} -> {:?}", self.cached_state, now);
            self.cached_state = now;
            self.request_redraw();
        }
    }

    fn request_redraw(&self) {
        if let RenderState::Active { window, .. } = &self.state {
            window.request_redraw();
        }
    }

    /// Build the paint scene for the current cached state, run layout,
    /// hand it to the framework-side `paint_adapter` walker, and submit
    /// the resulting `vello::Scene` to `HelloButtonRenderer`. Render is
    /// a no-op while the app is suspended (R46.3.4 lifecycle).
    fn render(&mut self) {
        let RenderState::Active { window, renderer } = &mut self.state else {
            return;
        };
        let size = window.inner_size();
        let Some(w) = std::num::NonZeroU32::new(size.width) else { return };
        let Some(h) = std::num::NonZeroU32::new(size.height) else { return };
        // §6.3 view-fn → §5.11 Scene tree; cached state drives the
        // ephemeral paint scene each frame.
        let frame = Frame::new();
        let mut paint_scene = view(self.cached_state, &frame);
        // R24 §5.21: taffy resolves every node's pixel rect before
        // paint. Pure function of (scene, viewport); no per-frame
        // cache needed at this scale.
        compute_layout(&mut paint_scene, w.get(), h.get());
        // R46.5 §5.16: framework-side Scene → vello::Scene walk via
        // paint_adapter (R46.3.1). hello-button has no app-specific
        // tag substitution, so the closure returns None unconditionally
        // and every Box honours its native `style.fill`. R47.3 §5.36 —
        // the Text arm consults `self.text_cache` (parley shaping LRU)
        // so the button's static labels shape once and hit the cache
        // on every subsequent frame.
        self.vello_scene.reset();
        let base = paint_adapter::root_background(&paint_scene);
        paint_adapter::to_vello(
            &paint_scene,
            &|_b: &BoxNode| None,
            &mut self.text_cache,
            &mut self.vello_scene,
        );
        if let Err(e) = renderer.render(&self.vello_scene, base) {
            eprintln!("hello-button: vello render: {e}");
        }
        // R48 §5.35: hand the post-layout paint scene to the framework
        // router. The router retains it for subsequent hit-tests and
        // re-resolves hover_target now (window resize may have moved
        // the button rect under a stationary cursor).
        self.router.update_paint_scene(paint_scene, &mut self.scene);
        self.refresh_state();
        self.drain_intents();
    }
}

impl ApplicationHandler<AppEvent> for App {
    /// R46.3.4 — winit may fire `resumed` more than once on platforms
    /// that suspend (Android, Wayland-compositor focus changes). The
    /// Vello canonical pattern caches the previous `Window` across the
    /// drop-and-recreate cycle so the OS-side handle survives, while
    /// the GPU `HelloButtonRenderer` is freshly constructed each time.
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if matches!(self.state, RenderState::Active { .. }) {
            return;
        }
        let cached = match std::mem::replace(&mut self.state, RenderState::Suspended(None)) {
            RenderState::Suspended(cached) => cached,
            RenderState::Active { .. } => unreachable!("matched as non-Active above"),
        };
        let window = if let Some(w) = cached {
            w
        } else {
            let attrs = Window::default_attributes()
                .with_title("pinion hello-button (R46.5 §5.16 Vello)")
                .with_inner_size(winit::dpi::LogicalSize::new(
                    f64::from(WIN_W),
                    f64::from(WIN_H),
                ));
            match event_loop.create_window(attrs) {
                Ok(w) => Arc::new(w),
                Err(e) => {
                    eprintln!("hello-button: window create failed: {e}");
                    event_loop.exit();
                    return;
                }
            }
        };
        let size = window.inner_size();
        let renderer = pollster::block_on(HelloButtonRenderer::new(
            Arc::clone(&window),
            size.width.max(1),
            size.height.max(1),
        ));
        let renderer = match renderer {
            Ok(r) => r,
            Err(e) => {
                eprintln!("hello-button: HelloButtonRenderer::new: {e}");
                // Keep the window cached so a subsequent resumed can
                // retry — only the renderer creation failed.
                self.state = RenderState::Suspended(Some(window));
                event_loop.exit();
                return;
            }
        };
        self.state = RenderState::Active {
            window,
            renderer: Box::new(renderer),
        };
        eprintln!(
            "hello-button: hover/click the window to drive the Button SCXML.\n           keys: d=Disable, e=Enable, Esc=quit\n           RPC: pipe JSON-RPC 2.0 frames (one per line) on stdin\n           §5.20: button.click intents log to stderr after each event"
        );
    }

    /// R46.3.4 — release the GPU-side renderer on suspend so the OS
    /// can reclaim the wgpu surface. The winit window itself is cached
    /// for the next `resumed` so its handle / OS state survives.
    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        if let RenderState::Active { window, .. } =
            std::mem::replace(&mut self.state, RenderState::Suspended(None))
        {
            self.state = RenderState::Suspended(Some(window));
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
                eprintln!("button: final state = {:?}", self.cached_state);
                event_loop.exit();
            }
            // R48 §5.35: all pointer routing flows through the framework
            // InputRouter. The handler bodies just forward the winit
            // event into the router; the router does the hit-test,
            // emits PointerEnter/Leave/Down/Up to the matching
            // ExternalNode, and the app only refreshes its cached
            // state + drains intents afterwards. CursorEntered is a
            // no-op (winit guarantees a CursorMoved follows, which
            // resolves the real cursor position).
            WindowEvent::CursorMoved { position, .. } => {
                self.router.cursor_moved(position.x, position.y, &mut self.scene);
                self.refresh_state();
                self.drain_intents();
            }
            WindowEvent::CursorLeft { .. } => {
                self.router.cursor_left(&mut self.scene);
                self.refresh_state();
                self.drain_intents();
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                self.router.pointer_down(&mut self.scene);
                self.refresh_state();
                self.drain_intents();
            }
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => {
                self.router.pointer_up(&mut self.scene);
                self.refresh_state();
                self.drain_intents();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                use winit::keyboard::{Key, NamedKey};
                if event.state == ElementState::Pressed {
                    match event.logical_key.as_ref() {
                        Key::Character("d") => self.forward(ButtonEvent::Disable),
                        Key::Character("e") => self.forward(ButtonEvent::Enable),
                        Key::Named(NamedKey::Escape) => event_loop.exit(),
                        _ => {}
                    }
                }
            }
            WindowEvent::Resized(size) => {
                if let RenderState::Active { renderer, .. } = &mut self.state {
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

/// Background thread: read JSON-RPC 2.0 lines from stdin and
/// forward each as an `AppEvent::RpcRequest` user event. Blank
/// lines are skipped; EOF or any read error terminates the thread
/// quietly (the GUI loop keeps running). The proxy `send_event`
/// fails only after the event loop has shut down, in which case we
/// also exit the thread.
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

fn main() {
    let event_loop = EventLoop::<AppEvent>::with_user_event()
        .build()
        .expect("winit EventLoop::with_user_event failed");
    event_loop.set_control_flow(ControlFlow::Wait);
    spawn_stdin_rpc_reader(event_loop.create_proxy());
    let mut app = App::new();
    if let Err(e) = event_loop.run_app(&mut app) {
        eprintln!("hello-button: event loop error: {e}");
    }
}
