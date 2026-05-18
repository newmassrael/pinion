//! `hello-toggle` — R51.29 §5.38 second visual dogfood + live §2
//! bidirectional RPC, exercising the [`pinion_core::widgets::toggle`]
//! Tier-1 widget catalog entry instead of [`button`].
//!
//! Why this binary exists (substrate-incompleteness-signal evidence):
//! pre-R51.29 the paint-side `App` shape had only one client
//! (hello-button). The substrate (`paint_adapter`, `InputRouter`,
//! `IntentQueue`, `LayoutCache`, `RenderState` lifecycle, `DispatchContext`
//! producer / resize-request closures) is engineered as framework
//! primitives, but a single client does not surface boilerplate
//! repetition or missing API; a second client does. Mirroring the
//! hello-button structure here with **only** the Toggle-specific
//! diff — view fn, cached state shape `(ToggleState, bool)`, event
//! routing — keeps the framework code unchanged and exposes any
//! gap the next refactor must close. The Toggle SCXML is byte-for-byte
//! the Button machine plus a `value: bool` sidecar that flips on
//! every `Pressed -> Hover` activate transition (R51.2 §5.38).
//!
//! Same architecture as hello-button (R17 bidirectional RPC live
//! dogfood):
//!
//!   * The app owns the **state scene**:
//!     `Scene::External(Box<ToggleExternal>)`. The live Toggle SCXML
//!     statechart is reachable via the §5.15 introspect surface —
//!     there is no other copy of the state.
//!   * **Input flows through a single channel**: winit pointer events
//!     and JSON-RPC frames both hit `ExternalIntrospect::invoke
//!     ("send", Text(<event name>))`. winit translates `WindowEvent`
//!     to a `ToggleEvent` variant name; `pinion_rpc::dispatch` routes
//!     `scene/invoke` to the same method. §2 invariant #2 ("RPC
//!     headless as AI primary path") is literal.
//!   * **Output flows through the §5.20 intent channel**: after every
//!     winit event or RPC dispatch, `walk_scene_and_drain` pulls any
//!     pending `Intent` (here, a `toggle` carrying `IntrospectValue::
//!     Bool(new_value)` on every `Pressed -> Hover` flip) and logs
//!     them to stderr. The same intents stay reachable through the
//!     `scene/intents` RPC method so AI agents see the same emission
//!     stream a human observer would.
//!   * The **paint scene** is separate: `view(state, on, &Frame) ->
//!     Scene` (§6.3 pure sync) builds a `Scene::Container` (background +
//!     "Dark mode" label + track-with-knob + status text) from the
//!     current `(ToggleState, bool)` each frame. `paint_adapter::to_vello`
//!     walks that tree into a `vello::Scene`. Model/view split —
//!     state scene is authoritative, paint scene is a derived view.
//!   * A background thread reads JSON-RPC 2.0 lines from stdin and
//!     forwards each as a winit `UserEvent`; the main thread handles
//!     it on the UI thread, refreshes the cached `(state, value)` and
//!     requests a redraw if anything changed.

use std::io::{BufRead, Write};
use std::sync::Arc;
use std::thread;

use pinion_core::external::IntrospectValue;
use pinion_core::scene::{BoxNode, ContainerNode, ExternalNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::widgets::toggle::{ToggleEvent, ToggleExternal, ToggleState};
use pinion_core::{Color, Frame, Scene};
use pinion_core::SceneRevision;
use pinion_rpc::{build_layout_node, dispatch, DispatchContext, LayoutNode, PreviewLedger};
use pinion_runtime::{compute_layout, paint_adapter, walk_scene_and_drain, InputRouter, IntentQueue};
use pinion_text::LayoutCache;
use vello::Scene as VelloScene;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::{Window, WindowId};

// pinion-forge codegen output. Defines `pub struct HelloToggleRenderer`
// + `pub enum HelloToggleRendererError` + async `new<W: Into<wgpu::
// SurfaceTarget<'static>>>` + sync `render(&vello::Scene, peniko::Color)`
// + sync `resize(u32, u32)`. R46.3.3 emit template uses fully-qualified
// `::vello::*` paths so the include is bare (no `mod` wrap).
include!(concat!(env!("OUT_DIR"), "/app.rs"));

/// Winit user-event variants — today only the stdin-fed RPC line.
/// Identical shape to hello-button so the `spawn_stdin_rpc_reader`
/// pattern stays uniform across visual examples.
#[derive(Debug, Clone)]
enum AppEvent {
    /// One JSON-RPC 2.0 frame read from stdin, awaiting dispatch.
    RpcRequest(String),
}

const WIN_W: u32 = 360;
const WIN_H: u32 = 220;
// Window background — same dark navy hello-button uses, for visual
// consistency across the example gallery.
const BG_FILL: Color = Color::rgb(0x20, 0x30, 0x40);
// Track is a 64x32 rounded pill (radius 16 = half height = full pill).
// Padding 4 around the inner area gives a 24-px-tall inner strip that
// exactly matches the 24x24 knob, so the knob is vertically centered
// by AlignItems::Center without manual offset math.
const TRACK_W: u32 = 64;
const TRACK_H: u32 = 32;
const TRACK_RADIUS: u32 = 16;
const TRACK_PAD: u32 = 4;
const KNOB_SIZE: u32 = 24;
const KNOB_RADIUS: u32 = 12;
// Gap between "Dark mode" label, track, and status line in the root
// flex column — matches the macOS / iOS system settings vertical
// rhythm (~16 px between related controls).
const ROW_GAP: u32 = 16;

/// view-fn (§6.3): pure sync mapping `(ToggleState, bool) -> Scene`.
/// `&Frame` slot is the §6.3 ZST hedge — zero-cost today, ready for
/// `dt` / `frame_index` without a `SemVer` major. Purity is the §2
/// `dry_run` invariant: same `(state, value, frame)` always yields
/// the same `Scene`.
///
/// Layout (top-to-bottom, centered):
/// 1. "Dark mode" label (18 px white) — descriptive caption.
/// 2. Toggle track (64x32 rounded pill, tag = `main_toggle`):
///    fill colour encodes the joint `(state, value)` cross product;
///    the inner 24x24 knob justifies Start when Off / End when On.
/// 3. Status line ("`<State>` | `<Value>`", 12 px grey) — text-only
///    state mirror so the AI side can verify by reading the Scene
///    tree even when the screenshot path is unavailable.
///
/// R48 §5.35: the `main_toggle` tag on the track container is the
/// `InputRouter`'s hit-test handle — pointer events resolve to that
/// node and route to the matching `Scene::External("main_toggle")` in
/// the state scene. The knob and the labels carry no tag.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: ToggleState, on: bool, _frame: &Frame) -> Scene {
    // Track fill — encodes the (state, value) cross product. The Off
    // column stays in greyscale; the On column shifts to a green
    // accent (system "active" affordance). Pressed darkens both
    // columns for haptic feedback. Disabled is a distinct muted
    // brown-grey so users can visually distinguish it from Hover-off
    // (matches the macOS / iOS convention that disabled controls are
    // chromatically muted, not just dimmer).
    let track_fill: Color = match (state, on) {
        (ToggleState::Idle, false) => Color::rgb(0x40, 0x40, 0x40),
        (ToggleState::Hover, false) => Color::rgb(0x55, 0x55, 0x55),
        (ToggleState::Pressed, false) => Color::rgb(0x30, 0x30, 0x30),
        (ToggleState::Idle, true) => Color::rgb(0x30, 0xa0, 0x50),
        (ToggleState::Hover, true) => Color::rgb(0x40, 0xb0, 0x60),
        (ToggleState::Pressed, true) => Color::rgb(0x20, 0x70, 0x40),
        (ToggleState::Disabled, _) => Color::rgb(0x4a, 0x42, 0x38),
    };
    // Knob stays pure white in interactive states (canonical iOS /
    // Material affordance for the thumb), drops to a muted grey
    // when the widget is Disabled so it visually reads as inactive.
    let knob_fill: Color = match state {
        ToggleState::Disabled => Color::rgb(0xa0, 0xa0, 0xa0),
        _ => Color::rgb(0xff, 0xff, 0xff),
    };
    // The animation-free "snap" form: Off positions the knob via
    // JustifyContent::Start, On via JustifyContent::End. Tween /
    // spring transitions are a §5.x carry — the framework needs a
    // time source on the view-fn before that can land.
    let knob_justify = if on {
        JustifyContent::End
    } else {
        JustifyContent::Start
    };
    let knob = Scene::Box(
        BoxNode::new(
            Rect::default(),
            BoxStyle::filled(knob_fill).with_corner_radius(KNOB_RADIUS),
        )
        .with_layout(LayoutStyle::new().with_size(Size::px(KNOB_SIZE, KNOB_SIZE))),
    );
    let track = Scene::Container(
        ContainerNode::new(vec![knob])
            // R48 §5.35: dispatch identifier matching the state-scene
            // `Scene::External("main_toggle")`. Hit-tests on the track
            // (not the inner knob) route to the live ToggleExternal.
            .with_tag("main_toggle")
            .with_style(BoxStyle::filled(track_fill).with_corner_radius(TRACK_RADIUS))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_justify(knob_justify)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(TRACK_W, TRACK_H))
                    .with_padding(Rect::new(TRACK_PAD, TRACK_PAD, TRACK_PAD, TRACK_PAD)),
            ),
    );
    let label = Scene::Text(TextNode::styled(
        "Dark mode",
        Rect::default(),
        TextStyle::new()
            .with_size_px(18)
            .with_fg(Color::rgb(0xe0, 0xe0, 0xe0)),
    ));
    let status_str = format!(
        "{} | {}",
        toggle_state_name(state),
        if on { "On" } else { "Off" },
    );
    let status = Scene::Text(TextNode::styled(
        status_str,
        Rect::default(),
        TextStyle::new()
            .with_size_px(12)
            .with_fg(Color::rgb(0x90, 0x90, 0x90)),
    ));
    Scene::Container(
        ContainerNode::new(vec![label, track, status])
            .with_style(BoxStyle::filled(BG_FILL))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center)
                    .with_gap(ROW_GAP),
            ),
    )
}

/// Read the current `(ToggleState, bool)` pair from the live state
/// scene through the §5.15 introspect channel — the same path an RPC
/// `scene/query /external/state` + `scene/query /external/value`
/// request uses. Returns `(Idle, false)` defensively if the scene
/// shape is unexpected (should not happen with the current `App::new`
/// setup).
fn read_state(scene: &Scene) -> (ToggleState, bool) {
    if let Scene::External(node) = scene {
        if let Some(intro) = node.handle.introspect() {
            let state = if let Some(IntrospectValue::Text(name)) = intro.query("state") {
                parse_toggle_state(&name)
            } else {
                ToggleState::Idle
            };
            let value = matches!(intro.query("value"), Some(IntrospectValue::Bool(true)));
            return (state, value);
        }
    }
    (ToggleState::Idle, false)
}

fn parse_toggle_state(name: &str) -> ToggleState {
    match name {
        "Hover" => ToggleState::Hover,
        "Pressed" => ToggleState::Pressed,
        "Disabled" => ToggleState::Disabled,
        // "Idle" + anything unexpected — defensive default.
        _ => ToggleState::Idle,
    }
}

fn toggle_state_name(state: ToggleState) -> &'static str {
    match state {
        ToggleState::Idle => "Idle",
        ToggleState::Hover => "Hover",
        ToggleState::Pressed => "Pressed",
        ToggleState::Disabled => "Disabled",
    }
}

/// Mirror of `parse_toggle_event` in `pinion-core::widgets::toggle` —
/// the winit handler side. Converts a typed `ToggleEvent` to the
/// string name the §5.15 `invoke("send", ...)` channel expects.
/// Future internal SCXML-only variants would route through the
/// wildcard with a sentinel name the parser rejects.
fn toggle_event_name(event: ToggleEvent) -> &'static str {
    match event {
        ToggleEvent::PointerEnter => "PointerEnter",
        ToggleEvent::PointerLeave => "PointerLeave",
        ToggleEvent::PointerDown => "PointerDown",
        ToggleEvent::PointerUp => "PointerUp",
        ToggleEvent::Disable => "Disable",
        ToggleEvent::Enable => "Enable",
        _ => "__internal__",
    }
}

/// Window + renderer lifecycle (R46.3.4 §5.16). Identical to
/// hello-button's `RenderState` — the framework primitive shape is
/// stable across visual examples. Mobile suspend/resume → Vello
/// surface drop-and-recreate; desktop fires `resumed` once at boot
/// and never `suspended`. `Suspended(Some(window))` caches the winit
/// `Window` across the renderer drop-and-recreate cycle.
enum RenderState {
    Active {
        window: Arc<Window>,
        /// Boxed because `HelloToggleRenderer` is ~1.5 KiB (wgpu /
        /// vello state) while `Suspended` is two words; without the
        /// indirection the whole enum would pay the larger size
        /// (clippy `large_enum_variant`, same R47.1.1 fix as
        /// hello-button).
        renderer: Box<HelloToggleRenderer>,
    },
    Suspended(Option<Arc<Window>>),
}

struct App {
    /// Authoritative state scene — owns the live `ToggleExternal` via
    /// `Box<dyn External>`. Both winit input and RPC dispatch reach
    /// the SCXML statechart through this single scene.
    scene: Scene,
    /// Cached projection of `(ToggleState, ToggleExternal::is_on())`,
    /// kept in sync by `refresh_state` after every input. Drives the
    /// change-detection redraw request and the paint scene's joint
    /// `(state, value)` colour mapping.
    cached_state: (ToggleState, bool),
    /// §5.20 intent harvest buffer. Refilled by `drain_intents` after
    /// every winit / RPC event; consumed by stderr logging (printed
    /// as `intent: toggle payload=Bool(<new_value>)`). The
    /// `scene/intents` RPC method drains the same source independently
    /// since the underlying `External::pending_intents` is the single
    /// queue.
    intent_queue: IntentQueue,
    /// §5.34 preview lifecycle ledger — passed into every
    /// `pinion_rpc::dispatch` call alongside the scene. The lifecycle
    /// RPC methods read or mutate it through interior mutability;
    /// non-lifecycle methods ignore it.
    previews: PreviewLedger,
    /// §5.34 R40.4 OCC revision token. `dispatch` auto-bumps on
    /// mutating RPC methods; [`forward`](App::forward) explicitly
    /// bumps after the winit-side `invoke` since that path bypasses
    /// the dispatcher entirely.
    revision: SceneRevision,
    /// R48 §5.35 framework-side input dispatch primitive. Owns the
    /// retained paint scene + cursor state + `hover_target` and
    /// routes pointer events to the matching `ExternalNode` in
    /// `self.scene` (here: tag = `main_toggle`).
    router: InputRouter,
    /// R46.5 §5.16 suspend / resume lifecycle (R46.3.4 pattern).
    state: RenderState,
    /// Reusable Vello scene buffer — reset (`scene.reset()`) at the
    /// start of each frame rather than reallocated. Vello's Scene API
    /// expects this pattern (Linebender Vello examples / Xilem).
    vello_scene: VelloScene,
    /// R47.3 §5.36 — owned [`LayoutCache`] (LRU 256). `paint_adapter`'s
    /// Text arm consults this cache for every `Scene::Text` it walks,
    /// so the toggle's static labels ("Dark mode" / status) shape
    /// once on first paint and hit the cache on every subsequent
    /// frame. The cache also owns parley's `FontContext` /
    /// `LayoutContext` so the App never holds parley state directly.
    text_cache: LayoutCache,
    /// R47.7.5 §5.12 — most recent winit-rendered frame's paint scene
    /// projected into a [`LayoutNode`] tree. `render()` refreshes
    /// this at the end of every paint pass; `dispatch_rpc` hands it
    /// to `DispatchContext::with_last_paint_layout` so AI clients
    /// reach the winit-actual frame via `scene/layout {viewport: null}`.
    /// `None` until the first frame has rendered.
    last_paint_layout: Option<LayoutNode>,
}

impl App {
    fn new() -> Self {
        // R22 §5.20: the scene-side `ExternalNode.tag` supplies the
        // widget identifier used as the intent-tag prefix. The
        // ToggleExternal itself emits the `"toggle"` kind on every
        // activate; the runtime walk composes `main_toggle.toggle` on
        // drain.
        let scene = Scene::External(
            ExternalNode::new(Box::new(ToggleExternal::new())).with_tag("main_toggle"),
        );
        // Initial (state, value) read via the same introspect channel
        // everything else uses — single source of truth.
        let cached_state = read_state(&scene);
        eprintln!(
            "toggle: initial state = {:?}, value = {}",
            cached_state.0, cached_state.1
        );
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
            last_paint_layout: None,
        }
    }

    /// Translate a typed `ToggleEvent` (from a winit handler) into
    /// the symbolic `invoke("send", Text(<name>))` call — the same
    /// channel the RPC `scene/invoke` route uses. Failures from the
    /// statechart (`InvokeError::Rejected` etc.) are swallowed: the
    /// SCXML decides whether a given transition fires.
    fn forward(&mut self, event: ToggleEvent) {
        let name = toggle_event_name(event);
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
    /// `scene/invoke /external/send PointerEnter` (and friends) drive
    /// the SCXML the same way a winit click would.
    ///
    /// R47.7.2 §5.12 — `scene/layout` requests reach the framework
    /// via `DispatchContext::with_paint_producer`: the closure captures
    /// `cached_state` (`Copy`) and `text_cache` (`&mut`), runs `view`
    /// and then `compute_layout` for the hypothetical viewport, and
    /// returns the freshly-measured paint scene. The dispatch block
    /// scope releases the split borrows before `self.refresh_state()`
    /// runs.
    fn dispatch_rpc(&mut self, request: &str) {
        let resp = {
            // Disjoint-field split mutable borrows so the producer
            // closure can capture `cached_state` + `text_cache` while
            // the dispatcher still gets `scene` + `previews` + `revision`.
            let scene_ptr = &mut self.scene;
            let previews = &self.previews;
            let revision = &self.revision;
            let cached_state = self.cached_state;
            let text_cache_ptr = &mut self.text_cache;
            let state_ref = &self.state;
            let last_paint = self.last_paint_layout.as_ref();
            let mut produce = |w: u32, h: u32| -> Scene {
                let frame = Frame::new();
                let mut paint = view(cached_state.0, cached_state.1, &frame);
                compute_layout(&mut paint, text_cache_ptr, w, h);
                paint
            };
            // R47.7.4.2 — `scene/resize` reaches winit through this
            // closure: `request_inner_size` queues a size change that
            // winit emits as a `Resized` event on the next loop pass,
            // and the explicit `request_redraw` shortens the gap to
            // the new paint scene observation.
            let mut resize_req = |w: u32, h: u32| {
                if let RenderState::Active { window, .. } = state_ref {
                    let _ = window.request_inner_size(LogicalSize::new(w, h));
                    window.request_redraw();
                }
            };
            // R47.7.5 §5.12 — surface the most recent winit-rendered
            // frame to the dispatcher so `scene/layout {viewport: null}`
            // returns the actual frame snapshot. Builder pattern keeps
            // the `Option` wiring branchless at the AI-client level.
            let mut ctx = DispatchContext::new(scene_ptr, previews, revision)
                .with_paint_producer(&mut produce)
                .with_resize_request(&mut resize_req);
            if let Some(snapshot) = last_paint {
                ctx = ctx.with_last_paint_layout(snapshot);
            }
            dispatch(&mut ctx, request)
        };
        if let Some(resp) = resp {
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
    /// into the local queue, log each one to stderr. The
    /// `scene/intents` RPC method races with this drain — whichever
    /// caller harvests first wins (poll-form, single-consumer v0).
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

    /// Re-read the cached `(ToggleState, bool)` from the live scene;
    /// log and repaint if either dimension changed since the previous
    /// refresh.
    fn refresh_state(&mut self) {
        let now = read_state(&self.scene);
        if now != self.cached_state {
            eprintln!(
                "toggle: ({:?}, {}) -> ({:?}, {})",
                self.cached_state.0, self.cached_state.1, now.0, now.1
            );
            self.cached_state = now;
            self.request_redraw();
        }
    }

    fn request_redraw(&self) {
        if let RenderState::Active { window, .. } = &self.state {
            window.request_redraw();
        }
    }

    /// Build the paint scene for the current cached `(state, value)`,
    /// run layout, hand it to the framework-side `paint_adapter`
    /// walker, and submit the resulting `vello::Scene` to
    /// `HelloToggleRenderer`. No-op while suspended (R46.3.4
    /// lifecycle).
    fn render(&mut self) {
        let RenderState::Active { window, renderer } = &mut self.state else {
            return;
        };
        let size = window.inner_size();
        let Some(w) = std::num::NonZeroU32::new(size.width) else { return };
        let Some(h) = std::num::NonZeroU32::new(size.height) else { return };
        // §6.3 view-fn → §5.11 Scene tree; cached (state, value)
        // drives the ephemeral paint scene each frame.
        let frame = Frame::new();
        let mut paint_scene = view(self.cached_state.0, self.cached_state.1, &frame);
        // R24 §5.21 + R47.4 §5.36: taffy resolves every node's pixel
        // rect before paint. Scene::Text leaves go through parley
        // intrinsic measure via `self.text_cache`; the same cache is
        // hit by the paint adapter below so shape work amortizes
        // across measure + paint within one frame.
        compute_layout(&mut paint_scene, &mut self.text_cache, w.get(), h.get());
        // R46.5 §5.16: framework-side Scene → vello::Scene walk via
        // paint_adapter (R46.3.1). hello-toggle has no app-specific
        // tag substitution (the styling cross-product lives entirely
        // inside `view`), so the closure returns None unconditionally
        // and every Box honours its native `style.fill`. R47.3 §5.36
        // — the Text arm consults `self.text_cache` (parley shaping
        // LRU) so the "Dark mode" label shapes once and hits the
        // cache on every subsequent frame; the status line re-shapes
        // only when (state, value) changes the rendered string.
        self.vello_scene.reset();
        let base = paint_adapter::root_background(&paint_scene);
        paint_adapter::to_vello(
            &paint_scene,
            &|_b: &BoxNode| None,
            &mut self.text_cache,
            &mut self.vello_scene,
        );
        if let Err(e) = renderer.render(&self.vello_scene, base) {
            eprintln!("hello-toggle: vello render: {e}");
        }
        // R47.7.5 §5.12 — snapshot the freshly-measured paint scene
        // into a `LayoutNode` tree so `scene/layout {viewport: null}`
        // can return the *actual* winit-rendered frame on the next
        // dispatch. Must run before `router.update_paint_scene` moves
        // `paint_scene` out of scope.
        self.last_paint_layout = Some(build_layout_node(&paint_scene, "/0"));
        // R48 §5.35: hand the post-layout paint scene to the framework
        // router. The router retains it for subsequent hit-tests and
        // re-resolves hover_target now (window resize may have moved
        // the track rect under a stationary cursor).
        self.router.update_paint_scene(paint_scene, &mut self.scene);
        self.refresh_state();
        self.drain_intents();
    }
}

impl ApplicationHandler<AppEvent> for App {
    /// R46.3.4 — winit may fire `resumed` more than once on platforms
    /// that suspend (Android, Wayland-compositor focus changes). The
    /// Vello canonical pattern caches the previous `Window` across
    /// the drop-and-recreate cycle so the OS-side handle survives,
    /// while the GPU `HelloToggleRenderer` is freshly constructed
    /// each time.
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
                .with_title("pinion hello-toggle (R51.29 §5.38 Vello)")
                .with_inner_size(winit::dpi::LogicalSize::new(
                    f64::from(WIN_W),
                    f64::from(WIN_H),
                ));
            match event_loop.create_window(attrs) {
                Ok(w) => Arc::new(w),
                Err(e) => {
                    eprintln!("hello-toggle: window create failed: {e}");
                    event_loop.exit();
                    return;
                }
            }
        };
        let size = window.inner_size();
        let renderer = pollster::block_on(HelloToggleRenderer::new(
            Arc::clone(&window),
            size.width.max(1),
            size.height.max(1),
        ));
        let renderer = match renderer {
            Ok(r) => r,
            Err(e) => {
                eprintln!("hello-toggle: HelloToggleRenderer::new: {e}");
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
        // R47.7.5 — winit does not auto-emit `RedrawRequested` on
        // `resumed` (platform-dependent). Explicitly request the
        // first redraw so `last_paint_layout` populates before the
        // first AI client `scene/layout {viewport: null}` lands.
        self.request_redraw();
        eprintln!(
            "hello-toggle: hover/click the track to flip the value.\n           keys: d=Disable, e=Enable, Esc=quit\n           RPC: pipe JSON-RPC 2.0 frames (one per line) on stdin\n           §5.20: toggle intents log to stderr after each activate"
        );
    }

    /// R46.3.4 — release the GPU-side renderer on suspend so the OS
    /// can reclaim the wgpu surface. The winit window itself is
    /// cached for the next `resumed` so its handle / OS state survives.
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
                eprintln!(
                    "toggle: final state = ({:?}, {})",
                    self.cached_state.0, self.cached_state.1
                );
                event_loop.exit();
            }
            // R48 §5.35: all pointer routing flows through the
            // framework InputRouter. The handler bodies just forward
            // the winit event into the router; the router does the
            // hit-test, emits PointerEnter/Leave/Down/Up to the
            // matching ExternalNode, and the app only refreshes its
            // cached state + drains intents afterwards.
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
                        Key::Character("d") => self.forward(ToggleEvent::Disable),
                        Key::Character("e") => self.forward(ToggleEvent::Enable),
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

/// Background thread: read JSON-RPC 2.0 lines from stdin and forward
/// each as an `AppEvent::RpcRequest` user event. Blank lines are
/// skipped; EOF or any read error terminates the thread quietly (the
/// GUI loop keeps running). The proxy `send_event` fails only after
/// the event loop has shut down, in which case we also exit the
/// thread.
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
        eprintln!("hello-toggle: event loop error: {e}");
    }
}
