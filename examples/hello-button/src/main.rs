//! `hello-button` — §4 first dogfood.
//!
//! Opens a winit window and pipes real pointer input into the R12
//! `Button` SCXML statechart (`pinion_core::widgets::button::Button`).
//! A sync `view(state, &Frame) -> Scene` (§6.3 view-fn signature, §2
//! purity invariant) builds a `Scene::Container` of two `Scene::Box`
//! children — a dark navy background covering the window and a
//! centered button whose fill reflects `ButtonState` (§5.2 enum +
//! §5.11 v0 `BoxNode` `fill`+`rect` schema). A recursive `paint`
//! walks the scene tree and writes each `BoxNode` into the
//! softbuffer pixel slice, with the §5.16 RHI still pending.

use std::num::NonZeroU32;
use std::rc::Rc;

use pinion_core::scene::{BoxNode, ContainerNode, Rect};
use pinion_core::widgets::button::{Button, ButtonEvent, ButtonState};
use pinion_core::{Frame, Scene};
use softbuffer::{Context, Surface};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

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
    Scene::Container(ContainerNode::new(vec![
        Scene::Box(BoxNode::new(BG_FILL, Rect::new(0, 0, WIN_W, WIN_H))),
        Scene::Box(BoxNode::new(btn_fill, BTN_RECT)),
    ]))
}

/// Recursive Scene-tree paint into the softbuffer pixel slice. v0
/// interprets `Scene::Box` (the only variant with a concrete v0
/// payload per §5.11) and `Scene::Container` (recurse over
/// children); other variants are deliberately skipped until their
/// §5.11 shape lands.
fn paint(scene: &Scene, buffer: &mut [u32], buf_w: usize, buf_h: usize) {
    match scene {
        Scene::Box(node) => paint_box(node, buffer, buf_w, buf_h),
        Scene::Container(node) => {
            for child in &node.children {
                paint(child, buffer, buf_w, buf_h);
            }
        }
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
    button: Button,
    last_logged_state: ButtonState,
}

impl App {
    fn new() -> Self {
        let button = Button::new();
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

impl ApplicationHandler for App {
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
}

fn main() {
    let event_loop = EventLoop::new().expect("winit EventLoop::new failed");
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut app = App::new();
    eprintln!(
        "hello-button: hover/click the window to drive the Button SCXML.\n           keys: d=Disable, e=Enable, Esc=quit"
    );
    if let Err(e) = event_loop.run_app(&mut app) {
        eprintln!("hello-button: event loop error: {e}");
    }
}
