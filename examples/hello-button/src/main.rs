//! `hello-button` — §4 first dogfood + first live §2 RPC dogfood.
//!
//! Opens a winit window and pipes real pointer input into the R12
//! `Button` SCXML statechart, wrapped in a `ButtonExternal` adapter
//! (§5.15 reference impl). A sync `view(state, &Frame) -> Scene`
//! (§6.3 view-fn signature, §2 purity invariant) builds a
//! `Scene::Container` carrying a dark navy background `Scene::Box`,
//! a centered button `Scene::Box` whose fill reflects `ButtonState`,
//! and a `Scene::Text` label. A recursive `paint` walks the tree
//! and writes `BoxNode`s into the softbuffer pixel slice;
//! `TextNode` is present-but-unrasterized until the cosmic-text
//! slice, with the §5.16 RHI still pending.
//!
//! In parallel, a background thread reads JSON-RPC 2.0 lines from
//! `stdin`, forwards each as a winit user event, and the main
//! thread dispatches it against a fresh `ButtonStateSnapshot` —
//! the first live exercise of §2 invariant #2 ("RPC headless as AI
//! primary path") against a running winit app. v0 is read-only:
//! `scene/query /external/state` succeeds, `intervene`-class
//! methods return `ReadOnly` per the snapshot contract. Bidirectional
//! RPC (RPC-driven `ButtonEvent`) requires a `Box<dyn External>`
//! downcast story and is carry-forward.

use std::io::{BufRead, Write};
use std::num::NonZeroU32;
use std::rc::Rc;
use std::thread;

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

struct App {
    window: Option<Rc<Window>>,
    surface: Option<Surface<Rc<Window>, Rc<Window>>>,
    context: Option<Context<Rc<Window>>>,
    button: ButtonExternal,
    last_logged_state: ButtonState,
}

impl App {
    fn new() -> Self {
        let button = ButtonExternal::new();
        let last_logged_state = button.state();
        eprintln!("button: initial state = {last_logged_state:?}");
        Self {
            window: None,
            surface: None,
            context: None,
            button,
            last_logged_state,
        }
    }

    fn forward(&mut self, event: ButtonEvent) {
        self.button.send(event);
        let now = self.button.state();
        if now != self.last_logged_state {
            eprintln!("button: {:?} -> {:?}", self.last_logged_state, now);
            self.last_logged_state = now;
            if let Some(window) = self.window.as_ref() {
                window.request_redraw();
            }
        }
    }

    /// Dispatch one JSON-RPC frame against a freshly snapshotted
    /// `Scene::External(ButtonStateSnapshot)`. The live `ButtonExternal`
    /// itself stays on the UI thread; only its `ButtonState` (a `Copy`
    /// enum) crosses into the snapshot, so this stays sound without any
    /// `Send`/`Sync` bound on `External`.
    fn dispatch_rpc(&self, request: &str) {
        let snap = self.button.snapshot();
        let mut scene = Scene::External(ExternalNode::new(Box::new(snap)));
        if let Some(resp) = dispatch(&mut scene, request) {
            let mut out = std::io::stdout().lock();
            if writeln!(out, "{resp}").is_err() {
                // stdout closed (downstream consumer gone) — silently
                // skip; do not abort the GUI loop on a broken pipe.
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
        // softbuffer pixel slice. Clear-to-zero first so any pixels
        // outside the v0 background rect (after window resize) read
        // as transparent black rather than stale frame data.
        let buf_w = width.get() as usize;
        let buf_h = height.get() as usize;
        let frame = Frame::new();
        let scene = view(self.button.state(), &frame);
        buffer.fill(0);
        paint(&scene, &mut buffer, buf_w, buf_h);
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
                eprintln!("button: final state = {:?}", self.button.state());
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
