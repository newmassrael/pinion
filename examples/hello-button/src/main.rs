//! `hello-button` — §4 first dogfood.
//!
//! Opens a winit window and pipes real pointer input into the R12
//! Button SCXML statechart (`pinion_core::widgets::button::Button`).
//! A sync `view(state, &Frame) -> Scene` (§6.3 view-fn signature, §2
//! purity invariant) then maps `ButtonState` to `Scene::Box(BoxNode
//! { fill })` (§5.2 enum + §5.11 v0 field schema). The softbuffer
//! pixel loop reads `BoxNode.fill` directly — proving the view-fn →
//! Scene → present chain end-to-end with the §5.16 RHI still pending.

use std::num::NonZeroU32;
use std::rc::Rc;

use pinion_core::scene::BoxNode;
use pinion_core::widgets::button::{Button, ButtonEvent, ButtonState};
use pinion_core::{Frame, Scene};
use softbuffer::{Context, Surface};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

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
    let fill: u32 = match state {
        ButtonState::Idle => 0x00ff_ffff,     // white
        ButtonState::Hover => 0x00d0_d0d0,    // light grey
        ButtonState::Pressed => 0x0050_5050,  // dark grey
        ButtonState::Disabled => 0x00b0_2020, // muted red
    };
    Scene::Box(BoxNode::new(fill))
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
        // §6.3 view-fn first real call; pattern-match the Scene::Box
        // payload for the v0 fill. `_` arm is required by `Scene`'s
        // `#[non_exhaustive]` even though v0 view-fn only returns Box.
        let frame = Frame::new();
        let colour = if let Scene::Box(node) = view(self.button.state(), &frame) {
            node.fill
        } else {
            0
        };
        for pixel in buffer.iter_mut() {
            *pixel = colour;
        }
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
            .with_inner_size(winit::dpi::LogicalSize::new(320.0, 200.0));
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
