//! R40.8 / R42 — propose/apply visual dogfood for §5.34's preview
//! lifecycle, simplified to a **single canonical scene** after R42's
//! nested-External path walker landed.
//!
//! Closes the §5.34 R40.x series with the first user-visible round-trip
//! through `scene/propose_change` → `scene/apply_preview`. Builds on
//! R39.4.3's locate+highlight dogfood: the original right-click /
//! locate / red-outline interaction is preserved verbatim, and four new
//! keybindings (`P`/`A`/`C`/`L`) drive the preview lifecycle against an
//! introspectable [`CountedExternal`] embedded **inside** the same
//! scene as the visible widgets.
//!
//! ## Single canonical scene (R42 textbook recovery)
//!
//! R40.8's first cut held two scenes — `state_scene` for RPC mutation
//! and `paint_scene` for rendering — because the v0 `rewind`/`query`
//! path walker only resolved root-`External` scenes. R42 lifted that
//! constraint: the `/external/` literal now acts as a separator
//! between scene-walk segments and the introspect path, so a scene
//! tree containing both visible widgets **and** a tagged
//! `ExternalNode` becomes addressable as `/counter/external/count`.
//!
//! Result: one [`Scene`] field holds buttons + `info_panel` +
//! `counter` (External) + overlay highlights. RPC mutations and
//! rendering target the same tree. Overlay state is updated in-place
//! via a sentinel-swap dance ([`Scene`] is `!Clone` because
//! `ExternalNode` owns a `Box<dyn External>`).
//!
//! Controls (unchanged from R40.8):
//!
//!   * **Right-click** — `scene/locate` (§5.32) + red outline
//!   * **Left-click**  — clear all overlay highlights
//!   * **Escape**      — clear highlights; second press exits
//!   * **R**           — print the canonical scene tree to stdout
//!   * **P**           — `scene/propose_change`: cycle `info_panel`'s
//!                       fill to the next palette entry. Yellow
//!                       border appears around `info_panel` while
//!                       the preview is in flight.
//!   * **A**           — `scene/apply_preview`: commit the most-
//!                       recent preview. `info_panel` colour shifts;
//!                       yellow overlay disappears.
//!   * **C**           — `scene/cancel_preview`: drop the most-
//!                       recent preview without mutating state.
//!   * **L**           — `scene/list_previews`: print every in-flight
//!                       preview to stdout.

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
use pinion_core::scene::{BoxNode, ContainerNode, EffectNode, ExternalNode, Rect};
use pinion_core::style::{Border, BoxStyle, Color};
use pinion_core::{Scene, SceneRevision};
use pinion_overlay::{clear_highlights, inject_highlight, HighlightStyle};
use pinion_rpc::{
    apply_preview, cancel_preview, list_previews, locate, propose_change, query, ApplyError,
    PreviewId, PreviewLedger, ProposeError, TypedProposal,
};

const WIN_W: u32 = 640;
const WIN_H: u32 = 360;

/// Background-colour palette `info_panel` cycles through as the
/// embedded `counter` External advances. Five entries keep the demo
/// loop tight while still showing apply has visible effect.
const PALETTE: &[u32] = &[
    0x002a_2a2a, // grey  (count % 5 == 0, initial)
    0x002a_5a8a, // blue
    0x008a_5a2a, // brown
    0x002a_8a5a, // green
    0x008a_2a5a, // magenta
];

/// Yellow border used to mark `info_panel` while a preview is in
/// flight. Distinct from the default red used by locate-highlight.
const PENDING_HIGHLIGHT: HighlightStyle = HighlightStyle::new()
    .with_stroke(Color::from_argb(0x00ff_d000))
    .with_stroke_width(3);

/// Map the introspectable `count` value to a palette colour.
fn palette_color(count: i64) -> Color {
    let len = PALETTE.len() as i64;
    let idx = count.rem_euclid(len) as usize;
    Color::from_argb(PALETTE[idx])
}

/// Build the canonical scene: 3 tagged buttons + `info_panel` (Box
/// whose fill is *derived* at paint time from the embedded counter)
/// + `counter` (tagged ExternalNode holding the CountedExternal). One
/// tree, addressable end-to-end by both RPC and the renderer.
fn build_initial_scene() -> Scene {
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
        // info_panel: fill at construction is a placeholder; the
        // paint loop substitutes palette_color(read_count(scene)) so
        // the visible colour tracks the External's state without
        // needing per-frame BoxStyle mutation.
        Scene::Box(
            BoxNode::filled(Rect::new(60, 200, 520, 100), palette_color(0))
                .with_tag("info_panel"),
        ),
        // counter: invisible ExternalNode the RPC layer addresses as
        // `/counter/external/count`. Zero-rect because the demo does
        // not paint it; its purpose is to hold the introspectable
        // state slot.
        Scene::External(ExternalNode::new(Box::new(CountedExternal::new(0))).with_tag("counter")),
    ]);
    root.rect = Rect::new(0, 0, WIN_W, WIN_H);
    root.style = BoxStyle::filled(Color::from_argb(0x0011_1116));
    Scene::Container(root)
}

/// Read the current `count` from the canonical scene's `counter`
/// External via R42's nested-External path. Returns `0` on any
/// failure so the paint loop has a defined fallback.
fn read_count(scene: &Scene) -> i64 {
    match query(scene, "/counter/external/count") {
        Ok(IntrospectValue::Int(n)) => n,
        _ => 0,
    }
}

struct App {
    /// **The** scene. Single canonical tree holding visible widgets,
    /// the embedded `counter` External, and any active overlay
    /// highlights. RPC mutations and rendering both target this.
    scene: Scene,
    revision: SceneRevision,
    ledger: PreviewLedger,
    last_preview: Option<PreviewId>,
    /// Path suffixes the user has locate-highlighted (red). Re-applied
    /// on every overlay refresh so they survive preview-state changes.
    locate_highlights: Vec<String>,
    cursor: (u32, u32),
    pending_escape_exit: bool,
    window: Option<Rc<Window>>,
    context: Option<Context<Rc<Window>>>,
    surface: Option<Surface<Rc<Window>, Rc<Window>>>,
}

impl App {
    fn new() -> Self {
        Self {
            scene: build_initial_scene(),
            revision: SceneRevision::new(),
            ledger: PreviewLedger::default(),
            last_preview: None,
            locate_highlights: Vec::new(),
            cursor: (0, 0),
            pending_escape_exit: false,
            window: None,
            context: None,
            surface: None,
        }
    }

    /// Refresh overlay state without rebuilding the canonical scene
    /// (which would destroy the embedded External's accumulated
    /// state). Strategy:
    ///
    ///   1. Take ownership of `self.scene` via [`std::mem::replace`]
    ///      with a cheap sentinel (`Scene::Effect`).
    ///   2. Strip existing `ai-overlay/*` children with
    ///      [`clear_highlights`].
    ///   3. Re-inject the user's locate-highlights (red) and the
    ///      preview-pending indicator (yellow) when applicable.
    ///   4. Put the result back into `self.scene`.
    ///
    /// The sentinel swap exists because `Scene` is intentionally
    /// `!Clone` (`ExternalNode` carries `Box<dyn External>`). Owning
    /// the scene for the duration of the overlay edit avoids the
    /// `&mut Scene` reborrow gymnastics that the in-place mutation
    /// alternatives would require.
    fn refresh_overlays(&mut self) {
        let taken = std::mem::replace(&mut self.scene, Scene::Effect(EffectNode::new()));
        let cleared = clear_highlights(taken);
        let with_locate = self
            .locate_highlights
            .iter()
            .fold(cleared, |s, path| inject_highlight(s, path, HighlightStyle::default()));
        let with_preview = if self.last_preview.is_some() {
            inject_highlight(with_locate, "info_panel", PENDING_HIGHLIGHT)
        } else {
            with_locate
        };
        self.scene = with_preview;
    }

    fn on_right_click(&mut self) {
        let (x, y) = self.cursor;
        match locate(&self.scene, x, y) {
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
        self.refresh_overlays();
        self.request_redraw();
    }

    fn on_clear(&mut self) {
        // Strip all locate-highlights; preview-pending overlay stays
        // until apply/cancel so the user sees unfinished RPC business.
        self.locate_highlights.clear();
        self.refresh_overlays();
        self.request_redraw();
    }

    /// `P` — propose to advance `count` by one palette step. R42:
    /// `signal_path` walks Container → `counter` ExternalNode → "count"
    /// in a single nested address.
    fn on_propose(&mut self) {
        let current = read_count(&self.scene);
        let proposed = current + 1;
        let proposal = TypedProposal::SetSignal {
            target_path: "/info_panel".to_string(),
            signal_path: "/counter/external/count".to_string(),
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
        self.refresh_overlays();
        self.request_redraw();
    }

    /// `A` — apply the most-recent preview. Mutates `counter`'s count
    /// inside `self.scene`; the next paint reads the new count and
    /// derives the new `info_panel` colour.
    fn on_apply(&mut self) {
        let Some(id) = self.last_preview else {
            println!("→ scene/apply_preview\n  err: no preview in flight (press P first)");
            return;
        };
        match apply_preview(&mut self.scene, &self.revision, &self.ledger, id) {
            Ok(outcome) => {
                let new_count = read_count(&self.scene);
                println!(
                    "→ scene/apply_preview\n  preview_id: {}\n  new_revision: {}\n  new_count: {} (palette idx {})\n  emitted_intents: {} (R40.9 channel)",
                    outcome.preview_id,
                    outcome.new_revision,
                    new_count,
                    new_count.rem_euclid(PALETTE.len() as i64),
                    outcome.emitted_intents.len(),
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
        self.refresh_overlays();
        self.request_redraw();
    }

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
        self.refresh_overlays();
        self.request_redraw();
    }

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
                Scene::External(e) => {
                    println!("{indent}External rect={:?} tag={:?}", e.rect, e.tag);
                }
                _ => println!("{indent}<{:?}>", std::mem::discriminant(s)),
            }
        }
        println!("--- scene tree (count={}) ---", read_count(&self.scene));
        walk(&self.scene, 0);
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
        let count = read_count(&self.scene);
        paint(&self.scene, count, &mut buffer, buf_w, buf_h);
        let _ = buffer.present();
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("pinion ai-introspect-demo (§5.32 + §5.33 + §5.34 + R42)")
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
        println!("R42 single-scene dogfood — §5.32 locate + §5.33 overlay + §5.34 lifecycle");
        println!("  right-click: locate + red highlight");
        println!("  left-click / Esc: clear highlights (Esc×2 exits)");
        println!("  R: print scene tree");
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
/// emitted by the overlay highlight). `info_panel` (tag-matched) is
/// painted with the palette colour derived from `count` instead of
/// its placeholder BoxStyle.fill. Other Scene variants are no-ops at
/// this demo's scope.
fn paint(scene: &Scene, count: i64, buf: &mut [u32], w: usize, h: usize) {
    match scene {
        Scene::Container(c) => {
            paint_filled_rect(c.rect, c.style.fill, buf, w, h);
            for child in &c.children {
                paint(child, count, buf, w, h);
            }
        }
        Scene::Box(b) => {
            let fill = if b.tag.as_deref() == Some("info_panel") {
                palette_color(count)
            } else {
                b.style.fill
            };
            paint_filled_rect(b.rect, fill, buf, w, h);
            if let Some(border) = b.style.border {
                paint_border(b.rect, border, buf, w, h);
            }
        }
        // External / Effect / Text / Path / Image: invisible at this
        // demo's scope. External holds the counter state but is not
        // rendered itself.
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
