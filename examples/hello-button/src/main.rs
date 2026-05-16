//! `hello-button` — §4 first dogfood + live §2 bidirectional RPC.
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
//!   * The **paint scene** is separate: `view(state, &Frame) -> Scene`
//!     (§6.3 pure sync) builds a `Scene::Container` (bg box +
//!     centered button box + text label) from the current
//!     `ButtonState` each frame. `paint` recurses over that.
//!     Model/view split — state scene is authoritative, paint scene
//!     is a derived view.
//!   * A background thread reads JSON-RPC 2.0 lines from `stdin`
//!     and forwards each as a winit `UserEvent`; the main thread
//!     handles it on the UI thread, then refreshes the cached state
//!     and requests a redraw if the RPC mutated anything.

use std::io::{BufRead, Write};
use std::num::NonZeroU32;
use std::rc::Rc;
use std::thread;

use pinion_core::external::IntrospectValue;
use pinion_core::scene::{BoxNode, ContainerNode, ExternalNode, Rect, TextNode};
use pinion_core::widgets::button::{ButtonEvent, ButtonExternal, ButtonState};
use pinion_core::{Frame, Scene};
use pinion_rpc::dispatch;
use softbuffer::{Context, Surface};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::{Window, WindowId};

/// Winit user-event variants — today only the stdin-fed RPC line.
#[derive(Debug, Clone)]
enum AppEvent {
    /// One JSON-RPC 2.0 frame read from stdin, awaiting dispatch.
    RpcRequest(String),
}

const WIN_W: u32 = 320;
const WIN_H: u32 = 200;
const BG_FILL: u32 = 0x0020_3040; // dark navy
const BTN_RECT: Rect = Rect::new(80, 60, 160, 80);

/// view-fn (§6.3): pure sync mapping `ButtonState` → `Scene`. The
/// `&Frame` slot is the §6.3 ZST hedge — zero-cost today, readied
/// for `dt`/`frame_index` without a `SemVer` major. Purity here is
/// the §2 `dry_run` invariant: same `(state, frame)` always yields
/// the same `Scene`.
//
// `&Frame` is intentional per the §6.3 signature contract even
// though `Frame` is presently a ZST: once real per-frame fields
// land, passing by value would force a `SemVer` major on every
// view-fn. Allow the lint at the view-fn boundary.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: ButtonState, _frame: &Frame) -> Scene {
    let btn_fill: u32 = match state {
        ButtonState::Idle => 0x00ff_ffff,     // white
        ButtonState::Hover => 0x00d0_d0d0,    // light grey
        ButtonState::Pressed => 0x0050_5050,  // dark grey
        ButtonState::Disabled => 0x00b0_2020, // muted red
    };
    let label = match state {
        ButtonState::Disabled => "Disabled",
        _ => "Click me!",
    };
    // Label rect is centered inside BTN_RECT — height 24px font slot.
    let label_rect = Rect::new(
        BTN_RECT.x,
        BTN_RECT.y + BTN_RECT.h / 2 - 12,
        BTN_RECT.w,
        24,
    );
    Scene::Container(ContainerNode::new(vec![
        Scene::Box(BoxNode::new(BG_FILL, Rect::new(0, 0, WIN_W, WIN_H))),
        Scene::Box(BoxNode::new(btn_fill, BTN_RECT)),
        Scene::Text(TextNode::new(label, label_rect)),
    ]))
}

/// Recursive Scene-tree paint into the softbuffer pixel slice. v0
/// interprets `Scene::Box` (rect-fill) and `Scene::Container`
/// (recurse over children); `Scene::Text` is explicitly skipped
/// until the cosmic-text rasterizer slice lands, even though its
/// §5.11 schema (`content`+`rect`) is already in the scene tree
/// for RPC introspection. Other variants are reserved.
fn paint(scene: &Scene, buffer: &mut [u32], buf_w: usize, buf_h: usize) {
    match scene {
        Scene::Box(node) => paint_box(node, buffer, buf_w, buf_h),
        Scene::Container(node) => {
            for child in &node.children {
                paint(child, buffer, buf_w, buf_h);
            }
        }
        // v0: `Scene::Text` rasterizer (cosmic-text) deferred;
        // Path/Image/Effect/External not yet wired into paint.
        _ => {}
    }
}

fn paint_box(node: &BoxNode, buffer: &mut [u32], buf_w: usize, buf_h: usize) {
    let r = node.rect;
    let x_start = (r.x as usize).min(buf_w);
    let y_start = (r.y as usize).min(buf_h);
    let x_end = (r.x.saturating_add(r.w) as usize).min(buf_w);
    let y_end = (r.y.saturating_add(r.h) as usize).min(buf_h);
    for y in y_start..y_end {
        let row = y * buf_w;
        buffer[row + x_start..row + x_end].fill(node.fill);
    }
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

struct App {
    window: Option<Rc<Window>>,
    surface: Option<Surface<Rc<Window>, Rc<Window>>>,
    context: Option<Context<Rc<Window>>>,
    /// Authoritative state scene — owns the live `ButtonExternal`
    /// via `Box<dyn External>`. Both winit input and RPC dispatch
    /// reach the SCXML statechart through this single scene.
    scene: Scene,
    /// Cached projection of the inner `ButtonState`, kept in sync
    /// by `refresh_state` after every input. Drives change-detection
    /// for the redraw request + the paint scene's fill mapping.
    cached_state: ButtonState,
}

impl App {
    fn new() -> Self {
        let scene = Scene::External(ExternalNode::new(Box::new(ButtonExternal::new())));
        // Initial state is whatever the freshly-constructed Button is
        // at — read it via the same introspect channel everything else
        // uses, so there is exactly one source of truth.
        let cached_state = read_state(&scene);
        eprintln!("button: initial state = {cached_state:?}");
        Self {
            window: None,
            surface: None,
            context: None,
            scene,
            cached_state,
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
        self.refresh_state();
    }

    /// Dispatch one JSON-RPC frame against the LIVE state scene.
    /// `scene/invoke /external/send PointerEnter` (and friends) now
    /// drive the SCXML the same way a winit click would.
    fn dispatch_rpc(&mut self, request: &str) {
        if let Some(resp) = dispatch(&mut self.scene, request) {
            let mut out = std::io::stdout().lock();
            if writeln!(out, "{resp}").is_err() {
                // stdout closed (downstream consumer gone) — silently
                // skip; do not abort the GUI loop on a broken pipe.
            }
        }
        // The RPC frame may have mutated state — re-read, log the
        // delta, and trigger a redraw if the visual changed.
        self.refresh_state();
    }

    /// Re-read the cached `ButtonState` from the live scene; log
    /// and repaint if it changed since the previous refresh.
    fn refresh_state(&mut self) {
        let now = read_state(&self.scene);
        if now != self.cached_state {
            eprintln!("button: {:?} -> {:?}", self.cached_state, now);
            self.cached_state = now;
            if let Some(window) = self.window.as_ref() {
                window.request_redraw();
            }
        }
    }

    fn render(&mut self) {
        let Some(surface) = self.surface.as_mut() else {
            return;
        };
        let Some(window) = self.window.as_ref() else {
            return;
        };
        let size = window.inner_size();
        let Some(width) = NonZeroU32::new(size.width) else {
            return;
        };
        let Some(height) = NonZeroU32::new(size.height) else {
            return;
        };
        if surface.resize(width, height).is_err() {
            return;
        }
        let Ok(mut buffer) = surface.buffer_mut() else {
            return;
        };
        // §6.3 view-fn → §5.11 Scene tree → recursive paint into the
        // softbuffer pixel slice. The state scene (self.scene) is the
        // model; `view` builds an ephemeral paint scene from the
        // cached state each frame. Clear-to-zero first so any pixels
        // outside the v0 background rect (after window resize) read
        // as transparent black rather than stale frame data.
        let buf_w = width.get() as usize;
        let buf_h = height.get() as usize;
        let frame = Frame::new();
        let paint_scene = view(self.cached_state, &frame);
        buffer.fill(0);
        paint(&paint_scene, &mut buffer, buf_w, buf_h);
        let _ = buffer.present();
    }
}

impl ApplicationHandler<AppEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        eprintln!("hello-button: resumed() fired; creating window...");
        let attrs = Window::default_attributes()
            .with_title("pinion hello-button (§4 first dogfood)")
            .with_inner_size(winit::dpi::LogicalSize::new(
                f64::from(WIN_W),
                f64::from(WIN_H),
            ));
        let window = match event_loop.create_window(attrs) {
            Ok(w) => {
                eprintln!("hello-button: window created id={:?}", w.id());
                Rc::new(w)
            }
            Err(e) => {
                eprintln!("hello-button: failed to create window: {e}");
                event_loop.exit();
                return;
            }
        };
        let context = match Context::new(Rc::clone(&window)) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("hello-button: softbuffer context failed: {e}");
                event_loop.exit();
                return;
            }
        };
        let surface = match Surface::new(&context, Rc::clone(&window)) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("hello-button: softbuffer surface failed: {e}");
                event_loop.exit();
                return;
            }
        };
        self.window = Some(window);
        self.context = Some(context);
        self.surface = Some(surface);
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
            WindowEvent::CursorEntered { .. } => self.forward(ButtonEvent::PointerEnter),
            WindowEvent::CursorLeft { .. } => self.forward(ButtonEvent::PointerLeave),
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => self.forward(ButtonEvent::PointerDown),
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => self.forward(ButtonEvent::PointerUp),
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
    eprintln!(
        "hello-button: hover/click the window to drive the Button SCXML.\n           keys: d=Disable, e=Enable, Esc=quit\n           RPC: pipe JSON-RPC 2.0 frames (one per line) on stdin"
    );
    if let Err(e) = event_loop.run_app(&mut app) {
        eprintln!("hello-button: event loop error: {e}");
    }
}
