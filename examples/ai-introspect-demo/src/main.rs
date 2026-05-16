//! R39.4.3 — first user-visible AI-overlay dogfood.
//!
//! A static scene with three tagged buttons. Right-click anywhere to
//! ask `scene/locate` (§5.32) where the click landed. The §5.33
//! `inject_highlight` overlay then paints a red outline around the
//! identified primitive *and* prints the structured JSON-shaped
//! result to stdout — proving the AI agent receives a semantic
//! `path + bbox + ancestors` instead of a screenshot.
//!
//! Controls:
//!
//!   * **Right-click** — locate + highlight
//!   * **Left-click**  — clear highlights
//!   * **Escape**      — clear highlights + exit on second press
//!   * **R**           — print the current scene tree as JSON to stdout
//!
//! Why this exists, beyond aesthetics: §5.32 + §5.33 are pinion's
//! "AI-native input/output channel" claim. Without a sighted dogfood
//! the protocol is abstract. With it, the framework's headline
//! differentiator is *demonstrable in five seconds*.
//!
//! This binary deliberately stays small (~300 LoC). No SCXML, no
//! `cosmic-text`, no taffy — the demonstration is the *protocol* doing
//! its job, not the framework's full rendering surface.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::doc_markdown,
)]

use std::num::NonZeroU32;
use std::rc::Rc;

use softbuffer::{Context, Surface};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

use pinion_core::scene::{BoxNode, ContainerNode, Rect};
use pinion_core::style::{Border, BoxStyle, Color};
use pinion_core::Scene;
use pinion_overlay::{clear_highlights, inject_highlight, HighlightStyle};
use pinion_rpc::locate;

const WIN_W: u32 = 640;
const WIN_H: u32 = 360;

/// Build the static demo scene. Three tagged buttons + a background
/// container — small enough to reason about, large enough to exercise
/// hit-test traversal with tags.
fn build_scene() -> Scene {
    let mut root = ContainerNode::new(vec![
        Scene::Box(
            BoxNode::filled(Rect::new(60, 80, 140, 60), Color::from_argb(0x00ff_3366))
                .with_tag("save_btn"),
        ),
        Scene::Box(
            BoxNode::filled(Rect::new(240, 80, 140, 60), Color::from_argb(0x0033_88ff))
                .with_tag("cancel_btn"),
        ),
        Scene::Box(
            BoxNode::filled(Rect::new(420, 80, 140, 60), Color::from_argb(0x00aa_aaaa))
                .with_tag("delete_btn"),
        ),
        Scene::Box(
            BoxNode::filled(Rect::new(60, 200, 520, 100), Color::from_argb(0x002a_2a2a))
                .with_tag("info_panel"),
        ),
    ]);
    root.rect = Rect::new(0, 0, WIN_W, WIN_H);
    root.style = BoxStyle::filled(Color::from_argb(0x0011_1116));
    Scene::Container(root)
}

struct App {
    base_scene: Scene,
    /// Scene currently painted — base + any active overlays.
    paint_scene: Scene,
    cursor: (u32, u32),
    /// Pressed-Escape counter so a single tap clears overlays and a
    /// second tap (within the same overlay-cleared state) exits.
    pending_escape_exit: bool,
    window: Option<Rc<Window>>,
    context: Option<Context<Rc<Window>>>,
    surface: Option<Surface<Rc<Window>, Rc<Window>>>,
}

impl App {
    fn new() -> Self {
        let scene = build_scene();
        // Clone-equivalent for Scene without Clone derive: rebuild the
        // identical tree. Scene intentionally omits Clone (see
        // pinion-core docs) so the demo holds an immutable "base" by
        // re-building rather than cloning.
        Self {
            base_scene: scene,
            paint_scene: build_scene(),
            cursor: (0, 0),
            pending_escape_exit: false,
            window: None,
            context: None,
            surface: None,
        }
    }

    fn on_right_click(&mut self) {
        let (x, y) = self.cursor;
        match locate(&self.paint_scene, x, y) {
            Ok(outcome) => {
                println!(
                    "→ scene/locate {{x:{x}, y:{y}}}\n  path: {}\n  bbox: x={} y={} w={} h={}\n  ancestors: {:?}",
                    outcome.path,
                    outcome.bbox.x,
                    outcome.bbox.y,
                    outcome.bbox.w,
                    outcome.bbox.h,
                    outcome.ancestor_paths,
                );
                let suffix = path_suffix(&outcome.path);
                self.paint_scene = inject_highlight(
                    std::mem::replace(&mut self.paint_scene, build_scene()),
                    suffix,
                    HighlightStyle::default(),
                );
                self.pending_escape_exit = false;
            }
            Err(err) => {
                println!("→ scene/locate {{x:{x}, y:{y}}}\n  err: {err:?}");
            }
        }
        self.request_redraw();
    }

    fn on_clear(&mut self) {
        self.paint_scene = clear_highlights(std::mem::replace(
            &mut self.paint_scene,
            build_scene(),
        ));
        self.request_redraw();
    }

    fn rebuild_paint_scene(&mut self) {
        // Used after a full clear when we want a guaranteed-clean tree.
        let _ = &self.base_scene; // referenced for future "view-fn" wiring
        self.paint_scene = build_scene();
    }

    fn print_scene_tree(&self) {
        fn walk(s: &Scene, depth: usize) {
            let indent = "  ".repeat(depth);
            match s {
                Scene::Container(c) => {
                    println!(
                        "{indent}Container rect={:?} tag={:?} children={}",
                        c.rect,
                        c.tag,
                        c.children.len(),
                    );
                    for child in &c.children {
                        walk(child, depth + 1);
                    }
                }
                Scene::Box(b) => {
                    println!("{indent}Box rect={:?} tag={:?}", b.rect, b.tag);
                }
                _ => println!("{indent}<{:?}>", std::mem::discriminant(s)),
            }
        }
        println!("--- scene tree ---");
        walk(&self.paint_scene, 0);
        println!("--- end ---");
    }

    fn request_redraw(&self) {
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    fn render(&mut self) {
        let Some(surface) = self.surface.as_mut() else { return };
        let Some(window) = self.window.as_ref() else { return };
        let size = window.inner_size();
        let Some(w) = NonZeroU32::new(size.width) else { return };
        let Some(h) = NonZeroU32::new(size.height) else { return };
        if surface.resize(w, h).is_err() {
            return;
        }
        let Ok(mut buffer) = surface.buffer_mut() else { return };
        let buf_w = w.get() as usize;
        let buf_h = h.get() as usize;
        buffer.fill(0);
        paint(&self.paint_scene, &mut buffer, buf_w, buf_h);
        let _ = buffer.present();
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("pinion ai-introspect-demo (§5.32 + §5.33)")
            .with_inner_size(LogicalSize::new(f64::from(WIN_W), f64::from(WIN_H)));
        let window = match event_loop.create_window(attrs) {
            Ok(w) => Rc::new(w),
            Err(e) => {
                eprintln!("ai-introspect-demo: window create failed: {e}");
                event_loop.exit();
                return;
            }
        };
        let context = match Context::new(Rc::clone(&window)) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("ai-introspect-demo: softbuffer ctx failed: {e}");
                event_loop.exit();
                return;
            }
        };
        let surface = match Surface::new(&context, Rc::clone(&window)) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("ai-introspect-demo: softbuffer surface failed: {e}");
                event_loop.exit();
                return;
            }
        };
        self.window = Some(window);
        self.context = Some(context);
        self.surface = Some(surface);
        println!("Right-click a box to locate via §5.32 RPC.");
        println!("Left-click or Esc to clear highlights. R prints scene tree. Esc twice to exit.");
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => self.render(),
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = (position.x.max(0.0) as u32, position.y.max(0.0) as u32);
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button,
                ..
            } => match button {
                MouseButton::Right => self.on_right_click(),
                MouseButton::Left => self.on_clear(),
                _ => {}
            },
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == ElementState::Pressed {
                    match event.logical_key.as_ref() {
                        Key::Named(NamedKey::Escape) => {
                            if self.pending_escape_exit {
                                event_loop.exit();
                            } else {
                                self.on_clear();
                                self.pending_escape_exit = true;
                            }
                        }
                        Key::Character("r" | "R") => self.print_scene_tree(),
                        Key::Character("0") => {
                            // Hidden helper: full rebuild (drops any overlay).
                            self.rebuild_paint_scene();
                            self.request_redraw();
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
}

/// Strip the `/window[<name>]/` prefix from a fully-qualified locate
/// path so [`inject_highlight`] can resolve segments relative to the
/// scene root.
fn path_suffix(full: &str) -> &str {
    let rest = full.strip_prefix("/window[").unwrap_or(full);
    let after_bracket = match rest.find(']') {
        Some(close) => &rest[close + 1..],
        None => rest,
    };
    after_bracket.trim_start_matches('/')
}

/// Minimal scene → pixels renderer. Handles fill + border (border is
/// emitted by the overlay highlight). Other Scene variants are no-ops
/// at this demo's scope.
fn paint(scene: &Scene, buf: &mut [u32], w: usize, h: usize) {
    match scene {
        Scene::Container(c) => {
            paint_filled_rect(c.rect, c.style.fill, buf, w, h);
            for child in &c.children {
                paint(child, buf, w, h);
            }
        }
        Scene::Box(b) => {
            paint_filled_rect(b.rect, b.style.fill, buf, w, h);
            if let Some(border) = b.style.border {
                paint_border(b.rect, border, buf, w, h);
            }
        }
        _ => {}
    }
}

fn paint_filled_rect(r: Rect, fill: Color, buf: &mut [u32], w: usize, h: usize) {
    if fill == Color::TRANSPARENT {
        return;
    }
    let x0 = (r.x as usize).min(w);
    let y0 = (r.y as usize).min(h);
    let x1 = (r.x.saturating_add(r.w) as usize).min(w);
    let y1 = (r.y.saturating_add(r.h) as usize).min(h);
    for y in y0..y1 {
        let row = y * w;
        buf[row + x0..row + x1].fill(fill.to_argb());
    }
}

fn paint_border(r: Rect, border: Border, buf: &mut [u32], w: usize, h: usize) {
    let tw = border.width;
    if tw == 0 {
        return;
    }
    // Top / bottom strips
    paint_filled_rect(Rect::new(r.x, r.y, r.w, tw), border.color, buf, w, h);
    paint_filled_rect(
        Rect::new(r.x, r.y.saturating_add(r.h).saturating_sub(tw), r.w, tw),
        border.color,
        buf,
        w,
        h,
    );
    // Left / right strips
    paint_filled_rect(Rect::new(r.x, r.y, tw, r.h), border.color, buf, w, h);
    paint_filled_rect(
        Rect::new(r.x.saturating_add(r.w).saturating_sub(tw), r.y, tw, r.h),
        border.color,
        buf,
        w,
        h,
    );
}

fn main() {
    let event_loop = match EventLoop::new() {
        Ok(el) => el,
        Err(e) => {
            eprintln!("ai-introspect-demo: event loop init failed: {e}");
            return;
        }
    };
    let mut app = App::new();
    if let Err(e) = event_loop.run_app(&mut app) {
        eprintln!("ai-introspect-demo: run_app exited: {e}");
    }
}
