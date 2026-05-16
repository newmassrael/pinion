//! R40.8 — propose/apply visual dogfood for §5.34's preview lifecycle.
//!
//! Closes the §5.34 R40.x series with the first user-visible round-trip
//! through `scene/propose_change` → `scene/apply_preview`. Builds on
//! R39.4.3's locate+highlight dogfood: the original right-click /
//! locate / red-outline interaction is preserved verbatim, and four new
//! keybindings (`P`/`A`/`C`/`L`) drive the preview lifecycle against an
//! introspectable `CountedExternal` whose value drives `info_panel`'s
//! fill colour.
//!
//! Two scenes coexist:
//!
//!   * **`state_scene`** — `Scene::External(CountedExternal)` rooted so
//!     the v0 `/external/count` path resolves. This is what the RPC
//!     layer mutates: [`propose_change`] / [`apply_preview`] take a
//!     reference to it, and [`SceneRevision`] tracks its version for
//!     OCC conflict detection.
//!
//!   * **`paint_scene`** — the visible scene. Three tagged buttons
//!     identical to R39.4.3 plus an `info_panel` whose fill is derived
//!     from `state_scene`'s count via [`palette_color`]. Rebuilt on
//!     every state change so an applied preview is immediately visible.
//!
//! The bridge is intentional: `target_path` in a typed proposal is the
//! anchor the AI agent reasons about ("/info_panel" — what the user
//! sees), while `signal_path` is the addressable slot in the state
//! scene ("/external/count"). Separating them keeps introspection
//! widget-centric while keeping mutation slot-centric.
//!
//! Controls:
//!
//!   * **Right-click** — `scene/locate` (§5.32) + red outline
//!   * **Left-click**  — clear all overlay highlights
//!   * **Escape**      — clear highlights; second press exits
//!   * **R**           — print the `paint_scene` tree to stdout
//!   * **P**           — `scene/propose_change`: cycle `info_panel`'s
//!                       fill to the next palette entry. Yellow border
//!                       appears around `info_panel` while the preview
//!                       is in flight.
//!   * **A**           — `scene/apply_preview`: commit the most-recent
//!                       preview. `info_panel` colour shifts; yellow
//!                       overlay disappears.
//!   * **C**           — `scene/cancel_preview`: drop the most-recent
//!                       preview without mutating state. Yellow overlay
//!                       disappears with no colour change.
//!   * **L**           — `scene/list_previews`: print every in-flight
//!                       preview to stdout (id / base_revision /
//!                       target_path / affected_paths / TTL remaining).
//!
//! This binary still deliberately stays small (~400 LoC). No SCXML,
//! no `cosmic-text`, no taffy — the demonstration is the *RPC
//! lifecycle protocol* doing its job, not the framework's full
//! rendering surface.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::doc_markdown,
)]

use std::num::NonZeroU32;
use std::rc::Rc;
use std::time::Instant;

use softbuffer::{Context, Surface};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

use pinion_core::external::{CountedExternal, IntrospectValue};
use pinion_core::scene::{BoxNode, ContainerNode, ExternalNode, Rect};
use pinion_core::style::{Border, BoxStyle, Color};
use pinion_core::{Scene, SceneRevision};
use pinion_overlay::{clear_highlights, inject_highlight, HighlightStyle};
use pinion_rpc::{
    apply_preview, cancel_preview, list_previews, locate, propose_change, query, ApplyError,
    PreviewId, PreviewLedger, ProposeError, TypedProposal,
};

const WIN_W: u32 = 640;
const WIN_H: u32 = 360;

/// Background-colour palette `info_panel` cycles through as `count`
/// advances. Five entries keep the demo loop tight (P P P P P → back
/// to where we started) while still showing apply has visible effect.
const PALETTE: &[u32] = &[
    0x002a_2a2a, // grey  (count % 5 == 0, initial)
    0x002a_5a8a, // blue
    0x008a_5a2a, // brown
    0x002a_8a5a, // green
    0x008a_2a5a, // magenta
];

/// Yellow border used to mark `info_panel` while a preview is in
/// flight. Distinct from the default red used by locate-highlight so
/// the two states never visually collide.
const PENDING_HIGHLIGHT: HighlightStyle = HighlightStyle::new()
    .with_stroke(Color::from_argb(0x00ff_d000))
    .with_stroke_width(3);

/// Map the introspectable `count` value to a palette colour.
fn palette_color(count: i64) -> Color {
    let len = PALETTE.len() as i64;
    // rem_euclid gives a non-negative index even for negative counts —
    // defensive since CountedExternal stores i64.
    let idx = count.rem_euclid(len) as usize;
    Color::from_argb(PALETTE[idx])
}

/// Construct the state scene: `Scene::External(CountedExternal)` so the
/// v0 `/external/count` path resolves through [`pinion_rpc::query`] /
/// [`pinion_rpc::rewind`].
fn build_state_scene() -> Scene {
    Scene::External(ExternalNode::new(Box::new(CountedExternal::new(0))))
}

/// Build the rendered scene: 3 tagged buttons + an `info_panel` whose
/// fill is `palette_color(count)`. Pure function of `count` — overlay
/// injection (locate-red / preview-yellow) layers on top in
/// [`App::rebuild_paint_scene`].
fn build_paint_scene(count: i64) -> Scene {
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
            BoxNode::filled(Rect::new(60, 200, 520, 100), palette_color(count))
                .with_tag("info_panel"),
        ),
    ]);
    root.rect = Rect::new(0, 0, WIN_W, WIN_H);
    root.style = BoxStyle::filled(Color::from_argb(0x0011_1116));
    Scene::Container(root)
}

/// Read the current `count` from a [`Scene::External(CountedExternal)`]
/// via the §5.12 `query` RPC dispatcher. Returns `0` on any failure
/// (mis-shaped scene, opted-out introspection) so the paint loop has
/// a defined fallback rather than panicking.
fn read_count(state_scene: &Scene) -> i64 {
    match query(state_scene, "/external/count") {
        Ok(IntrospectValue::Int(n)) => n,
        _ => 0,
    }
}

struct App {
    /// Scene the RPC layer mutates. Held as `Scene::External` so the
    /// v0 `/external/count` path resolves without nested traversal.
    state_scene: Scene,
    /// OCC token for the state scene. Bumped by `apply_preview`; used
    /// by `propose_change` to capture the base revision at propose time.
    revision: SceneRevision,
    /// In-flight preview ledger.
    ledger: PreviewLedger,
    /// Most-recently-proposed handle. `A` and `C` act on this; `L` is
    /// independent and lists everything in `ledger`. Cleared when the
    /// referenced preview is consumed (apply / cancel).
    last_preview: Option<PreviewId>,
    /// Path suffixes the user has locate-highlighted (red). Re-applied
    /// after every state-driven rebuild so a count change does not
    /// silently drop active highlights.
    locate_highlights: Vec<String>,
    /// Derived: build_paint_scene(count) + overlays. Reconstructed
    /// from the inputs above by [`App::rebuild_paint_scene`].
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
        let state_scene = build_state_scene();
        let count = read_count(&state_scene);
        Self {
            state_scene,
            revision: SceneRevision::new(),
            ledger: PreviewLedger::default(),
            last_preview: None,
            locate_highlights: Vec::new(),
            paint_scene: build_paint_scene(count),
            cursor: (0, 0),
            pending_escape_exit: false,
            window: None,
            context: None,
            surface: None,
        }
    }

    /// Re-derive `paint_scene` from current state. Layers (in order):
    ///   1. base scene with `info_panel.fill = palette(count)`,
    ///   2. yellow pending-preview outline around `info_panel` when a
    ///      preview is in flight,
    ///   3. every locate-highlight the user has requested (red).
    ///
    /// `inject_highlight` is idempotent per-tag so repeated rebuilds
    /// converge on a stable overlay set.
    fn rebuild_paint_scene(&mut self) {
        let count = read_count(&self.state_scene);
        let mut s = build_paint_scene(count);
        if self.last_preview.is_some() {
            s = inject_highlight(s, "info_panel", PENDING_HIGHLIGHT);
        }
        for path in &self.locate_highlights {
            s = inject_highlight(s, path, HighlightStyle::default());
        }
        self.paint_scene = s;
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
                let suffix = path_suffix(&outcome.path).to_string();
                if !self.locate_highlights.contains(&suffix) {
                    self.locate_highlights.push(suffix);
                }
                self.pending_escape_exit = false;
            }
            Err(err) => {
                println!("→ scene/locate {{x:{x}, y:{y}}}\n  err: {err:?}");
            }
        }
        self.rebuild_paint_scene();
        self.request_redraw();
    }

    fn on_clear(&mut self) {
        // Strip all locate-highlights; preview-pending overlay stays
        // until apply/cancel so the user can see they have unfinished
        // RPC business.
        self.locate_highlights.clear();
        // Defensive: if `paint_scene` ever drifts (e.g. injected by
        // future code paths), the explicit `clear_highlights` plus
        // rebuild guarantees a clean baseline.
        self.paint_scene = clear_highlights(std::mem::replace(
            &mut self.paint_scene,
            build_paint_scene(0),
        ));
        self.rebuild_paint_scene();
        self.request_redraw();
    }

    /// `P` — propose to advance `count` by one palette step. Records
    /// the resulting [`PreviewId`] as `last_preview`; the yellow
    /// preview-pending outline is painted on the next rebuild.
    fn on_propose(&mut self) {
        let current = read_count(&self.state_scene);
        let proposed = current + 1;
        let proposal = TypedProposal::SetSignal {
            target_path: "/info_panel".to_string(),
            signal_path: "/external/count".to_string(),
            value: serde_json::json!(proposed),
        };
        match propose_change(&self.ledger, &self.revision, proposal, None) {
            Ok(outcome) => {
                println!(
                    "→ scene/propose_change\n  preview_id: {}\n  base_revision: {}\n  proposed_count: {} (palette idx {})",
                    outcome.preview_id,
                    outcome.base_revision,
                    proposed,
                    proposed.rem_euclid(PALETTE.len() as i64),
                );
                self.last_preview = Some(outcome.preview_id);
            }
            Err(ProposeError::CapacityFull { capacity }) => {
                println!("→ scene/propose_change\n  err: ledger full (capacity={capacity})");
            }
            // ProposeError is #[non_exhaustive] per §5.34 carry-forward.
            Err(other) => {
                println!("→ scene/propose_change\n  err: {other:?}");
            }
        }
        self.rebuild_paint_scene();
        self.request_redraw();
    }

    /// `A` — apply the most-recent preview. Clears `last_preview` on
    /// both success and the consume-and-fail variant
    /// ([`ApplyError::ApplyRejected`]); leaves it set on
    /// [`ApplyError::BaseRevisionConflict`] so the user can either
    /// re-apply (after a future re-propose) or cancel.
    fn on_apply(&mut self) {
        let Some(id) = self.last_preview else {
            println!("→ scene/apply_preview\n  err: no preview in flight (press P first)");
            return;
        };
        match apply_preview(&mut self.state_scene, &self.revision, &self.ledger, id) {
            Ok(outcome) => {
                let new_count = read_count(&self.state_scene);
                println!(
                    "→ scene/apply_preview\n  preview_id: {}\n  new_revision: {}\n  new_count: {} (palette idx {})",
                    outcome.preview_id,
                    outcome.new_revision,
                    new_count,
                    new_count.rem_euclid(PALETTE.len() as i64),
                );
                self.last_preview = None;
            }
            Err(ApplyError::BaseRevisionConflict { expected, actual }) => {
                println!(
                    "→ scene/apply_preview\n  err: BaseRevisionConflict (expected={expected}, actual={actual}) — preview kept; cancel or re-propose"
                );
            }
            Err(other) => {
                println!("→ scene/apply_preview\n  err: {other:?}");
                self.last_preview = None;
            }
        }
        self.rebuild_paint_scene();
        self.request_redraw();
    }

    /// `C` — cancel the most-recent preview. Idempotent: a stale
    /// `last_preview` (already applied / expired) returns `false`
    /// from [`cancel_preview`] and is silently cleared.
    fn on_cancel(&mut self) {
        let Some(id) = self.last_preview else {
            println!("→ scene/cancel_preview\n  err: no preview in flight (press P first)");
            return;
        };
        let removed = cancel_preview(&self.ledger, id);
        println!(
            "→ scene/cancel_preview\n  preview_id: {id}\n  removed: {removed}"
        );
        self.last_preview = None;
        self.rebuild_paint_scene();
        self.request_redraw();
    }

    /// `L` — read-only snapshot of the ledger. Prints each entry's id,
    /// base_revision, target_path, affected_paths, and seconds-until-
    /// deadline so the user can correlate `P` presses against retained
    /// state.
    fn on_list(&self) {
        let now = Instant::now();
        let entries = list_previews(&self.ledger, now);
        if entries.is_empty() {
            println!("→ scene/list_previews\n  (ledger empty)");
            return;
        }
        println!("→ scene/list_previews ({} entry/entries)", entries.len());
        for (idx, view) in entries.iter().enumerate() {
            let remaining = view.deadline.saturating_duration_since(now);
            println!(
                "  [{idx}] id={} base_rev={} target={} affected={:?} ttl_remaining={:.1}s",
                view.id,
                view.base_revision,
                view.target_path,
                view.affected_paths,
                remaining.as_secs_f64(),
            );
        }
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
        println!("--- paint_scene tree (count={}) ---", read_count(&self.state_scene));
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
            .with_title("pinion ai-introspect-demo (§5.32 + §5.33 + §5.34)")
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
        println!("R40.8 propose/apply dogfood — §5.32 locate + §5.33 overlay + §5.34 lifecycle");
        println!("  right-click: locate + red highlight");
        println!("  left-click / Esc: clear highlights (Esc×2 exits)");
        println!("  R: print paint_scene tree");
        println!("  P: propose count change (yellow outline marks pending)");
        println!("  A: apply preview, C: cancel preview, L: list previews");
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
                        Key::Character("p" | "P") => self.on_propose(),
                        Key::Character("a" | "A") => self.on_apply(),
                        Key::Character("c" | "C") => self.on_cancel(),
                        Key::Character("l" | "L") => self.on_list(),
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
