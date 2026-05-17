//! R46.3 §5.16 — ai-introspect-demo on the Vello render path.
//!
//! Same RPC + locate + preview UX as the R42 single-scene dogfood,
//! retargeted from `softbuffer` to a pinion-forge-generated Vello
//! renderer. The renderer struct (`DemoRenderer`) is emitted by
//! `build.rs` from `app.pinion.xml` (`kind="renderer" backend="vello"`,
//! `aa` default = Area per R46.2.1) into `$OUT_DIR/app.rs`; this file
//! pulls it in via `include!` inside a private `gen_renderer` module
//! so the codegen's `use vello::*` imports don't collide with this
//! module's `use pinion_core::style::Color`.
//!
//! ## Single canonical scene (R42 textbook recovery, preserved)
//!
//! One [`Scene`] holds buttons + `info_panel` + `counter` (External)
//! + overlay highlights. RPC mutations and rendering target the same
//! tree. The pre-R46.3 `paint()` function that drew rects into a
//! softbuffer u32 buffer has been replaced by [`build_vello_scene`],
//! which walks the same [`Scene`] tree and emits `vello::Scene` fill /
//! stroke commands; the rest of the demo (R39.4.3 locate, R40.x
//! preview lifecycle) is unchanged.
//!
//! Controls (unchanged from R42):
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
    // R46.3 rustc 1.86 MSRV bump surfaced this on pre-existing R42
    // `PALETTE.len() as i64` patterns at palette_color / on_propose /
    // on_apply — usize→i64 cast on 64-bit targets can in principle
    // wrap, but PALETTE.len() = 5 (always < i64::MAX). Same scope
    // discipline as cast_possible_truncation above.
    clippy::cast_possible_wrap,
    clippy::doc_markdown,
    // Demo-narrative doc comments use visual alignment (continuation
    // lines indented to match the `**Key** — ` prefix). Rust 1.86
    // tightened doc-list lints to flag this as ambiguous markdown.
    // Example-scope prose, not framework API documentation.
    clippy::doc_overindented_list_items,
    clippy::doc_lazy_continuation,
)]

use std::sync::Arc;
use std::time::Instant;

use vello::Scene as VelloScene;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

use pinion_core::external::{CountedExternal, IntrospectValue};
use pinion_core::scene::{BoxNode, ContainerNode, EffectNode, ExternalNode, Rect};
use pinion_core::style::{BoxStyle, Color};
use pinion_core::{Scene, SceneRevision};
use pinion_overlay::{HighlightStyle, clear_highlights, inject_highlight};
use pinion_rpc::{
    ApplyError, PreviewId, PreviewLedger, ProposeError, TypedProposal, apply_preview,
    cancel_preview, list_previews, locate, propose_change, query,
};
use pinion_runtime::paint_adapter;

// pinion-forge codegen output. Defines `pub struct DemoRenderer { ... }`
// + `pub enum DemoRendererError` + async `new<W: Into<wgpu::SurfaceTarget<'static>>>`
// + sync `render(&vello::Scene, peniko::Color)` + sync `resize(u32, u32)`.
// R46.3.3 — the template uses fully-qualified `::vello::*` paths (no
// `use` items), so include!() at module scope no longer needs the
// previous `mod gen_renderer { ... }` namespace-isolation wrap.
// Matches the forge-counter reactive-emit consumer pattern.
include!(concat!(env!("OUT_DIR"), "/app.rs"));

const WIN_W: u32 = 640;
const WIN_H: u32 = 360;

/// Background-colour palette `info_panel` cycles through as the
/// embedded `counter` External advances. Five entries keep the demo
/// loop tight while still showing apply has visible effect.
///
/// R46.3.1 — typed `&[Color]` (rgb opaque) replaces the pre-R46.3 raw
/// `&[u32]` ARGB literals. The conversion through paint_adapter now
/// preserves alpha verbatim, so the previous `0x00RR_GGBB` shape
/// (alpha = 0) would render fully transparent on the Vello path.
const PALETTE: &[Color] = &[
    Color::rgb(0x2a, 0x2a, 0x2a), // grey  (count % 5 == 0, initial)
    Color::rgb(0x2a, 0x5a, 0x8a), // blue
    Color::rgb(0x8a, 0x5a, 0x2a), // brown
    Color::rgb(0x2a, 0x8a, 0x5a), // green
    Color::rgb(0x8a, 0x2a, 0x5a), // magenta
];

/// Yellow border used to mark `info_panel` while a preview is in
/// flight. Distinct from the default red used by locate-highlight.
const PENDING_HIGHLIGHT: HighlightStyle = HighlightStyle::new()
    .with_stroke(Color::rgb(0xff, 0xd0, 0x00))
    .with_stroke_width(3);

/// Map the introspectable `count` value to a palette colour.
fn palette_color(count: i64) -> Color {
    let len = PALETTE.len() as i64;
    let idx = count.rem_euclid(len) as usize;
    PALETTE[idx]
}

/// Build the canonical scene: 3 tagged buttons + `info_panel` (Box
/// whose fill is *derived* at paint time from the embedded counter)
/// + `counter` (tagged ExternalNode holding the CountedExternal). One
/// tree, addressable end-to-end by both RPC and the renderer.
fn build_initial_scene() -> Scene {
    let mut root = ContainerNode::new(vec![
        Scene::Box(
            BoxNode::filled(Rect::new(60, 80, 140, 60), Color::rgb(0xff, 0x33, 0x66))
                .with_tag("save_btn"),
        ),
        Scene::Box(
            BoxNode::filled(Rect::new(240, 80, 140, 60), Color::rgb(0x33, 0x88, 0xff))
                .with_tag("cancel_btn"),
        ),
        Scene::Box(
            BoxNode::filled(Rect::new(420, 80, 140, 60), Color::rgb(0xaa, 0xaa, 0xaa))
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
    root.style = BoxStyle::filled(Color::rgb(0x11, 0x11, 0x16));
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

/// Window + renderer lifecycle (R46.3.4 §5.16). Mirrors the Vello 0.6
/// canonical `RenderState` enum (Linebender examples / Xilem) so the
/// demo survives the Android / Wayland suspend → resume cycle where
/// the wgpu surface backing must be dropped and re-created. Desktop
/// targets fire `resumed` once at boot and never `suspended`; mobile
/// targets fire it on every focus change.
///
/// `Suspended(Some(window))` caches the winit `Window` across the
/// drop-and-recreate cycle so the user's window position / OS handle
/// survives, while the GPU-side `DemoRenderer` (which transitively
/// owns a `wgpu::Surface`) is released for the OS to reclaim.
enum RenderState {
    /// Window is on-screen + renderer is alive. The application paints
    /// here.
    Active {
        window: Arc<Window>,
        renderer: DemoRenderer,
    },
    /// Window is not currently visible. The `Option` carries the
    /// cached winit window when the previous `Active` state torn down;
    /// `None` is the initial state before `resumed` fires.
    Suspended(Option<Arc<Window>>),
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
    /// Suspend / resume lifecycle (R46.3.4). Replaces the previous
    /// `window: Option<Arc<Window>>` + `renderer: Option<DemoRenderer>`
    /// pair — keeping the two `Option`s in sync was error-prone and
    /// missed the mobile-suspend forward-compat requirement.
    state: RenderState,
    /// Reusable Vello scene buffer — reset (`scene.reset()`) at the
    /// start of each frame rather than reallocated. Vello's Scene API
    /// expects this pattern (see Linebender Vello examples / Xilem).
    vello_scene: VelloScene,
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
            state: RenderState::Suspended(None),
            vello_scene: VelloScene::new(),
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
        if let RenderState::Active { window, .. } = &self.state {
            window.request_redraw();
        }
    }

    /// Build a Vello scene from the canonical pinion [`Scene`] and
    /// submit one frame. The Vello buffer is reset at frame start so
    /// allocations amortize across the lifetime of the application
    /// (Linebender canonical pattern). On `wgpu::SurfaceError::Lost` /
    /// `Outdated`, the next `Resized` event will reconfigure the
    /// surface — the failure is logged and the frame is dropped, not
    /// retried, since winit will request another redraw shortly.
    ///
    /// R46.3.1 — the Scene → vello::Scene walk lives in
    /// `pinion_runtime::paint_adapter`. The closure passed to
    /// [`paint_adapter::to_vello`] supplies the only app-specific
    /// piece: the palette-indexed fill for the `info_panel` tag.
    /// R46.3.4 — render is a no-op while the app is suspended.
    fn render(&mut self) {
        let RenderState::Active { renderer, .. } = &mut self.state else { return };
        self.vello_scene.reset();
        let count = read_count(&self.scene);
        let base = paint_adapter::root_background(&self.scene);
        paint_adapter::to_vello(
            &self.scene,
            &|b: &BoxNode| {
                if b.tag.as_deref() == Some("info_panel") {
                    Some(palette_color(count))
                } else {
                    None
                }
            },
            &mut self.vello_scene,
        );
        if let Err(e) = renderer.render(&self.vello_scene, base) {
            eprintln!("ai-introspect-demo: vello render: {e}");
        }
    }
}

impl ApplicationHandler for App {
    /// R46.3.4 — winit may fire `resumed` more than once on platforms
    /// that suspend (Android, Wayland-compositor focus changes). The
    /// Vello canonical pattern caches the previous `Window` across the
    /// drop-and-recreate cycle so the OS-side handle survives, while
    /// the GPU `DemoRenderer` is freshly constructed each time.
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // Already on-screen — winit fired resumed twice in a row, no-op.
        if matches!(self.state, RenderState::Active { .. }) {
            return;
        }
        // Take the cached window (if any) without dropping the
        // surrounding state — `replace` keeps `self.state` valid
        // even on the create-failed early return below.
        let cached = match std::mem::replace(&mut self.state, RenderState::Suspended(None)) {
            RenderState::Suspended(cached) => cached,
            RenderState::Active { .. } => unreachable!("matched as non-Active above"),
        };
        let window = if let Some(w) = cached {
            w
        } else {
            let attrs = Window::default_attributes()
                .with_title("pinion ai-introspect-demo (R46.3.4 §5.16 Vello)")
                .with_inner_size(LogicalSize::new(f64::from(WIN_W), f64::from(WIN_H)));
            match event_loop.create_window(attrs) {
                Ok(w) => Arc::new(w),
                Err(e) => {
                    eprintln!("ai-introspect-demo: window create failed: {e}");
                    event_loop.exit();
                    return;
                }
            }
        };
        let size = window.inner_size();
        let renderer = pollster::block_on(DemoRenderer::new(
            Arc::clone(&window),
            size.width.max(1),
            size.height.max(1),
        ));
        let renderer = match renderer {
            Ok(r) => r,
            Err(e) => {
                eprintln!("ai-introspect-demo: DemoRenderer::new: {e}");
                // Keep the window cached so a subsequent resumed can
                // retry — only the renderer creation failed.
                self.state = RenderState::Suspended(Some(window));
                event_loop.exit();
                return;
            }
        };
        self.state = RenderState::Active { window, renderer };
        println!(
            "R46.3.4 §5.16 Vello dogfood — §5.32 locate + §5.33 overlay + §5.34 lifecycle"
        );
        println!("  right-click: locate + red highlight");
        println!("  left-click / Esc: clear highlights (Esc×2 exits)");
        println!("  R: print scene tree");
        println!("  P: propose count change (yellow outline marks pending)");
        println!("  A: apply preview, C: cancel preview, L: list previews");
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
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => self.render(),
            WindowEvent::Resized(size) => {
                if let RenderState::Active { renderer, .. } = &mut self.state {
                    renderer.resize(size.width.max(1), size.height.max(1));
                }
            }
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
